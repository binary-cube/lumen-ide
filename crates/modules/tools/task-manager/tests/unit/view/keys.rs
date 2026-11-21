//! The view's keyboard: every key at every class (the rows with ↑ / ↓, Page Up / Down,
//! Home / End, groups and programs with ← / →, Enter for the detail — a page on a narrow
//! page), Esc in its order, typing into the filter, Tab / ⇧Tab through the controls with a
//! ring shown after a key and not after a click, the popovers' rows by keys.

use super::*;
use crate::model::{Column, Grouping};
use crate::view::tests::{Pen, area, center, demo, manager};
use kernel::Point;
use std::sync::PoisonError;
use ui_components::{Viewer, ViewerCursor, ViewerKey};

/// Every class: its width (logical pixels) and a tall page.
const CLASSES: [f32; 5] = [1300.0, 900.0, 600.0, 400.0, 280.0];

fn press(view: &mut TaskManager, key: ViewerKey) -> bool {
    view.edit_key(key)
}

fn back_tab(view: &mut TaskManager) -> bool {
    view.edit_key_with(ViewerKey::Tab, KeyReach { word: false, extend: true })
}

/// The row the keys are on, by its place in the list.
fn at(view: &mut TaskManager) -> Option<usize> {
    let model = view.model();
    view.cursor_index(&model)
}

fn is_group(view: &mut TaskManager, i: usize) -> bool {
    matches!(view.model().items[i], Item::Group { .. })
}

#[test]
fn the_rows_move_with_every_key_at_every_class() {
    let mut pen = Pen::new();
    for w in CLASSES {
        let mut view = demo("plain");
        pen.paint(&mut view, w, 1400.0, 1.0);
        let count = view.model().items.len();
        assert!(press(&mut view, ViewerKey::Down), "{w}");
        let first = at(&mut view).expect("a row");
        assert!(!is_group(&mut view, first), "{w}: ↓ from nowhere goes to the first part");
        assert!(view.selected().is_some(), "{w}: its part selected");
        press(&mut view, ViewerKey::Down);
        assert_eq!(at(&mut view), Some(first + 1), "{w}: ↓");
        press(&mut view, ViewerKey::Up);
        assert_eq!(at(&mut view), Some(first), "{w}: ↑");
        press(&mut view, ViewerKey::Home);
        assert_eq!(at(&mut view), Some(0), "{w}: Home: the first row, a group's");
        assert!(is_group(&mut view, 0), "{w}: the group rows are rows too");
        press(&mut view, ViewerKey::End);
        assert_eq!(at(&mut view), Some(count - 1), "{w}: End");
        press(&mut view, ViewerKey::Home);
        press(&mut view, ViewerKey::PageDown);
        let page = at(&mut view).unwrap();
        assert!(page > 1 && page < count, "{w}: Page Down moves by a page ({page})");
        press(&mut view, ViewerKey::PageUp);
        assert_eq!(at(&mut view), Some(0), "{w}: Page Up");
        // The row brought into view: the last one's bottom is within the rows' area.
        press(&mut view, ViewerKey::End);
        pen.paint(&mut view, w, 1400.0, 1.0);
        let rows = view.page().unwrap().rows;
        let ring = view.cursor_rect.expect("its row drawn");
        assert!(ring.max_y() <= rows.max_y() + 1.0 && ring.min_y() >= rows.min_y() - 1.0, "{w}");
    }
}

