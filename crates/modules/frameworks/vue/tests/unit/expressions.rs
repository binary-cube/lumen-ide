use super::*;

#[test]
fn interpolations_are_found_between_the_braces() {
    let text = "Hi {{ user.name }}, {{count + 1}} {{}} and {{ unclosed";
    let found: Vec<&str> = interpolations(text, 0).into_iter().map(|r| &text[r]).collect();
    assert_eq!(found, [" user.name ", "count + 1"]);
    assert_eq!(interpolations("{{a}}", 10), vec![(12..13)], "offsets in the document");
}

#[test]
fn directives_are_told_apart() {
    for name in ["v-if", "v-for", ":class", "@click", "#item", "v-on:submit.prevent"] {
        assert!(is_directive(name), "{name}");
    }
    for name in ["class", "v-else", "v-cloak", "id", "href"] {
        assert!(!is_directive(name), "{name}");
    }
}
