use super::*;
use crate::Diagram;

const FLOW: &str = include_str!("../data/flow.drawio");

fn counter() -> impl FnMut() -> String {
    let mut n = 0;
    move || {
        n += 1;
        format!("p{n}")
    }
}

/// Copied: the shapes, what they hold, the edges between them (not the ones to shapes left
/// behind); pasted: new ids, linked among themselves, in the layer, moved by the offset.
#[test]
fn copied_and_pasted_with_new_ids() {
    let doc = Document::read(FLOW).unwrap();
    let xml = copy(&doc.pages[0], &["check", "save", "grp"]);
    assert!(xml.starts_with("<mxGraphModel"), "{xml}");
    let copied = Document::read(&xml).unwrap();
    let ids: Vec<&str> = copied.pages[0].cells.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["0", "1", "check", "save", "e2", "e2-label", "grp", "g-a", "g-b"]);
    let mut d = Diagram::new(doc);
    let (tx, top) = paste(&d.doc, 0, &xml, "1", (20.0, 20.0), &mut counter()).unwrap();
    d.edit(tx, None);
    assert_eq!(top, ["p1", "p2", "p3", "p5"], "the cells at the top (not the label, the group's)");
    let page = &d.doc.pages[0];
    let check = page.cell("p1").unwrap();
    assert_eq!(check.geometry.as_ref().unwrap().bounds(), (370.0, 180.0, 100.0, 80.0));
    let edge = page.cell("p3").unwrap();
    assert_eq!((edge.source.as_deref(), edge.target.as_deref()), (Some("p1"), Some("p2")));
    assert_eq!(page.cell("p4").unwrap().parent.as_deref(), Some("p3"), "the label on its edge");
    let inner = page.cell("p6").unwrap();
    assert_eq!(inner.parent.as_deref(), Some("p5"));
    assert_eq!(inner.geometry.as_ref().unwrap().bounds().0, 0.0, "inside: not moved");
    assert!(page.cell("check").is_some(), "the original stays");
}

/// An edge copied without its ends pastes loose; ids already taken are skipped; what isn't
/// a diagram is refused.
#[test]
fn ends_left_behind_and_taken_ids() {
    let doc = Document::read(FLOW).unwrap();
    let xml = copy(&doc.pages[0], &["e1"]);
    // The first id offered is already a cell's.
    let mut offered = ["start", "q1"].into_iter();
    let mut next = move || offered.next().unwrap_or("fresh").to_owned();
    let (tx, top) = paste(&doc, 0, &xml, "1", (0.0, 0.0), &mut next).unwrap();
    assert_eq!(top, ["q1"], "`start` is taken");
    let Change::Insert { cell, .. } = &tx.changes[0] else { panic!() };
    assert_eq!((cell.source.as_deref(), cell.target.as_deref()), (None, None));
    assert!(paste(&doc, 0, "<html/>", "1", (0.0, 0.0), &mut counter()).is_err());
}

/// A shape copied without its group: in the clipboard's layer, at its place on the page (as
/// draw.io would place it).
#[test]
fn a_shape_copied_out_of_its_group() {
    let doc = Document::read(FLOW).unwrap();
    let page = &doc.pages[0];
    let geo = |id: &str| page.cell(id).unwrap().geometry.clone().unwrap();
    let clip = Document::read(&copy(page, &["g-b"])).unwrap();
    let copied = clip.pages[0].cell("g-b").unwrap();
    assert_eq!(copied.parent.as_deref(), Some("1"));
    let g = copied.geometry.as_ref().unwrap();
    assert_eq!((g.x, g.y), (geo("grp").x + geo("g-b").x, geo("grp").y + geo("g-b").y));
    // Copied with its group: still inside it, its place the group's.
    let clip = Document::read(&copy(page, &["grp"])).unwrap();
    let inside = clip.pages[0].cell("g-b").unwrap();
    assert_eq!(inside.parent.as_deref(), Some("grp"));
    assert_eq!(inside.geometry.as_ref().unwrap().x, geo("g-b").x);
}
