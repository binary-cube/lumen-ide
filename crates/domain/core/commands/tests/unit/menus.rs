use super::*;

#[test]
fn a_section_is_on_the_systems_it_names() {
    let everywhere = MenuSectionSpec::new("file.open", "file", 10);
    assert!(everywhere.platforms.includes(true) && everywhere.platforms.includes(false));
    let mac = everywhere.on(Platforms::MacOs);
    assert!(mac.platforms.includes(true) && !mac.platforms.includes(false));
    let others = everywhere.on(Platforms::Others);
    assert!(!others.platforms.includes(true) && others.platforms.includes(false));
    assert_eq!(MenuAt::new("file.open", 20).order, 20);
}

/// Each requirement against the window's state; the first one missing is the hint's.
#[test]
fn requirements_hold_only_in_their_context() {
    let none = MenuContext::default();
    let all = [
        Requires::Project,
        Requires::Repository,
        Requires::Tab,
        Requires::TextFile,
        Requires::FileOnDisk,
        Requires::Language("sql"),
        Requires::ClosedTab,
        Requires::BackHistory,
        Requires::ForwardHistory,
        Requires::OtherTabs,
        Requires::Lockable,
    ];
    assert!(all.iter().all(|r| !none.satisfies(*r)), "nothing open: none");
    // A repository counts only with its project.
    let repo = MenuContext { repository: true, ..MenuContext::default() };
    assert!(!repo.satisfies(Requires::Repository));
    let project = MenuContext { project: true, ..MenuContext::default() };
    assert_eq!(
        project.missing(&[Requires::Project, Requires::Repository]),
        Some(Requires::Repository)
    );
    let sql = MenuContext {
        project: true,
        repository: true,
        tabs: 2,
        text_file: true,
        file_on_disk: true,
        language: Some("sql".into()),
        closed_tab: true,
        back: true,
        forward: true,
        modified: false,
        lockable: true,
    };
    assert!(all.iter().all(|r| sql.satisfies(*r)));
    let php = MenuContext { language: Some("php".into()), tabs: 1, ..sql };
    assert!(!php.satisfies(Requires::Language("sql")) && !php.satisfies(Requires::OtherTabs));
    assert_eq!(Requires::Repository.hint_key(), "menu.needs_repository");
    let image = MenuContext { lockable: false, ..php };
    assert_eq!(image.missing(&[Requires::Tab, Requires::Lockable]), Some(Requires::Lockable));
}
