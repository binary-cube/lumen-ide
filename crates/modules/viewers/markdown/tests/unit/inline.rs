use super::*;

#[test]
fn bare_urls_become_links() {
    let p = autolinks("See https://example.com/a_(b), or www.rust-lang.org. Not ahttps://x");
    assert_eq!(
        p,
        vec![
            Piece::Text("See "),
            Piece::Link("https://example.com/a_(b)", "https://example.com/a_(b)".into()),
            Piece::Text(", or "),
            Piece::Link("www.rust-lang.org", "http://www.rust-lang.org".into()),
            Piece::Text(". Not ahttps://x"),
        ]
    );
    assert_eq!(
        autolinks("(https://a.io)"),
        vec![
            Piece::Text("("),
            Piece::Link("https://a.io", "https://a.io".into()),
            Piece::Text(")"),
        ]
    );
    assert_eq!(autolinks("plain ă text"), vec![Piece::Text("plain ă text")]);
}

#[test]
fn shortcodes_become_emoji() {
    assert_eq!(
        emojify("Ship it :rocket: :+1: :not_an_emoji: 10:30"),
        "Ship it 🚀 👍 :not_an_emoji: 10:30"
    );
    assert_eq!(emoji("tada"), Some("🎉"));
}

#[test]
fn slugs_like_github() {
    let mut seen = HashMap::new();
    assert_eq!(slug("Hello, World!", &mut seen), "hello-world");
    assert_eq!(slug("Hello, World!", &mut seen), "hello-world-1");
    assert_eq!(slug("Ce mai faci? Ăsta e_ok", &mut seen), "ce-mai-faci-ăsta-e_ok");
}
