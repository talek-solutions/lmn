use crate::command::run::{BodyFormat, RunStats};

pub mod configure_template;
pub mod method;
pub mod run;

pub use method::HttpMethod;

pub use configure_template::ConfigureTemplateCommand;

pub trait Command {
    fn execute(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>>;
}

pub enum Body {
    Formatted { content: String, format: BodyFormat },
}

impl Into<String> for Body {
    fn into(self) -> String {
        match self {
            Body::Formatted { content, format: _ } => content,
        }
    }
}

pub enum Commands {
    Run(run::RunCommand),
    ConfigureRequest(ConfigureTemplateCommand),
    ConfigureResponse(ConfigureTemplateCommand),
}

impl Command for Commands {
    fn execute(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
        match self {
            Commands::Run(cmd) => cmd.execute(),
            Commands::ConfigureRequest(cmd) => cmd.execute(),
            Commands::ConfigureResponse(cmd) => cmd.execute(),
        }
    }
}
