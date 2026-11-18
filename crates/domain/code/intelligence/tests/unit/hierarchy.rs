use super::*;
use crate::{FileSymbols, Visibility};
use std::sync::Arc;

fn ty(fqn: &str, kind: SymbolKind, parents: &[&str]) -> Symbol {
    Symbol {
        name: fqn.rsplit('\\').next().unwrap().into(),
        kind,
        fqn: fqn.into(),
        container: None,
        parents: parents.iter().map(|p| Arc::from(*p)).collect(),
        range: 0..100,
        offset: 10,
        line: 1,
        detail: None,
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

fn method(name: &str, class: &str, detail: &str) -> Symbol {
    Symbol {
        container: Some(class.into()),
        detail: Some(detail.into()),
        fqn: name.into(),
        offset: 40,
        line: 3,
        ..ty(name, SymbolKind::Method, &[])
    }
}

fn index() -> SymbolIndex {
    let mut index = SymbolIndex::default();
    let mut add = |path: &str, symbols: Vec<Symbol>| {
        let file = FileSymbols { language: "php".into(), symbols, ..FileSymbols::default() };
        index.set_file(Path::new(path), file);
    };
    add(
        "/p/Shape.php",
        vec![
            ty("App\\Shape", SymbolKind::Interface, &["Countable"]),
            method("area", "App\\Shape", "(): float"),
        ],
    );
    add(
        "/p/Base.php",
        vec![
            ty("App\\Base", SymbolKind::Class, &["App\\Shape"]),
            method("area", "App\\Base", "(): float"),
        ],
    );
    add(
        "/p/Circle.php",
        vec![
            ty("App\\Circle", SymbolKind::Class, &["App\\Base"]),
            method("Area", "App\\Circle", "(): float"),
        ],
    );
    add("/p/Square.php", vec![ty("App\\Square", SymbolKind::Class, &["App\\Shape"])]);
    add("/p/Solid.php", vec![ty("App\\Solid", SymbolKind::Interface, &["App\\Shape"])]);
    add("/p/vendor/x/Ext.php", vec![ty("X\\Ext", SymbolKind::Class, &["App\\Shape"])]);
    index
}

const PHP: Naming =
    Naming { methods_ignore_case: true, namespace_separator: Some('\\'), ..Naming::PLAIN };

#[test]
fn implementations_go_down_the_chain_without_interfaces_vendor_last() {
    let index = index();
    let subtypes = Subtypes::of(&index);
    let found: Vec<String> =
        subtypes.implementations("app\\shape").into_iter().map(|p| p.fqn).collect();
    assert_eq!(found, ["App\\Base", "App\\Square", "X\\Ext", "App\\Circle"]);
    assert!(subtypes.implementations("App\\Circle").is_empty());
    assert_eq!(subtypes.direct("App\\Base").len(), 1);
}

#[test]
fn overrides_are_the_subtypes_own_methods() {
    let index = index();
    let subtypes = Subtypes::of(&index);
    let found = subtypes.overrides(&index, "App\\Shape", "area", &PHP);
    let containers: Vec<_> = found.iter().map(|p| p.container.clone().unwrap()).collect();
    assert_eq!(containers, ["App\\Base", "App\\Circle"], "Circle's `Area` too (PHP ignores case)");
    let strict = subtypes.overrides(&index, "App\\Shape", "area", &Naming::PLAIN);
    assert_eq!(strict.len(), 1);
}

#[test]
fn the_hierarchy_has_supertypes_and_subtypes() {
    let index = index();
    let h = type_hierarchy(&index, "App\\Base", &PHP).unwrap();
    assert_eq!(h.focus.fqn, "App\\Base");
    assert_eq!(h.supertypes.len(), 1);
    assert_eq!(h.supertypes[0].place.fqn, "App\\Shape");
    // Up the chain: a type the index doesn't know, by its name.
    let countable = &h.supertypes[0].children[0].place;
    assert_eq!((countable.name.as_str(), countable.path.is_none()), ("Countable", true));
    assert_eq!(h.subtypes.len(), 1);
    assert_eq!(h.subtypes[0].place.fqn, "App\\Circle");
    let rows: Vec<(i8, usize, &str)> =
        h.rows().into_iter().map(|(s, d, p)| (s, d, p.name.as_str())).collect();
    assert_eq!(rows, [(-1, 1, "Shape"), (-1, 2, "Countable"), (0, 0, "Base"), (1, 1, "Circle")]);
    assert!(type_hierarchy(&index, "App\\Nope", &PHP).is_none());
}

#[test]
fn a_declarations_type() {
    let property = Symbol {
        kind: SymbolKind::Property,
        detail: Some("?Cart".into()),
        ..ty("cart", SymbolKind::Property, &[])
    };
    assert_eq!(declared_type(&property), Some("?Cart"));
    let m = method("items", "App\\Shop", "(int $a = (1), array $b): ?Cart|null");
    assert_eq!(declared_type(&m), Some("?Cart|null"));
    assert_eq!(declared_type(&method("x", "A", "(int $a)")), None, "nothing returned");
    let arrow = method("f", "A", "(a) => Shape");
    assert_eq!(declared_type(&arrow), Some("Shape"));
    assert_eq!(class_in_type("?Cart|null"), Some("Cart"));
    assert_eq!(class_in_type("int|string"), None);
    assert_eq!(class_in_type("array<\\App\\User>"), Some("\\App\\User"));
    assert_eq!(class_in_type("static"), Some("static"));
}

#[test]
fn the_declaration_at_its_name() {
    let file = FileSymbols {
        symbols: vec![ty("App\\Base", SymbolKind::Class, &[])],
        ..FileSymbols::default()
    };
    assert!(declaration_at(&file, 12, None).is_some());
    assert!(declaration_at(&file, 30, None).is_none());
}
