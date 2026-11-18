use super::*;

fn commit(id: &str, author: &str, time: i64) -> Commit {
    Commit { id: id.into(), author: author.into(), time, summary: String::new() }
}

#[test]
fn days_go_both_ways() {
    for day in [0, 1, 59, 60, 365, 10_957, 19_723, 20_454] {
        let start = day * 86_400;
        let (y, m, d) = crate::date(start);
        assert_eq!(day_start(y, m, d), start, "{y}-{m}-{d}");
    }
    assert_eq!(parse_day("2026-01-03"), Some(day_start(2026, 1, 3)));
    assert_eq!(parse_day(" 2024-2-29 "), Some(day_start(2024, 2, 29)));
    assert_eq!(parse_day("2025-02-29"), None, "not a leap year");
    assert_eq!(parse_day("2025-13-01"), None);
    assert_eq!(parse_day("yesterday"), None);
}

#[test]
fn text_matches_the_message_or_the_hash() {
    let plain = TextFilter { text: "Fix".into(), ..TextFilter::default() };
    let m = TextMatcher::new(&plain).unwrap().unwrap();
    assert!(m.matches("abc", "a fix for it"), "case ignored");
    let case = TextFilter { match_case: true, ..plain };
    let m = TextMatcher::new(&case).unwrap().unwrap();
    assert!(!m.matches("abc", "a fix for it") && m.matches("abc", "Fix it"));
    let hash = TextFilter { text: "B9224".into(), ..TextFilter::default() };
    let m = TextMatcher::new(&hash).unwrap().unwrap();
    assert!(m.matches("b92241067c", "Login"));
    let regex = TextFilter { text: "^(Login|Logout)$".into(), regex: true, match_case: false };
    let m = TextMatcher::new(&regex).unwrap().unwrap();
    assert!(m.matches("x", "login") && !m.matches("x", "login page"));
    let dots = TextFilter { text: "a.b".into(), ..TextFilter::default() };
    let m = TextMatcher::new(&dots).unwrap().unwrap();
    assert!(!m.matches("x", "axb"), "plain text is plain");
    let bad = TextFilter { text: "(".into(), regex: true, match_case: false };
    assert!(TextMatcher::new(&bad).is_err());
    assert!(TextMatcher::new(&TextFilter::default()).unwrap().is_none());
}

#[test]
fn the_filters_combine() {
    let filter = LogFilter {
        authors: vec!["Ana".into(), "bob@x.ro".into()],
        since: Some(100),
        until: Some(200),
        ..LogFilter::default()
    };
    assert!(!filter.keeps_every_commit() && !filter.is_empty());
    assert!(filter.matches(None, &commit("a", "ana", 150), "", ""));
    assert!(filter.matches(None, &commit("a", "Bob", 150), "BOB@x.ro", ""));
    assert!(!filter.matches(None, &commit("a", "Eve", 150), "eve@x.ro", ""));
    assert!(!filter.matches(None, &commit("a", "Ana", 99), "", ""));
    assert!(!filter.matches(None, &commit("a", "Ana", 200), "", ""));
    let text = TextMatcher::new(&TextFilter { text: "cart".into(), ..TextFilter::default() })
        .unwrap()
        .unwrap();
    assert!(!filter.matches(Some(&text), &commit("a", "Ana", 150), "", "shop"));
    let only_branch = LogFilter { references: vec!["main".into()], ..LogFilter::default() };
    assert!(only_branch.keeps_every_commit() && !only_branch.is_empty());
    assert!(LogFilter::default().is_empty());
}
