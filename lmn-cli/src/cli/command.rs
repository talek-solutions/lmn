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
#[command(name = "lmn")]
#[command(bin_name = "lmn")]
#[command(styles = CLAP_STYLING)]
pub enum LoadTestRunCli {
    Run(Box<RunArgs>),
    ConfigureRequest(ConfigureRequestArgs),
    ConfigureResponse(ConfigureResponseArgs),
}

#[derive(clap::Args)]
#[command(version, about)]
#[command(name = "run")]
#[command(long_about = "Run the test")]
pub struct RunArgs {
    #[arg(short = 'H')]
    #[arg(long)]
    #[arg(help = "The host to run lumen against (required unless set via run.host in --config)")]
    pub host: Option<String>,

    #[arg(short = 'R')]
    #[arg(long)]
    #[arg(help = "The count of API requests to run (default: 100)")]
    #[arg(allow_negative_numbers = false)]
    pub request_count: Option<u32>,

    #[arg(short = 'C')]
    #[arg(long)]
    #[arg(help = "Max number of requests in-flight at any one time (default: 100)")]
    #[arg(allow_negative_numbers = false)]
    pub concurrency: Option<u32>,

    #[arg(short = 'M')]
    #[arg(long)]
    #[arg(help = "HTTP method to use (default: get)")]
    pub method: Option<HttpMethod>,

    #[arg(short = 'B')]
    #[arg(long)]
    #[arg(help = "Request body (JSON only)")]
    #[arg(value_parser = parse_json)]
    #[arg(conflicts_with_all = ["request_template", "request_alias"])]
    pub body: Option<String>,

    #[arg(short = 'T')]
    #[arg(long = "request-template")]
    #[arg(help = "Path to a JSON template file with placeholder definitions")]
    #[arg(conflicts_with_all = ["body", "request_alias"])]
    pub request_template: Option<PathBuf>,

    #[arg(short = 'A')]
    #[arg(long = "request-alias")]
    #[arg(help = "Alias of a stored request template (name or name.json)")]
    #[arg(conflicts_with_all = ["body", "request_template"])]
    pub request_alias: Option<String>,

    #[arg(short = 'S')]
    #[arg(long = "response-template")]
    #[arg(help = "Path to a JSON response template for tracking response fields")]
    #[arg(conflicts_with = "response_alias")]
    pub response_template: Option<PathBuf>,

    #[arg(short = 'E')]
    #[arg(long = "response-alias")]
    #[arg(help = "Alias of a stored response template (name or name.json)")]
    #[arg(conflicts_with = "response_template")]
    pub response_alias: Option<String>,

    #[arg(short = 'L')]
    #[arg(long = "load-curve")]
    #[arg(help = "Path to a load curve JSON file (conflicts with -R and -C)")]
    #[arg(conflicts_with_all = ["request_count", "concurrency"])]
    pub load_curve: Option<std::path::PathBuf>,

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

    #[arg(long = "header")]
    #[arg(
        help = "Custom HTTP header in 'Name: Value' format (repeatable, e.g. --header 'Authorization: Bearer token')"
    )]
    #[arg(value_parser = parse_header)]
    pub headers: Vec<String>,
}

#[derive(clap::Args)]
#[command(version, about)]
#[command(name = "configure-request")]
#[command(long_about = "Configure a persistent request body template")]
pub struct ConfigureRequestArgs {
    #[arg(short = 'A')]
    #[arg(long)]
    #[arg(help = "Template alias")]
    #[arg(required = true)]
    pub alias: String,

    #[arg(short = 'B')]
    #[arg(long)]
    #[arg(help = "Request body (JSON only)")]
    #[arg(value_parser = parse_json)]
    #[arg(conflicts_with = "template_path")]
    #[arg(requires = "alias")]
    pub body: Option<String>,

    #[arg(short = 'T')]
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
    #[arg(short = 'A')]
    #[arg(long)]
    #[arg(help = "Template alias")]
    #[arg(required = true)]
    pub alias: String,

    #[arg(short = 'B')]
    #[arg(long)]
    #[arg(help = "Response body (JSON only)")]
    #[arg(value_parser = parse_json)]
    #[arg(conflicts_with = "template_path")]
    pub body: Option<String>,

    #[arg(short = 'T')]
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

fn parse_header(s: &str) -> Result<String, String> {
    match s.find(": ") {
        None => Err(format!(
            "invalid header format '{s}': expected 'Name: Value'"
        )),
        Some(0) => Err(format!(
            "invalid header format '{s}': header name cannot be empty"
        )),
        Some(pos) => {
            let name = &s[..pos];
            // Header field names: printable ASCII, no spaces or separators (RFC 7230)
            if name
                .chars()
                .any(|c| !c.is_ascii_graphic() || "()<>@,;:\\\"/[]?={} \t".contains(c))
            {
                Err(format!(
                    "invalid header name '{name}': contains disallowed characters"
                ))
            } else {
                Ok(s.to_string())
            }
        }
    }
}

pub const CLAP_STYLING: clap::builder::styling::Styles = clap::builder::styling::Styles::styled()
    .header(clap_cargo::style::HEADER)
    .usage(clap_cargo::style::USAGE)
    .literal(clap_cargo::style::LITERAL)
    .placeholder(clap_cargo::style::PLACEHOLDER)
    .error(clap_cargo::style::ERROR)
    .valid(clap_cargo::style::VALID)
    .invalid(clap_cargo::style::INVALID);

#[cfg(test)]
mod tests {
    use super::parse_header;

    #[test]
    fn parse_header_rejects_empty_name() {
        let result = parse_header(": Value");
        assert!(result.is_err());
        let msg = result.err().unwrap();
        assert!(
            msg.contains("header name cannot be empty"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn parse_header_rejects_missing_separator() {
        let result = parse_header("Authorization:Bearer token");
        assert!(result.is_err());
        let msg = result.err().unwrap();
        assert!(
            msg.contains("expected 'Name: Value'"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn parse_header_rejects_space_in_name() {
        let result = parse_header("My Header: value");
        assert!(result.is_err());
        let msg = result.err().unwrap();
        assert!(
            msg.contains("disallowed characters"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn parse_header_accepts_valid() {
        let result = parse_header("Authorization: Bearer token123");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Authorization: Bearer token123");
    }

    #[test]
    fn parse_header_accepts_hyphenated_name() {
        let result = parse_header("X-Api-Key: secret");
        assert!(result.is_ok());
    }
}
