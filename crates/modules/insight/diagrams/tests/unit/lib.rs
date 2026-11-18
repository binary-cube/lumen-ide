use super::*;
use std::path::PathBuf;

fn view() -> DiagramView {
    let mut focus = code_intelligence::DiagramNode {
        fqn: "App\\Repo".into(),
        name: "Repo".into(),
        kind: SymbolKind::Class,
        role: DiagramRole::Focus,
        path: Some(PathBuf::from("/p/Repo.php")),
        line: 4,
        members: Vec::new(),
    };
    focus.members.push(code_intelligence::DiagramMember {
        name: "find".into(),
        kind: SymbolKind::Method,
        visibility: Visibility::Public,
        is_static: false,
        detail: Some("(int $id)".into()),
        line: 9,
    });
    let parent = code_intelligence::DiagramNode {
        fqn: "App\\Base".into(),
        name: "Base".into(),
        role: DiagramRole::Ancestor(1),
        path: Some(PathBuf::from("/p/Base.php")),
        members: Vec::new(),
        ..focus.clone()
    };
    let mut view = DiagramView::default();
    view.fqn = "App\\Repo".into();
    view.set_diagram(Some(ClassDiagram {
        nodes: vec![focus, parent],
        edges: vec![code_intelligence::DiagramEdge { from: 0, to: 1, relation: Relation::Extends }],
    }));
    view
}

#[test]
fn members_open_their_code_and_the_focus_its_declaration() {
    let mut v = view();
    assert_eq!(
        v.activate(Hit::Member(0, 0)),
        Some(ViewerAction::OpenAt { path: "/p/Repo.php".into(), line: 9 })
    );
    assert_eq!(
        v.activate(Hit::Card(0)),
        Some(ViewerAction::OpenAt { path: "/p/Repo.php".into(), line: 4 })
    );
}

#[test]
fn another_type_becomes_the_focus_and_back_returns() {
    let mut v = view();
    // No queries in the test: the rebuild doesn't start, but the history is kept.
    assert_eq!(v.activate(Hit::Card(1)), None);
    assert_eq!(v.history.items(), ["App\\Repo"]);
    assert_eq!(v.fqn, "App\\Base");
    assert!(v.command("back"), "its action (⌫)");
    assert!(v.history.is_empty());
}

#[test]
fn a_click_acts_on_release_but_a_drag_pans() {
    let mut v = view();
    v.view.scale = 1.0;
    v.view.zoom = 1.0;
    v.hits = vec![(Rect::new(0.0, 0.0, 50.0, 20.0), Hit::Member(0, 0))];
    v.click(Point::new(10.0, 10.0));
    assert!(v.release());
    assert!(matches!(v.take_action(), Some(ViewerAction::OpenAt { line: 9, .. })));
    v.click(Point::new(10.0, 10.0));
    assert!(v.drag(Point::new(60.0, 10.0)));
    v.release();
    assert_eq!(v.take_action(), None);
    assert_eq!(v.view.pan.x, -50.0);
}
