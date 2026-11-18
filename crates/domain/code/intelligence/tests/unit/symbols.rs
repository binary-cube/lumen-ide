use super::*;

fn file() -> FileSymbols {
    FileSymbols {
        language: "php".into(),
        namespace: Some("App\\Http".into()),
        uses: vec![
            UseImport { alias: "User".into(), fqn: "App\\Models\\User".into() },
            UseImport { alias: "Rules".into(), fqn: "Illuminate\\Validation\\Rule".into() },
        ],
        symbols: vec![Symbol {
            name: "Controller".into(),
            kind: SymbolKind::Class,
            fqn: "App\\Http\\Controller".into(),
            container: None,
            parents: Box::new(["App\\Http\\Base".into()]),
            range: 100..500,
            offset: 110,
            line: 5,
            detail: None,
            is_static: false,
            visibility: Default::default(),
            note: None,
            types: None,
        }],
    }
}

/// PHP's naming: `\\` separates namespaces, `self` / `static` / `parent`, any case.
const PHP: crate::Naming = crate::Naming {
    receiver: Some("$this"),
    names_ignore_case: true,
    namespace_separator: Some('\\'),
    own_type: &["self", "static"],
    parent_type: Some("parent"),
    type_name_member: Some("class"),
    sigil: Some('$'),
    implicit_locals: true,
    constructor: Some("__construct"),
    methods_ignore_case: true,
    type_per_file: Some("php"),
    ..crate::Naming::PLAIN
};

#[test]
fn php_name_resolution() {
    let f = file();
    let resolve = |name: &str, at: usize| f.resolve_class(name, at, &PHP);
    assert_eq!(resolve("User", 0), "App\\Models\\User");
    assert_eq!(resolve("\\DateTime", 0), "DateTime");
    assert_eq!(resolve("Rules\\In", 0), "Illuminate\\Validation\\Rule\\In");
    assert_eq!(resolve("Request", 0), "App\\Http\\Request");
    assert_eq!(resolve("self", 200), "App\\Http\\Controller");
    assert_eq!(resolve("Static", 200), "App\\Http\\Controller");
    assert_eq!(resolve("parent", 200), "App\\Http\\Base");
}

/// Without a separator or the type's own names (a language like JavaScript): an import's
/// alias, else the name as written; aliases match by case.
#[test]
fn plain_name_resolution() {
    let f = file();
    let plain = crate::Naming::default();
    assert_eq!(f.resolve_class("User", 0, &plain), "App\\Models\\User");
    assert_eq!(f.resolve_class("user", 0, &plain), "user");
    assert_eq!(f.resolve_class("self", 200, &plain), "self");
    assert_eq!(f.resolve_class("Widget", 0, &plain), "Widget");
}

/// A local variable's scope: the innermost function around it, ends included.
#[test]
fn the_function_around_an_offset() {
    let function = |name: &str, kind, range: std::ops::Range<usize>| Symbol {
        name: name.into(),
        kind,
        fqn: name.into(),
        container: None,
        parents: Box::default(),
        range,
        offset: 0,
        line: 0,
        detail: None,
        is_static: false,
        visibility: Default::default(),
        note: None,
        types: None,
    };
    let f = FileSymbols {
        symbols: vec![
            function("A", SymbolKind::Class, 0..100),
            function("outer", SymbolKind::Function, 10..90),
            function("inner", SymbolKind::Method, 20..40),
        ],
        ..FileSymbols::default()
    };
    assert_eq!(f.function_at(30).map(|s| &*s.name), Some("inner"));
    assert_eq!(f.function_at(40).map(|s| &*s.name), Some("inner"));
    assert_eq!(f.function_at(50).map(|s| &*s.name), Some("outer"));
    assert!(f.function_at(95).is_none(), "a class isn't a function");
}
