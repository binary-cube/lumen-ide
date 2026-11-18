use super::*;

fn ticket(at_ms: u64, pid: u32, display: &str) -> Ticket {
    Ticket { at_ms, pid, display: display.into() }
}

/// A view over every window said in a line and read back: closed, opened, on the whole
/// screen (a screen's name with spaces kept); what isn't ours: nothing.
#[test]
fn what_s_over_every_window_is_told_in_a_line() {
    let plain = Over { extension: "view-relax".into(), full: None };
    let full = Over {
        extension: "view-relax".into(),
        full: Some(ticket(1700, 42, "Built-in Retina Display@0,0")),
    };
    for over in [None, Some(&plain), Some(&full)] {
        assert_eq!(parse(&message(over)), Some(over.cloned()), "{over:?}");
    }
    assert_eq!(message(None), "over\n");
    assert_eq!(message(Some(&plain)), "over view-relax\n");
    assert_eq!(parse("hello"), None);
    assert_eq!(parse("over view-relax half 1 2 x"), None, "not ours");
    assert_eq!(parse("over view-relax full x 2 d"), None, "not a time");
}

/// Of two on one screen the first stays; on two screens both do.
#[test]
fn of_two_on_one_screen_the_first_stays() {
    let first = ticket(1000, 7, "A");
    let later = ticket(1500, 3, "A");
    assert!(later.yields_to(&first) && !first.yields_to(&later));
    // At the same moment: the lower process number stays.
    let (low, high) = (ticket(1000, 3, "A"), ticket(1000, 9, "A"));
    assert!(high.yields_to(&low) && !low.yields_to(&high));
    // Another screen: neither gives way.
    let elsewhere = ticket(500, 1, "B");
    assert!(!later.yields_to(&elsewhere) && !elsewhere.yields_to(&later));
    assert!(!first.yields_to(&first), "never to itself");
}
