use super::*;

/// File › Open Recent: the other recent projects (the window's own left out, the user's
/// groups too), then the project's files relative to it, the latest first; the menu bar
/// shows them in a submenu ending with Clear Recent, which is greyed out when both are
/// empty. The recent store is this test thread's own (never the user's).
#[test]
fn open_recent_lists_the_other_projects_and_the_files() {
    let base = std::env::temp_dir().join(format!("open-recent-{}", std::process::id()));
    let (a, b, grouped) = (base.join("alpha"), base.join("beta"), base.join("grouped"));
    for dir in [&a, &b, &grouped] {
        std::fs::create_dir_all(dir).unwrap();
        crate::recent::add(dir);
    }
    crate::recent::update(|list| {
        let group = list.new_group("Clients", None);
        list.move_to(&grouped, group, workspace::recent::Place::Anywhere)
    });
    let files = [a.join("src").join("old.php"), a.join("new.php"), base.join("elsewhere.php")];
    let menu = recent_menu(Some(&a), &files);
    let paths: Vec<_> = menu.projects.iter().map(|(_, p)| p.clone()).collect();
    assert_eq!(paths, std::slice::from_ref(&b), "the window's own and the grouped one left out");
    let labels: Vec<&str> = menu.files.iter().map(|(l, _)| l.as_str()).collect();
    assert_eq!(labels, ["new.php", &format!("src{}old.php", std::path::MAIN_SEPARATOR)]);

    assert!(cmd::recent_menu_is_empty());
    assert!(cmd::set_recent_menu(menu.clone()));
    assert!(!cmd::set_recent_menu(menu), "unchanged: the menus aren't made again");
    let bar = cmd::menus_on(cmd::System::Windows);
    let recent = bar[0]
        .items
        .iter()
        .find_map(|i| match i {
            platform_api::MenuItem::Submenu { title, items, .. }
                if *title == i18n::t("menu.open_recent") =>
            {
                Some(items.clone())
            }
            _ => None,
        })
        .expect("File › Open Recent");
    let ids: Vec<String> = recent
        .iter()
        .map(|i| match i {
            platform_api::MenuItem::Command { id, .. } => id.clone(),
            _ => "---".into(),
        })
        .collect();
    assert_eq!(
        ids,
        [
            format!("{}{}", cmd::OPEN_RECENT_PROJECT, b.display()),
            "---".into(),
            format!("{}{}", cmd::OPEN_RECENT_FILE, a.join("new.php").display()),
            format!("{}{}", cmd::OPEN_RECENT_FILE, a.join("src").join("old.php").display()),
            "---".into(),
            cmd::CLEAR_RECENT.0.into(),
        ]
    );
    assert_eq!(cmd::menu_icon(&ids[0]), Some("folder"));
    assert_eq!(cmd::menu_icon(&ids[2]), Some("file-text"));
    assert!(cmd::set_recent_menu(cmd::RecentMenu::default()));
    assert!(cmd::recent_menu_is_empty());
    let _ = std::fs::remove_dir_all(&base);
}
