use super::*;

#[test]
fn the_element_still_open_is_found() {
    let html = "<div>\n  <ul>\n    <li>a</li>\n    <br>\n    <img src=x />\n";
    assert_eq!(unclosed_tag(html), html.find("<ul>"));
    assert_eq!(unclosed_tag("<p>a</p>"), None);
    assert_eq!(unclosed_tag("<div><!-- <span> -->"), Some(0));
}