#[test]
fn left_and_right_fold_open_and_go_up_at_every_class() {
    let mut pen = Pen::new();
    for w in CLASSES {
        let mut view = demo("plain");
        pen.paint(&mut view, w, 1400.0, 1.0);
        // On a part: ← goes up to its group; ← again folds it; → opens it, → again its first row.
        press(&mut view, ViewerKey::Down);
        let part = at(&mut view).unwrap();
        press(&mut view, ViewerKey::Left);
        let group = at(&mut view).unwrap();
        assert!(group < part && is_group(&mut view, group), "{w}: ← up to the group");
        let Item::Group { key, .. } = view.model().items[group].clone() else { unreachable!() };
        press(&mut view, ViewerKey::Left);
        assert!(view.state.folded.contains(&key), "{w}: ← folds it");
        assert_eq!(at(&mut view), Some(group), "{w}: the keys stay on it");
        press(&mut view, ViewerKey::Right);
        assert!(!view.state.folded.contains(&key), "{w}: → opens it");
        press(&mut view, ViewerKey::Right);
        assert_eq!(at(&mut view), Some(group + 1), "{w}: → again: its first row");
        // A part with programs: → lists them, → again goes into them, ← back to it, ← folds.
        let terminal = view.model().items.iter().position(|i| match i {
            Item::Part { index, .. } => view.frame.lines[*index].name == i18n::t("tasks.terminal"),
            _ => false,
        });
        let terminal = terminal.expect("listed");
        view.go_to(terminal);
        press(&mut view, ViewerKey::Right);
        let key = view.selected().unwrap().to_owned();
        assert!(view.state.expanded.contains(&key), "{w}: → lists its programs");
        press(&mut view, ViewerKey::Right);
        assert!(
            matches!(view.cursor, Some(RowRef::Process(_, 4242))),
            "{w}: → into its first program: {:?}",
            view.cursor
        );
        assert_eq!(view.selected(), Some(key.as_str()), "{w}: its part stays selected");
        press(&mut view, ViewerKey::Left);
        assert_eq!(view.cursor, Some(RowRef::Part(key.clone())), "{w}: ← back to its part");
        press(&mut view, ViewerKey::Left);
        assert!(!view.state.expanded.contains(&key), "{w}: ← folds its programs");
    }
}

#[test]
fn enter_opens_the_detail_a_page_where_narrow() {
    let mut pen = Pen::new();
    for (w, h, page) in
        [(1300.0, 900.0, false), (900.0, 900.0, false), (600.0, 500.0, true), (400.0, 900.0, true)]
    {
        let mut view = demo("plain");
        pen.paint(&mut view, w, h, 1.0);
        press(&mut view, ViewerKey::Down);
        assert!(!view.detail_open() || !page, "{w}: on a page ↓ doesn't open it");
        press(&mut view, ViewerKey::Enter);
        assert!(view.detail_open(), "{w}×{h}: Enter opens the detail");
        pen.paint(&mut view, w, h, 1.0);
        assert_eq!(view.place() == DetailPlace::Page, page, "{w}×{h}");
        if page {
            assert_eq!(view.focus, Focus::Detail, "{w}: the page has the keys");
            // ↓ scrolls the page (it doesn't move the rows behind it).
            let row = at(&mut view);
            press(&mut view, ViewerKey::Down);
            assert_eq!(at(&mut view), row, "{w}");
        }
        // Esc: back to the list, the same row.
        let row = at(&mut view);
        press(&mut view, ViewerKey::Escape);
        assert!(!view.detail_open(), "{w}: Esc closes it");
        assert_eq!(at(&mut view), row, "{w}: the same row");
        assert_eq!(view.focus, Focus::Table);
    }
    // Enter on a group's row folds it.
    let mut view = demo("plain");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    press(&mut view, ViewerKey::Home);
    let Item::Group { key, .. } = view.model().items[0].clone() else { unreachable!() };
    press(&mut view, ViewerKey::Enter);
    assert!(view.state.folded.contains(&key));
}

#[test]
fn esc_empties_the_filter_then_closes_the_detail_then_unselects() {
    let mut pen = Pen::new();
    let mut view = demo("wide");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    assert!(view.detail_open());
    view.text_input("term");
    assert_eq!(view.focus, Focus::Tool(Tool::Filter), "typing: the filter has the keys");
    press(&mut view, ViewerKey::Escape);
    assert!(view.filter.text().is_empty(), "first the filter");
    assert!(view.detail_open());
    assert_eq!(view.focus, Focus::Table, "then the table has the keys");
    press(&mut view, ViewerKey::Escape);
    assert!(!view.detail_open(), "then the detail");
    assert!(view.selected().is_some());
    press(&mut view, ViewerKey::Escape);
    assert!(view.selected().is_none(), "then the selection");
    assert!(view.cursor.is_none());
    assert!(!press(&mut view, ViewerKey::Escape), "then nothing to undo");
    // The popover first of all.
    view.text_input("x");
    view.open_popover(Popover::Interval);
    press(&mut view, ViewerKey::Escape);
    assert!(view.popover().is_none());
    assert_eq!(view.filter.text(), "x", "the popover went, the filter stays");
}

