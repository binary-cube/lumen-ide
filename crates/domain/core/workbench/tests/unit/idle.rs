use super::*;

const FIVE: Duration = Duration::from_secs(300);

#[test]
#[allow(clippy::disallowed_methods)] // The test's own starting point in time.
fn it_comes_once_after_the_time_without_input_and_input_starts_again() {
    let start = Instant::now();
    let mut watch = IdleWatch::new(start);
    assert_eq!(watch.deadline(FIVE), Some(start + FIVE));
    assert!(!watch.take_due(start + Duration::from_secs(299), FIVE), "not yet");
    assert!(watch.take_due(start + FIVE, FIVE), "due");
    assert!(!watch.take_due(start + FIVE * 2, FIVE), "once a spell");
    assert_eq!(watch.deadline(FIVE), None, "nothing more to wait for");
    // Input: a new spell, counted from it.
    let back = start + FIVE * 3;
    watch.input(back);
    assert_eq!(watch.deadline(FIVE), Some(back + FIVE));
    assert!(!watch.take_due(back + Duration::from_secs(60), FIVE));
    // Input meanwhile pushes it further.
    watch.input(back + Duration::from_secs(200));
    assert!(!watch.take_due(back + FIVE, FIVE));
    assert!(watch.take_due(back + Duration::from_secs(200) + FIVE, FIVE));
}

#[test]
#[allow(clippy::disallowed_methods)]
fn activity_in_another_program_counts_once_and_only_when_newer() {
    let start = Instant::now();
    let mut watch = IdleWatch::new(start);
    let elsewhere = start + Duration::from_secs(200);
    watch.active_at(elsewhere);
    assert_eq!(watch.deadline(FIVE), Some(elsewhere + FIVE), "the wait starts from it");
    assert!(watch.take_due(elsewhere + FIVE, FIVE));
    watch.active_at(elsewhere);
    assert_eq!(watch.deadline(FIVE), None, "the same moment read again: nothing starts over");
}
