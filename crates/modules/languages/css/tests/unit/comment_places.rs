use super::*;

/// What the place at `§` in `src` is (the marker taken out), in SCSS or CSS.
fn at(src: &str, scss: bool) -> Option<CommentPlace> {
    let offset = src.find('§').expect("a § in the snippet");
    place(&src.replacen('§', "", 1), offset, scss)
}

fn named(name: &str, kind: SymbolKind) -> Option<CommentPlace> {
    Some(CommentPlace::Code(Target::Named { name: name.into(), kinds: vec![kind] }))
}

const TEXT: Option<CommentPlace> = Some(CommentPlace::Text);

#[test]
fn css_comments_are_text() {
    assert_eq!(at("/* the .bt§n is old */ .btn {}", false), TEXT);
    assert_eq!(at("/* see $prim§ary */", false), TEXT);
    assert_eq!(at(".btn { color: red; } .ca§rd {}", false), None, "outside");
    assert_eq!(at("a { content: '/*'; } .ca§rd {}", false), None, "`/*` in a string");
    assert_eq!(at("// .bt§n", false), None, "not a comment in CSS");
}

#[test]
fn scss_comments_are_text_but_sass_doc_names_code() {
    assert_eq!(at("// the .bt§n is old\n", true), TEXT);
    assert_eq!(at("a { background: url(http://x.com/a.png); } .ca§rd {}", true), None, "a URL");
    assert_eq!(at("/// Makes a but§ton.\n@mixin button() {}", true), TEXT);
    assert_eq!(at("/// @see but§ton\n", true), named("button", SymbolKind::Mixin));
    assert_eq!(at("/// @require $prim§ary\n", true), named("$primary", SymbolKind::CssVariable));
    assert_eq!(at("/// @see %place§holder\n", true), TEXT);
    assert_eq!(at("/// @see button and the te§xt\n", true), TEXT);
    assert_eq!(at("/// @group but§tons\n", true), TEXT);
    assert_eq!(at("/// @link https://sass§doc.com\n", true), TEXT);
}

#[test]
fn a_param_leads_to_the_mixin_s_parameter() {
    let src = "/// @param {Color} $co§lor [red] the color\n/// @param {Number} $size\n@mixin tint($color, $size: 1) {}";
    let offset = src.replacen('§', "", 1).find("$color,").unwrap();
    assert_eq!(at(src, true), Some(CommentPlace::Declared(offset)));
    assert_eq!(at("/// @param {Col§or} $color\n@mixin tint($color) {}", true), TEXT, "its type");
    assert_eq!(at("/// @param {Color} $mis§sing\n@mixin tint($color) {}", true), TEXT);
}
