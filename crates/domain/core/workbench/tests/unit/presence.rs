use super::*;

struct Answered;
impl Request for Answered {
    type Answer = ();
}
struct Unanswered;
impl Request for Unanswered {
    type Answer = ();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test's clock
fn it_says_who_answers_once_told_and_assumes_everyone_before() {
    let open = CommandId("x.open");
    assert!(command(open) && request::<Unanswered>() && view("view-x"), "not told yet");
    let book = AnswerBook::new(
        [crate::Answers::of(|_: Answered, _| ())],
        std::time::Instant::now,
        |_, _| {},
    );
    set([open], std::sync::Arc::new(book), |extension| extension == "view-x");
    assert!(command(open) && !command(CommandId("y.open")));
    assert!(request::<Answered>() && !request::<Unanswered>());
    assert!(view("view-x") && !view("view-y"));
    *PRESENT.write().unwrap() = None;
}

#[test]
fn the_opener_is_unknown_until_told() {
    assert_eq!(opener("drawio"), None);
    set_opener(|extension| (extension == "drawio").then_some("diagram-web"));
    assert_eq!(opener("drawio"), Some("diagram-web"));
    assert_eq!(opener("png"), None);
    *OPENER.write().unwrap() = None;
}
