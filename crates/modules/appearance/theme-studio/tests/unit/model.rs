use super::*;

/// The search folds case and diacritics, and every word must be found.
#[test]
fn the_search_ignores_case_and_diacritics() {
    assert_eq!(fold("Zăpadă ȘI Țară"), "zapada si tara");
    let entries = vec![
        Entry {
            id: SettingId::Name,
            category: Category::General,
            group: "Tema".into(),
            label: "Numele temei".into(),
            words: String::new(),
        },
        Entry {
            id: SettingId::Base,
            category: Category::General,
            group: "Tema".into(),
            label: "Pornește de la".into(),
            words: "Dracula Latte".into(),
        },
    ];
    assert_eq!(search(&entries, "porneste").len(), 1);
    assert_eq!(search(&entries, "DRACULA").len(), 1);
    assert_eq!(search(&entries, "tema numele").len(), 1, "every word, in any order");
    assert_eq!(search(&entries, "tema").len(), 2);
    assert!(search(&entries, "  ").is_empty());
    assert!(search(&entries, "nothing").is_empty());
}
