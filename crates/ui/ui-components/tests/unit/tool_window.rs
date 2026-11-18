use super::*;

#[test]
fn a_window_described_by_data_makes_nothing_and_says_what_it_is() {
    let git = ToolWindow {
        id: "git",
        icon: "git-branch",
        title_key: "tooltip.git",
        dock: Dock::Bottom,
        order: 10,
        strip_end: false,
        group: "",
        toggle: None,
        content: ToolContent::Views { command: "git.log", new: None },
    };
    assert_eq!(
        (git.id(), git.icon(), git.dock(), git.order()),
        ("git", "git-branch", Dock::Bottom, 10)
    );
    assert_eq!(git.title(), i18n::t("tooltip.git"));
    assert_eq!(git.content(), ToolContent::Views { command: "git.log", new: None });
    // Registering costs only the description: no panel is made.
    assert!(git.create().is_none());
}

#[test]
fn the_docks_sort_top_left_bottom_right() {
    let mut docks = [Dock::Right, Dock::Left, Dock::Bottom, Dock::Top];
    docks.sort();
    assert_eq!(docks, [Dock::Top, Dock::Left, Dock::Bottom, Dock::Right]);
}
