use super::{split_mime, split_url};

/// A content type split as CEF takes it: the type alone, its charset apart.
#[test]
fn a_content_type_is_split_for_the_engine() {
    assert_eq!(split_mime("text/html; charset=utf-8"), ("text/html", Some("utf-8")));
    assert_eq!(split_mime("image/png"), ("image/png", None));
}

/// An address split into its host and its path (the query left out).
#[test]
fn an_address_gives_its_host_and_path() {
    assert_eq!(split_url("local-app://page/abc"), (Some("page"), "abc"));
    assert_eq!(split_url("local-app://localhost/a/b.js?x=1"), (Some("localhost"), "a/b.js"));
}

/// An HTML page is kept under a random token of the scheme's own host for HTML (never an
/// app's), found by it while kept, let go as it drops.
#[test]
fn an_html_page_is_found_by_its_token_while_kept() {
    let page = super::put_html("<p>x</p>".into());
    let other = super::put_html(String::new());
    assert_ne!(page.url, other.url, "a token each");
    let (host, token) = split_url(&page.url);
    let token = token.to_owned();
    assert_eq!(host, Some(super::HTML_HOST));
    assert!(page.url.starts_with(&format!("{}://", web_pages::SCHEME)));
    assert_eq!(token.len(), 32, "a UUID's 128 bits");
    assert_eq!(super::html_of(&token).as_deref(), Some("<p>x</p>"));
    drop(page);
    assert!(super::html_of(&token).is_none(), "let go");
}
