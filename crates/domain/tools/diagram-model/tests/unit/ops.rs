use super::*;
use crate::{Diagram, Point};

const FLOW: &str = include_str!("../data/flow.drawio");

fn diagram() -> Diagram {
    Diagram::new(Document::read(FLOW).unwrap())
}

fn geometry<'a>(d: &'a Diagram, id: &str) -> &'a Geometry {
    d.doc.pages[0].cell(id).unwrap().geometry.as_ref().unwrap()
}

/// Applied, then undone: the file is back byte for byte; redone: the edit again.
fn undoes_exactly(d: &mut Diagram, tx: Transaction) {
    let before = d.doc.write();
    assert!(d.edit(tx, None));
    let after = d.doc.write();
    assert_ne!(after, before, "it changed something");
    assert!(d.undo());
    assert_eq!(d.doc.write(), before, "undone exactly");
    assert!(d.redo());
    assert_eq!(d.doc.write(), after, "redone");
}

/// Shapes moved: their boxes, the points of the edge between two of them; a shape inside a
/// moved group isn't moved twice; a loose edge chosen moves its ends.
#[test]
fn moving_shapes_takes_their_edges_along() {
    let mut d = diagram();
    let tx = move_by(&d.doc, 0, &["check", "save", "grp", "g-a", "loose"], 10.0, -5.0);
    undoes_exactly(&mut d, tx);
    assert_eq!(geometry(&d, "check").bounds(), (360.0, 155.0, 100.0, 80.0));
    assert_eq!(geometry(&d, "e2").points[0], Point::new(510.0, 195.0), "between check and save");
    assert_eq!(geometry(&d, "e1").points.len(), 0, "start didn't move: e1 has no points anyway");
    assert_eq!(geometry(&d, "grp").bounds().0, 50.0);
    assert_eq!(geometry(&d, "g-a").bounds().0, 0.0, "inside the group: carried, not moved");
    assert_eq!(geometry(&d, "loose").source_point, Some(Point::new(90.0, 295.0)));
}

/// Deleted with what it holds and the edges on it (and their labels); undone, all back in
/// their places.
#[test]
fn deleting_takes_what_it_holds_and_its_edges() {
    let mut d = diagram();
    let tx = delete(&d.doc, 0, &["check"]);
    undoes_exactly(&mut d, tx);
    let page = &d.doc.pages[0];
    for gone in ["check", "e1", "e2", "e2-label"] {
        assert!(page.cell(gone).is_none(), "{gone}");
    }
    assert!(page.cell("start").is_some() && page.cell("save").is_some());
    let mut d = diagram();
    let tx = delete(&d.doc, 0, &["grp"]);
    d.edit(tx, None);
    assert!(d.doc.pages[0].cell("g-a").is_none() && d.doc.pages[0].cell("g-b").is_none());
}

/// Resized, restyled, its text set, an edge reconnected: each one undone exactly.
#[test]
fn a_cell_s_box_style_text_and_ends_change() {
    let mut d = diagram();
    let tx = resize(&d.doc, 0, "save", (560.0, 170.0, 0.0, 90.0));
    undoes_exactly(&mut d, tx);
    assert_eq!(geometry(&d, "save").bounds(), (560.0, 170.0, 1.0, 90.0), "at least 1");
    let tx = set_style(&d.doc, 0, &["start", "save"], "fillColor", Some("#f8cecc"));
    undoes_exactly(&mut d, tx);
    assert_eq!(d.doc.pages[0].cell("save").unwrap().style.get("fillColor"), Some("#f8cecc"));
    let tx = set_style(&d.doc, 0, &["start"], "fillColor", None);
    d.edit(tx, None);
    assert_eq!(d.doc.pages[0].cell("start").unwrap().style.get("fillColor"), None);
    let tx = set_value(&d.doc, 0, "start", "Begin");
    undoes_exactly(&mut d, tx);
    let tx = connect(&d.doc, 0, "e1", Some("start"), Some("save"));
    undoes_exactly(&mut d, tx);
    assert_eq!(d.doc.pages[0].cell("e1").unwrap().target.as_deref(), Some("save"));
}

