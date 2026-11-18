use super::*;

#[test]
fn markers_and_names() {
    assert_eq!(comment_marker("// language=JSON"), Some("json".into()));
    assert_eq!(comment_marker("/* language=yaml */"), Some("yaml".into()));
    assert_eq!(comment_marker("// nimic"), None);
    assert_eq!(normalize_language("JS").as_str(), "javascript");
    assert_eq!(normalize_language("HTML").as_str(), "html");
    for label in ["SQL", "PGSQL", "MySQL", "SQLITE", "postgres"] {
        assert_eq!(normalize_language(label).as_str(), "sql", "{label}");
    }
}

/// A Markdown code block's short names reach their languages.
#[test]
fn a_code_block_s_short_names_reach_their_languages() {
    for (label, language) in [
        ("sh", "bash"),
        ("zsh", "bash"),
        ("ts", "typescript"),
        ("md", "markdown"),
        ("jsonc", "json"),
    ] {
        assert_eq!(normalize_language(label).as_str(), language, "{label}");
    }
}
