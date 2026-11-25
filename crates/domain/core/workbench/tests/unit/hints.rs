use super::*;

const RUN: CommandId = CommandId("test.run");

fn step(anchor: Option<&'static str>, actions: Vec<HintAction>) -> HintStep {
    HintStep {
        icon: None,
        eyebrow: None,
        title: "hint.title",
        body: "hint.body",
        place: None,
        anchor,
        actions,
    }
}

/// One step: "Don't show again", then its actions; ↵ presses the main one; the keys move
/// through them, wrapping.
#[test]
fn a_hint_s_buttons_and_its_main_one() {
    let hint = Hint {
        id: "test.one",
        steps: vec![step(
            Some("editor.active-tab"),
            vec![
                HintAction::secondary("hint.settings", CommandId("test.settings")),
                HintAction::primary("hint.on", RUN),
            ],
        )],
    };
    let mut tour = HintTour::new(hint);
    assert_eq!(tour.counter(), None, "not a tour");
    assert_eq!(tour.buttons(), [HintButton::Dismiss, HintButton::Action(0), HintButton::Action(1)]);
    assert!(tour.is_main(HintButton::Action(1)));
    assert_eq!(tour.enter(), HintButton::Action(1));
    tour.move_focus(1);
    assert_eq!(tour.focus, Some(2), "first the main one");
    tour.move_focus(1);
    assert_eq!(tour.enter(), HintButton::Dismiss, "wrapped");
    tour.move_focus(-1);
    assert_eq!(tour.enter(), HintButton::Action(1));
    assert_eq!(
        tour.press(HintButton::Action(1)),
        HintOutcome::Close { command: Some(RUN), seen: true }
    );
    assert_eq!(tour.press(HintButton::Dismiss), HintOutcome::Close { command: None, seen: true });
}

/// A tour: Back past the first step, Next until the last, Done there; the counter says
/// where; moving drops the keys' focus.
#[test]
fn a_tour_goes_back_and_forth() {
    let hint = Hint { id: "test.tour", steps: vec![step(None, vec![]); 3] };
    let mut tour = HintTour::new(hint);
    assert_eq!(tour.counter(), Some((1, 3)));
    assert_eq!(tour.buttons(), [HintButton::Dismiss, HintButton::Next]);
    tour.move_focus(1);
    assert_eq!(tour.press(HintButton::Next), HintOutcome::Stay);
    assert_eq!((tour.counter(), tour.focus), (Some((2, 3)), None));
    assert_eq!(tour.buttons(), [HintButton::Dismiss, HintButton::Back, HintButton::Next]);
    tour.press(HintButton::Next);
    assert_eq!(tour.buttons(), [HintButton::Dismiss, HintButton::Back, HintButton::Done]);
    assert_eq!(tour.enter(), HintButton::Done);
    tour.press(HintButton::Back);
    assert_eq!(tour.counter(), Some((2, 3)));
    assert_eq!(tour.press(HintButton::Done), HintOutcome::Close { command: None, seen: true });
}

/// Seen for good: never again; closed: not again this run; the settings saved only when
/// what's seen changed.
#[test]
fn seen_hints_are_never_shown_again() {
    let mut book = HintBook::default();
    assert!(book.may_show("a"));
    assert!(!book.close("a", false), "closed for now: nothing to save");
    assert!(!book.may_show("a"), "not again this run");
    let mut next_run = HintBook { seen: book.seen.clone(), closed: Vec::new() };
    assert!(next_run.may_show("a"), "the next run may");
    assert!(next_run.close("a", true));
    assert!(!next_run.close("a", true), "already seen");
    assert_eq!(next_run.seen, ["a"]);
    let later = HintBook { seen: next_run.seen, closed: Vec::new() };
    assert!(!later.may_show("a"));
}