#[test]
fn typing_goes_into_the_filter_its_caret_moves_there_only() {
    let mut pen = Pen::new();
    let mut view = manager();
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    view.text_input("inter");
    assert_eq!(view.shown_names(), ["Interface"]);
    press(&mut view, ViewerKey::Left);
    press(&mut view, ViewerKey::Backspace);
    assert_eq!(view.filter.text(), "intr", "← moved the caret in the filter");
    press(&mut view, ViewerKey::Home);
    assert_eq!(view.filter.input.cursor(), 0, "Home: the filter's start");
    // ↓ from the filter: through what it finds (the filter keeps the keys, as a search's
    // list).
    view.clear_filter();
    view.text_input("Interface");
    press(&mut view, ViewerKey::Down);
    assert_eq!(view.focus, Focus::Tool(Tool::Filter));
    assert_eq!(view.selected(), Some("app/Interface"));
    // ← / → in the table don't touch the filter.
    view.focus_on(Focus::Table);
    let before = view.filter.input.cursor();
    press(&mut view, ViewerKey::Right);
    assert_eq!(view.filter.input.cursor(), before);
    // A narrow page: typing opens the filter over the bar.
    let mut narrow = manager();
    pen.paint(&mut narrow, 340.0, 900.0, 1.0);
    assert!(narrow.page().unwrap().filter_icon);
    narrow.text_input("p");
    assert!(narrow.filter_open);
}

#[test]
fn tab_goes_through_the_controls_with_a_ring_after_a_key_only() {
    let mut pen = Pen::new();
    for w in CLASSES {
        // The widest with a part's detail beside the list.
        let mut view = demo(if w > 1000.0 { "wide" } else { "plain" });
        view.focused = true;
        pen.paint(&mut view, w, 1400.0, 1.0);
        let stops: Vec<Focus> = view.stops.iter().map(|(_, f)| *f).collect();
        assert!(stops.contains(&Focus::Table), "{w}: the table");
        assert!(stops.contains(&Focus::Tool(Tool::Filter)), "{w}: the filter");
        // The bar, the summary, the table, the detail: in the order drawn.
        let place = |f: Focus| stops.iter().position(|s| *s == f);
        let table = place(Focus::Table).unwrap();
        assert!(place(Focus::Tool(Tool::Filter)).unwrap() < table, "{w}");
        if let Some(card) = place(Focus::Card(Card::Cpu)) {
            assert!(card < table, "{w}: the summary before the table");
        }
        match place(Focus::Detail) {
            Some(detail) => assert!(detail > table, "{w}: the detail after it"),
            None => assert!(w < 1000.0, "{w}: the detail open beside"),
        }
        // Tab from the table: the next one; ⇧Tab back; all the way round.
        assert_eq!(view.focus, Focus::Table);
        press(&mut view, ViewerKey::Tab);
        assert_eq!(view.focus, stops[(table + 1) % stops.len()], "{w}");
        back_tab(&mut view);
        assert_eq!(view.focus, Focus::Table, "{w}: ⇧Tab");
        for _ in 0..stops.len() {
            press(&mut view, ViewerKey::Tab);
        }
        assert_eq!(view.focus, Focus::Table, "{w}: round");
        // Each one's ring drawn around it (the cards draw their own).
        for &stop in &stops {
            view.focus_on(stop);
            pen.paint(&mut view, w, 1400.0, 1.0);
            assert!(view.shows_ring(), "{w}: after a key");
            if !matches!(stop, Focus::Card(_)) {
                let ring = view.ring_rect().expect("a ring");
                let bounds = view.page().unwrap().bounds;
                assert!(ring.intersect(&bounds).is_some(), "{w}: {stop:?} in view");
            }
        }
        // A click: no ring (a key shows it again); what it pressed has the keys.
        let filter = area(&view, |h| matches!(h, Hit::Filter | Hit::FilterIcon)).unwrap();
        view.click(center(filter));
        assert!(!view.shows_ring(), "{w}: no ring after a click");
        assert_eq!(view.focus, Focus::Tool(Tool::Filter), "{w}");
    }
}

#[test]
fn enter_and_arrows_work_the_controls() {
    let mut pen = Pen::new();
    let mut view = demo("plain");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    view.focus_on(Focus::Tool(Tool::Scope));
    press(&mut view, ViewerKey::Right);
    assert!(view.shows_all(), "→ on the scope: every project");
    press(&mut view, ViewerKey::Enter);
    assert!(!view.shows_all(), "Enter: the other one");
    view.focus_on(Focus::Tool(Tool::Grouping));
    press(&mut view, ViewerKey::Enter);
    assert_eq!(view.grouping(), Grouping::List, "Enter: the next grouping");
    view.focus_on(Focus::Tool(Tool::Pause));
    press(&mut view, ViewerKey::Enter);
    assert!(view.is_paused());
    view.focus_on(Focus::Card(Card::Cpu));
    press(&mut view, ViewerKey::Right);
    assert_eq!(view.focus, Focus::Card(Card::Memory), "→ the next card");
    press(&mut view, ViewerKey::Enter);
    assert_eq!(view.sorted_by().0, Column::Memory, "Enter sorts by its metric");
}

