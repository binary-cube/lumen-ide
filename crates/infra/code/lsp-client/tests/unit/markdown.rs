use super::*;

#[test]
fn the_first_code_block_is_the_signature() {
    let markdown = "```typescript\nfunction add(a: number, b: number): number\n```\nAdds *two* numbers.\n\n```ts\nadd(1, 2)\n```";
    let (code, doc) = split(markdown);
    assert_eq!(code, "function add(a: number, b: number): number");
    assert_eq!(doc.as_deref(), Some("Adds two numbers.\n\nadd(1, 2)"));
}

#[test]
fn text_without_code_is_all_documentation() {
    let (code, doc) = split("Just **text** with `code` and a [link](https://example.com).");
    assert_eq!(code, "");
    assert_eq!(doc.as_deref(), Some("Just text with code and a link."));
}

#[test]
fn rules_and_blank_edges_are_dropped() {
    let (code, doc) = split("```js\nlet x\n```\n\n---\n\n");
    assert_eq!(code, "let x");
    assert_eq!(doc, None);
}

#[test]
fn headings_quotes_and_escapes_become_plain() {
    assert_eq!(plain("## Title"), "Title");
    assert_eq!(plain("> quoted"), "quoted");
    assert_eq!(plain(r"a \* b"), "a * b");
    assert_eq!(plain("snake_case_name"), "snake_case_name");
    assert_eq!(plain("_emphasis_"), "emphasis");
    assert_eq!(plain("[not a link"), "[not a link");
}

/// PHP's manual writes HTML in its descriptions: a table's rows on lines, its cells joined,
/// paragraphs apart, entities as characters — and `array<int>` (no tag of HTML) as written.
#[test]
fn html_in_the_documentation_becomes_text() {
    let markdown = "```php\nfunction phpinfo(int $flags = INFO_ALL): true\n```\nOutputs information.\n\n<table><tbody><tr valign=\"top\"><td>Name</td><td>Value</td></tr><tr valign=\"top\"><td>INFO_GENERAL</td><td>1</td></tr></tbody></table><p>See &quot;php.ini&quot; &amp; more&#46;</p>Returns array<int>.";
    let (code, doc) = split(markdown);
    assert_eq!(code, "function phpinfo(int $flags = INFO_ALL): true");
    assert_eq!(
        doc.as_deref(),
        Some(
            "Outputs information.\n\nName · Value\nINFO_GENERAL · 1\nSee \"php.ini\" & more.\nReturns array<int>."
        )
    );
}

/// A code block's text is never touched (`<?php` and `&&` stay).
#[test]
fn html_in_code_blocks_stays() {
    let (code, _) = split("```php\n<?php if ($a && $b) {}\n```\n<p>doc</p>");
    assert_eq!(code, "<?php if ($a && $b) {}");
}
