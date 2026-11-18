use super::*;
use crate::{CellKind, Point};

/// A fixture's text (embedded: the domain reads no disk, its tests neither).
fn fixture(name: &str) -> String {
    match name {
        "flow.drawio" => include_str!("../data/flow.drawio"),
        "compressed.drawio" => include_str!("../data/compressed.drawio"),
        other => panic!("no fixture {other}"),
    }
    .to_owned()
}

/// The lines of `b` that differ from `a`'s (both as many lines).
fn changed_lines(a: &str, b: &str) -> Vec<String> {
    let (a, b): (Vec<_>, Vec<_>) = (a.lines().collect(), b.lines().collect());
    assert_eq!(a.len(), b.len(), "as many lines:\n{}", b.join("\n"));
    a.iter().zip(&b).filter(|(x, y)| x != y).map(|(_, y)| y.trim().to_owned()).collect()
}

/// A file draw.io saved, read and written without a change: the same bytes.
#[test]
fn a_file_unchanged_is_written_back_the_same() {
    let text = fixture("flow.drawio");
    let doc = Document::read(&text).unwrap();
    assert_eq!(doc.write(), text);
}

/// What's read: the pages, the cells' kinds, texts (entities and line breaks decoded),
/// styles, geometry, an edge's points and ends, a wrapper's properties, a hidden layer.
#[test]
fn the_cells_are_read_into_their_fields() {
    let doc = Document::read(&fixture("flow.drawio")).unwrap();
    let names: Vec<&str> = doc.pages.iter().map(Page::name).collect();
    assert_eq!(names, ["Flow", "Layers"]);
    let flow = &doc.pages[0];
    assert_eq!(flow.cell("0").unwrap().kind, CellKind::Holder);
    let check = flow.cell("check").unwrap();
    assert_eq!((check.kind, check.value.as_str()), (CellKind::Vertex, "<b>Valid?</b>"));
    assert_eq!(check.style.shape(), Some("rhombus"));
    assert_eq!(check.style.get("fillColor"), Some("#fff2cc"));
    assert_eq!(check.geometry.as_ref().unwrap().bounds(), (350.0, 160.0, 100.0, 80.0));
    assert_eq!(flow.cell("save").unwrap().value, "Save & notify\nthe owner");
    let e2 = flow.cell("e2").unwrap();
    assert_eq!((e2.source.as_deref(), e2.target.as_deref()), (Some("check"), Some("save")));
    let way = e2.geometry.as_ref().unwrap();
    assert_eq!(way.points, [Point::new(500.0, 200.0), Point::new(500.0, 210.5)]);
    assert_eq!(way.offset, Some(Point::new(-10.0, 5.0)));
    assert!(e2.style.flag("dashed") && e2.style.get("endArrow") == Some("block"));
    let label = flow.cell("e2-label").unwrap();
    assert_eq!(label.parent.as_deref(), Some("e2"));
    assert!(label.geometry.as_ref().unwrap().relative);
    assert_eq!(label.geometry.as_ref().unwrap().x, -0.2);
    let loose = flow.cell("loose").unwrap().geometry.clone().unwrap();
    assert_eq!(loose.source_point, Some(Point::new(80.0, 300.0)));
    assert_eq!(loose.target_point, Some(Point::new(200.0, 300.0)));
    assert_eq!(flow.cell("g-b").unwrap().parent.as_deref(), Some("grp"));
    let docs = flow.cell("docs").unwrap();
    assert!(docs.is_wrapped());
    assert_eq!(docs.value, "Docs");
    let props: Vec<_> = docs.properties().collect();
    assert_eq!(
        props,
        [("link", "https://example.com/docs"), ("tooltip", "Read me"), ("owner", "team-a")]
    );
    let layers = &doc.pages[1];
    assert!(!layers.grid());
    assert_eq!(layers.grid_size(), 20.0);
    assert_eq!(layers.background(), Some("#F5F5F5"));
    assert_eq!(layers.page_size(), (1169.0, 827.0));
    assert!(!layers.cell("hidden").unwrap().visible);
    assert_eq!(layers.cell("n1").unwrap().value, "Ştefan's note: ünïcödé ✓");
}

