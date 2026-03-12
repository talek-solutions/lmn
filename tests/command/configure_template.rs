use std::path::PathBuf;
use loadtest::cli::command::ConfigureRequestArgs;
use loadtest::command::{Command, ConfigureTemplateCommand};

struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[test]
fn execute_creates_file_from_body() {
    let alias = "__test_exec_body";
    let path = PathBuf::from(".templates/requests").join(format!("{}.json", alias));
    let _ = std::fs::remove_file(&path);
    let _cleanup = Cleanup(path.clone());

    let cmd = ConfigureTemplateCommand::from(ConfigureRequestArgs {
        alias: alias.to_string(),
        body: Some(r#"{"ok":true}"#.to_string()),
        template_path: None,
    });

    assert!(cmd.execute().is_ok());
    assert!(path.exists());
}

#[test]
fn execute_returns_error_when_no_body_or_path() {
    let cmd = ConfigureTemplateCommand::from(ConfigureRequestArgs {
        alias: "irrelevant".to_string(),
        body: None,
        template_path: None,
    });

    assert!(cmd.execute().is_err());
}

#[test]
fn execute_returns_error_on_duplicate_alias() {
    let alias = "__test_exec_duplicate";
    let path = PathBuf::from(".templates/requests").join(format!("{}.json", alias));
    let _ = std::fs::remove_file(&path);
    let _cleanup = Cleanup(path.clone());

    let make_cmd = || ConfigureTemplateCommand::from(ConfigureRequestArgs {
        alias: alias.to_string(),
        body: Some("{}".to_string()),
        template_path: None,
    });

    assert!(make_cmd().execute().is_ok());
    assert!(make_cmd().execute().is_err());
}
