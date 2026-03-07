use clap::Parser;
use loadtest::cli::command::LoadTestRunCli;
use loadtest::command::{Command, Commands};
use loadtest::command::run::RunCommand;

fn main() {
    let LoadTestRunCli::Run(args) = LoadTestRunCli::parse();
    Commands::Run(RunCommand::from(args)).execute();
}
