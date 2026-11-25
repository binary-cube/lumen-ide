use std::time::Instant;

use super::{ACCESS_EVERY, AccessState, Due};

/// A tree is told when it changed, at once the first time, then at most every
/// [`ACCESS_EVERY`] (the changes between told together, when due); unchanged, never.
#[test]
fn a_tree_is_told_at_most_every_so_often() {
    let mut state = AccessState::default();
    let now = Instant::now();
    assert!(matches!(state.due(now), Due::Nothing), "nothing changed");
    state.changed();
    assert!(matches!(state.due(now), Due::Now(_)), "the first: at once");
    assert!(matches!(state.due(now), Due::Nothing), "told: nothing left");
    state.changed();
    let soon = now + ACCESS_EVERY / 2;
    assert!(matches!(state.due(soon), Due::At(at) if at == now + ACCESS_EVERY), "held back");
    state.changed();
    assert!(matches!(state.due(now + ACCESS_EVERY), Due::Now(_)), "due: told once");
    assert!(matches!(state.due(now + ACCESS_EVERY * 3), Due::Nothing));
}
