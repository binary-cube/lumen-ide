use super::*;

/// HTML's lines kept, tags left out, entities read; plain text untouched.
#[test]
fn a_label_s_html_is_read_as_its_text() {
    assert_eq!(plain("<b>Valid?</b>", true), "Valid?");
    assert_eq!(plain("one<br>two<br/>three", true), "one\ntwo\nthree");
    assert_eq!(plain("<div>a</div><div>b</div>", true), "a\nb");
    assert_eq!(
        plain("Tom &amp; Jerry&nbsp;&#39;s &#x2713; &lt;x&gt;", true),
        "Tom & Jerry 's ✓ <x>"
    );
    assert_eq!(plain("a < b & c", true), "a < b & c", "what opens nothing is text");
    assert_eq!(plain("<b>x</b> &unknown; y", true), "x &unknown; y");
    assert_eq!(plain("<b>raw</b>", false), "<b>raw</b>");
    assert_eq!(plain("ăîșț <i>ü</i>", true), "ăîșț ü");
}
