//! The view's commands and right-click menus: a row's (the keys move to it; show it in the
//! detail, copy it, copy its programs' ids — run from the menu), a header's (sort by it,
//! hide it — saved —, the chooser), the palette's actions (pause, filter, sort, columns).

use super::*;
use crate::view::tests::{Pen, area, center, demo};
use std::sync::PoisonError;
use ui_components::{ViewMenuEntry, Viewer};

fn ids(entries: &[ViewMenuEntry]) -> Vec<&str> {
    entries
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect()
}

/// The first row drawn of the part named `name`.
fn row_of(view: &TaskManager, name: &str) -> kernel::Point {
    let model = &view.model;
    let i = model
        .items
        .iter()
        .position(|i| match i {
            Item::Part { index, .. } => model.frame.lines[*index].name == name,
            _ => false,
        })
        .expect("listed");
    center(area(view, |h| *h == Hit::Row(i)).expect("drawn"))
}

fn copied(view: &mut TaskManager) -> Option<String> {
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::CopyText(text))) => Some(text),
        _ => None,
    }
}

#[test]
fn a_rows_menu_runs_its_actions() {
    let mut pen = Pen::new();
    let mut view = demo("plain");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let at = row_of(&view, &i18n::t("tasks.terminal"));
    let entries = view.context_menu(at).expect("a row's menu");
    // Its actions first (closing its session), then its own rows.
    assert_eq!(ids(&entries), ["act:close-session:p4242", "detail", "copy-row", "copy-pid"]);
    let key = view.selected().expect("the keys moved to it").to_owned();
    assert_eq!(view.frame.line(&key).unwrap().name, i18n::t("tasks.terminal"));
    assert!(!view.detail_open(), "only selected: the menu opens nothing");
    // Copy the row: its columns tab-separated.
    assert!(view.command("copy-row"));
    let row = copied(&mut view).expect("copied");
    let cells: Vec<&str> = row.split('\t').collect();
    assert_eq!(cells[0], i18n::t("tasks.terminal"));
    assert_eq!(cells.len(), COLUMNS.len() - 1, "every column but the trend: {row}");
    assert_eq!(view.selected_text(), Some(row), "⌘C copies the same");
    // Its programs' ids.
    assert!(view.command("copy-pid"));
    assert_eq!(copied(&mut view).as_deref(), Some("4242 4243"));
    // Show in the detail.
    assert!(view.command("detail"));
    assert!(view.detail_open());
    // A program's row: its own id.
    view.toggle_expanded(&key);
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let program = view.model().items.iter().position(|i| matches!(i, Item::Process { .. }));
    let program = area(&view, |h| *h == Hit::Row(program.unwrap())).unwrap();
    view.context_menu(center(program)).expect("a program's menu");
    assert!(view.command("copy-pid"));
    assert_eq!(copied(&mut view).as_deref(), Some("4242"));
    assert!(view.command("copy-row"));
    assert!(copied(&mut view).unwrap().starts_with("zsh · 4242\t"));
    // A part without programs offers no id to copy.
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let at = row_of(&view, &i18n::t("tasks.ui"));
    assert_eq!(ids(&view.context_menu(at).unwrap()), ["detail", "copy-row"]);
    assert!(!view.command("copy-pid"));
}

#[test]
fn a_groups_menu_folds_it() {
    let mut pen = Pen::new();
    let mut view = demo("plain");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let group = area(&view, |h| matches!(h, Hit::Group(_))).unwrap();
    let Some(Hit::Group(key)) = view.hit(center(group)) else { unreachable!() };
    let entries = view.context_menu(center(group)).unwrap();
    assert_eq!(ids(&entries), ["toggle-group", "copy-row"]);
    assert!(view.command("toggle-group"));
    assert!(view.state.folded.contains(&key));
    assert!(view.command("copy-row"));
    assert!(copied(&mut view).is_some());
}

#[test]
fn a_headers_menu_sorts_and_hides_its_column() {
    let _turn = crate::view::PREFERENCES_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut pen = Pen::new();
    let mut view = demo("plain");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let header = area(&view, |h| *h == Hit::Header(ColumnId::Memory)).unwrap();
    let entries = view.context_menu(center(header)).unwrap();
    assert_eq!(ids(&entries), ["sort:memory", "hide-column:memory", "columns", "reset-columns"]);
    assert!(view.command("hide-column:memory"));
    assert_eq!(view.choices()[ColumnId::Memory.index()], Choice::Hidden);
    assert_eq!(view.take_action(), Some(ViewerAction::RunCommand(crate::SAVE.0.to_owned())));
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    assert!(!view.page().unwrap().columns.shows(ColumnId::Memory), "hidden");
    assert!(view.command("sort:memory"));
    assert_eq!(view.sorted_by(), (Column::Memory, true));
    // The name can't be hidden.
    let name = area(&view, |h| *h == Hit::Header(ColumnId::Name)).unwrap();
    assert!(!ids(&view.context_menu(center(name)).unwrap()).contains(&"hide-column:name"));
    assert!(!view.command("hide-column:name"));
    assert!(view.command("reset-columns"));
    assert_eq!(view.choices()[ColumnId::Memory.index()], Choice::Auto);
    // Elsewhere: no menu of its own.
    assert!(view.context_menu(kernel::Point::new(-10.0, -10.0)).is_none());
}

#[test]
fn the_palettes_actions_reach_the_view() {
    let mut pen = Pen::new();
    let mut view = demo("plain");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    // Every action the module declares is one the view runs.
    for (_, _, _, action) in crate::ACTIONS {
        let mut probe = demo("plain");
        pen.paint(&mut probe, 1300.0, 900.0, 1.0);
        // On a part with programs (an id to copy).
        let terminal = probe.model().items.iter().position(|i| match i {
            Item::Part { index, .. } => probe.frame.lines[*index].name == i18n::t("tasks.terminal"),
            _ => false,
        });
        probe.go_to(terminal.unwrap());
        assert!(probe.command(action), "{action}");
    }
    assert!(view.command("pause"));
    assert!(view.is_paused());
    assert!(view.command("pause"));
    assert!(!view.is_paused());
    assert!(view.command("sort:disk"));
    assert_eq!(view.sorted_by().0, Column::Disk);
    assert!(view.command("find"));
    assert_eq!(view.focus, super::super::Focus::Tool(crate::layout::Tool::Filter));
    assert!(view.command("columns"));
    assert_eq!(view.popover(), Some(Popover::Columns));
    assert!(!view.command("nonsense"));
    assert!(!view.command("sort:trend"), "the trend sorts nothing");
}
