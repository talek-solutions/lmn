use clap::Parser;
use loadtest::cli::command::LoadTestRunCli;
use loadtest::command::run::RunCommand;
use loadtest::command::{Command, Commands, ConfigureTemplateCommand};
use loadtest::monitoring::SpanName;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_sdk::Resource;
use tracing_subscriber::Registry;
use tracing_subscriber::layer::SubscriberExt;
fn main() {
    // Endpoint is read from OTEL_EXPORTER_OTLP_ENDPOINT env var at runtime,
    // falling back to http://localhost:4318 if unset.
    let exporter = opentelemetry_otlp::SpanExporter::builder()
        .with_http()
        .build()
        .expect("failed to build OTLP exporter");

    let resource = Resource::builder()
        .with_service_name("loadtest")
        .build();

    let provider = SdkTracerProvider::builder()
        .with_batch_exporter(exporter)
        .with_resource(resource)
        .build();

    let tracer = provider.tracer("loadtest");

    // Create a tracing layer with the configured tracer
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    let subscriber = Registry::default().with(telemetry);

    // Trace executed code
    tracing::subscriber::set_global_default(subscriber).expect("failed to set global tracing subscriber");

    let root = tracing::span!(tracing::Level::INFO, SpanName::RUN);
    let _enter = root.enter();

    let cmd = match LoadTestRunCli::parse() {
        LoadTestRunCli::Run(args) => Commands::Run(RunCommand::from(args)),
        LoadTestRunCli::ConfigureRequest(args) => {
            Commands::ConfigureRequest(ConfigureTemplateCommand::from(args))
        }
        LoadTestRunCli::ConfigureResponse(args) => {
            Commands::ConfigureResponse(ConfigureTemplateCommand::from(args))
        }
    };

    let exit_code = match cmd.execute() {
        Ok(_) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    };

    drop(_enter);
    drop(root);
    let _ = provider.shutdown();
    std::process::exit(exit_code);
}
