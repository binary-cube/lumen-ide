use super::*;
use crate::{FileSymbols, Symbol};

fn symbol(name: &str, kind: SymbolKind, container: Option<&str>) -> Symbol {
    Symbol {
        name: name.into(),
        kind,
        fqn: name.into(),
        container: container.map(Into::into),
        parents: Box::default(),
        range: 0..0,
        offset: 0,
        line: 0,
        detail: None,
        is_static: false,
        visibility: Default::default(),
        note: None,
        types: None,
    }
}

fn index() -> SymbolIndex {
    let mut index = SymbolIndex::default();
    let file = |language: &str, symbols: Vec<Symbol>| FileSymbols {
        language: language.into(),
        symbols,
        ..FileSymbols::default()
    };
    index.set_file(
        Path::new("/p/src/UserController.php"),
        file(
            "php",
            vec![
                symbol("UserController", SymbolKind::Class, None),
                symbol("update", SymbolKind::Method, Some("UserController")),
            ],
        ),
    );
    index.set_file(
        Path::new("/p/vendor/x/UserCtrl.php"),
        file("php", vec![symbol("UserCtrl", SymbolKind::Class, None)]),
    );
    index.set_file(
        Path::new("/p/assets/user.js"),
        file("javascript", vec![symbol("userControls", SymbolKind::Function, None)]),
    );
    index
}

#[test]
fn fuzzy_names_with_filters() {
    // The folders the languages' modules register (Composer's, npm's, the builds').
    kernel::paths::register(kernel::ProjectDirs {
        dependencies: &["vendor", "node_modules"],
        unlisted: &["node_modules"],
        generated: &["dist", "build", "storage", "public/build", "var/cache"],
        ..Default::default()
    });
    let index = index();
    let base =
        SymbolQuery { text: "uctrl", root: Some(Path::new("/p")), limit: 10, ..Default::default() };
    let names =
        |q: &SymbolQuery| search_symbols(&index, q).into_iter().map(|h| h.name).collect::<Vec<_>>();
    let mut found = names(&base);
    found.sort();
    assert_eq!(found, ["UserController", "userControls"], "no vendor by default");
    let with_vendor = SymbolQuery { dependencies: true, ..base.clone() };
    assert!(names(&with_vendor).contains(&"UserCtrl".to_owned()));
    let classes = SymbolQuery { kinds: &[SymbolKind::Class], ..base.clone() };
    assert_eq!(names(&classes), ["UserController"]);
    let js = SymbolQuery { languages: &["javascript"], ..base.clone() };
    assert_eq!(names(&js), ["userControls"]);
    let one = SymbolQuery { limit: 1, ..base };
    assert_eq!(names(&one).len(), 1);
}
