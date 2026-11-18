use super::*;
use crate::{FileSymbols, Symbol, Visibility};

#[test]
fn names_at_the_cursor() {
    let text = "$total = $this->user_name + .card-body;";
    let php = Some('$');
    let at = |needle: &str, extra: &str| {
        let offset = text.find(needle).unwrap() + 1;
        name_at(text, offset, extra, php).map(|r| &text[r])
    };
    assert_eq!(at("total", ""), Some("$total"));
    assert_eq!(name_at(text, 0, "", php).map(|r| &text[r]), Some("$total"), "on the `$`");
    assert_eq!(at("user_name", ""), Some("user_name"));
    assert_eq!(at("card", ""), Some("card"));
    let css = SymbolKind::CssClass.name_chars();
    assert_eq!(at("card", css), Some("card-body"));
    // Without a sigil, `$` isn't part of a name.
    let offset = text.find("total").unwrap() + 1;
    assert_eq!(name_at(text, offset, "", None).map(|r| &text[r]), Some("total"));
}

fn symbol(name: &str, kind: SymbolKind, offset: usize, range: Range<usize>) -> Symbol {
    Symbol {
        name: name.into(),
        kind,
        fqn: name.into(),
        container: None,
        parents: Box::default(),
        range,
        offset,
        line: 0,
        detail: None,
        is_static: false,
        visibility: Visibility::Public,
        note: None,
        types: None,
    }
}

/// On a declaration, on a use (by the definition found), on a variable only assigned (its
/// function its scope); never the receiver.
#[test]
fn what_a_rename_at_the_cursor_renames() {
    let source = "function save() { $sum = 1; save(); $this; }";
    let path = Path::new("/p/a.php");
    let mut index = SymbolIndex::default();
    let save = symbol("save", SymbolKind::Function, 9, 0..source.len());
    index.set_file(
        path,
        FileSymbols { language: "php".into(), symbols: vec![save], ..Default::default() },
    );
    let php = |_: &str| Naming {
        sigil: Some('$'),
        receiver: Some("$this"),
        implicit_locals: true,
        ..Naming::PLAIN
    };
    let at = |needle: &str, definition: Option<Location>| {
        let head = source.find(needle).unwrap() + 1;
        subject_at(&index, path, source, head, "php".into(), definition, &php)
    };
    let declared = at("save()", None).unwrap();
    assert_eq!(
        (declared.name.as_str(), declared.declared.kind),
        ("save", Some(SymbolKind::Function))
    );
    let call = source.rfind("save()").unwrap();
    let used = at(&source[call..], Some(Location { path: path.into(), offset: 9, line: 0 }));
    assert_eq!(used.unwrap().declared.at.offset, 9, "the declaration of what's called");
    let variable = at("$sum", None).unwrap();
    assert_eq!((variable.name.as_str(), variable.local), ("$sum", Some(0..source.len())));
    assert!(at("$this", None).is_none(), "the receiver");
}

#[test]
fn a_method_s_family_is_renamed_together() {
    let class = |name: &str, parents: &[&str], start: usize| {
        let mut s = symbol(name, SymbolKind::Class, start, start..start + 100);
        s.fqn = name.into();
        s.parents = parents.iter().map(|p| (*p).into()).collect();
        s
    };
    let method = |container: &str, offset: usize| {
        let mut s = symbol("describe", SymbolKind::Method, offset, offset..offset + 10);
        s.container = Some(container.into());
        s
    };
    let mut index = SymbolIndex::default();
    let files = [
        ("/p/Entity.php", class("Entity", &[], 0), method("Entity", 20)),
        ("/p/User.php", class("User", &["Entity"], 0), method("User", 30)),
        ("/p/Admin.php", class("Admin", &["User"], 0), method("Admin", 40)),
        ("/p/Other.php", class("Other", &[], 0), method("Other", 50)),
    ];
    for (path, c, m) in files {
        index.set_file(Path::new(path), FileSymbols { symbols: vec![c, m], ..Default::default() });
    }
    let at = |path: &str, offset: usize| Location { path: path.into(), offset, line: 0 };
    let user = Declared {
        at: at("/p/User.php", 30),
        kind: Some(SymbolKind::Method),
        fqn: "describe".into(),
        container: Some("User".into()),
    };
    assert_eq!(
        user.by_definitions_in(&[at("/p/Entity.php", 20)], &index),
        Match::Target,
        "its parent's"
    );
    assert_eq!(
        user.by_definitions_in(&[at("/p/Admin.php", 40)], &index),
        Match::Target,
        "an override"
    );
    assert_eq!(
        user.by_definitions_in(&[at("/p/Other.php", 50)], &index),
        Match::Other,
        "unrelated"
    );
    assert_eq!(user.by_definitions(&[at("/p/Entity.php", 20)]), Match::Other, "the plain rule");
}
