use super::*;

fn language(id: &str, name: &str) -> SettingsLanguage {
    SettingsLanguage { id: id.into(), name: name.into() }
}

/// The chooser lists each language once (the first name given), the usual ones first, the
/// others by name — whatever order the blocks gave them in.
#[test]
fn languages_merge_once_in_a_fixed_order() {
    let lists = [
        language("toml", "TOML"),
        language("sql", "SQL"),
        language("c", "C"),
        language("php", "PHP"),
        language("typescript", "TypeScript"),
        language("php", "php (another name)"),
    ];
    let names = |l: Vec<SettingsLanguage>| l.into_iter().map(|l| l.name).collect::<Vec<_>>();
    let expected = ["PHP", "TypeScript", "SQL", "C", "TOML"];
    assert_eq!(names(merge_languages(lists.clone())), expected);
    let mut reversed = lists.to_vec();
    reversed.reverse();
    // The first name given wins: reversed, the other one.
    assert_eq!(names(merge_languages(reversed))[0], "php (another name)");
}

/// The pages' order: by group, then their place, then their id.
#[test]
fn pages_sort_by_group_then_place() {
    let mut pages = [
        SettingsPage::DATABASE,
        SettingsPage::FILES,
        SettingsPage::LANGUAGES,
        SettingsPage::EDITOR,
    ];
    pages.sort_by_key(SettingsPage::sort_key);
    let ids: Vec<_> = pages.iter().map(|p| p.id).collect();
    assert_eq!(ids, ["editor", "files", "languages", "database"]);
}
