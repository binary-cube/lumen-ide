use super::super::tests::{scene_of, texts};

#[test]
fn both_sides_of_the_root() {
    let scene = scene_of(
        "mindmap\n  root((Ideas))\n    A\n      A1\n    B[Square]\n    C(Round)\n    D))Bang((\n      D1)Cloud(",
    );
    let t = texts(&scene);
    for w in ["Ideas", "A", "A1", "Square", "Round", "Bang", "Cloud"] {
        assert!(t.contains(&w), "{w}: {t:?}");
    }
    assert!(scene.width > 300.0);
}

#[test]
fn a_small_map_is_radial_around_its_root() {
    let scene = scene_of(
        "mindmap\n  Root\n    A\n      A1\n      A2\n    B\n      B1\n    C\n    D\n      D1",
    );
    let at = |name: &str| {
        scene
            .prims
            .iter()
            .find_map(|p| match p {
                super::super::scene::Prim::Text(t) if t.text == name => Some(t.at),
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name}"))
    };
    let root = at("Root");
    // Branches all around: some above the root, some below, some left, some right.
    let others = ["A", "B", "C", "D"].map(at);
    assert!(
        others.iter().any(|p| p.y < root.y - 10.0) && others.iter().any(|p| p.y > root.y + 10.0)
    );
    assert!(
        others.iter().any(|p| p.x < root.x - 10.0) && others.iter().any(|p| p.x > root.x + 10.0)
    );
}
