use super::*;

/// The completion of `text` with `|` where the cursor is (just after the typed `?`).
fn typed(text: &str) -> Option<TypedInsert> {
    let at = text.find('|').expect("a cursor");
    let text = text.replacen('|', "", 1);
    open_tag(&text, at, '?')
}

#[test]
fn a_tag_in_the_html_becomes_php_and_a_new_line() {
    let insert = typed("<?|").expect("completed");
    assert_eq!(insert.text, "php\n");
    assert_eq!(insert.cursor, insert.text.len(), "the cursor on the new line");
    assert_eq!(insert.retract_on, &['='], "`<?=` stays possible");
    let insert = typed("<div>\n    <?|\n</div>").expect("completed");
    assert_eq!(insert.text, "php\n    ", "the new line indented as the tag's");
    let insert = typed("<p><?|?></p>").expect("completed");
    assert_eq!(insert.text, "php ", "code after it on the line: a space");
    assert!(typed("<?php echo 1; ?>\n<?|").is_some(), "after a closed block");
}

#[test]
fn not_inside_the_code_nor_where_it_is_written_already() {
    assert!(typed("<?php\n$a = '<?|';").is_none(), "in a string");
    assert!(typed("<?php\n// <?|").is_none(), "in a comment");
    assert!(typed("<?php $s = '?>'; <?|").is_none(), "a `?>` in a string doesn't close it");
    assert!(typed("<?|php").is_none());
    assert!(typed("<?|=").is_none());
    assert!(typed("<?|xml version=\"1.0\"?>").is_none());
    assert!(typed("a?|").is_none(), "only after `<`");
    assert!(open_tag("<?", 2, '>').is_none(), "only on `?`");
}
