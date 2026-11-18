use super::*;

const PHP: DocRules = DocRules {
    opener: "/**",
    closer: "*/",
    attribute: Some("#["),
    decorator: None,
    sigil: Some('$'),
};
const JS: DocRules =
    DocRules { opener: "/**", closer: "*/", attribute: None, decorator: Some('@'), sigil: None };

#[test]
fn reads_php_and_js_doc_comments() {
    let php = "<?php\n/**\n * DeviceRegistration\n *\n * The row a call is about,\n * plus `created`.\n */\n#[Final]\nfinal class X {\n    /**\n     * @param Device $device  the managed row,\n     *                        guaranteed to exist\n     * @param bool   $created whether this call inserted it\n     */\n    public function __construct() {}\n}";
    let class = doc_before(php, php.find("final class").unwrap(), &PHP).unwrap();
    assert_eq!(class.summary, "DeviceRegistration\nThe row a call is about, plus created.");
    let ctor = doc_before(php, php.find("public function").unwrap(), &PHP).unwrap();
    assert_eq!(ctor.param("$device"), Some("the managed row, guaranteed to exist"));
    assert_eq!(ctor.param("created"), Some("whether this call inserted it"));
    let js = "/**\n * Adds.\n * @param {number} a - the first\n * @param {number} [b=0] the second\n */\nfunction add(a, b) {}";
    let doc = doc_before(js, js.find("function").unwrap(), &JS).unwrap();
    assert_eq!(doc.summary, "Adds.");
    assert_eq!(doc.param("a"), Some("the first"));
    assert_eq!(doc.param("b"), Some("the second"));
    assert_eq!(doc.param_type("a"), Some("number"));
    assert_eq!(ctor.param_type("device"), Some("Device"));
    assert!(doc_before("x();\nfunction f() {}", 5, &JS).is_none());
}

/// The doc comment above attributes written on several lines (with brackets inside) is
/// still the declaration's.
#[test]
fn a_doc_comment_above_attributes_on_several_lines() {
    let php = "<?php\n/**\n * Signs every admin out.\n */\n#[AsCommand(\n    name: 'app:auth:flush',\n    aliases: ['flush'],\n)]\n#[Final]\nfinal class X {}";
    let doc = doc_before(php, php.find("final class").unwrap(), &PHP).unwrap();
    assert_eq!(doc.summary, "Signs every admin out.");
    // An array ending the line before isn't an attribute.
    assert!(doc_before("<?php\n$a = [1];\nfunction f() {}", 20, &PHP).is_none());
}

/// Attributes the brackets can't be matched in (a comment after one, a `[` in a string)
/// still don't hide the doc comment above them.
#[test]
fn a_doc_comment_above_attributes_with_comments_or_brackets_in_strings() {
    for attribute in ["#[Route('/x')] // the home page", "#[Assert\\Regex('/[/')]"] {
        let text = format!("/**\n * Shows it.\n */\n{attribute}\npublic function show() {{}}");
        let at = text.find("public").unwrap();
        let doc = doc_before(&text, at, &PHP).expect(attribute);
        assert_eq!(doc.summary, "Shows it.", "{attribute}");
    }
}

#[test]
fn a_deprecated_tag_is_noticed() {
    let php = "<?php\n/**\n * Old.\n * @deprecated use new()\n */\nfunction old() {}\n";
    let doc = doc_before(php, php.find("function").unwrap(), &PHP).unwrap();
    assert!(doc.deprecated);
    let php = "<?php\n/** Fine, not @deprecated. */\nfunction f() {}\n";
    assert!(!doc_before(php, php.find("function").unwrap(), &PHP).unwrap().deprecated);
}