/// Grouped: a group around them, their places inside it; ungrouped: back where they were.
#[test]
fn grouping_and_ungrouping() {
    let mut d = diagram();
    let tx = group(&d.doc, 0, &["start", "check"], "g1").unwrap();
    undoes_exactly(&mut d, tx);
    assert_eq!(geometry(&d, "g1").bounds(), (340.0, 40.0, 120.0, 200.0));
    assert_eq!(geometry(&d, "check").bounds(), (10.0, 120.0, 100.0, 80.0));
    assert_eq!(d.doc.pages[0].cell("check").unwrap().parent.as_deref(), Some("g1"));
    let tx = ungroup(&d.doc, 0, "g1").unwrap();
    d.edit(tx, None);
    assert!(d.doc.pages[0].cell("g1").is_none());
    assert_eq!(geometry(&d, "check").bounds(), (350.0, 160.0, 100.0, 80.0));
    assert!(group(&d.doc, 0, &["start"], "g2").is_none(), "one shape");
    assert!(group(&d.doc, 0, &["start", "g-a"], "g2").is_none(), "two parents");
    assert!(ungroup(&d.doc, 0, "start").is_none(), "not a group");
}

/// Brought to the front: drawn last; sent to the back: first among its siblings.
#[test]
fn bringing_to_the_front_and_sending_to_the_back() {
    let mut d = diagram();
    let order = |d: &Diagram| d.doc.pages[0].cells.iter().map(|c| c.id.clone()).collect::<Vec<_>>();
    let tx = reorder(&d.doc, 0, &["start"], true);
    undoes_exactly(&mut d, tx);
    assert_eq!(order(&d).last().map(String::as_str), Some("start"));
    let tx = reorder(&d.doc, 0, &["pool"], false);
    d.edit(tx, None);
    assert_eq!(&order(&d)[..3], ["0", "1", "pool"]);
}

/// A cell added on top; the history's steps of one drag are one undo.
#[test]
fn a_drag_is_one_step() {
    let mut d = diagram();
    let before = d.doc.write();
    for _ in 0..5 {
        let tx = move_by(&d.doc, 0, &["start"], 2.0, 1.0);
        d.edit(tx, Some(7));
    }
    assert_eq!(geometry(&d, "start").bounds().0, 350.0);
    d.history.seal();
    let tx = move_by(&d.doc, 0, &["start"], 1.0, 0.0);
    d.edit(tx, Some(7));
    assert!(d.undo(), "the last gesture");
    assert!(d.undo(), "the drag, as one");
    assert_eq!(d.doc.write(), before);
    assert!(!d.undo());
    let mut shape = Cell::new("new", CellKind::Vertex, "1");
    shape.geometry = Some(Geometry { width: 10.0, height: 10.0, ..Geometry::default() });
    let tx = add(&d.doc, 0, shape);
    undoes_exactly(&mut d, tx);
    assert_eq!(d.doc.pages[0].cells.last().map(|c| c.id.as_str()), Some("new"));
}

/// An edge's points and loose end set, undone exactly.
#[test]
fn an_edge_s_geometry_is_set() {
    let mut d = diagram();
    let mut g = geometry(&d, "e1").clone();
    g.points = vec![Point::new(420.0, 130.0)];
    g.target_point = Some(Point::new(600.0, 400.0));
    let tx = set_geometry(&d.doc, 0, "e1", g);
    undoes_exactly(&mut d, tx);
    assert_eq!(geometry(&d, "e1").points, [Point::new(420.0, 130.0)]);
    assert_eq!(geometry(&d, "e1").target_point, Some(Point::new(600.0, 400.0)));
}

/// A cell changed in several ways at once: one step.
#[test]
fn a_cell_updated_at_once() {
    let mut d = diagram();
    let tx = update(&d.doc, 0, "e1", |c| {
        c.target = None;
        c.geometry.get_or_insert_with(Geometry::default).target_point = Some(Point::new(1.0, 2.0));
        c.style.set("curved", "1");
    });
    undoes_exactly(&mut d, tx);
    let e1 = d.doc.pages[0].cell("e1").unwrap();
    assert_eq!((e1.target.as_deref(), e1.style.get("curved")), (None, Some("1")));
    assert!(update(&d.doc, 0, "nope", |_| {}).is_empty());
}

/// A page's setting set and taken away, each undone exactly; the same value: nothing.
#[test]
fn a_page_setting_is_set() {
    let mut d = diagram();
    let tx = set_page_setting(&d.doc, 0, "gridSize", Some("20"));
    undoes_exactly(&mut d, tx);
    assert_eq!(d.doc.pages[0].grid_size(), 20.0);
    let tx = set_page_setting(&d.doc, 0, "background", Some("#ffffff"));
    undoes_exactly(&mut d, tx);
    let tx = set_page_setting(&d.doc, 0, "background", None);
    undoes_exactly(&mut d, tx);
    assert!(set_page_setting(&d.doc, 0, "grid", Some("1")).is_empty(), "already so");
}

