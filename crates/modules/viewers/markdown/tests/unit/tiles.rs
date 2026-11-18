use kernel::{Point, Rgba};

use super::*;
use crate::mermaid::scene::{self, Line};

fn sample() -> Scene {
    let mut s = Scene { width: 300.0, height: 200.0, ..Scene::default() };
    s.rect(
        Rect::new(10.0, 10.0, 120.0, 80.0),
        8.0,
        Some(Rgba::rgb(0xdd3322)),
        Some(Line::solid(Rgba::rgb(0x112233), 2.0)),
    );
    s.path(
        scene::ellipse(Rect::new(80.0, 40.0, 100.0, 100.0)),
        Some(Rgba::rgb(0x3355ee).with_alpha(0.8)),
        None,
    );
    s.polyline(
        &[Point::new(0.0, 150.0), Point::new(290.0, 180.0)],
        Line::dashed(Rgba::rgb(0x22aa44), 3.0),
    );
    s
}

/// The masks drawn in order over `background`, as the renderer does.
fn composite(masks: &[Mask], w: u32, h: u32, background: [f32; 3]) -> Vec<[f32; 3]> {
    let mut out = vec![background; (w * h) as usize];
    for (image, r) in masks {
        let color = image.tint.expect("a mask");
        image.with_pixels(|cov| {
            for y in 0..image.height {
                for x in 0..image.width {
                    let a = f32::from(cov[(y * image.width + x) as usize]) / 255.0;
                    let (px, py) = (r.min_x() as u32 + x, r.min_y() as u32 + y);
                    let o = &mut out[(py * w + px) as usize];
                    for (c, v) in [(0, color.r), (1, color.g), (2, color.b)] {
                        o[c] = v * a + o[c] * (1.0 - a);
                    }
                }
            }
        });
    }
    out
}

#[test]
fn masks_draw_what_painting_does_in_a_quarter_of_the_memory() {
    let scene = sample();
    let k = 2.0;
    let masks = all(&scene, k);
    let (w, h) = ((scene.width * k) as u32, (scene.height * k) as u32);
    // The same shapes painted directly (RGBA), over black.
    let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("pixmap");
    pixmap.fill(tiny_skia::Color::BLACK);
    let mut s = scene;
    s.prims.retain(|p| matches!(p, Prim::Path { .. }));
    let png = scene::picture(&s, k, Rgba::BLACK, &mut NoText).expect("picture").2;
    let mine = composite(&masks, w, h, [0.0, 0.0, 0.0]);
    let mut worst = 0.0f32;
    for i in 0..(w * h) as usize {
        for c in 0..3 {
            worst = worst.max((mine[i][c] - f32::from(png[i * 4 + c]) / 255.0).abs());
        }
    }
    assert!(worst < 4.0 / 255.0, "off by {worst}");
    let bytes: usize = masks.iter().map(|(i, _)| (i.width * i.height) as usize).sum();
    assert!(
        bytes * 4 < (w * h * 4) as usize * 2,
        "{bytes} bytes of masks for {} of RGBA",
        w * h * 4
    );
    let _ = pixmap;
}

#[test]
fn empty_tiles_cost_nothing() {
    let mut s = Scene { width: 2000.0, height: 2000.0, ..Scene::default() };
    s.rect(Rect::new(10.0, 10.0, 50.0, 50.0), 0.0, Some(Rgba::WHITE), None);
    assert!(!tile(&s, 1.0, 0, 0).is_empty());
    assert!(tile(&s, 1.0, 3, 3).is_empty());
    assert_eq!(grid(&s, 1.0), (4, 4));
}

struct NoText;

impl scene::GlyphPainter for NoText {
    fn glyphs(&mut self, _: &scene::Text, _: f32) -> scene::ShapedText {
        scene::ShapedText { width: 0.0, ascent: 0.0, descent: 0.0, glyphs: Vec::new() }
    }
}

#[test]
fn a_box_over_a_line_hides_it() {
    let mut s = Scene { width: 100.0, height: 100.0, ..Scene::default() };
    s.polyline(
        &[Point::new(50.0, 0.0), Point::new(50.0, 100.0)],
        Line::solid(Rgba::rgb(0xaaaaaa), 2.0),
    );
    s.rect(
        Rect::new(20.0, 40.0, 60.0, 20.0),
        3.0,
        Some(Rgba::rgb(0x202020).with_alpha(0.92)),
        None,
    );
    let masks = all(&s, 1.0);
    let mine = composite(&masks, 100, 100, [0.125, 0.125, 0.125]);
    let at = mine[(50 * 100 + 50) as usize];
    assert!(
        (at[0] - 0.125).abs() < 0.08,
        "{at:?} {:?}",
        masks.iter().map(|(i, r)| (i.tint, *r)).collect::<Vec<_>>()
    );
}

#[test]
fn a_flowchart_label_hides_its_line() {
    let scene = crate::mermaid::tests::scene_of(
        "flowchart TD\n A[Christmas] -->|Get money| B(Go shopping)",
    );
    let label = scene
        .prims
        .iter()
        .find_map(|p| match p {
            Prim::Text(t) if t.text == "Get money" => Some(t.at),
            _ => None,
        })
        .expect("label");
    for k in [2.0f32, 1.37, 1.0] {
        let (w, h) = ((scene.width * k).ceil() as u32, (scene.height * k).ceil() as u32);
        let masks = all(&scene, k);
        let bg = crate::mermaid::tests::look().background;
        let mine = composite(&masks, w, h, [bg.r, bg.g, bg.b]);
        // Just left of the text's start, on the line's x.
        let (x, y) = ((label.x * k) as u32, (label.y * k) as u32);
        let at = mine[(y * w + x) as usize];
        assert!(
            (at[0] - bg.r).abs() < 0.08,
            "{k}: {at:?} vs {bg:?}; masks {:?}",
            masks.iter().map(|(i, r)| (i.tint, *r)).collect::<Vec<_>>()
        );
    }
}

/// Masks against a whole RGBA image, for real diagrams (run with `--ignored --nocapture`).
#[test]
#[ignore]
fn measure_masks_against_rgba() {
    let sources = [
        "flowchart TD\n  A[Start] --> B{Check}\n  B -->|yes| C[Do it]\n  B -->|no| D[(Store)]\n  C --> E((End))\n  D --> E\n  subgraph S\n    F --> G --> H\n  end\n  E --> F",
        "sequenceDiagram\n  actor U\n  U->>+W: GET\n  W->>DB: SELECT\n  DB-->>W: rows\n  W-->>-U: 200\n  loop every 5s\n    U-)W: ping\n  end",
        "classDiagram\n  Animal <|-- Duck\n  Animal <|-- Fish\n  class Duck {\n    +swim()\n  }",
        "pie title P\n  \"A\" : 40\n  \"B\" : 30\n  \"C\" : 30",
    ];
    for source in sources {
        let scene = crate::mermaid::tests::scene_of(source);
        let k = 2.0;
        let rgba = (scene.width * k).ceil() as u64 * (scene.height * k).ceil() as u64 * 4;
        let masks: u64 = all(&scene, k).iter().map(|(i, _)| u64::from(i.width * i.height)).sum();
        eprintln!(
            "MASKS {}: RGBA {} KB, masks {} KB ({:.0}%)",
            source.lines().next().unwrap_or(""),
            rgba / 1024,
            masks / 1024,
            masks as f64 * 100.0 / rgba as f64
        );
    }
}
