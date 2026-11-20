//! The diagram's filter as a control: its × isn't its, its keys a character at a time, Esc
//! empties it then gives the keys back, ↵ gives them back.

use super::*;
use ui_components::control::Controls;

#[test]
fn its_keys_go_through_the_controls_and_esc_leaves_it() {
    let mut input = TextInput::new("");
    let (mut edited, mut left) = (false, None);
    let mut controls = Controls::default();
    {
        let mut f = FilterField {
            input: &mut input,
            area: Rect::new(0.0, 0.0, 200.0, 30.0),
            clear: Rect::new(170.0, 5.0, 20.0, 20.0),
            edited: &mut edited,
            left: &mut left,
        };
        assert!(!f.hit(Point::new(180.0, 15.0)), "the × is the view's");
        let mut parts: [&mut dyn Control; 1] = [&mut f];
        assert_eq!(controls.press(&mut parts, Point::new(20.0, 15.0), 1), Some((0, Press::Held)));
        assert_eq!(controls.text(&mut parts, "ord"), Some(0));
        assert_eq!(controls.key(&mut parts, ViewerKey::Escape, KeyReach::default()), Some(0));
        assert_eq!(controls.key(&mut parts, ViewerKey::Escape, KeyReach::default()), Some(0));
        assert_eq!(controls.key(&mut parts, ViewerKey::Tab, KeyReach::default()), None);
    }
    assert_eq!(input.text(), "");
    assert!(edited);
    assert_eq!(left, Some(false), "the second Esc gives the keys back");
}
