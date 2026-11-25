use std::rc::Rc;
use std::time::Instant;

use web_engine_protocol::{Event, Hello, PROTOCOL, PageId, Size};
use web_host_core::{Acks, ClientId, Kept, Outbox, PageKey};

use super::{Kind, PageParts, PageState, may_go};
use crate::transfer::Transfer;

/// An app's page whose events are kept to be read back.
fn page() -> (PageState, Kept) {
    let (outbox, kept) = Outbox::kept();
    let hello =
        Hello { protocol: PROTOCOL, pid: std::process::id(), mach_service: None, adapter: None };
    let state = PageState::new(PageParts {
        key: PageKey { client: ClientId(1), page: PageId(1) },
        size: Size { width: 100, height: 100, scale: 1.0 },
        outbox,
        acks: Acks::default(),
        transfer: Rc::new(Transfer::to(&hello)),
        kind: Kind::App,
        pixels: None,
    });
    (state, kept)
}

/// An app's page goes only to its own files (or an empty frame); an HTML page only to its
/// own address; neither to the network nor to a `data:` document.
#[test]
fn a_page_goes_only_where_it_may() {
    let html = "local-app://page/abc";
    let at = Kind::Html(html.to_owned());
    assert!(may_go(&Kind::App, &web_pages::entry_url("index.html")));
    assert!(may_go(&Kind::App, "about:blank"));
    assert!(!may_go(&Kind::App, "https://example.com/"));
    assert!(!may_go(&Kind::App, "data:text/html,<p>x</p>"));
    assert!(may_go(&at, html));
    assert!(may_go(&at, "about:blank"));
    assert!(!may_go(&at, "local-app://page/other"), "another HTML page's");
    assert!(!may_go(&at, &web_pages::entry_url("index.html")), "an app's files");
    assert!(!may_go(&Kind::Html(String::new()), ""), "nowhere before its address is known");
}

/// Once a page closed nothing more of it is told, whatever its handlers still hear (CEF lets
/// them go later).
#[test]
fn nothing_is_told_after_a_page_closed() {
    let (page, kept) = page();
    page.link("local-app://localhost/a".into(), true);
    assert!(matches!(kept.told().as_slice(), [(_, Event::OpenLink { .. })]));
    page.set_access(true);
    page.set_access(true);
    page.close();
    page.link("local-app://localhost/b".into(), true);
    page.send(Event::Crashed { page: PageId(1) });
    assert_eq!(page.tell_access(Instant::now()), None);
    assert!(kept.told().is_empty(), "nothing after close");
}

/// The tree asked for: Chromium told once; asked again while on, the tree kept is told again
/// (Chromium not told twice); not asked: let go, Chromium told; not asked again: nothing.
#[test]
fn the_tree_asked_and_let_go() {
    let (page, kept) = page();
    assert!(page.set_access(true), "on: Chromium told");
    assert_eq!(page.tell_access(Instant::now()), None);
    assert!(kept.told().is_empty(), "nothing changed yet");
    assert!(!page.set_access(true), "on again: Chromium not told");
    assert_eq!(page.tell_access(Instant::now()), None);
    assert!(matches!(kept.told().as_slice(), [(_, Event::Access { .. })]), "told again whole");
    assert!(page.set_access(false), "off: Chromium told");
    page.access_changed(|_| unreachable!("not kept while off"));
    assert_eq!(page.tell_access(Instant::now()), None);
    assert!(!page.set_access(false), "off again: nothing");
    assert!(kept.told().is_empty());
}
