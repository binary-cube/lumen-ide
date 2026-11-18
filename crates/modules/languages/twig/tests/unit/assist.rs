use std::path::Path;

use super::*;

fn at<'a>(path: &'a str, text: &'a str, offset: usize) -> AssistContext<'a> {
    AssistContext {
        project: None,
        root: None,
        path: Path::new(path),
        language: "twig",
        text,
        offset,
        version: 0,
    }
}

/// The suggestions where `¦` is.
fn labels(marked: &str) -> Option<Vec<String>> {
    let offset = marked.find('¦').unwrap();
    let text = marked.replace('¦', "");
    let answer = TwigAssist::default().complete(&at("a.html.twig", &text, offset))?;
    Some(answer.items.into_iter().map(|i| i.label).collect())
}

fn offers(marked: &str, word: &str) -> bool {
    labels(marked).is_some_and(|all| all.iter().any(|l| l == word))
}

#[test]
fn the_data_reads_whole() {
    let words = TwigAssist::default();
    assert!(words.words().len() > 80);
}

#[test]
fn a_tag_right_after_its_delimiter() {
    assert!(offers("{% fo¦", "for"));
    assert!(offers("{%- endi¦ %}", "endif"));
    assert!(!offers("{% fo¦", "format"), "not a filter");
}

#[test]
fn filters_after_a_bar() {
    assert!(offers("{{ name|¦ }}", "upper"));
    assert!(offers("{{ name | dat¦ }}", "date"));
    assert!(offers("{% if a|len¦ %}", "length"));
}

#[test]
fn functions_in_an_expression_and_tests_after_is() {
    assert!(offers("{{ ran¦ }}", "range"));
    assert!(offers("{% if ran¦ %}", "range"), "a tag's argument, not a tag");
    assert!(offers("{% if a is ev¦ %}", "even"));
    assert!(offers("{% if a is not nul¦ %}", "null"));
}

#[test]
fn a_filter_with_arguments_leaves_the_cursor_inside() {
    let text = "{{ d|dat";
    let answer = TwigAssist::default().complete(&at("a.twig", text, text.len())).unwrap();
    assert_eq!(answer.replace, 5..8);
    let date = answer.items.iter().find(|i| i.label == "date").unwrap();
    assert_eq!((date.insert.as_str(), date.cursor_back), ("date()", 1));
}

#[test]
fn a_string_a_comment_and_the_html_are_left_alone() {
    assert_eq!(labels("{{ path('ho¦') }}"), None);
    assert_eq!(labels("{# fo¦ #}"), None);
    assert_eq!(labels("{{ a }} fo¦"), None);
    assert_eq!(labels("{{ user.na¦ }}"), None);
}

#[test]
fn the_variables_the_template_declares() {
    assert!(offers("{% set total = 1 %}{{ tot¦ }}", "total"));
    assert!(offers("{% for user in users %}{{ us¦ }}{% endfor %}", "user"));
}

#[test]
fn the_loop_only_inside_one() {
    assert!(offers("{% for a in b %}{{ loop.¦ }}{% endfor %}", "index"));
    assert_eq!(labels("{{ loop.¦ }}"), None);
}

#[test]
fn only_in_twig_files() {
    let text = "{{ ran";
    assert!(TwigAssist::default().complete(&at("a.html", text, text.len())).is_none());
}

#[test]
fn a_bar_asks_for_the_list() {
    assert_eq!(TwigAssist::default().triggers(), "|");
}
