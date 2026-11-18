//! Cookies set and sent back where they belong, ended when their time comes, kept as text.

use super::*;

const NOW: u64 = 1_762_862_400;

fn url(text: &str) -> Url {
    Url::parse(text).unwrap()
}

#[test]
fn a_cookie_goes_back_to_its_host_and_path_only() {
    let mut jar = CookieJar::default();
    let login = url("https://api.example.com/auth/login");
    jar.store(&login, "session=abc; Path=/; HttpOnly; Secure", NOW);
    jar.store(&login, "step=2", NOW);
    assert_eq!(
        jar.header_for(&url("https://api.example.com/users"), NOW).as_deref(),
        Some("session=abc")
    );
    assert_eq!(
        jar.header_for(&url("https://api.example.com/auth/me"), NOW).as_deref(),
        Some("step=2; session=abc"),
        "the longer path first; the default path is the request's folder"
    );
    assert_eq!(
        jar.header_for(&url("http://api.example.com/users"), NOW),
        None,
        "Secure: https only"
    );
    assert_eq!(jar.header_for(&url("https://other.example.com/"), NOW), None, "the host only");
}

#[test]
fn a_domain_reaches_its_subdomains_and_a_foreign_one_is_refused() {
    let mut jar = CookieJar::default();
    let site = url("https://www.example.com/");
    jar.store(&site, "wide=1; Domain=.example.com", NOW);
    jar.store(&site, "evil=1; Domain=attacker.com", NOW);
    assert_eq!(jar.cookies().len(), 1);
    assert_eq!(jar.header_for(&url("https://api.example.com/"), NOW).as_deref(), Some("wide=1"));
    assert_eq!(jar.header_for(&url("https://notexample.com/"), NOW), None);
}

#[test]
fn cookies_end_when_their_time_comes_and_a_server_deletes_them() {
    let mut jar = CookieJar::default();
    let site = url("https://e.com/");
    jar.store(&site, "short=1; Max-Age=60", NOW);
    jar.store(&site, "dated=1; Expires=Wed, 11 Nov 2026 13:00:00 GMT", NOW);
    assert_eq!(jar.header_for(&site, NOW).as_deref(), Some("short=1; dated=1"));
    assert_eq!(jar.header_for(&site, NOW + 61).as_deref(), Some("dated=1"), "Max-Age passed");
    jar.store(&site, "dated=; Max-Age=0", NOW);
    assert_eq!(jar.header_for(&site, NOW).as_deref(), Some("short=1"), "deleted by the server");
    assert_eq!(http_date("Wed, 11 Nov 2026 13:00:00 GMT"), Some(1_794_402_000));
    assert_eq!(http_date("Thursday, 01-Jan-70 00:00:01 GMT"), Some(1));
}

#[test]
fn the_jar_is_kept_as_text_without_what_ended() {
    let mut jar = CookieJar::default();
    let site = url("https://e.com/app/");
    jar.store(&site, "a=1; Path=/app; Secure", NOW);
    jar.store(&site, "b=2; Max-Age=10", NOW);
    let text = jar.to_text();
    assert_eq!(CookieJar::from_text(&text, NOW), jar);
    assert_eq!(CookieJar::from_text(&text, NOW + 11).cookies().len(), 1, "b ended");
    assert!(CookieJar::from_text("garbage\nline", NOW).cookies().is_empty());
}

#[test]
fn a_run_s_changes_are_applied_without_undoing_another_s() {
    let url = Url::parse("https://e.com/").unwrap();
    let mut kept = CookieJar::default();
    kept.store(&url, "a=1", 0);
    kept.store(&url, "gone=1", 0);
    let before = kept.clone();
    // Meanwhile another run set its own.
    kept.store(&url, "other=2", 0);
    let mut after = before.clone();
    after.store(&url, "a=changed", 0);
    after.store(&url, "gone=; Max-Age=0", 0);
    after.store(&url, "new=3", 0);
    assert!(kept.apply_changes(&before, &after));
    let mut names: Vec<_> =
        kept.cookies().iter().map(|c| format!("{}={}", c.name, c.value)).collect();
    names.sort();
    assert_eq!(names, ["a=changed", "new=3", "other=2"]);
    assert!(!kept.apply_changes(&after, &after), "nothing changed: nothing to write");
}

#[test]
fn huge_lifetimes_and_years_saturate_instead_of_overflowing() {
    let url = Url::parse("https://e.com/").unwrap();
    let mut jar = CookieJar::default();
    jar.store(&url, "a=1; Max-Age=9223372036854775807", u64::MAX - 10);
    assert_eq!(jar.cookies()[0].expires, Some(u64::MAX), "kept, not wrapped into the past");
    assert_eq!(http_date("Fri, 31 Dec 99999999999999 23:59:59 GMT"), None);
}
