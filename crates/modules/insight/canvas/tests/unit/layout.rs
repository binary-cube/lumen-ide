//! The center file's neighbors: who uses it (left), what it uses that doesn't use it back
//! (right), in the files' order — found in one pass over the edges.

use super::*;
use code_intelligence::{GraphEdge, GraphFile};

fn file() -> GraphFile {
    GraphFile {
        path: "a.php".into(),
        absolute: "/p/a.php".into(),
        language: "php".into(),
        lines: 1,
        preview: Vec::new(),
        preview_start: 0,
        symbols: Vec::new(),
        unconnected: false,
    }
}

fn edge(from: usize, to: usize) -> GraphEdge {
    GraphEdge { from, line: 0, to, symbol: None, kind: EdgeKind::Type }
}

#[test]
fn the_center_s_neighbors_are_those_using_it_then_those_it_uses() {
    let graph = DepGraph {
        focus: 1,
        files: vec![file(); 5],
        edges: vec![edge(3, 1), edge(1, 4), edge(1, 3), edge(0, 1), edge(1, 1), edge(2, 4)],
        ..DepGraph::default()
    };
    let (incoming, outgoing) = neighbors(&graph);
    assert_eq!(incoming, [0, 3], "who uses it, in order (not itself)");
    assert_eq!(outgoing, [4], "what it uses, but those using it back");
}
