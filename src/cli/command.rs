use clap::Parser;

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
    #[arg(short='h')]
    #[arg(long)]
    #[arg(help = "The host to run loadtest against")]
    pub host: String,

    #[arg(short='r')]
    #[arg(long)]
    #[arg(help = "The count of API requests to run")]
    #[arg(allow_negative_numbers = false)]
    #[arg(default_value="100")]
    pub request_count: u32,

    #[arg(short='t')]
    #[arg(long)]
    #[arg(help = "The count of CPU thread requests to run")]
    #[arg(allow_negative_numbers = false)]
    #[arg(default_value="1")]
    pub threads: u16,
}

pub const CLAP_STYLING: clap::builder::styling::Styles = clap::builder::styling::Styles::styled()
    .header(clap_cargo::style::HEADER)
    .usage(clap_cargo::style::USAGE)
    .literal(clap_cargo::style::LITERAL)
    .placeholder(clap_cargo::style::PLACEHOLDER)
    .error(clap_cargo::style::ERROR)
    .valid(clap_cargo::style::VALID)
    .invalid(clap_cargo::style::INVALID);