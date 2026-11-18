use super::*;

#[test]
fn every_rule_uses_a_known_icon() {
    let builtin = ["file-code", "file-json", "file-text", "file-style", "file"];
    for (_, icon, _) in RULES {
        assert!(
            ICONS.iter().any(|i| i.name == *icon) || builtin.contains(icon),
            "unknown icon: {icon}"
        );
    }
}

#[test]
fn every_folder_has_one_rule() {
    let mut names: Vec<String> =
        FOLDERS.iter().flat_map(|(names, _)| names.iter().map(|n| n.to_lowercase())).collect();
    let all = names.len();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), all, "a folder named twice");
    assert!(FOLDERS.iter().any(|(names, color)| names.contains(&"src") && *color == "source"));
}
