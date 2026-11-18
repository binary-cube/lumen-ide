use super::*;

#[test]
fn a_page_holds_what_is_left_of_the_total() {
    let mut p = Paging::new(Some(500));
    assert_eq!(p.rows(Some(1234), 0), 500);
    assert_eq!(p.span(Some(1234), 0), Some((1, 500)));
    assert!(!p.can_back());
    assert!(p.can_forward(Some(1234), true));
    p.forward();
    assert_eq!(p.span(Some(1234), 0), Some((501, 1000)));
    p.last(1234);
    assert_eq!(p.start, 1000);
    assert_eq!(p.rows(Some(1234), 0), 234);
    assert_eq!(p.span(Some(1234), 0), Some((1001, 1234)));
    assert!(!p.can_forward(Some(1234), true));
    p.back();
    assert_eq!(p.start, 500);
    p.first();
    assert_eq!(p.start, 0);
    assert_eq!(p.rows(Some(0), 0), 0);
    assert_eq!(p.span(Some(0), 0), None);
}

#[test]
fn the_last_page_of_an_exact_multiple_is_full() {
    let mut p = Paging::new(Some(100));
    p.last(300);
    assert_eq!(p.start, 200);
    p.last(0);
    assert_eq!(p.start, 0);
}

#[test]
fn without_a_total_a_full_page_may_have_another() {
    let p = Paging::new(Some(10));
    assert_eq!(p.rows(None, 0), 10);
    assert!(p.can_forward(None, true));
    assert!(!p.can_forward(None, false));
}

#[test]
fn all_rows_are_one_page() {
    let mut p = Paging::new(None);
    assert_eq!(p.rows(Some(70_000), 0), 70_000);
    assert_eq!(p.rows(None, 420), 420);
    assert!(!p.can_back());
    assert!(!p.can_forward(Some(70_000), true));
    p.forward();
    assert_eq!(p.start, 0);
}

#[test]
fn another_size_keeps_the_first_row_in_view() {
    let mut p = Paging::new(Some(100));
    p.start = 700;
    p.resize(Some(500));
    assert_eq!(p.start, 500);
    p.resize(Some(10));
    assert_eq!(p.start, 500);
    p.resize(None);
    assert_eq!(p.start, 0);
    p.resize(Some(0));
    assert_eq!(p.size, Some(1));
}

#[test]
fn a_shrunk_total_brings_the_page_back() {
    let mut p = Paging::new(Some(100));
    p.start = 900;
    p.clamp(250);
    assert_eq!(p.start, 200);
    p.clamp(1000);
    assert_eq!(p.start, 200);
}