#[test]
fn a_popover_is_worked_by_its_keys() {
    let _turn = crate::view::PREFERENCES_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
    let before = crate::view::preferences();
    let mut pen = Pen::new();
    let mut view = demo("plain");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    view.focus_on(Focus::Tool(Tool::Interval));
    press(&mut view, ViewerKey::Enter);
    assert_eq!(view.popover(), Some(Popover::Interval));
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let first = view.menu_at.expect("its first row");
    assert!(!matches!(view.menu[first], MenuAction::Heading(_)), "never a heading");
    press(&mut view, ViewerKey::Down);
    assert_eq!(view.menu[view.menu_at.unwrap()], MenuAction::Interval(2));
    press(&mut view, ViewerKey::Enter);
    assert!(view.popover().is_none(), "chosen: closed");
    assert_eq!(crate::view::preferences().interval, 2);
    // The columns' chooser stays open while its rows are chosen.
    view.focus_on(Focus::Chooser);
    press(&mut view, ViewerKey::Enter);
    assert_eq!(view.popover(), Some(Popover::Columns));
    press(&mut view, ViewerKey::End);
    assert_eq!(view.menu[view.menu_at.unwrap()], MenuAction::ResetColumns);
    press(&mut view, ViewerKey::Tab);
    assert!(view.popover().is_none(), "Tab leaves it");
    view.set_timing(Some(before.interval), Some(before.history));
}

/// ⌘A, ⌘X, ⌘Z / ⌘⇧Z while the filter has the keys: its text, the rows filtered again.
#[test]
fn the_app_s_editing_commands_reach_the_filter() {
    let mut pen = Pen::new();
    let mut view = manager();
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    view.text_input("inter");
    assert!(view.command("select_all"));
    assert_eq!(view.filter.input.selected_text(), "inter", "⌘A: all of it");
    assert!(view.command("cut"));
    assert!(view.filter.text().is_empty(), "⌘X: gone");
    assert!(view.shown_names().len() > 1, "the rows filtered again");
    assert!(view.command("undo"));
    assert_eq!(view.filter.text(), "inter", "⌘Z: back");
    assert_eq!(view.shown_names(), ["Interface"]);
    // The table with the keys: ⌘A isn't the filter's.
    press(&mut view, ViewerKey::Escape);
    press(&mut view, ViewerKey::Escape);
    assert!(!view.command("select_all"));
}

/// The ring reaching the filter's stop gives its part the keys, leaving it takes them away;
/// the parts' own Tab never stops at it.
#[test]
fn the_ring_gives_the_filter_the_keys_and_takes_them_back() {
    let mut pen = Pen::new();
    let mut view = demo("plain");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let stops: Vec<Focus> = view.stops.iter().map(|(_, f)| *f).collect();
    let filter = stops.iter().position(|s| *s == Focus::Tool(Tool::Filter)).expect("its stop");
    assert!(!view.wants_text(), "the table has the keys");
    // From the stop before it, Tab reaches it.
    view.focus_on(stops[(filter + stops.len() - 1) % stops.len()]);
    assert!(!view.wants_text());
    press(&mut view, ViewerKey::Tab);
    assert_eq!(view.focus, Focus::Tool(Tool::Filter));
    assert!(view.wants_text(), "reached: the filter has the keys");
    press(&mut view, ViewerKey::Tab);
    assert_ne!(view.focus, Focus::Tool(Tool::Filter));
    assert!(!view.wants_text(), "left: no longer");
    back_tab(&mut view);
    assert!(view.wants_text(), "⇧Tab back to it");
    // Esc leaves it for the table: the keys go with it.
    press(&mut view, ViewerKey::Escape);
    assert_eq!(view.focus, Focus::Table);
    assert!(!view.wants_text());
    // A click on the field gives them, one elsewhere takes them.
    let field = area(&view, |h| *h == Hit::Filter).expect("the field");
    view.click(center(field));
    assert!(view.wants_text(), "clicked: the filter");
    let row = area(&view, |h| matches!(h, Hit::Row(_))).expect("a row");
    view.click(center(row));
    assert!(!view.wants_text(), "a row clicked: the table");
}

