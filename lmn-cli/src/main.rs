use std::time::Instant;

use clap::Parser;
use cli::command::{LoadTestRunCli, OutputFormat};
use cli::json_output::{JsonDest, WriteJsonOutputParams, write_json_output};
use cli::output::{PrintStatsParams, print_stats};
use lmn_core::command::{Command, Commands, ConfigureTemplateCommand};
use lmn_core::monitoring::SpanName;
use lmn_core::output::{RunReport, RunReportParams};
use lmn_core::threshold::{evaluate, EvaluateParams};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;

mod cli;

fn main() {
    let cli_args = LoadTestRunCli::parse();

    // Endpoint is read from OTEL_EXPORTER_OTLP_ENDPOINT env var at runtime,
    // falling back to http://localhost:4318 if unset.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .expect("failed to build OTLP exporter");

    let resource = Resource::builder()
        .with_service_name("lmn")
        .build();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer("lmn");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);
    let subscriber = Registry::default().with(telemetry);
    tracing::subscriber::set_global_default(subscriber)
        .expect("failed to set global tracing subscriber");

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    let exit_code = runtime.block_on(async {
        let root = tracing::span!(tracing::Level::INFO, SpanName::RUN);
        let _enter = root.enter();

        // Resolve run args (merges --config file values with CLI flags).
        let cmd = match cli_args {
            LoadTestRunCli::Run(args) => {
                match cli::adapter::RunArgsResolved::try_from(args) {
                    Ok(resolved) => {
                        let output_format = resolved.output;
                        let output_file = resolved.output_file.clone();
                        let reservoir_size = resolved.sampling.result_buffer;
                        let thresholds = resolved.thresholds.clone();
                        let run_cmd = resolved.into_run_command();
                        (Commands::Run(run_cmd), thresholds, output_format, output_file, reservoir_size)
                    }
                    Err(e) => {
                        eprintln!("error: {e}");
                        return 1;
                    }
                }
            }
            LoadTestRunCli::ConfigureRequest(args) => {
                (
                    Commands::ConfigureRequest(ConfigureTemplateCommand::from(args)),
                    None,
                    OutputFormat::Table,
                    None,
                    100_000,
                )
            }
            LoadTestRunCli::ConfigureResponse(args) => {
                (
                    Commands::ConfigureResponse(ConfigureTemplateCommand::from(args)),
                    None,
                    OutputFormat::Table,
                    None,
                    100_000,
                )
            }
        };

        let (commands, thresholds, output_format, output_file, reservoir_size) = cmd;

        let run_start = Instant::now();
        let result = commands.execute().await;

        let code = match result {
            Ok(Some(stats)) => {
                let mut report = match stats.curve_stages.as_deref() {
                    Some(stages) => RunReport::from_params_with_curve(
                        RunReportParams { stats: &stats, reservoir_size, run_start },
                        stages,
                    ),
                    None => RunReport::from_params(RunReportParams {
                        stats: &stats,
                        reservoir_size,
                        run_start,
                    }),
                };

                // Evaluate thresholds and attach to report so JSON output includes them.
                // exit code 2 = threshold failure; 1 = run error; 0 = success.
                let threshold_failed = if let Some(ref rules) = thresholds {
                    let tr = evaluate(EvaluateParams {
                        report: &report,
                        thresholds: rules,
                    });
                    let failed = !tr.all_passed();
                    report.thresholds = Some(tr);
                    failed
                } else {
                    false
                };

                // Determine whether to also write JSON to a file.
                // When --output-file is set, JSON is always written to the
                // file regardless of --output (TECH.md §4.2).
                if let Some(ref path) = output_file {
                    if let Err(e) = write_json_output(WriteJsonOutputParams {
                        report: &report,
                        dest: JsonDest::File(path.clone()),
                    }) {
                        eprintln!("error: {e}");
                        return 1;
                    }
                }

                match output_format {
                    OutputFormat::Table => {
                        // Table always goes to stdout regardless of --output-file.
                        print_stats(PrintStatsParams {
                            results: &stats.results,
                            stats: &stats,
                            threshold_report: report.thresholds.as_ref(),
                        });
                    }
                    OutputFormat::Json => {
                        // JSON is always emitted to stdout when --output json is set,
                        // whether or not --output-file is also provided (TECH.md §4.2
                        // behaviour matrix: rows 3 and 4 both produce stdout JSON).
                        if let Err(e) = write_json_output(WriteJsonOutputParams {
                            report: &report,
                            dest: JsonDest::Stdout,
                        }) {
                            eprintln!("error: {e}");
                            return 1;
                        }
                    }
                }

                // Exit code 2 when thresholds were evaluated and one or more failed.
                if threshold_failed { 2 } else { 0 }
            }
            Ok(None) => 0,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };

        drop(_enter);
        drop(root);
        code
    });

    let _ = provider.shutdown();
    std::process::exit(exit_code);
}
