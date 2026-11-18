use super::*;

fn found(language: &str, line: &str) -> Vec<(&'static str, String)> {
    Colors
        .find(language, line)
        .into_iter()
        .map(|l| {
            let kind = match l.format {
                ColorFormat::Hex => "hex",
                ColorFormat::Rgb => "rgb",
                ColorFormat::Hsl => "hsl",
                ColorFormat::Named => "name",
            };
            (kind, line[l.range].to_owned())
        })
        .collect()
}

#[test]
fn finds_colors_in_context() {
    assert_eq!(found("css", "  background: red;"), [("name", "red".to_owned())]);
    assert_eq!(found("css", "  border: 1px solid #e5484d;"), [("hex", "#e5484d".to_owned())]);
    assert_eq!(
        found("css", "#fade { color: rgba(0, 0, 0, .5) }"),
        [("rgb", "rgba(0, 0, 0, .5)".to_owned())]
    );
    assert_eq!(
        found("php", r#"echo "<div style=\"color: red ; \">";"#),
        [("name", "red".to_owned())]
    );
    // An arbitrary word is not a color; nor is an HTML entity or an anchor.
    assert!(found("php", "$red = 'red'; // &#123; page#abc").is_empty());
    assert!(found("css", ".red-text { color: inherit }").is_empty());
    assert!(found("python", "#fff").is_empty());
}

#[test]
fn round_trips() {
    let c = Colors.parse("hsl(358, 75%, 59%)").unwrap();
    assert_eq!(format_color(c.color, ColorFormat::Hex), "#e5484d");
    let c = Colors.parse("#ff000080").unwrap();
    assert_eq!(format_color(c.color, ColorFormat::Rgb), "rgba(255, 0, 0, 0.5)");
    assert_eq!(format_color(Rgba::rgb(0xff0000), ColorFormat::Named), "red");
    assert_eq!(Colors.parse("RebeccaPurple").map(|c| c.format), Some(ColorFormat::Named));
}
