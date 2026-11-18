use super::*;

fn open(name: &str, attrs: &[(&str, &str)]) -> Token {
    Token::Open {
        name: name.into(),
        attrs: attrs.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect(),
        closed: false,
    }
}

#[test]
fn tags_attributes_and_text() {
    let t = tokens(r#"<p align="center"><img src='a.png' width=120 alt="A &amp; B"/></p>"#);
    assert_eq!(t[0], open("p", &[("align", "center")]));
    assert_eq!(t[1].attr("src"), Some("a.png"));
    assert_eq!(t[1].attr("width"), Some("120"));
    assert_eq!(t[1].attr("alt"), Some("A & B"));
    assert!(matches!(&t[1], Token::Open { closed: true, .. }));
    assert_eq!(t[2], Token::Close("p".into()));
}

#[test]
fn comments_go_and_stray_brackets_are_text() {
    let t = tokens("a <!-- hidden --> b < c > d <kbd>⌘</kbd>");
    let text: String = t
        .iter()
        .filter_map(|t| match t {
            Token::Text(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(text, "a  b < c > d ⌘");
    assert!(t.contains(&open("kbd", &[])));
    // Text starting with a character of several bytes.
    assert_eq!(tokens("ă<b>"), vec![Token::Text("ă".into()), open("b", &[])]);
}

#[test]
fn entities() {
    assert_eq!(decode("&lt;a&gt; &#65;&#x42; &nbsp;&unknown; & x"), "<a> AB \u{a0}&unknown; & x");
}
