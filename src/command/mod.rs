pub mod run;

pub trait Command {
    fn execute(self) -> Result<(), Box<dyn std::error::Error>>;
}

pub enum Commands {
    Run(run::RunCommand),
}

impl Command for Commands {
    fn execute(self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Commands::Run(cmd) => cmd.execute(),
        }
    }
}
