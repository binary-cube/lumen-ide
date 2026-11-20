use super::*;

/// A part that keeps nothing of its own: hit everywhere, it counts its presses' clicks.
#[derive(Default)]
struct Plain {
    clicks: u32,
}

impl Control for Plain {
    fn hit(&self, _p: Point) -> bool {
        true
    }
    fn press(&mut self, _p: Point, clicks: u32) -> Press {
        self.clicks = clicks;
        Press::Took
    }
}

/// Tuned, it keeps Tab and the wheel; untuned it's the part itself (its presses as they
/// come).
#[test]
fn a_tuned_part_keeps_tab_and_the_wheel() {
    let mut plain = Plain::default();
    let at = Point::new(0.0, 0.0);
    let tune = Tune { keeps_tab: true, keeps_wheel: true };
    let tuned = Tuned { part: &mut plain, tune };
    assert!(tuned.keeps_tab());
    assert!(tuned.takes_wheel(at), "the wheel is its own");
    let mut as_is = Tuned { part: &mut plain, tune: Tune::default() };
    assert!(!as_is.keeps_tab() && !as_is.takes_wheel(at) && !as_is.scroll(0.0, 10.0));
    assert_eq!(as_is.cursor(at), None);
    as_is.press(at, 2);
    assert_eq!(plain.clicks, 2);
}
