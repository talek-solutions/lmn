use crate::command::run::RunStats;
use crate::http::BodyFormat;

pub mod configure_template;
pub mod method;
pub mod run;

pub use method::HttpMethod;

pub use configure_template::ConfigureTemplateCommand;

#[allow(async_fn_in_trait)]
pub trait Command {
    async fn execute(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>>;
}

pub enum Body {
    Formatted { content: String, format: BodyFormat },
}

impl From<Body> for String {
    fn from(body: Body) -> String {
        match body {
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
    async fn execute(self) -> Result<Option<RunStats>, Box<dyn std::error::Error>> {
        match self {
            Commands::Run(cmd) => cmd.execute().await,
            Commands::ConfigureRequest(cmd) => cmd.execute().await,
            Commands::ConfigureResponse(cmd) => cmd.execute().await,
        }
    }
}
