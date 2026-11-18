//! A page with a `<base>` where it came from, so its relative links are found there.

use super::*;

#[test]
fn the_base_goes_in_its_head_or_first() {
    let url = "https://e.com/a/?x=1&y=\"2\"";
    assert_eq!(
        with_base("<html><head><title>t</title></head></html>", url),
        "<html><head><base href=\"https://e.com/a/?x=1&amp;y=&quot;2&quot;\"><title>t</title></head></html>"
    );
    assert_eq!(
        with_base("<HTML><HEAD lang=en></HEAD>", "https://e.com/"),
        "<HTML><HEAD lang=en><base href=\"https://e.com/\"></HEAD>"
    );
    assert_eq!(
        with_base("<header>x</header>", "https://e.com/"),
        "<base href=\"https://e.com/\"><header>x</header>",
        "a <header> isn't its head"
    );
    assert_eq!(
        with_base("<!DOCTYPE html><body>x</body>", "https://e.com/"),
        "<!DOCTYPE html><base href=\"https://e.com/\"><body>x</body>",
        "after its doctype, not before"
    );
    let own = "<head><base href=\"/x/\"></head>";
    assert_eq!(with_base(own, "https://e.com/"), own, "its own kept");
}
