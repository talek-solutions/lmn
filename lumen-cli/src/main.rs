use std::time::Instant;

use clap::Parser;
use cli::command::{LoadTestRunCli, OutputFormat};
use cli::json_output::{JsonDest, WriteJsonOutputParams, write_json_output};
use cli::output::print_stats;
use lumen_core::command::run::RunCommand;
use lumen_core::command::{Command, Commands, ConfigureTemplateCommand};
use lumen_core::monitoring::SpanName;
use lumen_core::output::{RunReport, RunReportParams};
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;

mod cli;

fn main() {
    let cli_args = LoadTestRunCli::parse();

    // Capture output-related args before consuming cli_args below.
    // `reservoir_size` is also captured here so it can be passed to
    // RunReportParams once Dev 1's output module is merged (TECH.md R4).
    let (output_format, output_file, reservoir_size) = match &cli_args {
        LoadTestRunCli::Run(args) => (args.output, args.output_file.clone(), args.result_buffer),
        _ => (OutputFormat::Table, None, 100_000),
    };


    // Endpoint is read from OTEL_EXPORTER_OTLP_ENDPOINT env var at runtime,
    // falling back to http://localhost:4318 if unset.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .expect("failed to build OTLP exporter");

    let resource = Resource::builder()
        .with_service_name("lumen")
        .build();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer("lumen");
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

        let cmd = match cli_args {
            LoadTestRunCli::Run(args) => match RunCommand::try_from(args) {
                Ok(cmd) => Commands::Run(cmd),
                Err(e) => {
                    eprintln!("error: {e}");
                    return 1;
                }
            },
            LoadTestRunCli::ConfigureRequest(args) => {
                Commands::ConfigureRequest(ConfigureTemplateCommand::from(args))
            }
            LoadTestRunCli::ConfigureResponse(args) => {
                Commands::ConfigureResponse(ConfigureTemplateCommand::from(args))
            }
        };

        let run_start = Instant::now();
        let result = cmd.execute().await;

        let code = match result {
            Ok(Some(stats)) => {
                let report = match stats.curve_stages.as_deref() {
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
                        print_stats(&stats.results, &stats);
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
                0
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
