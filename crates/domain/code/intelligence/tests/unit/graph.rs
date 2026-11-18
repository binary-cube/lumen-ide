use super::*;
use crate::symbols::Symbol;

fn sym(
    name: &str,
    kind: SymbolKind,
    fqn: &str,
    range: std::ops::Range<usize>,
    container: Option<&str>,
) -> Symbol {
    Symbol {
        name: name.into(),
        kind,
        fqn: fqn.into(),
        container: container.map(Into::into),
        parents: Box::default(),
        offset: range.start,
        range,
        line: 0,
        detail: None,
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

/// PHP's rules, as its module gives them.
const PHP: SourceRules = SourceRules {
    extensions: &["php"],
    includes: &["require", "include"],
    include_suffixes: &[],
    before_type: &["new", "extends", "implements", "instanceof"],
    static_access: Some("::"),
    member_access: &["->", "::"],
    declares_function: &["function"],
    declares_type: &["class", "interface", "trait", "enum"],
    modifiers: &["public", "private", "protected", "static", "function"],
    naming: Naming {
        sigil: Some('$'),
        namespace_separator: Some('\\'),
        names_ignore_case: true,
        methods_ignore_case: true,
        ..Naming::PLAIN
    },
    comments: None,
};

#[test]
fn neighbours_in_both_directions() {
    let class = "<?php\nclass Car {\n  public function stop() {}\n}\n";
    let index_php = "<?php\nrequire 'class.php';\n$car = new Car();\n$car->stop();\n";
    let root = Path::new("/p");
    let mut index = SymbolIndex::default();
    let start = class.find("class").unwrap();
    let stop = class.find("public").unwrap();
    index.set_file(
        &root.join("class.php"),
        FileSymbols {
            language: "php".into(),
            symbols: vec![
                sym("Car", SymbolKind::Class, "Car", start..class.len() - 1, None),
                sym("stop", SymbolKind::Method, "stop", stop..stop + 24, Some("Car")),
            ],
            ..FileSymbols::default()
        },
    );
    // By name: the root joined with a file is written `\\` on Windows.
    let read = |p: &Path| match p.file_name()?.to_str()? {
        "class.php" => Some(class.to_owned()),
        "index.php" => Some(index_php.to_owned()),
        _ => None,
    };
    let project = Project {
        root: Some(root),
        read: &read,
        list: &|_| Vec::new(),
        modified: &|_| None,
        levels: &[],
        languages: &[],
    };
    let files = vec!["class.php".to_owned(), "index.php".to_owned()];
    let focus = root.join("class.php");
    let graph = build_graph(&index, &project, &files, &focus, &|_| "php".into(), &[PHP]).unwrap();
    assert_eq!(graph.files.len(), 2);
    assert_eq!(graph.files[0].symbols[1].signature, "stop()");
    let kinds: Vec<EdgeKind> = graph.edges.iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&EdgeKind::Include) && kinds.contains(&EdgeKind::Call), "{kinds:?}");
    let call = graph.edges.iter().find(|e| e.kind == EdgeKind::Call).unwrap();
    assert_eq!((call.from, call.to, call.symbol, call.line), (1, 0, Some(1), 3));
}

/// A file whose language gives no rules shows only its declarations: nothing followed.
#[test]
fn a_file_without_rules_has_no_links() {
    let root = Path::new("/p");
    let text = "require 'class.php';\nnew Car();\n";
    let read = |p: &Path| (p == Path::new("/p/notes.txt")).then(|| text.to_owned());
    let project = Project {
        root: Some(root),
        read: &read,
        list: &|_| Vec::new(),
        modified: &|_| None,
        levels: &[],
        languages: &[],
    };
    let files = vec!["notes.txt".to_owned()];
    let index = SymbolIndex::default();
    let focus = root.join("notes.txt");
    let graph = build_graph(&index, &project, &files, &focus, &|_| String::new(), &[PHP]).unwrap();
    assert_eq!(graph.files.len(), 1);
    assert!(graph.edges.is_empty());
}
