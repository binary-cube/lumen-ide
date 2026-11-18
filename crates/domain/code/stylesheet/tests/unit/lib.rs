use super::*;

#[test]
fn selectors_in_a_style_block() {
    let html = "<style>\n/* .no */\n.abc, #main > .card:not(.x) { color: #fff; }\n@media (max-width: 9px) { .mob { a: b } }\n</style>";
    let start = html.find('\n').unwrap();
    let end = html.find("</style>").unwrap();
    let mut file = FileSymbols::default();
    add_css_selectors(html, start..end, &mut file);
    let names: Vec<_> = file.symbols.iter().map(|s| (s.name.as_ref(), s.kind)).collect();
    assert_eq!(
        names,
        [
            ("abc", SymbolKind::CssClass),
            ("main", SymbolKind::CssId),
            ("card", SymbolKind::CssClass),
            ("mob", SymbolKind::CssClass)
        ]
    );
    assert_eq!(&html[file.symbols[0].offset..file.symbols[0].offset + 3], "abc");
    assert_eq!(file.symbols[0].line, 2);
    let mut page = FileSymbols::default();
    add_style_blocks("<p>.no { }</p><STYLE type=\"text/css\">.yes{}</STYLE>", &mut page);
    let names: Vec<_> = page.symbols.iter().map(|s| s.name.as_ref()).collect();
    assert_eq!(names, ["yes"]);
}

#[test]
fn css_rules() {
    let html = "<style>\n.abc,\n.def {\n  background: red;\n  color: #fff;\n}\n</style>";
    let at = html.find("abc").unwrap();
    assert_eq!(
        rule_at(html, at).unwrap(),
        ".abc, .def {\n    background: red;\n    color: #fff;\n}"
    );
}
