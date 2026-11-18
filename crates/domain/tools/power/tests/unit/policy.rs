use super::*;

#[test]
fn a_hold_is_wanted_as_the_mode_says() {
    assert!(!wants_hold(Mode::Off, true));
    assert!(wants_hold(Mode::Always, false));
    assert!(wants_hold(Mode::WhileWorking, true));
    assert!(!wants_hold(Mode::WhileWorking, false), "nothing runs: the computer may sleep");
    for mode in Mode::ALL {
        assert_eq!(Mode::parse(mode.id()), Some(mode));
    }
    assert_eq!(Mode::parse("sometimes"), None);
}

#[test]
fn the_dim_comes_gently_to_its_level() {
    let care = ScreenCare { dim_after: Some(Duration::from_secs(300)), level: 0.8, drift: true };
    assert_eq!(care.dim_at(Duration::ZERO), 0.0);
    let half = care.dim_at(Duration::from_secs_f32(DIM_FADE / 2.0));
    assert!((half - 0.4).abs() < 1e-4, "{half}");
    assert!((care.dim_at(Duration::from_secs(60)) - 0.8).abs() < 1e-6);
    let early = care.dim_at(Duration::from_secs_f32(DIM_FADE / 10.0));
    assert!(early < 0.08 * 0.5, "slow at first: {early}");
}

/// The mark moves (it never stays where it was) and is always wholly inside its area.
#[test]
fn the_mark_drifts_inside_its_area() {
    let (area, item) = ((1000.0, 600.0), (120.0, 40.0));
    let mut last = drift_position(0.0, area, item);
    let mut moved = 0.0;
    for step in 1..=600 {
        let at = drift_position(step as f32, area, item);
        assert!(at.0 >= 0.0 && at.0 + item.0 <= area.0 + 1e-3, "{at:?}");
        assert!(at.1 >= 0.0 && at.1 + item.1 <= area.1 + 1e-3, "{at:?}");
        moved += (at.0 - last.0).abs() + (at.1 - last.1).abs();
        last = at;
    }
    assert!(moved > 2000.0, "it travels: {moved}");
    // An area smaller than the mark: it stays at the corner.
    assert_eq!(drift_position(42.0, (50.0, 20.0), item), (0.0, 0.0));
}
