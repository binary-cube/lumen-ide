use super::super::tests::{scene_of, texts};

#[test]
fn pie_gantt_and_timeline() {
    let pie =
        scene_of("pie showData title Pets\n  \"Dogs\" : 386\n  \"Cats\" : 85\n  \"Rats\" : 15");
    let t = texts(&pie);
    assert!(t.contains(&"Pets") && t.contains(&"Dogs [386]") && t.contains(&"79%"), "{t:?}");
    let gantt = scene_of(
        "gantt\n  title Plan\n  section A\n  First :a1, 2024-01-01, 30d\n  Second :after a1, 20d\n  Gate :milestone, 2024-03-01, 0d",
    );
    let t = texts(&gantt);
    assert!(
        t.contains(&"Plan") && t.contains(&"First") && t.contains(&"Gate") && t.contains(&"A"),
        "{t:?}"
    );
    assert!(t.iter().any(|s| s.starts_with("2024-")), "{t:?}");
    let timeline = scene_of(
        "timeline\n  title History\n  section Old\n  2002 : LinkedIn\n  2004 : Facebook : Google",
    );
    let t = texts(&timeline);
    for w in ["History", "Old", "2002", "LinkedIn", "Google"] {
        assert!(t.contains(&w), "{w}: {t:?}");
    }
}
