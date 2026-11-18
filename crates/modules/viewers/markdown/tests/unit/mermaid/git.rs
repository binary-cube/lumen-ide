use super::super::tests::{scene_of, texts};

#[test]
fn branches_commits_and_tags() {
    let scene = scene_of(
        "gitGraph\n  commit id: \"init\"\n  branch develop\n  commit tag: \"v0.1\"\n  checkout main\n  merge develop\n  commit type: REVERSE",
    );
    let t = texts(&scene);
    for w in ["main", "develop", "init", "v0.1"] {
        assert!(t.contains(&w), "{w}: {t:?}");
    }
    let tb = scene_of("gitGraph TB:\n  commit\n  branch x\n  commit");
    assert!(tb.height > tb.width * 0.5);
}
