use clap::Parser;
use loadtest::cli::command::LoadTestRunCli;
use loadtest::command::run::RunCommand;
use loadtest::command::{Command, Commands, ConfigureTemplateCommand};

fn main() {
    let cmd = match LoadTestRunCli::parse() {
        LoadTestRunCli::Run(args) => Commands::Run(RunCommand::from(args)),
        LoadTestRunCli::ConfigureRequest(args) => Commands::ConfigureRequest(ConfigureTemplateCommand::from(args)),
        LoadTestRunCli::ConfigureResponse(args) => Commands::ConfigureResponse(ConfigureTemplateCommand::from(args)),
    };

    if let Err(e) = cmd.execute() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
