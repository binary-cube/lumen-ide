//! A file's requests and variables, as Structure shows them.

use super::*;

#[test]
fn requests_by_name_else_method_and_url_and_the_variables_without_values() {
    let text = "@host = h\n@token = s3cret\n\n### Log in\n# @name login\nPOST https://{{host}}/login\n\n###\nGET https://{{host}}/users\n";
    let symbols = symbols(text).symbols;
    let names: Vec<(&str, SymbolKind)> = symbols.iter().map(|s| (&*s.name, s.kind)).collect();
    assert_eq!(
        names,
        [
            ("host", SymbolKind::Variable),
            ("token", SymbolKind::Variable),
            ("login", SymbolKind::Function),
            ("GET https://{{host}}/users", SymbolKind::Function),
        ]
    );
    assert!(symbols.iter().all(|s| s.detail.as_deref() != Some("s3cret")), "no value shown");
    assert_eq!(symbols[2].detail.as_deref(), Some("POST https://{{host}}/login"));
    assert_eq!(symbols[2].line, 5);
    assert!(symbols[2].range.contains(&text.find("POST").unwrap()));
}
