use super::*;

fn names(text: &str) -> Vec<String> {
    visible(text, text.len()).names
}

#[test]
fn set_in_both_forms_and_several_at_once() {
    assert_eq!(names("{% set a = 1 %}{% set b %}x{% endset %}"), ["a", "b"]);
    assert_eq!(names("{%- set c, d = 1, 2 -%}"), ["c", "d"]);
}

#[test]
fn a_loop_s_names_end_with_it() {
    assert_eq!(names("{% for k, v in m %}"), ["k", "v"]);
    assert!(names("{% for k in m %}{% endfor %}").is_empty());
    assert!(visible("{% for k in m %}", 16).in_loop);
    assert!(!visible("{% for k in m %}{% endfor %}", 28).in_loop);
}

#[test]
fn a_macro_s_arguments_stay_in_it() {
    assert_eq!(names("{% macro f(a, b = 1) %}"), ["a", "b"]);
    assert!(names("{% macro f(a) %}{% endmacro %}").is_empty());
}

#[test]
fn imports_give_their_names() {
    assert_eq!(names("{% import 'm.twig' as forms %}"), ["forms"]);
    assert_eq!(names("{% from 'm.twig' import a as b, c %}"), ["b", "c"]);
}

#[test]
fn a_set_inside_a_loop_ends_with_it() {
    assert!(names("{% for a in b %}{% set c = 1 %}{% endfor %}").is_empty());
}

#[test]
fn an_unfinished_tag_changes_nothing() {
    assert_eq!(names("{% set a = 1 %}{% set b"), ["a"]);
}
