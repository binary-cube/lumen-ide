use super::*;

const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20"><rect width="100%" height="100%" fill="#ffffff"/><rect x="5" y="5" width="10" height="10" fill="#5b9bf8"/></svg>"##;

/// An `.svg` is written as it is, anything else as a PNG; no temporary file is left.
#[test]
fn a_chart_is_saved_as_svg_or_png() {
    let dir = std::env::temp_dir().join(format!("chart-export-{:032x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let svg = dir.join("chart.svg");
    assert_eq!(write(&svg, SVG), Ok("chart.svg".to_owned()));
    assert_eq!(std::fs::read_to_string(&svg).expect("read"), SVG);
    let png = dir.join("chart.png");
    assert_eq!(write(&png, SVG), Ok("chart.png".to_owned()));
    let bytes = std::fs::read(&png).expect("read");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    assert_eq!(std::fs::read_dir(&dir).expect("listed").count(), 2);
    let _ = std::fs::remove_dir_all(&dir);
}