/// A shape moved: one line of the file changes, nothing else (its other numbers, the
/// entities elsewhere, the attributes' order).
#[test]
fn a_shape_moved_changes_one_line() {
    let text = fixture("flow.drawio");
    let mut doc = Document::read(&text).unwrap();
    let check = doc.pages[0].cell_mut("check").unwrap();
    check.geometry.as_mut().unwrap().x = 370.5;
    let written = doc.write();
    assert_eq!(
        changed_lines(&text, &written),
        [r#"<mxGeometry x="370.5" y="160" width="100" height="80" as="geometry" />"#]
    );
}

/// A style key changed in its place, a new one at the end; a text changed escaped as
/// draw.io does: each the cell's line only.
#[test]
fn a_style_and_a_text_changed_change_their_line() {
    let text = fixture("flow.drawio");
    let mut doc = Document::read(&text).unwrap();
    let start = doc.pages[0].cell_mut("start").unwrap();
    start.style.set("fillColor", "#dae8fc");
    start.style.set("shadow", "1");
    start.value = "Go \"now\" & <then>".into();
    let written = doc.write();
    assert_eq!(
        changed_lines(&text, &written),
        [
            r#"<mxCell id="start" value="Go &quot;now&quot; &amp; &lt;then&gt;" style="ellipse;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#82b366;shadow=1;" vertex="1" parent="1">"#
        ]
    );
    let again = Document::read(&written).unwrap();
    assert_eq!(again.pages[0].cell("start").unwrap().value, "Go \"now\" & <then>");
}

/// An edge's points changed: its points written anew, the rest of it kept.
#[test]
fn an_edge_s_points_changed_are_written_anew() {
    let mut doc = Document::read(&fixture("flow.drawio")).unwrap();
    let e2 = doc.pages[0].cell_mut("e2").unwrap();
    e2.geometry.as_mut().unwrap().points = vec![Point::new(520.0, 200.0)];
    let again = Document::read(&doc.write()).unwrap();
    let e2 = again.pages[0].cell("e2").unwrap().geometry.clone().unwrap();
    assert_eq!(e2.points, [Point::new(520.0, 200.0)]);
    assert_eq!(e2.offset, Some(Point::new(-10.0, 5.0)), "kept");
}

/// What isn't understood is kept: a swimlane's `alternateBounds`, `collapsed`, a wrapper's
/// properties — written back after a change to the same cell.
#[test]
fn what_isn_t_understood_is_kept() {
    let mut doc = Document::read(&fixture("flow.drawio")).unwrap();
    doc.pages[0].cell_mut("pool").unwrap().value = "Lane".into();
    doc.pages[0].cell_mut("docs").unwrap().value = "Manual".into();
    let written = doc.write();
    assert!(written.contains(r#"<mxCell id="pool" value="Lane" style="swimlane;startSize=23;" vertex="1" collapsed="1" parent="1">"#));
    assert!(written.contains(
        r#"<mxRectangle x="40" y="520" width="400" height="200" as="alternateBounds" />"#
    ));
    assert!(written.contains(r#"<UserObject label="Manual" link="https://example.com/docs" tooltip="Read me" owner="team-a" id="docs">"#));
}

/// A page draw.io kept compressed is read; written, it's plain XML that reads the same.
#[test]
fn a_compressed_page_is_read_and_written_plain() {
    let doc = Document::read(&fixture("compressed.drawio")).unwrap();
    let page = &doc.pages[0];
    assert_eq!(page.name(), "Compressed");
    let ids: Vec<&str> = page.cells.iter().map(|c| c.id.as_str()).collect();
    assert_eq!(ids, ["0", "1", "a", "b", "ab"]);
    assert_eq!(page.cell("a").unwrap().value, "Ünïcode & more");
    assert_eq!(page.cell("ab").unwrap().kind, CellKind::Edge);
    let written = doc.write();
    assert!(written.contains("<mxGraphModel dx=\"800\""), "{written}");
    let again = Document::read(&written).unwrap();
    for (a, b) in doc.pages[0].cells.iter().zip(&again.pages[0].cells) {
        assert_eq!(
            (&a.id, &a.value, &a.style, &a.geometry),
            (&b.id, &b.value, &b.style, &b.geometry)
        );
    }
}

/// A new file: one page with its root and layer, as draw.io makes it; a shape added to it
/// written in draw.io's order and read back.
#[test]
fn a_new_file_and_a_new_shape() {
    let mut doc = Document::new("Page-1", "p1");
    let mut shape = Cell::new("s1", CellKind::Vertex, "1");
    shape.value = "Hello".into();
    shape.style = crate::Style::parse("rounded=0;whiteSpace=wrap;html=1;");
    shape.geometry = Some(crate::Geometry {
        x: 40.0,
        y: 30.0,
        width: 120.0,
        height: 60.0,
        ..Default::default()
    });
    doc.pages[0].cells.push(shape);
    let written = doc.write();
    assert!(
        written.starts_with("<mxfile host=\"Electron\">\n  <diagram name=\"Page-1\" id=\"p1\">\n"),
        "{written}"
    );
    assert!(
        written.contains("        <mxCell id=\"0\" />\n        <mxCell id=\"1\" parent=\"0\" />\n"),
        "{written}"
    );
    assert!(written.contains(concat!(
        "        <mxCell id=\"s1\" value=\"Hello\" style=\"rounded=0;whiteSpace=wrap;html=1;\" vertex=\"1\" parent=\"1\">\n",
        "          <mxGeometry x=\"40\" y=\"30\" width=\"120\" height=\"60\" as=\"geometry\" />\n",
        "        </mxCell>\n"
    )), "{written}");
    assert!(written.ends_with("</mxfile>\n"));
    let again = Document::read(&written).unwrap();
    assert_eq!(again.pages[0].cell("s1").unwrap().geometry.as_ref().unwrap().width, 120.0);
    assert_eq!(again.write(), written, "and the same again");
}

/// A bare `<mxGraphModel>` (what some tools write): one page, written back the same.
#[test]
fn a_bare_model_is_one_page() {
    let text = "<mxGraphModel grid=\"1\">\n  <root>\n    <mxCell id=\"0\" />\n    <mxCell id=\"1\" parent=\"0\" />\n  </root>\n</mxGraphModel>\n";
    let doc = Document::read(text).unwrap();
    assert_eq!(doc.pages.len(), 1);
    assert_eq!(doc.write(), text);
}

/// What isn't a diagram, or is hostile, is refused (never a panic).
#[test]
fn what_isn_t_a_diagram_is_refused() {
    assert!(matches!(Document::read("not xml"), Err(Error::Xml(_))));
    assert_eq!(Document::read("<svg/>"), Err(Error::NotDrawio));
    let twice = "<mxGraphModel><root><mxCell id=\"0\"/><mxCell id=\"0\" parent=\"0\"/></root></mxGraphModel>";
    assert_eq!(Document::read(twice), Err(Error::DuplicateId("0".into())));
    let deep = format!("<mxfile>{}{}</mxfile>", "<a>".repeat(80), "</a>".repeat(80));
    assert_eq!(Document::read(&deep), Err(Error::TooDeep));
    let dtd = "<!DOCTYPE x [<!ENTITY a \"aaaa\">]><mxfile>&a;</mxfile>";
    assert!(matches!(Document::read(dtd), Err(Error::Xml(_))));
    let broken = "<mxfile><diagram id=\"x\">%%%not base64%%%</diagram></mxfile>";
    assert!(matches!(Document::read(broken), Err(Error::Compressed(_))));
}

/// A diagram of 500 shapes and 600 edges, read and written (plan 80's measure: run with
/// `--ignored --nocapture`).
#[test]
#[ignore]
#[allow(clippy::disallowed_methods)] // The time is measured by the test, not the domain.
fn measure_a_large_diagram() {
    let mut doc = Document::new("Big", "big");
    for i in 0..500 {
        let mut c = Cell::new(format!("v{i}"), CellKind::Vertex, "1");
        c.value = format!("Shape {i}");
        c.style = crate::Style::parse(
            "rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;",
        );
        c.geometry = Some(crate::Geometry {
            x: f64::from(i % 25) * 160.0,
            y: f64::from(i / 25) * 100.0,
            width: 120.0,
            height: 60.0,
            ..Default::default()
        });
        doc.pages[0].cells.push(c);
    }
    for i in 0..600 {
        let mut e = Cell::new(format!("e{i}"), CellKind::Edge, "1");
        e.style = crate::Style::parse("edgeStyle=orthogonalEdgeStyle;rounded=0;html=1;");
        e.source = Some(format!("v{}", i % 500));
        e.target = Some(format!("v{}", (i * 7 + 1) % 500));
        e.geometry = Some(crate::Geometry { relative: true, ..Default::default() });
        doc.pages[0].cells.push(e);
    }
    let text = doc.write();
    let started = std::time::Instant::now();
    let read = Document::read(&text).unwrap();
    let reading = started.elapsed();
    let started = std::time::Instant::now();
    let again = read.write();
    let writing = started.elapsed();
    assert_eq!(again, text);
    eprintln!("MEASURE {} bytes, read {reading:?}, write {writing:?}", text.len());
}

/// `xml` as draw.io compresses a page (raw deflate, base64; no characters to URI-encode).
fn compressed(xml: &str) -> String {
    const ABC: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = miniz_oxide::deflate::compress_to_vec(xml.as_bytes(), 6);
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ABC[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The elements of all the file's compressed pages count together: pages each under the
/// limit, together over it, are refused (a small file can't fill the memory).
#[test]
fn compressed_pages_share_one_budget() {
    let page = format!("<mxGraphModel><root>{}</root></mxGraphModel>", "<a/>".repeat(400_000));
    let one = compressed(&page);
    let file = |pages: usize| {
        let diagrams: String = (0..pages)
            .map(|i| format!(r#"<diagram id="p{i}" name="P{i}">{one}</diagram>"#))
            .collect();
        format!("<mxfile>{diagrams}</mxfile>")
    };
    assert!(Document::read(&file(2)).is_ok(), "800 000 elements");
    assert!(matches!(Document::read(&file(3)), Err(Error::TooManyCells)), "1 200 000");
}

/// A value written between single quotes holding a `"`: saved, the file still reads (and
/// the value is the same).
#[test]
fn single_quoted_values_survive_a_save() {
    let text = r#"<mxfile><diagram name='a "q" b' id="x"><mxGraphModel><root><mxCell id="0"/></root></mxGraphModel></diagram></mxfile>"#;
    let doc = Document::read(text).unwrap();
    let again = Document::read(&doc.write()).expect("still readable");
    assert_eq!(again.pages[0].name(), r#"a "q" b"#);
}

/// Tabs and line breaks in a text survive a save; characters XML can't hold are left out
/// (the file stays readable).
#[test]
fn control_characters_in_a_text() {
    let mut doc = Document::read(&fixture("flow.drawio")).unwrap();
    doc.pages[0].cells.iter_mut().find(|c| c.id == "start").unwrap().value =
        "a\tb\r\nc\u{1}d".into();
    let again = Document::read(&doc.write()).expect("readable");
    assert_eq!(again.pages[0].cell("start").unwrap().value, "a\tb\r\ncd");
}
