use super::*;

/// How lines were broken before: each word added and the line shaped again.
fn by_words(text: &mut TextSystem, font: &Font, paragraph: &str, room: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in paragraph.split(' ') {
        let candidate =
            if current.is_empty() { word.to_owned() } else { format!("{current} {word}") };
        if !current.is_empty() && text.shape_line(&candidate, font).width > room {
            lines.push(std::mem::replace(&mut current, word.to_owned()));
        } else {
            current = candidate;
        }
    }
    lines.push(current);
    lines
}

/// One shaping a paragraph breaks its lines exactly where shaping each line did — at every
/// width, size and kind of text (two spaces, a long word, accents, emoji, right to left).
#[test]
fn lines_break_as_before() {
    let mut text = TextSystem::new();
    let paragraphs = [
        "Save & notify the owner of the order",
        "Shape number 1234 with a longer label",
        "two  spaces and   three",
        "Supercalifragilisticexpialidocious word",
        "Ştefan's note: ünïcödé ✓ — ăîâșț ĂÎÂȘȚ",
        "emoji 🚀 in 🎉 the ✨ middle",
        "مرحبا بالعالم هذا نص",
        "",
        " leading and trailing ",
    ];
    for size in [8.0, 12.0, 17.5, 40.0] {
        let font = Font::new(FontFamily::SansSerif, size);
        for paragraph in paragraphs {
            for room in (5..60).map(|r| r as f32 * size * 0.37) {
                let mut lines = Vec::new();
                wrap(&mut text, &font, paragraph, room, &mut lines);
                assert_eq!(
                    lines,
                    by_words(&mut text, &font, paragraph, room),
                    "{paragraph:?} at {size} in {room}"
                );
            }
        }
    }
}

/// 2 000 labels laid out at a new size, word by word and from one shaping (plan 80's
/// measure: `--release -- --ignored --nocapture`).
#[test]
#[ignore]
fn measure_wrapping() {
    let mut text = TextSystem::new();
    let labels: Vec<String> =
        (0..2000).map(|i| format!("Shape number {i} with a longer label to wrap")).collect();
    for (round, size) in [(0, 12.0f32), (1, 12.5)] {
        let font = Font::new(FontFamily::SansSerif, size);
        let started = std::time::Instant::now();
        for l in &labels {
            std::hint::black_box(by_words(&mut text, &font, l, size * 8.0));
        }
        let old = started.elapsed();
        let font = Font::new(FontFamily::SansSerif, size + 0.25);
        let started = std::time::Instant::now();
        for l in &labels {
            let mut lines = Vec::new();
            wrap(&mut text, &font, l, (size + 0.25) * 8.0, &mut lines);
            std::hint::black_box(lines);
        }
        eprintln!(
            "MEASURE round {round}: word by word {old:?}, one shaping {:?}",
            started.elapsed()
        );
    }
}

/// A long label (a hostile file's) wraps in time linear in its length: its glyphs found by
/// halving, not scanned from the start for each word.
#[test]
fn a_long_label_wraps_quickly() {
    let mut text = TextSystem::new();
    let font = Font::new(FontFamily::SansSerif, 12.0);
    let label = "word ".repeat(12_000);
    let started = std::time::Instant::now();
    let mut lines = Vec::new();
    wrap(&mut text, &font, &label, 60.0, &mut lines);
    assert!(lines.len() > 1000, "{}", lines.len());
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
}

/// Laid out at a step near the scale shown, a right-aligned line still ends at its box's
/// right side and a centered one sits in its middle (the glyphs aren't stretched: their
/// side is).
#[test]
fn a_stretched_line_keeps_its_alignment() {
    use crate::look::{Align, VAlign};
    let mut text = TextSystem::new();
    let family = FontFamily::SansSerif;
    for align in [Align::Right, Align::Center] {
        let t = Text {
            area: Rect::new(0.0, 0.0, 200.0, 40.0),
            text: "Aligned".into(),
            size: 12.0,
            color: kernel::Rgba::BLACK,
            bold: false,
            italic: false,
            align,
            valign: VAlign::Middle,
            wrap: false,
            backdrop: None,
            reach: (1.0, 1.0),
        };
        let (k, kq) = (1.03f32, 1.0f32);
        let (lines, line_h) = lay(&mut text, &family, &t, t.area, kq);
        let mut layer = render_api::Layer::default();
        draw(&mut layer, &lines, line_h, &t, Point::new(0.0, 0.0), (kq, k / kq));
        let shaped = &lines[0].shaped;
        let x = layer.glyphs[0].origin.x - shaped.glyphs[0].x;
        let want = match align {
            Align::Right => 200.0 * k - 2.0 * k - shaped.width,
            _ => 100.0 * k - shaped.width / 2.0,
        };
        assert!((x - want).abs() < 1.5, "{align:?}: {x} vs {want}");
    }
}
