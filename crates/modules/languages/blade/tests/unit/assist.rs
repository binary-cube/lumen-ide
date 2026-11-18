use std::path::Path;

use super::*;

fn answer(path: &str, marked: &str) -> Option<FrameworkCompletion> {
    let offset = marked.find('¦').unwrap();
    let text = marked.replace('¦', "");
    let at = AssistContext {
        project: None,
        root: None,
        path: Path::new(path),
        language: "blade",
        text: &text,
        offset,
        version: 0,
    };
    BladeAssist::default().complete(&at)
}

fn offers(marked: &str, word: &str) -> bool {
    answer("a.blade.php", marked).is_some_and(|a| a.items.iter().any(|i| i.label == word))
}

#[test]
fn the_data_reads_whole() {
    assert!(BladeAssist::default().words().len() > 70);
}

#[test]
fn directives_after_the_sign() {
    assert!(offers("@fore¦", "@foreach"));
    assert!(offers("<p>@¦</p>", "@if"));
    assert!(offers("@endfo¦", "@endforeach"));
}

#[test]
fn a_directive_with_an_argument_leaves_the_cursor_inside() {
    let answer = answer("a.blade.php", "@yiel¦").unwrap();
    assert_eq!(answer.replace, 0..5);
    assert_eq!((answer.items[0].insert.as_str(), answer.items[0].cursor_back), ("@yield()", 1));
}

#[test]
fn the_variables_of_the_template() {
    assert!(offers("@foreach($users as $user) {{ $us¦", "$user"));
}

#[test]
fn loop_s_members_only_in_a_loop() {
    assert!(offers("@foreach($a as $b) {{ $loop->¦", "iteration"));
    assert!(answer("a.blade.php", "{{ $loop->¦").is_none());
}

#[test]
fn only_blade_files() {
    assert!(answer("a.php", "@fore¦").is_none());
}

#[test]
fn a_stylesheet_or_a_script_keeps_its_own_signs() {
    let at = AssistContext {
        project: None,
        root: None,
        path: Path::new("a.blade.php"),
        language: "css",
        text: "@me",
        offset: 3,
        version: 0,
    };
    assert!(BladeAssist::default().complete(&at).is_none());
}

#[test]
fn it_s_asked_for_the_whole_file_whatever_is_at_the_cursor() {
    let assist = BladeAssist::default();
    assert!(assist.asked_for("bash", "blade"));
    assert!(!assist.asked_for("php", "php"));
}
