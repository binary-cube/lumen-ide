use super::*;

struct Twice(u32);
impl Request for Twice {
    type Answer = u32;
}

struct Unanswered;
impl Request for Unanswered {
    type Answer = ();
}

#[test]
#[allow(clippy::disallowed_methods)] // a test's clock
fn the_book_holds_one_answer_per_request() {
    let answers = [Answers::of(|q: Twice, _| q.0 * 2), Answers::of(|_: Twice, _| 0)];
    let book = AnswerBook::new(answers, Instant::now, |_, _| {});
    assert!(book.has::<Twice>());
    assert!(!book.has::<Unanswered>());
    assert_eq!(book.answers.len(), 1, "the second one is refused");
    assert_eq!(book.twice().len(), 1);
}
