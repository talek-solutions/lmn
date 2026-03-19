use std::path::PathBuf;

use clap::Parser;

#[derive(clap::ValueEnum, Clone, Copy, Debug, Default)]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
}

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Parser)]
#[command(name = "lumen")]
#[command(bin_name = "lumen")]
#[command(styles = CLAP_STYLING)]
pub enum LoadTestRunCli {
    Run(RunArgs),
    ConfigureRequest(ConfigureRequestArgs),
    ConfigureResponse(ConfigureResponseArgs),
}

#[derive(clap::Args)]
#[command(version, about)]
#[command(name = "run")]
#[command(long_about = "Run the test")]
pub struct RunArgs {
    #[arg(short='H')]
    #[arg(long)]
    #[arg(help = "The host to run lumen against (required unless set via run.host in --config)")]
    pub host: Option<String>,

    #[arg(short='R')]
    #[arg(long)]
    #[arg(help = "The count of API requests to run (default: 100)")]
    #[arg(allow_negative_numbers = false)]
    pub request_count: Option<u32>,

    #[arg(short='C')]
    #[arg(long)]
    #[arg(help = "Max number of requests in-flight at any one time (default: 100)")]
    #[arg(allow_negative_numbers = false)]
    pub concurrency: Option<u32>,

    #[arg(short='M')]
    #[arg(long)]
    #[arg(help = "HTTP method to use (default: get)")]
    pub method: Option<HttpMethod>,

    #[arg(short='B')]
    #[arg(long)]
    #[arg(help = "Request body (JSON only)")]
    #[arg(value_parser = parse_json)]
    #[arg(conflicts_with_all = ["request_template", "request_alias"])]
    pub body: Option<String>,

    #[arg(short='T')]
    #[arg(long = "request-template")]
    #[arg(help = "Path to a JSON template file with placeholder definitions")]
    #[arg(conflicts_with_all = ["body", "request_alias"])]
    pub request_template: Option<PathBuf>,

    #[arg(short='A')]
    #[arg(long = "request-alias")]
    #[arg(help = "Alias of a stored request template (name or name.json)")]
    #[arg(conflicts_with_all = ["body", "request_template"])]
    pub request_alias: Option<String>,

    #[arg(short='S')]
    #[arg(long = "response-template")]
    #[arg(help = "Path to a JSON response template for tracking response fields")]
    #[arg(conflicts_with = "response_alias")]
    pub response_template: Option<PathBuf>,

    #[arg(short='E')]
    #[arg(long = "response-alias")]
    #[arg(help = "Alias of a stored response template (name or name.json)")]
    #[arg(conflicts_with = "response_template")]
    pub response_alias: Option<String>,

    #[arg(short='L')]
    #[arg(long = "load-curve")]
    #[arg(help = "Path to a load curve JSON file (conflicts with -R and -C)")]
    #[arg(conflicts_with_all = ["request_count", "concurrency"])]
    pub load_curve: Option<std::path::PathBuf>,

    #[arg(long = "sample-threshold")]
    #[arg(help = "VU count below which all results are collected (0 = disabled) (default: 50)")]
    pub sample_threshold: Option<usize>,

    #[arg(long = "result-buffer")]
    #[arg(help = "Max results to retain for percentile computation (default: 100000)")]
    pub result_buffer: Option<usize>,

    #[arg(long = "output")]
    #[arg(help = "Output format: table (default) or json")]
    pub output: Option<OutputFormat>,

    #[arg(long = "output-file")]
    #[arg(help = "Write JSON result to <path> (always JSON regardless of --output)")]
    pub output_file: Option<PathBuf>,

    #[arg(short = 'f')]
    #[arg(long = "config")]
    #[arg(help = "Path to a YAML config file. CLI flags take precedence over config values.")]
    pub config: Option<PathBuf>,
}


#[derive(clap::Args)]
#[command(version, about)]
#[command(name = "configure-request")]
#[command(long_about = "Configure a persistent request body template")]
pub struct ConfigureRequestArgs {
    #[arg(short='A')]
    #[arg(long)]
    #[arg(help = "Template alias")]
    #[arg(required = true)]
    pub alias: String,
    
    #[arg(short='B')]
    #[arg(long)]
    #[arg(help = "Request body (JSON only)")]
    #[arg(value_parser = parse_json)]
    #[arg(conflicts_with = "template_path")]
    #[arg(requires = "alias")]
    pub body: Option<String>,
    
    #[arg(short='T')]
    #[arg(long)]
    #[arg(help = "Path to a request template")]
    #[arg(conflicts_with = "body")]
    pub template_path: Option<PathBuf>,
}

#[derive(clap::Args)]
#[command(version, about)]
#[command(name = "configure-response")]
#[command(long_about = "Configure a persistent request body template")]
pub struct ConfigureResponseArgs {
    #[arg(short='A')]
    #[arg(long)]
    #[arg(help = "Template alias")]
    #[arg(required = true)]
    pub alias: String,

    #[arg(short='B')]
    #[arg(long)]
    #[arg(help = "Response body (JSON only)")]
    #[arg(value_parser = parse_json)]
    #[arg(conflicts_with = "template_path")]
    pub body: Option<String>,
    
    #[arg(short='T')]
    #[arg(long)]
    #[arg(help = "Path to a response template")]
    #[arg(conflicts_with = "body")]
    pub template_path: Option<PathBuf>,
}

fn parse_json(s: &str) -> Result<String, String> {
    serde_json::from_str::<serde_json::Value>(s)
        .map(|_| s.to_string())
        .map_err(|e| format!("invalid JSON: {e}"))
}

pub const CLAP_STYLING: clap::builder::styling::Styles = clap::builder::styling::Styles::styled()
    .header(clap_cargo::style::HEADER)
    .usage(clap_cargo::style::USAGE)
    .literal(clap_cargo::style::LITERAL)
    .placeholder(clap_cargo::style::PLACEHOLDER)
    .error(clap_cargo::style::ERROR)
    .valid(clap_cargo::style::VALID)
    .invalid(clap_cargo::style::INVALID);