/// A page of `cells` (the root and the layer added).
fn page_of(cells: &str) -> Diagram {
    let text = format!(
        r#"<mxfile><diagram id="p" name="P"><mxGraphModel><root><mxCell id="0"/><mxCell id="1" parent="0"/>{cells}</root></mxGraphModel></diagram></mxfile>"#
    );
    Diagram::new(Document::read(&text).unwrap())
}

/// Ungrouped: an edge in the group keeps its place on the page (its points were the
/// group's).
#[test]
fn ungrouping_moves_the_edges_inside_too() {
    let mut d = page_of(
        r#"<mxCell id="g" value="" style="group" vertex="1" parent="1"><mxGeometry x="100" y="100" width="200" height="200" as="geometry"/></mxCell>
<mxCell id="e" edge="1" parent="g"><mxGeometry relative="1" as="geometry"><mxPoint x="10" y="10" as="sourcePoint"/><mxPoint x="90" y="90" as="targetPoint"/><Array as="points"><mxPoint x="50" y="50"/></Array></mxGeometry></mxCell>"#,
    );
    let tx = ungroup(&d.doc, 0, "g").unwrap();
    d.edit(tx, None);
    let g = geometry(&d, "e");
    assert_eq!((g.points[0].x, g.points[0].y), (150.0, 150.0));
    assert_eq!(g.source_point.map(|p| (p.x, p.y)), Some((110.0, 110.0)));
    assert_eq!(g.target_point.map(|p| (p.x, p.y)), Some((190.0, 190.0)));
}

/// Shapes without a box: nothing to group (never `inf` in the file).
#[test]
fn shapes_without_a_box_aren_t_grouped() {
    let d =
        page_of(r#"<mxCell id="a" vertex="1" parent="1"/><mxCell id="b" vertex="1" parent="1"/>"#);
    assert!(group(&d.doc, 0, &["a", "b"], "g").is_none());
    assert_eq!(crate::number(f64::INFINITY), "0");
    assert_eq!(crate::number(f64::NAN), "0");
}

/// Sent to the back with cells of two parents: each before its own parent's first other
/// child; one step undone exactly.
#[test]
fn sending_to_the_back_across_parents() {
    let shape = |id: &str, parent: &str| {
        format!(
            r#"<mxCell id="{id}" vertex="1" parent="{parent}"><mxGeometry width="10" height="10" as="geometry"/></mxCell>"#
        )
    };
    let cells =
        [shape("G", "1"), shape("B", "G"), shape("D", "G"), shape("A", "1"), shape("C", "1")]
            .concat();
    let mut d = page_of(&cells);
    let tx = reorder(&d.doc, 0, &["D", "C"], false);
    undoes_exactly(&mut d, tx);
    let order: Vec<String> = d.doc.pages[0].cells.iter().map(|c| c.id.clone()).collect();
    assert_eq!(order, ["0", "1", "C", "G", "D", "B", "A"]);
}

/// Several keys set at once, each cell its own (a shape's fill and line, an edge's line): one
/// step, undone exactly.
#[test]
fn several_style_keys_are_set_as_one_step() {
    let mut d = diagram();
    let tx = set_styles(&d.doc, 0, &["start", "e1"], |c| match c.kind {
        CellKind::Edge => vec![("strokeColor", Some("#b85450")), ("dashed", Some("1"))],
        _ => vec![("fillColor", Some("#f8cecc")), ("strokeColor", Some("#b85450"))],
    });
    assert_eq!(tx.changes.len(), 2, "one change a cell");
    undoes_exactly(&mut d, tx.clone());
    assert!(d.edit(tx, None));
    let page = &d.doc.pages[0];
    let (start, e1) = (page.cell("start").unwrap(), page.cell("e1").unwrap());
    assert_eq!(start.style.get("fillColor"), Some("#f8cecc"));
    assert_eq!(start.style.get("strokeColor"), Some("#b85450"));
    assert_eq!((e1.style.get("strokeColor"), e1.style.get("dashed")), (Some("#b85450"), Some("1")));
    assert_eq!(e1.style.get("fillColor"), None);
}
