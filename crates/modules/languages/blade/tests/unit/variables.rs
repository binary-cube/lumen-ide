use super::*;

fn names(text: &str) -> Vec<String> {
    visible(text, text.len()).names
}

#[test]
fn a_loop_s_names_and_loop_end_with_it() {
    assert_eq!(names("@foreach($a as $k => $v)"), ["$k", "$loop", "$v"]);
    assert!(names("@foreach($a as $k) x @endforeach").is_empty());
}

#[test]
fn what_php_assigns() {
    assert_eq!(names("@php $a = 1; $b = 2; @endphp"), ["$a", "$b"]);
    assert_eq!(names("@php($c = 3)"), ["$c"]);
}

#[test]
fn props_are_variables() {
    assert_eq!(names("@props(['type', 'size' => 1])"), ["$size", "$type"]);
}

#[test]
fn an_unfinished_directive_is_a_loop_without_names_yet() {
    let found = visible("@foreach($a as", 14);
    assert!(found.in_loop);
    assert_eq!(found.names, ["$loop"]);
}
