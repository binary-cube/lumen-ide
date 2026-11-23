use super::*;

/// Aliases are counted as what they name, without expanding them; a small file nesting them
/// past the budget is refused (no table) at once, never loaded.
#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn nested_yaml_aliases_are_counted_and_refused_past_the_budget() {
    assert_eq!(expanded_yaml_nodes("a: &x [1, 2]\nb: *x\n"), 1 + 1 + 3 + 1 + 3);
    let mut bomb = String::from("a0: &a0 [x, x, x, x, x, x, x, x, x]\n");
    for i in 1..10 {
        let p = i - 1;
        bomb.push_str(&format!(
            "a{i}: &a{i} [*a{p}, *a{p}, *a{p}, *a{p}, *a{p}, *a{p}, *a{p}, *a{p}, *a{p}]\n"
        ));
    }
    assert!(bomb.len() < 2_000);
    let started = std::time::Instant::now();
    assert!(expanded_yaml_nodes(&bomb) > MOST_YAML_NODES);
    assert!(yaml(&bomb, Limits::default()).is_empty());
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "never expanded");
    // A plain document still reads.
    assert!(!yaml("- {a: 1, b: 2}\n- {a: 3, b: 4}\n", Limits::default()).is_empty());
}
