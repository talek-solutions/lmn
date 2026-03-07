use clap::Parser;
use loadtest::cli::command::LoadTestRunCli;
use loadtest::command::run::RunCommand;
use loadtest::command::{Command, Commands};

fn main() {
    let LoadTestRunCli::Run(args) = LoadTestRunCli::parse();
    if let Err(e) = Commands::Run(RunCommand::from(args)).execute() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
