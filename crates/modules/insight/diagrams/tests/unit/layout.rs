use super::*;
use code_intelligence::{DiagramEdge, DiagramNode, SymbolKind};

fn node(fqn: &str, role: DiagramRole, members: usize) -> DiagramNode {
    DiagramNode {
        fqn: fqn.into(),
        name: fqn.into(),
        kind: SymbolKind::Class,
        role,
        path: None,
        line: 0,
        members: (0..members)
            .map(|i| code_intelligence::DiagramMember {
                name: format!("m{i}"),
                kind: SymbolKind::Method,
                visibility: Default::default(),
                is_static: false,
                detail: None,
                line: 0,
            })
            .collect(),
    }
}

fn diagram() -> ClassDiagram {
    ClassDiagram {
        nodes: vec![
            node("Focus", DiagramRole::Focus, 20),
            node("Parent", DiagramRole::Ancestor(1), 2),
            node("Grand", DiagramRole::Ancestor(2), 0),
            node("Child", DiagramRole::Child, 0),
            node("Dep", DiagramRole::Dependency, 0),
        ],
        edges: vec![
            DiagramEdge { from: 0, to: 1, relation: Relation::Extends },
            DiagramEdge { from: 1, to: 2, relation: Relation::Extends },
            DiagramEdge { from: 3, to: 0, relation: Relation::Extends },
            DiagramEdge { from: 0, to: 4, relation: Relation::Depends },
        ],
    }
}

#[test]
fn ancestors_above_children_below_dependencies_right() {
    let layout = Layout::new(&diagram());
    let rect = |i: usize| layout.card(i).unwrap().rect;
    assert!(rect(2).max_y() < rect(1).min_y());
    assert!(rect(1).max_y() < rect(0).min_y());
    assert!(rect(0).max_y() < rect(3).min_y());
    assert!(rect(4).min_x() > rect(0).max_x());
    // The focus lists more members, the rest are counted.
    let focus = layout.card(0).unwrap();
    assert_eq!((focus.shown, focus.more), (14, 6));
    assert_eq!(layout.edges.len(), 4);
}

#[test]
fn edges_are_axis_aligned_and_end_on_their_target() {
    let layout = Layout::new(&diagram());
    for edge in &layout.edges {
        for pair in edge.points.windows(2) {
            assert!(pair[0].x == pair[1].x || pair[0].y == pair[1].y, "{edge:?}");
        }
    }
    let up = &layout.edges[0];
    assert_eq!(up.dir, Dir::Up);
    assert_eq!(up.points.last().unwrap().y, layout.card(1).unwrap().rect.max_y());
    let right = layout.edges.iter().find(|e| e.kind == Relation::Depends).unwrap();
    assert_eq!(
        (right.dir, right.points.last().unwrap().x),
        (Dir::Right, layout.card(4).unwrap().rect.min_x())
    );
}
