use super::*;
use crate::FileSymbols;
use std::path::Path;
use std::sync::Arc;

fn ty(fqn: &str, kind: SymbolKind, parents: &[&str]) -> Symbol {
    Symbol {
        name: fqn.rsplit('\\').next().unwrap().into(),
        kind,
        fqn: fqn.into(),
        container: None,
        parents: parents.iter().map(|p| Arc::from(*p)).collect(),
        range: 0..0,
        offset: 0,
        line: 1,
        detail: None,
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

fn member(name: &str, kind: SymbolKind, class: &str, detail: Option<&str>) -> Symbol {
    Symbol {
        container: Some(class.into()),
        detail: detail.map(Into::into),
        name: name.into(),
        fqn: name.into(),
        ..ty(name, kind, &[])
    }
}

fn file(namespace: &str, symbols: Vec<Symbol>) -> FileSymbols {
    FileSymbols {
        language: "php".into(),
        namespace: Some(namespace.into()),
        uses: vec![crate::UseImport { alias: "Clock".into(), fqn: "Psr\\Clock\\Clock".into() }],
        symbols,
    }
}

fn index() -> SymbolIndex {
    let mut index = SymbolIndex::default();
    index.set_file(
        Path::new("/p/src/Repo.php"),
        file(
            "App",
            vec![
                ty("App\\Repo", SymbolKind::Class, &["App\\Base", "App\\Finds", "App\\Logs"]),
                member(
                    "__construct",
                    SymbolKind::Method,
                    "App\\Repo",
                    Some("(#[Inject(a: [1, 2])] private readonly Db $db, ?Clock $clock = null, int $n, A|B $x)"),
                ),
                member("find", SymbolKind::Method, "App\\Repo", Some("(int $id)")),
                member("table", SymbolKind::Property, "App\\Repo", Some("string")),
            ],
        ),
    );
    index.set_file(
        Path::new("/p/src/Base.php"),
        file("App", vec![ty("App\\Base", SymbolKind::Class, &["App\\Model"])]),
    );
    index.set_file(
        Path::new("/p/src/Finds.php"),
        file("App", vec![ty("App\\Finds", SymbolKind::Interface, &["App\\Countable"])]),
    );
    index.set_file(
        Path::new("/p/src/Logs.php"),
        file("App", vec![ty("App\\Logs", SymbolKind::Trait, &[])]),
    );
    index.set_file(
        Path::new("/p/src/UserRepo.php"),
        file("App", vec![ty("App\\UserRepo", SymbolKind::Class, &["App\\Repo"])]),
    );
    index
}

fn edge(d: &ClassDiagram, from: &str, to: &str) -> Option<Relation> {
    let at = |fqn: &str| d.nodes.iter().position(|n| n.fqn == fqn).unwrap();
    let (from, to) = (at(from), at(to));
    d.edges.iter().find(|e| e.from == from && e.to == to).map(|e| e.relation)
}

#[test]
fn a_class_with_its_ancestors_children_and_dependencies() {
    let d = class_diagram(&index(), "app\\repo", &php, &constructor_types).unwrap();
    let focus = d.focus().unwrap();
    assert_eq!((focus.fqn.as_str(), focus.role), ("App\\Repo", DiagramRole::Focus));
    // Properties before methods.
    let members: Vec<_> = focus.members.iter().map(|m| m.name.as_str()).collect();
    assert_eq!(members, ["table", "__construct", "find"]);

    assert_eq!(edge(&d, "App\\Repo", "App\\Base"), Some(Relation::Extends));
    assert_eq!(edge(&d, "App\\Repo", "App\\Finds"), Some(Relation::Implements));
    assert_eq!(edge(&d, "App\\Repo", "App\\Logs"), Some(Relation::UsesTrait));
    // Two levels up; an interface extends interfaces; unknown types are still nodes.
    assert_eq!(edge(&d, "App\\Base", "App\\Model"), Some(Relation::Extends));
    assert_eq!(edge(&d, "App\\Finds", "App\\Countable"), Some(Relation::Extends));
    let model = d.nodes.iter().find(|n| n.fqn == "App\\Model").unwrap();
    assert_eq!((model.role, model.path.as_ref()), (DiagramRole::Ancestor(2), None));

    assert_eq!(edge(&d, "App\\UserRepo", "App\\Repo"), Some(Relation::Extends));
    for dep in ["App\\Db", "Psr\\Clock\\Clock", "App\\A", "App\\B"] {
        assert_eq!(edge(&d, "App\\Repo", dep), Some(Relation::Depends), "{dep}");
    }
    assert!(!d.nodes.iter().any(|n| n.name == "int" || n.name == "readonly"));
}

#[test]
fn unknown_types_have_no_diagram() {
    assert!(class_diagram(&index(), "App\\Nope", &php, &constructor_types).is_none());
}

/// PHP's constructor (method names ignore case) and namespaces.
fn php(_: &str) -> crate::Naming {
    crate::Naming {
        constructor: Some("__construct"),
        methods_ignore_case: true,
        names_ignore_case: true,
        namespace_separator: Some('\\'),
        ..crate::Naming::default()
    }
}

/// The types the language reads in `Repo`'s constructor (its parsing is the language's).
fn constructor_types(_: &str, detail: &str) -> Vec<String> {
    assert!(detail.contains("Db $db"), "the constructor's signature is given");
    ["Db", "Clock", "A", "B"].map(String::from).to_vec()
}
