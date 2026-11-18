use super::*;
use crate::{CompletionContext, ContextKind, FileSymbols, Symbol, Target, Visibility};
use kernel::LanguageId;

/// A language whose calls are plain functions of the index.
#[derive(Debug)]
struct Plain;

impl LanguageIntelligence for Plain {
    fn language(&self) -> LanguageId {
        LanguageId::new("plain")
    }
    fn extensions(&self) -> &[&'static str] {
        &["p"]
    }
    fn extract(&self, _text: &str) -> FileSymbols {
        FileSymbols::default()
    }
    fn context(&self, _text: &str, offset: usize) -> CompletionContext {
        CompletionContext {
            replace: offset..offset,
            prefix: String::new(),
            kind: ContextKind::None,
        }
    }
    fn target(&self, text: &str, offset: usize) -> Option<Target> {
        let range = crate::word_at(text, offset, "")?;
        Some(Target::Function(text[range].to_owned()))
    }
    fn locals(&self, _text: &str, _offset: usize) -> Vec<String> {
        Vec::new()
    }
    fn keywords(&self) -> &[&'static str] {
        &[]
    }
}

fn function(name: &str, detail: &str) -> Symbol {
    // Each at its own offset (a declaration is found by it).
    let offset = name.len() * 10;
    Symbol {
        name: name.into(),
        kind: SymbolKind::Function,
        fqn: name.into(),
        container: None,
        parents: Box::default(),
        range: offset..offset + 1,
        offset,
        line: 0,
        detail: Some(detail.into()),
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

fn hints(text: &str) -> Vec<(String, String)> {
    let mut index = SymbolIndex::default();
    let symbols = vec![
        function("total", "(a, b)"),
        function("setTotal", "(total)"),
        function("log", "(level, ...parts)"),
    ];
    index.set_file(Path::new("/p/lib.p"), FileSymbols { symbols, ..FileSymbols::default() });
    let found = parameter_hints(
        &Plain,
        &index,
        Path::new("/p/main.p"),
        text,
        0..text.len(),
        &Project::none(),
    );
    found.into_iter().map(|h| (h.label, text[h.offset..].chars().take(3).collect())).collect()
}

#[test]
fn literal_arguments_get_their_parameters_name() {
    assert_eq!(hints("total(1, 'x')"), [("a:".into(), "1, ".into()), ("b:".into(), "'x'".into())]);
    // A variable says itself; a call too.
    assert_eq!(hints("total(x, other())"), Vec::<(String, String)>::new());
    assert_eq!(hints("total(-2, [1, 2])").len(), 2, "negative numbers and arrays");
    assert_eq!(hints("total(true, null)").len(), 2);
}

#[test]
fn what_already_says_its_parameter_gets_none() {
    assert!(hints("total(a: 1, b: 2)").is_empty(), "named arguments");
    assert!(hints("setTotal(5)").is_empty(), "the callee's name ends with it");
    assert!(hints("unknown(1)").is_empty(), "nothing known of the callee");
    assert!(hints("if (1) {}").is_empty(), "not a call");
    assert!(hints("function total(1, 2)").is_empty(), "a declaration");
    assert!(hints("// total(1, 2)\n'total(1, 2)'").is_empty(), "comments and strings");
}

#[test]
fn a_variadic_parameter_is_named_once() {
    let found = hints("log(1, 'a', 'b')");
    let labels: Vec<&str> = found.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels, ["level:", "...parts:"]);
}

#[test]
fn only_the_calls_in_the_range_count() {
    let text = "total(1, 2)\ntotal(3, 4)\n";
    let mut index = SymbolIndex::default();
    index.set_file(
        Path::new("/p/lib.p"),
        FileSymbols { symbols: vec![function("total", "(a, b)")], ..FileSymbols::default() },
    );
    let second = text.find("total(3").unwrap();
    let found = parameter_hints(
        &Plain,
        &index,
        Path::new("/p/m.p"),
        text,
        second..text.len(),
        &Project::none(),
    );
    assert_eq!(found.len(), 2);
    assert!(found.iter().all(|h| h.offset > second));
}
