use code_intelligence::servers::Engine;

use super::*;

#[test]
fn engine_entries_are_read_and_written() {
    assert_eq!(
        parse_engine("typescript=built-in"),
        Some(("typescript".into(), Choice { engine: Engine::BuiltIn, server: None }))
    );
    assert_eq!(
        parse_engine("tsx=server:mine"),
        Some(("tsx".into(), Choice { engine: Engine::Server, server: Some("mine".into()) }))
    );
    assert_eq!(
        parse_engine("js=both"),
        Some(("js".into(), Choice { engine: Engine::Both, server: None }))
    );
    for bad in ["typescript", "=server", "x=lsp", "x=built-in:mine", "x=server:"] {
        assert_eq!(parse_engine(bad), None, "{bad}");
    }
    for entry in ["typescript=built-in", "tsx=server:mine", "js=both:other"] {
        let (language, choice) = parse_engine(entry).unwrap();
        assert_eq!(engine_entry(&language, &choice), entry);
    }
}

#[test]
fn a_language_not_listed_has_its_server() {
    let config = Config {
        engines: vec![("css".into(), Choice { engine: Engine::BuiltIn, server: None })],
        ..Config::default()
    };
    assert_eq!(config.choice("typescript"), Choice::default());
    assert_eq!(config.choice("typescript").engine, Engine::Server);
    assert_eq!(config.choice("css").engine, Engine::BuiltIn);
}

#[test]
fn the_defaults() {
    assert_eq!(IDLE_MINUTES.default_value(), 2);
    assert_eq!(MAX_MEMORY.default_value(), 3072);
    assert!(!TRACE.default_value());
    assert!(ENGINES.default_value().is_empty());
    assert!(!IDLE_MINUTES.accepts(&7));
    assert!(!MAX_MEMORY.accepts(&100));
    assert!(!ENGINES.accepts(&vec!["x=nope".into()]));
}
