use super::*;
use crate::symbols::Visibility;

fn sym(name: &str, kind: SymbolKind, range: std::ops::Range<usize>) -> Symbol {
    Symbol {
        name: name.into(),
        kind,
        fqn: name.into(),
        container: None,
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

#[test]
fn nesting_and_scope() {
    let file = FileSymbols {
        symbols: vec![
            sym("stop", SymbolKind::Method, 40..60),
            sym("Car", SymbolKind::Class, 10..100),
            sym("start", SymbolKind::Method, 20..30),
            sym("helper", SymbolKind::Function, 120..140),
        ],
        ..FileSymbols::default()
    };
    let tree: Vec<(usize, usize)> = outline(&file).iter().map(|o| (o.index, o.depth)).collect();
    assert_eq!(tree, [(1, 0), (2, 1), (0, 1), (3, 0)]);
    assert_eq!(scope_at(&file, 45), [1, 0]);
    assert_eq!(scope_at(&file, 110), Vec::<usize>::new());
    assert_eq!(label(&file.symbols[0], Some('$')), "stop()");
}