/// Typing and the editing keys in the filter: the caret moved, by word, a selection
/// extended and typed over, Delete; the rows filtered again each time.
#[test]
fn the_filter_s_editing_keys_go_through_its_part() {
    let word = KeyReach { word: true, extend: false };
    let extend = KeyReach { word: false, extend: true };
    let mut pen = Pen::new();
    let mut view = manager();
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    assert!(!view.text_input("\n"), "nothing to type: the keys stay");
    assert_eq!(view.focus, Focus::Table);
    view.text_input("app inter");
    assert!(view.wants_text());
    assert!(view.edit_key_with(ViewerKey::Backspace, word));
    assert_eq!(view.filter.text(), "app ", "⌥⌫: the word");
    press(&mut view, ViewerKey::Home);
    assert!(view.edit_key_with(ViewerKey::End, extend));
    assert_eq!(view.filter.input.selected_text(), "app ", "⇧End: selected");
    view.text_input("inter");
    assert_eq!(view.filter.text(), "inter", "typed over the selection");
    press(&mut view, ViewerKey::Home);
    press(&mut view, ViewerKey::Delete);
    assert_eq!(view.filter.text(), "nter", "Delete: the next one");
    assert_eq!(view.shown_names(), ["Interface"], "filtered again");
    // Backspace with the keys on the table still erases what was typed, the focus stays.
    press(&mut view, ViewerKey::End);
    view.focus_on(Focus::Table);
    press(&mut view, ViewerKey::Backspace);
    assert_eq!(view.filter.text(), "nte");
    assert_eq!(view.focus, Focus::Table);
    assert!(!view.wants_text());
}

/// ⌘A selects the filter, ⌘C copies its selection (else the keys' row), what's pasted goes
/// in on one line (its breaks as spaces, the shared field's way) — only while it has the
/// keys.
#[test]
fn select_all_copy_and_paste_in_the_filter() {
    let mut pen = Pen::new();
    let mut view = manager();
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    view.text_input("inter");
    assert_eq!(view.selected_text(), None, "nothing selected, no row");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("inter"), "⌘C: the filter's");
    assert!(view.paste("Inter\nface"));
    assert_eq!(view.filter.text(), "Inter face", "pasted over it, on one line: a space");
    assert!(view.command("select_all"));
    assert!(view.paste("Interface"));
    assert_eq!(view.shown_names(), ["Interface"]);
    // The table with the keys: nothing pasted, the row copied.
    press(&mut view, ViewerKey::Down);
    view.focus_on(Focus::Table);
    assert!(!view.paste("x"));
    assert_eq!(view.filter.text(), "Interface");
    assert!(!view.command("select_all"));
    let row = view.selected_text().expect("the keys' row");
    assert!(row.contains("Interface"), "{row}");
}

/// The filter is the app's search field: pressed, it has the keys and a drag selects; its ×
/// (shown with text, a hand under the mouse) empties it and the rows are filtered again; the
/// keys elsewhere, it draws no caret (its focus is the view's one).
#[test]
fn the_filter_s_clear_button_empties_it_and_filters_again() {
    let mut pen = Pen::new();
    let mut view = manager();
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let field = area(&view, |h| *h == Hit::Filter).expect("the field");
    // A press puts the caret there, a drag selects what it went over.
    view.click(center(field));
    assert!(view.filter_has_keys(), "pressed: the filter's");
    view.text_input("inter");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let all = view.model().items.len();
    // × sits at the field's right end (the shared field's room for it).
    let clear = Point::new(field.max_x() - 17.0, field.min_y() + field.size.height / 2.0);
    assert!(view.hover(clear), "a hand on ×");
    assert_eq!(view.cursor_at(clear), Some(ViewerCursor::Pointer));
    assert!(view.tooltip_at(clear).is_some(), "× says what it does");
    view.click(clear);
    assert!(view.filter.text().is_empty(), "× empties it");
    assert!(view.model().items.len() >= all, "the rows filtered again");
    assert!(view.shown_names().len() > 1);
    // The keys on the table: the filter no longer has them (its caret goes).
    view.focus_on(Focus::Table);
    assert!(!view.filter_has_keys() && !view.wants_text());
}
