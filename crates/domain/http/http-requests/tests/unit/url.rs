//! A URL's parts, and a redirect's location taken from it.

use super::*;

#[test]
fn a_url_is_split_into_what_requests_need() {
    let url = Url::parse("HTTPS://user:pw@API.Example.com:8443/v1/users?page=2#top").unwrap();
    assert_eq!((&*url.scheme, &*url.host, url.port), ("https", "api.example.com", Some(8443)));
    assert_eq!((&*url.path, url.query.as_deref()), ("/v1/users", Some("page=2")));
    assert!(url.is_secure());
    let bare = Url::parse("http://localhost").unwrap();
    assert_eq!((&*bare.path, bare.port_or_default()), ("/", 80));
    let v6 = Url::parse("http://[::1]:8080/x").unwrap();
    assert_eq!((&*v6.host, v6.port), ("::1", Some(8080)));
    assert_eq!(Url::parse("example.com/x"), None);
    assert_eq!(Url::parse("http://:80/"), None);
}

#[test]
fn a_redirect_s_location_is_taken_from_where_it_came() {
    let url = Url::parse("https://e.com:8443/a/b?x=1").unwrap();
    assert_eq!(url.join("https://other.com/z"), "https://other.com/z");
    assert_eq!(url.join("/root"), "https://e.com:8443/root");
    assert_eq!(url.join("c"), "https://e.com:8443/a/c");
    assert_eq!(url.join("?y=2"), "https://e.com:8443/a/b?y=2");
    assert_eq!(url.join("//cdn.e.com/i"), "https://cdn.e.com/i");
    assert_eq!(
        url.join("/login?next=https://x.com/"),
        "https://e.com:8443/login?next=https://x.com/",
        "a `://` in its query: still relative"
    );
}
