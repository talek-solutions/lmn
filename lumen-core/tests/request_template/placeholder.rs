use std::path::Path;
use lumen_core::request_template::{parse_placeholder, Template};

#[test]
fn parses_simple_placeholder() {
    let ph = parse_placeholder("{{name}}").unwrap();
    assert_eq!(ph.name, "name");
    assert!(!ph.once);
}

#[test]
fn parses_once_placeholder() {
    let ph = parse_placeholder("{{name:once}}").unwrap();
    assert_eq!(ph.name, "name");
    assert!(ph.once);
}

#[test]
fn returns_none_for_plain_string() {
    assert!(parse_placeholder("plain text").is_none());
}

#[test]
fn returns_none_for_empty_braces() {
    assert!(parse_placeholder("{{}}").is_none());
}

#[test]
fn template_parse_succeeds_with_fixture() {
    let path = Path::new(".templates.example/json/placeholder.json");
    assert!(Template::parse(path).is_ok());
}

#[test]
fn pre_generate_returns_correct_count() {
    let path = Path::new(".templates.example/json/placeholder.json");
    let template = Template::parse(path).unwrap();
    assert_eq!(template.pre_generate(5).len(), 5);
}

#[test]
fn pre_generate_produces_valid_json() {
    let path = Path::new(".templates.example/json/placeholder.json");
    let template = Template::parse(path).unwrap();
    for body in template.pre_generate(3) {
        assert!(serde_json::from_str::<serde_json::Value>(&body).is_ok());
    }
}
