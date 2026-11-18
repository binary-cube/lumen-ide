use super::*;

const A: CommandId = CommandId("a");
const B: CommandId = CommandId("b");
const C: CommandId = CommandId("c");

fn menu() -> MenuContribution {
    MenuContribution {
        id: "git",
        label_key: "git.menu",
        icon: "git-branch",
        places: &[MenuPlace::Editor, MenuPlace::TreeFile],
        rows: vec![
            MenuRow::Command(A, &[MenuPlace::Editor]),
            MenuRow::Separator,
            MenuRow::Separator,
            MenuRow::Command(B, EVERYWHERE),
            MenuRow::Separator,
            MenuRow::Command(C, EVERYWHERE),
            MenuRow::Separator,
        ],
    }
}

#[test]
fn rows_follow_the_place_and_what_is_enabled() {
    let all = |_| true;
    assert_eq!(menu().rows_in(MenuPlace::Editor, &all).len(), 5);
    // A isn't in the tree: no leading separator.
    let tree = menu().rows_in(MenuPlace::TreeFile, &all);
    assert_eq!(
        tree,
        [MenuRow::Command(B, EVERYWHERE), MenuRow::Separator, MenuRow::Command(C, EVERYWHERE)]
    );
    // B disabled: one separator between A and C.
    let rows = menu().rows_in(MenuPlace::Editor, &|id| id != B);
    assert_eq!(rows.len(), 3);
    // Not contributed there at all.
    assert!(menu().rows_in(MenuPlace::ToolStrip, &all).is_empty());
}

/// The rows by section and order (equal: as contributed), a separator between sections,
/// "New" and "Compare" as submenus; another place's and disabled ones left out.
#[test]
fn context_items_are_arranged_by_section() {
    const EDITOR: &[MenuPlace] = &[MenuPlace::Editor];
    const TREE: &[MenuPlace] = &[MenuPlace::TreeFile];
    let items = [
        ContextMenuItem::new(C, EDITOR, MenuSection::Clipboard, 10),
        ContextMenuItem::new(B, EDITOR, MenuSection::Refactor, 20),
        ContextMenuItem::new(A, EDITOR, MenuSection::Refactor, 10),
        ContextMenuItem::new(CommandId("d"), EDITOR, MenuSection::Refactor, 20),
        ContextMenuItem::new(CommandId("n"), EDITOR, MenuSection::New, 0),
        ContextMenuItem::new(CommandId("t"), TREE, MenuSection::Navigate, 0),
        ContextMenuItem::new(CommandId("off"), EDITOR, MenuSection::Navigate, 0),
    ];
    let enabled = |i: &ContextMenuItem| i.command.0 != "off";
    let ids: Vec<String> = arrange(&items, MenuPlace::Editor, &enabled)
        .iter()
        .map(|row| match row {
            Arranged::Item(i) => i.command.0.to_owned(),
            Arranged::Submenu(section, items) => {
                let ids: Vec<&str> = items.iter().map(|i| i.command.0).collect();
                format!("{section:?}[{}]", ids.join(","))
            }
            Arranged::Separator => "-".to_owned(),
        })
        .collect();
    assert_eq!(ids, ["New[n]", "-", "a", "b", "d", "-", "c"]);
    assert!(arrange(&items, MenuPlace::ToolStrip, &enabled).is_empty());
}

/// A file's own tool's rows (Run) come before every other section, the code's (Comment)
/// between the navigation's and the clipboard's; a row applies everywhere unless it says
/// where.
#[test]
fn the_run_section_comes_first_and_rows_say_where_they_apply() {
    const EDITOR: &[MenuPlace] = &[MenuPlace::Editor];
    let items = [
        ContextMenuItem::new(C, EDITOR, MenuSection::Clipboard, 10),
        ContextMenuItem::new(B, EDITOR, MenuSection::Code, 10),
        ContextMenuItem::new(CommandId("n"), EDITOR, MenuSection::New, 0),
        ContextMenuItem::new(CommandId("g"), EDITOR, MenuSection::Navigate, 0),
        ContextMenuItem::new(A, EDITOR, MenuSection::Run, 10),
    ];
    let ids: Vec<String> = arrange(&items, MenuPlace::Editor, &|_| true)
        .iter()
        .map(|row| match row {
            Arranged::Item(i) => i.command.0.to_owned(),
            Arranged::Submenu(section, _) => format!("{section:?}"),
            Arranged::Separator => "-".to_owned(),
        })
        .collect();
    assert_eq!(ids, ["a", "-", "New", "-", "g", "-", "b", "-", "c"]);
    assert_eq!(items[0].when, MenuWhen::Always);
    let only_sql = items[1].when(MenuWhen::Language("sql"));
    assert_eq!(only_sql.when, MenuWhen::Language("sql"));
    assert_eq!(only_sql.command, B, "the rest kept");
}
