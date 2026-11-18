use super::*;
use workbench_model::Follow;

#[test]
fn off_asks_nothing_on_asks_as_the_settings_say_with_the_effects_still() {
    let config = Config { gray: 0.75, dim: 0.1, follow: Follow::Keyboard };
    assert_eq!(request(false, &config), None);
    let asked = request(true, &config).unwrap();
    assert_eq!((asked.gray, asked.dim, asked.follow), (0.75, 0.1, Follow::Keyboard));
    assert!(asked.pause_effects);
}

#[test]
fn the_contribution_follows_the_state() {
    crate::state::set(false);
    assert!(FocusEmphasis.request().is_none());
    crate::state::set(true);
    assert!(FocusEmphasis.request().is_some_and(|r| r.gray > 0.0));
    crate::state::set(false);
}
