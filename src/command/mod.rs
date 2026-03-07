pub mod run;

pub trait Command {
    fn execute(self);
}

pub enum Commands {
    Run(run::RunCommand),
}

impl Command for Commands {
    fn execute(self) {
        match self {
            Commands::Run(cmd) => cmd.execute(),
        }
    }
}
