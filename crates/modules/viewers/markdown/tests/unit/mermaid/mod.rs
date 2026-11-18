use ui_components::Palette;

use super::scene::Prim;
use super::*;

pub(crate) fn look() -> Look {
    Look::of(&Palette::default())
}

/// Lays out `source` with a real text system.
pub(crate) fn scene_of(source: &str) -> Scene {
    let parsed = mermaid::parse(source).expect("parsed");
    let mut text = TextSystem::new();
    let look = look();
    let mut m = Measurer { text: &mut text, family: look.font.clone() };
    let mut scene = layout(&parsed.diagram, &look, &mut m);
    if let Some(title) = &parsed.meta.title {
        titled(&mut scene, title, &look, &mut m);
    }
    scene
}

pub(crate) fn texts(scene: &Scene) -> Vec<&str> {
    scene
        .prims
        .iter()
        .filter_map(|p| match p {
            Prim::Text(t) => Some(t.text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn rendering_gives_a_scene_and_its_image() {
    let r = render("graph LR\nA[Start] --> B{Ok?}\nB -->|yes| C[Done]", &look())
        .unwrap_or_else(|_| panic!("drawn"));
    assert!(r.scene.width > 100.0 && r.scene.height > 30.0);
    let masks = crate::tiles::all(&r.scene, 2.0);
    assert!(!masks.is_empty());
    let t = texts(&r.scene);
    for word in ["Start", "Ok?", "yes", "Done"] {
        assert!(t.contains(&word), "{t:?}");
    }
}

#[test]
fn errors_come_back_with_their_line() {
    match render("graph TD\nA-->\n", &look()) {
        Err(Failure::Error(e)) => assert_eq!(e.line, 2),
        _ => panic!("an error"),
    }
    let text = describe(&Error::new(3, ErrorKind::Unexpected("x".into())));
    assert!(text.contains('3') && text.contains('x'), "{text}");
}

#[test]
fn a_big_flowchart_is_laid_out_quickly() {
    let mut source = String::from("flowchart TD\n");
    for i in 0..200 {
        source.push_str(&format!("  n{i}[Node number {i}] --> n{}\n", (i * 7 + 3) % 200));
        if i % 3 == 0 {
            source.push_str(&format!("  n{i} -->|label {i}| n{}\n", (i + 1) % 200));
        }
    }
    let started = std::time::Instant::now();
    let r = render(&source, &look()).unwrap_or_else(|_| panic!("drawn"));
    let (cols, rows) = crate::tiles::grid(&r.scene, 2.0);
    let masks: usize = (0..rows)
        .flat_map(|y| (0..cols).map(move |x| (x, y)))
        .map(|(x, y)| crate::tiles::tile(&r.scene, 2.0, x, y).len())
        .sum();
    assert!(masks > 0);
    let took = started.elapsed();
    eprintln!("200 nodes, 267 edges: {took:?}, {} x {}", r.scene.width, r.scene.height);
    // Alone ~2 s; generous, for a machine busy with every other test.
    assert!(took.as_secs_f32() < 60.0);
}
