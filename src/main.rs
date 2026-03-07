use clap::Parser;
use loadtest::cli::command::LoadTestRunCli;

fn main() {
    let LoadTestRunCli::Run(args) = LoadTestRunCli::parse();
    loadtest::process_run_command(args);
}