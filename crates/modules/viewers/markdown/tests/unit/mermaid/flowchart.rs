use super::super::tests::{scene_of, texts};

#[test]
fn every_shape_and_subgraphs_are_drawn() {
    let scene = scene_of(
        "flowchart LR\n  subgraph one [Group One]\n    a1[Rect] --> a2(Round)\n  end\n\
         subgraph two\n    direction TB\n    b1([Stadium]) --> b2[(Base)] --> b3((Circle))\n  end\n\
         one --> two\n  a2 -.->|maybe| c{Choice}\n  c ==> d{{Hex}} --o e[/Para/] --x f>Flag]\n\
         click a1 \"https://example.com\"",
    );
    let t = texts(&scene);
    for word in [
        "Group One",
        "two",
        "Rect",
        "Round",
        "Stadium",
        "Base",
        "Circle",
        "maybe",
        "Choice",
        "Hex",
        "Para",
        "Flag",
    ] {
        assert!(t.contains(&word), "{word}: {t:?}");
    }
    assert_eq!(scene.links.len(), 1);
    assert!(scene.width > 300.0);
}
