use super::*;
use crate::drawing::draw;
use diagram_model::Document;

const FLOW: &str = include_str!("../../../../../domain/tools/diagram-model/tests/data/flow.drawio");

fn flow() -> Drawing {
    draw(&Document::read(FLOW).unwrap().pages[0], &light())
}

fn family() -> FontFamily {
    FontFamily::SansSerif
}

/// An SVG of the page: its size with the margin, a path per item, its texts escaped (one
/// `<text>` per line), its background; and it reads back as XML.
#[test]
fn a_page_as_svg() {
    let d = flow();
    let mut text = TextSystem::new();
    let out = svg(&d, Some(Rgba::rgb(0xdae8fc)), &mut text, &family());
    let w = d.bounds.size.width + 2.0 * MARGIN;
    assert!(out.starts_with(&format!(r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}""#)));
    assert_eq!(out.matches("<path ").count(), d.items.len());
    assert!(out.contains(r##"<rect width="100%" height="100%" fill="#dae8fc"/>"##));
    assert!(
        out.contains(">Save &amp; notify</text>") && out.contains(">the owner</text>"),
        "a line each"
    );
    assert!(out.contains(">Pool</text>"));
    // Well-formed: each text closed, no bare `&`.
    assert_eq!(out.matches("<text ").count(), out.matches("</text>").count());
    assert!(
        out.match_indices('&').all(|(i, _)| ["&amp;", "&lt;", "&gt;", "&quot;"]
            .iter()
            .any(|e| out[i..].starts_with(e)))
    );
    assert!(out.ends_with("</svg>\n"));
}

/// Its texts are where the view puts them: the same lines at scale 1.
#[test]
fn the_svg_s_texts_break_as_the_view_s() {
    let d = flow();
    let mut text = TextSystem::new();
    let t = d.texts.iter().find(|t| t.text.starts_with("Save")).unwrap();
    let (lines, _) = crate::texts::lay(&mut text, &family(), t, t.area, 1.0);
    assert_eq!(
        lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
        ["Save & notify", "the owner"]
    );
}

/// A PNG of the page: a real PNG, its size at the scale (margin included), white where
/// nothing is, darker where the shapes' lines and texts are.
#[test]
fn a_page_as_png() {
    let d = flow();
    let mut text = TextSystem::new();
    let bytes = png(&d, None, &mut text, &family(), 1.0).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let pixmap = tiny_skia::Pixmap::decode_png(&bytes).unwrap();
    let (w, h) = (pixmap.width(), pixmap.height());
    assert_eq!(w, (d.bounds.size.width + 2.0 * MARGIN).ceil() as u32);
    assert_eq!(h, (d.bounds.size.height + 2.0 * MARGIN).ceil() as u32);
    let at = |x: u32, y: u32| pixmap.pixel(x, y).unwrap();
    let corner = at(1, 1);
    assert_eq!((corner.red(), corner.green(), corner.blue(), corner.alpha()), (255, 255, 255, 255));
    let dark = pixmap.pixels().iter().filter(|p| p.red() < 128).count();
    assert!(dark > 500, "lines and texts drawn: {dark}");
}

/// A page too big for the scale: exported smaller, its largest side within the limit.
#[test]
fn a_huge_page_is_exported_smaller() {
    let mut d = flow();
    d.bounds.size.width = 20_000.0;
    let mut text = TextSystem::new();
    let bytes = png(&d, None, &mut text, &family(), PNG_SCALE).unwrap();
    let pixmap = tiny_skia::Pixmap::decode_png(&bytes).unwrap();
    assert!(pixmap.width() <= MAX_SIDE as u32);
    // Far past any scale: still within the limit, never a huge image.
    d.bounds.size.width = 1e9;
    let bytes = png(&d, None, &mut text, &family(), PNG_SCALE).unwrap();
    assert!(tiny_skia::Pixmap::decode_png(&bytes).unwrap().width() <= MAX_SIDE as u32);
    d.bounds.size.width = f32::INFINITY;
    assert!(png(&d, None, &mut text, &family(), PNG_SCALE).is_err());
}

/// What XML can't hold is escaped or left out.
#[test]
fn texts_are_escaped() {
    assert_eq!(escape("a<b & \"c\">\u{1}"), "a&lt;b &amp; &quot;c&quot;&gt;");
}

/// The page's exports, to look at (`EDITOR_DIAGRAM_SHOTS=<dir>`).
#[test]
fn shots() {
    let Some(dir) = std::env::var_os("EDITOR_DIAGRAM_SHOTS") else { return };
    let dir = std::path::PathBuf::from(dir);
    let d = flow();
    let mut text = TextSystem::new();
    std::fs::write(dir.join("flow.svg"), svg(&d, None, &mut text, &family())).unwrap();
    std::fs::write(dir.join("flow.png"), png(&d, None, &mut text, &family(), PNG_SCALE).unwrap())
        .unwrap();
}
