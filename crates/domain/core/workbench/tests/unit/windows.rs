use super::*;

/// Each window keeps its number while open; a new one takes the lowest free; past nine,
/// none.
#[test]
fn numbers_are_kept_freed_and_reused() {
    let open = |ids: &[u64]| ids.iter().map(|w| (*w, None)).collect::<Vec<_>>();
    let given = numbered(&open(&[10, 20, 30]), &[], &[]);
    assert_eq!(given, [(10, 1), (20, 2), (30, 3)]);
    // The second closed: the others keep theirs; a new one takes 2.
    let given = numbered(&open(&[10, 30]), &given, &[]);
    assert_eq!(given, [(10, 1), (30, 3)]);
    let given = numbered(&open(&[10, 30, 40]), &given, &[]);
    assert_eq!(given, [(10, 1), (40, 2), (30, 3)]);
    let many: Vec<u64> = (1..=11).collect();
    assert_eq!(numbered(&open(&many), &[], &[]).len(), 9, "nine numbers at most");
}

/// A project opened takes the number it prefers when free; a number chosen swaps with the
/// project that had it, once (the next look keeps them as they are).
#[test]
fn preferred_numbers_and_choices_swap() {
    let windows = [(10, None), (20, Some(5)), (30, Some(5))];
    let given = numbered(&windows, &[], &[]);
    assert_eq!(given, [(10, 1), (30, 2), (20, 5)], "the first to prefer 5 has it");
    // 10 chooses 5: it swaps with 20 (who gets 1).
    let given = numbered(&windows, &given, &[(10, 5)]);
    assert_eq!(given, [(20, 1), (30, 2), (10, 5)]);
    assert_eq!(numbered(&windows, &given, &[]), given, "settled: no more swaps");
}
