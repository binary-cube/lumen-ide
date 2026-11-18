use super::*;

fn x(abbreviation: &str) -> String {
    expand(abbreviation, "", "  ").unwrap()
}

#[test]
fn elements_classes_ids_attributes() {
    assert_eq!(x("div.class#bla"), r#"<div class="class" id="bla"></div>"#);
    assert_eq!(x(".card.big"), r#"<div class="card big"></div>"#);
    assert_eq!(x("a"), r#"<a href=""></a>"#);
    assert_eq!(x("img"), r#"<img src="" alt="">"#);
    assert_eq!(x("input:email[required]"), r#"<input type="email" required>"#);
    assert_eq!(x("btn:s{Trimite}"), r#"<button type="submit">Trimite</button>"#);
    assert_eq!(x("p{Salut}"), "<p>Salut</p>");
}

#[test]
fn nesting_siblings_multiplication() {
    assert_eq!(
        x("ul>li.item$*3"),
        "<ul>\n  <li class=\"item1\"></li>\n  <li class=\"item2\"></li>\n  <li class=\"item3\"></li>\n</ul>"
    );
    assert_eq!(x("ul>.x"), "<ul>\n  <li class=\"x\"></li>\n</ul>");
    assert_eq!(x("h1+p"), "<h1></h1>\n<p></p>");
    assert_eq!(x("div>p>span"), "<div>\n  <p><span></span></p>\n</div>");
    assert_eq!(x("table+"), "<table>\n  <tr>\n    <td></td>\n  </tr>\n</table>");
    assert!(x("!").starts_with("<!DOCTYPE html>"));
    assert!(x("p>lorem5").contains("Lorem ipsum dolor sit amet."));
    assert!(x("ul>li*2>a{Link $}").contains("Link 2"), "the child's `$` belongs to `li*2`");
}

#[test]
fn extraction_and_cursor() {
    let e = Emmet;
    let r = e.expand("    <p>div.a", "    ", "  ").unwrap();
    assert_eq!(r.replace, "div.a".len());
    assert_eq!(&r.text[..r.cursor], r#"<div class="a">"#);
    let r = e.expand("a[href]", "", "  ").unwrap();
    assert_eq!(&r.text[..r.cursor], "<a href=\"", "the cursor in the empty attribute");
    assert!(e.expand("hello", "", "  ").is_none(), "plain words don't expand");
    assert!(e.expand("<div class=\"x", "", "  ").is_none(), "not inside a tag");
    assert!(e.expand("Salut lume", "", "  ").is_none());
}

/// Abbreviations that would expand past what an editor can hold (a billion elements, a
/// text of gigabytes) aren't expanded; those within the limits still are.
#[test]
fn an_expansion_past_the_limits_is_refused() {
    assert!(expand("div*1000>div*1000>div*1000", "", "  ").is_none());
    assert!(expand("lorem999999999", "", "  ").is_none());
    assert!(expand("ul>li*100", "", "  ").is_some_and(|t| t.matches("<li>").count() == 100));
    assert!(expand("lorem5", "", "  ").is_some());
}
