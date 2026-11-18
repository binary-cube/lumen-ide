use super::*;

fn page(width: usize) -> Page<'static> {
    Page { width, indent: 2, margin: "", column: 0, newline: "\n" }
}

fn words(list: &[&'static str]) -> Doc<'static> {
    let mut docs = Vec::new();
    for (k, w) in list.iter().enumerate() {
        if k > 0 {
            docs.push(Doc::text(","));
            docs.push(Doc::Line);
        }
        docs.push(Doc::text(*w));
    }
    Doc::group(vec![
        Doc::text("f("),
        Doc::indent(vec![Doc::Soft, Doc::seq(docs)]),
        Doc::Soft,
        Doc::text(")"),
    ])
}

#[test]
fn a_group_stays_on_its_line_when_it_fits() {
    assert_eq!(print(&words(&["a", "b", "c"]), page(40)), "f(a, b, c)");
}

#[test]
fn a_group_too_wide_breaks_every_place() {
    assert_eq!(
        print(&words(&["alpha", "beta", "gamma"]), page(10)),
        "f(\n  alpha,\n  beta,\n  gamma\n)"
    );
}

#[test]
fn what_follows_a_group_counts_in_its_width() {
    let doc = Doc::seq(vec![words(&["a", "b"]), Doc::text("1234567")]);
    // `f(a, b)` is 7 wide, with what follows up to the next break 14.
    assert_eq!(print(&doc, page(14)), "f(a, b)1234567");
    assert_eq!(print(&doc, page(13)), "f(\n  a,\n  b\n)1234567");
}

#[test]
fn a_hard_break_breaks_the_groups_around_it() {
    let doc = Doc::group(vec![
        Doc::text("a"),
        Doc::indent(vec![Doc::Line, Doc::text("b"), Doc::Space, Doc::text("-- c"), Doc::Hard]),
        Doc::Line,
        Doc::text("d"),
    ]);
    assert_eq!(print(&doc, page(80)), "a\n  b -- c\nd");
}

#[test]
fn breaks_in_a_row_leave_no_blank_line_nor_trailing_spaces() {
    let doc = Doc::seq(vec![
        Doc::text("a"),
        Doc::Space,
        Doc::Hard,
        Doc::Hard,
        Doc::indent(vec![Doc::Hard, Doc::text("b")]),
    ]);
    assert_eq!(print(&doc, page(80)), "a\n  b");
    // Nothing breaks before the first line's text.
    assert_eq!(print(&Doc::seq(vec![Doc::Hard, Doc::text("x")]), page(80)), "x");
}

#[test]
fn the_margin_starts_every_line_after_the_first() {
    let doc = Doc::seq(vec![Doc::text("a"), Doc::indent(vec![Doc::Hard, Doc::text("b")])]);
    let page = Page { width: 80, indent: 4, margin: "\t", column: 4, newline: "\r\n" };
    assert_eq!(print(&doc, page), "a\r\n\t    b");
}

#[test]
fn a_text_of_several_lines_is_written_whole_and_breaks_its_group() {
    let doc = Doc::group(vec![Doc::text("x"), Doc::Line, Doc::text("/* a\n b */")]);
    assert!(doc.hard());
    assert_eq!(print(&doc, page(80)), "x\n/* a\n b */");
    assert_eq!(width("a\tb"), 6);
}
