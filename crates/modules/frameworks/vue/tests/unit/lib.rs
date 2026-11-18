use super::*;

#[test]
fn directives_in_tags() {
    let src = "<li v-f";
    assert_eq!(
        code_intelligence::attribute_name_at(src, src.len(), &['-', ':']).map(|r| &src[r]),
        Some("v-f")
    );
    assert!(code_intelligence::attribute_name_at("<li>v-f", 7, &['-', ':']).is_none());
    assert!(API.iter().all(|(_, sig)| sig.starts_with('(')));
}
