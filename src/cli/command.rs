use std::path::PathBuf;

use clap::Parser;

#[derive(clap::ValueEnum, Clone, Copy, Debug)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

#[derive(Parser)]
#[command(name = "loadtest")]
#[command(bin_name = "loadtest")]
#[command(styles = CLAP_STYLING)]
pub enum LoadTestRunCli {
    Run(RunArgs),
}

#[derive(clap::Args)]
#[command(version, about)]
#[command(name = "run")]
#[command(long_about = "Run the test")]
pub struct RunArgs {
    #[arg(short='H')]
    #[arg(long)]
    #[arg(help = "The host to run loadtest against")]
    pub host: String,

    #[arg(short='R')]
    #[arg(long)]
    #[arg(help = "The count of API requests to run")]
    #[arg(allow_negative_numbers = false)]
    #[arg(default_value="100")]
    pub request_count: u32,

    #[arg(short='T')]
    #[arg(long)]
    #[arg(help = "The count of CPU thread requests to run")]
    #[arg(allow_negative_numbers = false)]
    #[arg(default_value="1")]
    pub threads: u16,

    #[arg(short='C')]
    #[arg(long)]
    #[arg(help = "Max number of requests in-flight at any one time")]
    #[arg(allow_negative_numbers = false)]
    #[arg(default_value="100")]
    pub concurrency: u32,

    #[arg(short='M')]
    #[arg(long)]
    #[arg(help = "HTTP method to use")]
    #[arg(default_value="get")]
    pub method: HttpMethod,

    #[arg(short='B')]
    #[arg(long)]
    #[arg(help = "Request body (JSON only)")]
    #[arg(value_parser = parse_json)]
    #[arg(conflicts_with = "template")]
    pub body: Option<String>,
    
    #[arg(long)]
    #[arg(help = "Path to a JSON template file with placeholder definitions")]
    #[arg(conflicts_with = "body")]
    pub template: Option<PathBuf>,
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