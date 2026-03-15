use clap::Parser;
use cli::command::LoadTestRunCli;
use cli::output::print_stats;
use lumen_core::command::run::RunCommand;
use lumen_core::command::{Command, Commands, ConfigureTemplateCommand};
use lumen_core::monitoring::SpanName;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;

mod cli;

fn main() {
    let cli_args = LoadTestRunCli::parse();

    // Extract thread count before consuming args — controls the tokio worker pool size.
    let threads = match &cli_args {
        LoadTestRunCli::Run(args) => args.threads as usize,
        _ => 1,
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
        .worker_threads(threads)
        .enable_all()
        .build()
        .expect("failed to create tokio runtime");

    let exit_code = runtime.block_on(async {
        let root = tracing::span!(tracing::Level::INFO, SpanName::RUN);
        let _enter = root.enter();

        let cmd = match cli_args {
            LoadTestRunCli::Run(args) => Commands::Run(RunCommand::from(args)),
            LoadTestRunCli::ConfigureRequest(args) => {
                Commands::ConfigureRequest(ConfigureTemplateCommand::from(args))
            }
            LoadTestRunCli::ConfigureResponse(args) => {
                Commands::ConfigureResponse(ConfigureTemplateCommand::from(args))
            }
        };

        let code = match cmd.execute().await {
            Ok(Some(stats)) => {
                print_stats(&stats.results, &stats);
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
