use super::*;
use crate::CompletionKind;

const SYMFONY: &[&str] = &["->render("];
const LARAVEL: &[&str] = &["view(", "View::make("];

fn completion(labels: &[&str], at: Range<usize>, merge: bool) -> FrameworkCompletion {
    FrameworkCompletion {
        replace: at,
        items: labels
            .iter()
            .map(|l| CompletionItem::word(l, CompletionKind::Value, None))
            .collect(),
        merge,
    }
}

#[test]
fn an_array_s_first_level_keys_are_passed() {
    let php = "return $this->render('a.twig', ['user' => $u, 'list' => ['nested' => 1]]);";
    assert_eq!(
        passed_variables(php, SYMFONY),
        [("a.twig".to_owned(), vec!["user".to_owned(), "list".to_owned()])]
    );
}

#[test]
fn compact_and_chained_with_are_passed() {
    let php = "view('a.b', compact('x', 'y'))->with('z', 1)->with(['w' => 2]);";
    let (template, names) = passed_variables(php, LARAVEL).remove(0);
    assert_eq!(template, "a.b");
    assert_eq!(names, ["x", "y", "z", "w"]);
}

#[test]
fn a_call_without_a_literal_name_or_inside_another_word_is_skipped() {
    assert!(passed_variables("view($name, ['a' => 1]);", LARAVEL).is_empty());
    assert!(passed_variables("myview('a', ['a' => 1]);", LARAVEL).is_empty());
}

#[test]
fn no_variables_is_still_a_template() {
    assert_eq!(passed_variables("view('a');", LARAVEL), [("a".to_owned(), Vec::new())]);
}

#[test]
fn an_unfinished_call_ends_quietly() {
    assert!(passed_variables("view('a', ['x' =>", LARAVEL).is_empty());
}

#[test]
fn the_same_place_joins_without_repeating() {
    let joined = join_suggestions(
        Some(completion(&["a", "b"], 1..2, false)),
        Some(completion(&["b", "c"], 1..2, false)),
    )
    .unwrap();
    let labels: Vec<_> = joined.items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["b", "c", "a"]);
}

#[test]
fn another_place_keeps_the_module_s() {
    let joined = join_suggestions(
        Some(completion(&["a"], 1..2, false)),
        Some(completion(&["b"], 0..2, false)),
    )
    .unwrap();
    assert_eq!(joined.items[0].label, "a");
}

#[test]
fn a_member_names_its_object() {
    let (place, range) = twig_place("{{ app.us", 9).unwrap();
    assert_eq!(place, TwigPlace::Member { object: "app".into() });
    assert_eq!(range, 7..9);
}

#[test]
fn a_blade_directive_starts_at_its_sign_and_an_address_is_none() {
    assert_eq!(blade_place("@fo", 3), Some((BladePlace::Directive, 0..3)));
    assert_eq!(blade_place("me@fo", 5), None);
    assert_eq!(blade_place("@@fo", 4), None);
    assert_eq!(blade_place("{{ @fo", 6), None);
}

#[test]
fn a_blade_variable_and_a_member() {
    assert_eq!(blade_place("{{ $us", 6), Some((BladePlace::Variable, 3..6)));
    assert_eq!(blade_place("{{ '$us", 7), None);
    let member = blade_place("{{ $loop->fi", 12).unwrap();
    assert_eq!(member, (BladePlace::Member { object: "loop".into() }, 10..12));
}

#[test]
fn props_are_the_first_level_names() {
    let text = "@props(['type' => 'info', 'message', 'map' => ['nested' => 1]])";
    assert_eq!(declared_props(text), ["type", "message", "map"]);
    assert!(declared_props("<div>").is_empty());
}

#[test]
fn help_for_the_language_s_place_joins_it() {
    let mut items = vec![CompletionItem::word("a", CompletionKind::Value, None)];
    add_to_language(&mut items, &(1..2), Some(completion(&["b"], 1..2, true)));
    assert_eq!(items.len(), 2);
    add_to_language(&mut items, &(1..2), Some(completion(&["c"], 0..2, true)));
    assert_eq!(items.len(), 2, "another place");
}

#[test]
fn a_closed_delimiter_is_not_open() {
    assert_eq!(open_delimiter("{{ a }} b", &[("{{", "}}")]), None);
    assert_eq!(open_delimiter("{{ a }} {{ b", &[("{{", "}}")]), Some((8, "{{")));
}
