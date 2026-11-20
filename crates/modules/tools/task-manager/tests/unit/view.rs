//! The Task Manager's view: sorting (headers, cards), filtering, folding, selecting (the
//! detail on the right, below or as a page at each class, "‹ Back" to the same row and
//! scroll, a part gone kept with its last values), the columns' chooser saved and applied,
//! pausing, the empty states, every icon button's tooltip, long names cut with their whole
//! in a tooltip, the separator's handle on top, a continuous resize from 1200 to 260 px
//! without overlaps, and a repaint of the same frame allocating nothing.

use super::*;
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use ui_components::{Viewer, ViewerKey};
use workbench_model::resources::{APP, INDEXING, MODULES, SERVICES};
use workbench_model::{Activity, ActivityKind, ActivityUnit};

pub(super) fn line(name: &str, owner: &str, group: &'static str, cpu: f32, memory: u64) -> Line {
    let mut line = Line::new(name, owner, group, None);
    line.cpu = cpu;
    line.memory = Some(memory);
    line.threads = 1;
    line
}

pub(super) fn manager() -> TaskManager {
    let mut view = TaskManager::default();
    view.set_all(false);
    view.take_frame(Frame {
        lines: vec![
            line("Index and search", "app", INDEXING, 12.0, 30),
            line("Interface", "app", APP, 40.0, 5),
            line("Remote: shop", "remote.projects", SERVICES, 1.0, 80),
            line("PHP", "lang.php", MODULES, 0.0, 0),
        ],
        ..Frame::default()
    });
    view
}

/// What draws the view in the tests: a real text system, the icons, a theme.
pub(super) struct Pen {
    text: TextSystem,
    icons: icons::Icons,
    palette: ui_components::Palette,
    scene: Scene,
}

impl Pen {
    pub(super) fn new() -> Self {
        Self {
            text: TextSystem::new(),
            icons: icons::Icons::builtin(),
            palette: ui_components::Palette::dark(),
            scene: Scene::default(),
        }
    }

    /// `view` drawn in `w` × `h` logical pixels at `scale`.
    pub(super) fn paint(&mut self, view: &mut TaskManager, w: f32, h: f32, scale: f32) {
        self.scene.reset(self.palette.editor_background);
        let bounds = Rect::new(0.0, 0.0, w * scale, h * scale);
        view.draw(&mut self.scene, &mut self.text, &self.icons, &self.palette, bounds, scale);
    }
}

pub(super) fn demo(scenario: &str) -> TaskManager {
    let mut view = TaskManager::default();
    view.use_demo(scenario);
    view
}

/// The area of the first hit `wanted` says yes to.
pub(super) fn area(view: &TaskManager, wanted: impl Fn(&Hit) -> bool) -> Option<Rect> {
    view.hits.iter().find(|(_, h)| wanted(h)).map(|(r, _)| *r)
}

pub(super) fn center(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

/// A click on the first hit `wanted` says yes to.
pub(super) fn click(
    view: &mut TaskManager,
    wanted: impl Fn(&Hit) -> bool,
) -> Option<ui_components::ViewerAction> {
    let r = area(view, wanted).expect("drawn");
    view.click(center(r))
}

#[test]
fn rows_sort_by_a_column_and_filter_as_typed() {
    let mut view = manager();
    assert_eq!(view.shown_names()[0], "Interface", "the busiest first");
    view.sort_by(Column::Memory);
    assert_eq!(view.shown_names()[0], "Remote: shop", "the largest first");
    view.sort_by(Column::Memory);
    assert_eq!(view.shown_names()[0], "PHP", "again: the other way");
    view.text_input("remote");
    assert_eq!(view.shown_names(), ["Remote: shop"]);
    view.edit_key(ViewerKey::Escape);
    assert_eq!(view.shown_names().len(), 4, "Esc clears the filter first");
}

#[test]
fn groups_fold_the_modules_start_folded() {
    let mut view = manager();
    assert_eq!(
        view.visible_keys(),
        ["app/Interface", "app/Index and search", "remote.projects/Remote: shop"]
    );
    view.toggle_group(GroupKey::Kind(MODULES));
    assert!(view.visible_keys().contains(&"lang.php/PHP".to_owned()));
    view.toggle_group(GroupKey::Kind(APP));
    assert!(!view.visible_keys().contains(&"app/Interface".to_owned()));
    view.text_input("interface");
    assert_eq!(view.visible_keys(), ["app/Interface"]);
}

#[test]
fn the_model_is_built_once_per_look_or_change() {
    let mut view = manager();
    let first = view.model();
    assert!(Arc::ptr_eq(&first, &view.model()), "the same look and state: the same model");
    view.sort_by(Column::Name);
    let sorted = view.model();
    assert!(!Arc::ptr_eq(&first, &sorted), "a change: built again");
    view.take_frame(Frame::default());
    assert!(!Arc::ptr_eq(&sorted, &view.model()), "a look: built again");
}

/// A part selected opens its detail; it survives the looks; gone, it stays with its last
/// values, said gone.
#[test]
fn a_selected_part_stays_and_says_when_it_is_gone() {
    let mut view = manager();
    view.edit_key(ViewerKey::Down);
    assert_eq!(view.selected(), Some("app/Interface"));
    view.select("app/Interface");
    assert!(view.selected().is_none(), "selected again: unselected");
    view.select("remote.projects/Remote: shop");
    assert!(view.detail_open());
    view.take_frame(Frame {
        lines: vec![line("Remote: shop", "remote.projects", SERVICES, 2.0, 1)],
        ..Frame::default()
    });
    assert_eq!(view.selected(), Some("remote.projects/Remote: shop"));
    view.take_frame(Frame {
        lines: vec![line("Interface", "app", APP, 1.0, 1)],
        ..Frame::default()
    });
    let selection = view.selection().expect("kept");
    assert!(selection.gone, "gone, said so");
    assert_eq!(selection.line.memory, Some(1), "its last values");
    assert!(view.detail_open(), "the detail isn't shut on it");
}

#[test]
fn the_scope_is_kept_and_groups_by_project() {
    let mut view = TaskManager::default();
    view.set_all(true);
    assert_eq!(view.state.grouping, Grouping::Project);
    assert!(TaskManager::default().shows_all());
    view.set_all(false);
    assert_eq!(view.state.grouping, Grouping::Kind);
    assert!(!TaskManager::default().shows_all());
}

#[test]
fn an_operation_selects_its_part() {
    let mut remote = line("Remote: shop", "remote.projects", SERVICES, 1.0, 80);
    remote.activities = vec![Activity {
        id: kernel::id::new_uuid(),
        project: None,
        kind: ActivityKind::Upload,
        subject: "index.php".into(),
        detail: String::new(),
        text: None,
        unit: ActivityUnit::Bytes,
        done: 1,
        total: Some(2),
        started: Instant::now(),
        ended: None,
        stoppable: false,
    }];
    let mut view = TaskManager::default();
    view.take_frame(Frame { lines: vec![remote], ..Frame::default() });
    let row = view.model().items.iter().position(|i| matches!(i, Item::Operation(_))).unwrap();
    view.click_hit(Hit::Row(row));
    assert_eq!(view.selected(), Some("remote.projects/Remote: shop"));
}

#[test]
fn a_purge_builds_everything_again() {
    let mut view = manager();
    let mut pen = Pen::new();
    pen.paint(&mut view, 1200.0, 800.0, 1.0);
    let before = view.model();
    view.purge();
    assert!(view.page().is_none(), "the layout went");
    let after = view.model();
    assert!(!Arc::ptr_eq(&before, &after));
    assert_eq!(before.keys, after.keys);
    pen.paint(&mut view, 1200.0, 800.0, 1.0);
    assert!(view.page().is_some());
}

/// A click on a header sorts by it (again: the other way); on a card, by its metric; on a
/// group's header, it folds; on a row, it's selected with its detail.
#[test]
fn clicks_on_headers_cards_groups_and_rows() {
    // The columns are the settings' (another test hides one).
    let _turn = PREFERENCES_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut view = demo("plain");
    let mut pen = Pen::new();
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    click(&mut view, |h| *h == Hit::Header(ColumnId::Memory));
    assert_eq!(view.sorted_by(), (Column::Memory, true));
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    click(&mut view, |h| *h == Hit::Header(ColumnId::Memory));
    assert_eq!(view.sorted_by(), (Column::Memory, false), "again: the other way");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    click(&mut view, |h| *h == Hit::Card(Card::Cpu));
    assert_eq!(view.sorted_by().0, Column::Cpu, "a card sorts by its metric");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let folded = view.model().items.len();
    click(&mut view, |h| matches!(h, Hit::Group(GroupKey::Kind(g)) if *g == APP));
    assert!(view.model().items.len() < folded, "the group folded");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    click(&mut view, |h| matches!(h, Hit::Row(_)));
    assert!(view.selected().is_some() && view.detail_open());
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    assert_eq!(view.page().unwrap().place, DetailPlace::Right);
    assert!(view.page().unwrap().detail.is_some());
    // A part's chevron opens its programs.
    let before = view.model().items.len();
    click(&mut view, |h| matches!(h, Hit::Expand(_)));
    assert!(view.model().items.len() != before);
}

/// The detail at each class: on the right, below, a page — by the width and the height.
#[test]
fn the_detail_at_every_class() {
    let mut pen = Pen::new();
    for (w, h, place) in [
        (1300.0, 900.0, DetailPlace::Right),
        (900.0, 900.0, DetailPlace::Below),
        (900.0, 500.0, DetailPlace::Page),
        (600.0, 900.0, DetailPlace::Below),
        (600.0, 600.0, DetailPlace::Page),
        (420.0, 900.0, DetailPlace::Page),
        (280.0, 900.0, DetailPlace::Page),
    ] {
        let mut view = demo("wide");
        pen.paint(&mut view, w, h, 1.0);
        let page = view.page().unwrap();
        assert_eq!(page.place, place, "{w}×{h}");
        let detail = page.detail.expect("open");
        match place {
            DetailPlace::Right => assert!(detail.min_x() >= page.table.unwrap().max_x()),
            DetailPlace::Below => assert!(detail.min_y() >= page.table.unwrap().max_y()),
            DetailPlace::Page => {
                assert!(page.table.is_none(), "{w}×{h}: in place of the table");
                assert!(area(&view, |h| *h == Hit::Back).is_some(), "‹ Back");
            }
        }
    }
}

/// On a page, "‹ Back" (or Esc) returns to the list at the same row and scroll.
#[test]
fn back_returns_to_the_same_row_and_scroll() {
    let mut view = demo("plain");
    let mut pen = Pen::new();
    pen.paint(&mut view, 420.0, 400.0, 1.0);
    view.scroll(0.0, 60.0);
    pen.paint(&mut view, 420.0, 400.0, 1.0);
    let scroll = view.scroll;
    assert!(scroll > 0.0);
    let row = area(&view, |h| matches!(h, Hit::Row(_))).unwrap();
    view.click(center(row));
    let selected = view.selected().map(str::to_owned);
    assert!(view.detail_open());
    pen.paint(&mut view, 420.0, 400.0, 1.0);
    assert!(view.page().unwrap().table.is_none());
    click(&mut view, |h| *h == Hit::Back);
    assert!(!view.detail_open());
    assert_eq!(view.scroll, scroll, "the same scroll");
    assert_eq!(view.selected().map(str::to_owned), selected, "the same row");
    // Esc does the same.
    view.open_detail();
    view.edit_key(ViewerKey::Escape);
    assert!(!view.detail_open() && view.scroll == scroll);
}

/// The chooser: a column unchecked is hidden (saved through the module's command), checked
/// again it's kept; "By priority" forgets the choices.
#[test]
fn the_columns_chooser_is_saved_and_applied() {
    let _turn = PREFERENCES_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut view = demo("plain");
    let mut pen = Pen::new();
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    assert!(view.page().unwrap().columns.shows(ColumnId::Gpu));
    click(&mut view, |h| *h == Hit::Open(Popover::Columns));
    assert_eq!(view.popover(), Some(Popover::Columns));
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let action = click(&mut view, |h| *h == Hit::Menu(MenuAction::Column(ColumnId::Gpu)));
    assert_eq!(action, Some(ui_components::ViewerAction::RunCommand(crate::SAVE.0.to_owned())));
    assert_eq!(view.choices()[ColumnId::Gpu.index()], Choice::Hidden);
    assert_eq!(preferences().choices[ColumnId::Gpu.index()], Choice::Hidden, "handed to the save");
    assert_eq!(view.popover(), Some(Popover::Columns), "still open: several in a row");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    assert!(!view.page().unwrap().columns.shows(ColumnId::Gpu), "applied");
    click(&mut view, |h| *h == Hit::Menu(MenuAction::Column(ColumnId::Gpu)));
    assert_eq!(view.choices()[ColumnId::Gpu.index()], Choice::Pinned, "checked again: kept");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    click(&mut view, |h| *h == Hit::Menu(MenuAction::ResetColumns));
    assert!(view.choices().iter().all(|c| *c == Choice::Auto));
    // Written as the setting says them, and read back.
    let mut choices = [Choice::Auto; COLUMNS.len()];
    choices[ColumnId::Gpu.index()] = Choice::Pinned;
    choices[ColumnId::Threads.index()] = Choice::Hidden;
    let text = crate::settings::columns_text(&choices);
    assert_eq!(text, "+gpu,-threads");
    assert_eq!(crate::settings::choices_of(&text), choices);
    // A press outside closes the chooser, nothing else.
    view.popover = Some((Popover::Columns, Rect::default()));
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let sorted = view.sorted_by();
    click(&mut view, |h| *h == Hit::Header(ColumnId::Memory));
    assert!(view.popover().is_none());
    assert_eq!(view.sorted_by(), sorted);
    set_preferences(Preferences::default());
}

/// Paused: what's shown stays, the next look waits; resumed, it's shown.
#[test]
fn pause_keeps_what_is_shown() {
    let mut view = manager();
    let mut pen = Pen::new();
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    click(&mut view, |h| *h == Hit::Pause);
    assert!(view.is_paused());
    view.take_frame(Frame::default());
    assert_eq!(view.shown_names().len(), 4, "the look waits");
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    click(&mut view, |h| *h == Hit::Resume);
    assert!(!view.is_paused());
    assert!(view.shown_names().is_empty(), "the last look shown");
}

/// Nothing matching the filter: said, with a button clearing it.
#[test]
fn nothing_matches_offers_to_clear_the_filter() {
    let mut view = manager();
    let mut pen = Pen::new();
    view.text_input("zzzz");
    pen.paint(&mut view, 900.0, 700.0, 1.0);
    click(&mut view, |h| *h == Hit::ClearFilter);
    assert_eq!(view.shown_names().len(), 4);
}

/// Every icon button and header says what it is.
#[test]
fn every_icon_button_has_a_tooltip() {
    // The counter counts (an allocation seen as one).
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    let probe: Vec<u8> = Vec::with_capacity(16);
    COUNTING.with(|on| on.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 1);
    drop(probe);
    let mut pen = Pen::new();
    for (w, scenario) in [(1300.0, "wide"), (700.0, "plain"), (300.0, "page")] {
        let mut view = demo(scenario);
        view.text_input("e");
        pen.paint(&mut view, w, 900.0, 1.0);
        let icons = view.hits.iter().filter(|(_, h)| {
            matches!(
                h,
                Hit::FilterIcon
                    | Hit::FilterClear
                    | Hit::Pause
                    | Hit::Open(_)
                    | Hit::CloseDetail
                    | Hit::Back
                    | Hit::Header(_)
            )
        });
        let mut seen = 0;
        for (r, hit) in icons.clone() {
            let tip = view.tooltip_at(center(*r));
            assert!(tip.is_some_and(|t| !t.label.is_empty()), "{hit:?} at {w} has no tooltip");
            seen += 1;
        }
        assert!(seen >= 3, "{w}: {seen} buttons");
    }
}

/// A long name is cut with "…" inside its column; its whole is its tooltip.
#[test]
fn long_names_are_cut_and_whole_in_their_tooltip() {
    let long = "A part with a very long name that never fits in a narrow table's column";
    let mut view = manager();
    let mut frame = (*view.frame).clone();
    frame.lines.push(line(long, "app", APP, 99.0, 9));
    view.take_frame(frame);
    let mut pen = Pen::new();
    pen.paint(&mut view, 560.0, 900.0, 1.0);
    let (r, _) = *view.cut.first().expect("cut");
    let column = view.page().unwrap().columns.get(ColumnId::Name).unwrap();
    assert!(r.max_x() <= column.0 + column.1 + 0.5, "inside its column");
    let tip = view.tooltip_at(center(r)).expect("its tooltip");
    assert_eq!(tip.label, long);
}

/// The separator's handle is drawn over both panes (nothing painted after it covers it).
#[test]
fn the_separator_is_drawn_on_top() {
    let mut pen = Pen::new();
    for (w, h) in [(1300.0, 900.0), (900.0, 900.0)] {
        let mut view = demo("wide");
        pen.paint(&mut view, w, h, 2.0);
        assert!(view.splitter().is_on_top(&pen.scene), "{w}×{h}");
    }
}

/// The detail's separator dragged: the share follows (within its limits), saved on
/// release.
#[test]
fn dragging_the_separator_keeps_a_ratio() {
    let _turn = PREFERENCES_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
    let mut view = demo("wide");
    let mut pen = Pen::new();
    pen.paint(&mut view, 1400.0, 900.0, 1.0);
    let handle = view.splitter().handle();
    assert!(view.click(center(handle)).is_none());
    let to = Point::new(center(handle).x - 200.0, center(handle).y);
    assert!(view.drag(to));
    assert!(view.ratio_right > 0.30);
    assert!(view.release());
    assert_eq!(
        view.action.take(),
        Some(ui_components::ViewerAction::RunCommand(crate::SAVE.0.to_owned()))
    );
    assert!((preferences().ratio - view.ratio_right).abs() < 1e-6);
    set_preferences(Preferences::default());
}

/// The detail's share is the project's: let go, it's kept in its sizes (and in the
/// settings, for projects where it was never dragged); the next view of the project takes
/// it, whatever the settings say.
#[test]
fn the_detail_s_share_is_kept_in_the_project_s_sizes() {
    let _turn = PREFERENCES_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
    let sizes = workbench_model::LayoutSizes::new();
    let mut view = demo("wide");
    view.set_data(&sizes);
    let mut pen = Pen::new();
    pen.paint(&mut view, 1400.0, 900.0, 1.0);
    let handle = view.splitter().handle();
    assert!(view.click(center(handle)).is_none());
    assert!(view.drag(Point::new(center(handle).x - 150.0, center(handle).y)));
    assert!(view.release());
    let kept = sizes.get(DETAIL_RIGHT).expect("kept in the project");
    assert!((kept - view.ratio_right).abs() < 1e-3);
    set_preferences(Preferences::default());
    let mut next = demo("wide");
    assert!((next.ratio_right - kept).abs() > 1e-3);
    next.set_data(&sizes);
    assert!((next.ratio_right - kept).abs() < 1e-6);
    // Below the list, its own share.
    sizes.set(DETAIL_BELOW, 0.6);
    next.set_data(&sizes);
    assert!((next.ratio_below - 0.6).abs() < 1e-6);
}

/// A column's edge dragged: the cursor says so over it, the width follows the mouse
/// (never under the column's least), the last column takes what's left, the widths are
/// saved on release and a new view takes them; Reset Columns forgets them.
#[test]
fn dragging_a_column_s_edge_resizes_it_and_is_kept() {
    let _turn = PREFERENCES_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
    set_preferences(Preferences::default());
    let mut view = demo("plain");
    let mut pen = Pen::new();
    pen.paint(&mut view, 1300.0, 900.0, 2.0);
    let before = view.page().unwrap().columns.get(ColumnId::Name).unwrap();
    let edge = area(&view, |h| *h == Hit::Edge(ColumnId::Name)).expect("the name's edge");
    assert_eq!(view.cursor_at(center(edge)), Some(ui_components::ViewerCursor::ResizeHorizontal));
    assert!(view.hover(center(edge)), "lit under the mouse");
    // The last column shown has no edge (it reaches the chooser).
    let last = view.page().unwrap().columns.shown.last().unwrap().0;
    assert!(area(&view, |h| *h == Hit::Edge(last)).is_none());
    // Pressed (not a sort), dragged 100 logical pixels narrower.
    let sorted = view.sorted_by();
    assert!(view.click(center(edge)).is_none());
    assert_eq!(view.sorted_by(), sorted);
    assert!(view.drag(Point::new(center(edge).x - 200.0, center(edge).y)));
    assert_eq!(view.column_widths()[ColumnId::Name.index()], (before.1 / 2.0 - 100.0).round());
    pen.paint(&mut view, 1300.0, 900.0, 2.0);
    let page = view.page().unwrap();
    let name = page.columns.get(ColumnId::Name).unwrap();
    assert!((name.1 - (before.1 - 200.0)).abs() <= 2.0, "{name:?} from {before:?}");
    let (_, x, w) = *page.columns.shown.last().unwrap();
    let chooser = page.chooser.unwrap();
    assert!(chooser.min_x() - (x + w) <= 2.0 * 12.0, "the last column takes what's left");
    // Far past its least: it stops there.
    assert!(view.drag(Point::new(0.0, center(edge).y)));
    assert_eq!(view.column_widths()[ColumnId::Name.index()], ColumnId::Name.min_width());
    // Let go: saved, and the next view has it.
    assert!(view.release());
    assert_eq!(
        view.action.take(),
        Some(ui_components::ViewerAction::RunCommand(crate::SAVE.0.to_owned()))
    );
    assert_eq!(preferences().widths[ColumnId::Name.index()], ColumnId::Name.min_width());
    let mut next = demo("plain");
    pen.paint(&mut next, 1300.0, 900.0, 2.0);
    let name = next.page().unwrap().columns.get(ColumnId::Name).unwrap();
    assert_eq!(name.1, ColumnId::Name.min_width() * 2.0);
    // Reset: as their specs say again.
    next.reset_columns();
    assert!(next.column_widths().iter().all(|w| *w == 0.0));
    assert_eq!(preferences().widths, [0.0; COLUMNS.len()]);
    set_preferences(Preferences::default());
}

/// A column's edge double-clicked: the column fits its widest content (a long name shown
/// whole, a number's column its widest value).
#[test]
fn a_double_click_on_an_edge_fits_the_column() {
    let _turn = PREFERENCES_TESTS.lock().unwrap_or_else(PoisonError::into_inner);
    set_preferences(Preferences::default());
    let long = "Formatters and checkers of every language the project has";
    let mut view = manager();
    let mut frame = (*view.frame).clone();
    frame.lines.push(line(long, "app", APP, 99.0, 9));
    view.take_frame(frame);
    let mut pen = Pen::new();
    pen.paint(&mut view, 1600.0, 900.0, 1.0);
    let edge = area(&view, |h| *h == Hit::Edge(ColumnId::Name)).expect("the name's edge");
    // Narrowed first, so the long name is cut.
    assert!(view.click(center(edge)).is_none());
    view.drag(Point::new(0.0, center(edge).y));
    view.release();
    view.action.take();
    pen.paint(&mut view, 1600.0, 900.0, 1.0);
    assert!(!view.cut.is_empty(), "cut while narrow");
    let edge = area(&view, |h| *h == Hit::Edge(ColumnId::Name)).unwrap();
    let action = view.pointer_down(center(edge), 2, ui_components::PressMods::default());
    assert!(action.is_none(), "fitted at the next paint");
    pen.paint(&mut view, 1600.0, 900.0, 1.0);
    assert!(
        view.cut.is_empty(),
        "every name whole once fitted: {:?} {:?} {:?}",
        view.cut,
        view.page().unwrap().columns.get(ColumnId::Name),
        view.column_widths()
    );
    let fitted = view.column_widths()[ColumnId::Name.index()];
    let name = view.page().unwrap().columns.get(ColumnId::Name).unwrap();
    assert_eq!(name.1, fitted);
    assert_eq!(preferences().widths[ColumnId::Name.index()], fitted, "saved");
    // A number's column: its widest text (its header's or a value's).
    pen.paint(&mut view, 1600.0, 900.0, 1.0);
    let edge = area(&view, |h| *h == Hit::Edge(ColumnId::Memory)).unwrap();
    view.pointer_down(center(edge), 2, ui_components::PressMods::default());
    pen.paint(&mut view, 1600.0, 900.0, 1.0);
    let memory = view.column_widths()[ColumnId::Memory.index()];
    assert!(memory > 0.0 && memory < ColumnId::Memory.spec().1 + 20.0, "{memory}");
    set_preferences(Preferences::default());
}

/// Region by region, the hits of a page: the toolbar, the summary, the table, the detail.
fn region(hit: &Hit) -> Option<u8> {
    Some(match hit {
        Hit::Scope(_)
        | Hit::Grouping(_)
        | Hit::Filter
        | Hit::FilterIcon
        | Hit::FilterClear
        | Hit::Pause => 0,
        Hit::Open(Popover::More | Popover::Interval | Popover::Sort) => 0,
        Hit::Card(_) | Hit::CardValue(_) | Hit::CardChart(_) => 1,
        Hit::Header(_)
        | Hit::Edge(_)
        | Hit::Group(_)
        | Hit::Row(_)
        | Hit::Expand(_)
        | Hit::Open(Popover::Columns)
        | Hit::ClearFilter
        | Hit::Resume => 2,
        Hit::CloseDetail | Hit::Back | Hit::DetailChart(_) | Hit::Action(_) => 3,
        Hit::Menu(_) | Hit::PopoverArea => {
            return None;
        }
    })
}

/// Dragged from 1200 to 260 px, at two heights and two scales: no panic, every clickable
/// area inside the view, no two regions' areas overlapping.
#[test]
fn a_continuous_resize_keeps_everything_apart() {
    let mut pen = Pen::new();
    for scale in [1.0, 2.0] {
        for h in [900.0, 480.0] {
            let mut view = demo("wide");
            let mut w = 1200.0;
            while w >= 260.0 {
                pen.paint(&mut view, w, h, scale);
                let bounds = Rect::new(0.0, 0.0, w * scale, h * scale);
                for (r, hit) in &view.hits {
                    assert!(
                        r.min_x() >= -0.5
                            && r.min_y() >= -0.5
                            && r.max_x() <= bounds.max_x() + 0.5
                            && r.max_y() <= bounds.max_y() + 0.5,
                        "{hit:?} {r:?} out of {w}×{h}@{scale}"
                    );
                }
                for (i, (a, ha)) in view.hits.iter().enumerate() {
                    for (b, hb) in &view.hits[i + 1..] {
                        let (Some(ra), Some(rb)) = (region(ha), region(hb)) else { continue };
                        if ra != rb {
                            let overlap = a
                                .intersect(b)
                                .is_some_and(|r| r.size.width > 0.5 && r.size.height > 0.5);
                            assert!(!overlap, "{ha:?} {a:?} over {hb:?} {b:?} at {w}×{h}@{scale}");
                        }
                    }
                }
                w -= 7.0;
            }
        }
    }
}

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}

/// The system's allocator, counting what this thread allocates while asked to.
struct Counting;

fn counted() {
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            let _ = ALLOCATIONS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

// SAFETY: every call goes to the system's allocator as it came; counting only reads and
// writes this thread's own cells (const-initialized: touching them allocates nothing).
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        counted();
        // SAFETY: the caller's layout, handed on.
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        counted();
        // SAFETY: as above.
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: a pointer this allocator gave, with its layout.
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        counted();
        // SAFETY: as above.
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The same frame drawn again (the scene warmed up): nothing allocated, at every class.
#[test]
fn an_identical_repaint_allocates_nothing() {
    // The counter counts (an allocation seen as one).
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    let probe: Vec<u8> = Vec::with_capacity(16);
    COUNTING.with(|on| on.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 1);
    drop(probe);
    let mut pen = Pen::new();
    for (w, scenario) in [
        (1300.0, "wide"),
        (900.0, "wide"),
        (600.0, "split"),
        (420.0, "plain"),
        (300.0, "page"),
        (1300.0, "columns"),
        (1300.0, "all"),
    ] {
        let mut view = demo(scenario);
        view.hover(Point::new(40.0, 300.0));
        // Warm up: the texts written, the layout worked out, the plots built, the shapes kept.
        for _ in 0..3 {
            pen.paint(&mut view, w, 900.0, 2.0);
        }
        ALLOCATIONS.with(|n| n.set(0));
        COUNTING.with(|on| on.set(true));
        pen.paint(&mut view, w, 900.0, 2.0);
        COUNTING.with(|on| on.set(false));
        let allocations = ALLOCATIONS.with(Cell::get);
        assert_eq!(allocations, 0, "{scenario} at {w}: {allocations} allocations");
    }
}

/// The frames' budgets (plan 40, "Performanță"), written in its status: a repaint of the same
/// page (only drawing what's kept), a frame of a resize (the layout and the charts' columns
/// worked out again) and a new look (the list and texts again), on the whole tab and in a
/// narrow split (a release build).
#[test]
#[cfg_attr(debug_assertions, ignore = "a budget: cargo test -p task-manager --release budget")]
fn budget_frames_are_drawn_in_time() {
    use std::time::{Duration, Instant};
    let median = |mut times: Vec<Duration>| {
        times.sort();
        times[times.len() / 2]
    };
    let mut pen = Pen::new();
    for (scenario, w) in [("wide", 1300.0), ("compact", 400.0)] {
        let mut view = demo(scenario);
        pen.paint(&mut view, w, 900.0, 2.0);
        pen.paint(&mut view, w, 900.0, 2.0);
        let repaint = median(
            (0..31)
                .map(|_| {
                    let t = Instant::now();
                    pen.paint(&mut view, w, 900.0, 2.0);
                    t.elapsed()
                })
                .collect(),
        );
        let resize = median(
            (0..31)
                .map(|i| {
                    let t = Instant::now();
                    pen.paint(&mut view, w - 20.0 + i as f32, 900.0, 2.0);
                    t.elapsed()
                })
                .collect(),
        );
        let frame = (*view.frame).clone();
        let look = median(
            (0..31)
                .map(|_| {
                    view.take_frame(frame.clone());
                    let t = Instant::now();
                    pen.paint(&mut view, w, 900.0, 2.0);
                    t.elapsed()
                })
                .collect(),
        );
        eprintln!(
            "budget: {scenario} ({w} px): repaint {repaint:?}, resize frame {resize:?}, new look {look:?}"
        );
        assert!(repaint <= Duration::from_millis(1), "{scenario}: repaint {repaint:?}");
        assert!(resize <= Duration::from_millis(2), "{scenario}: resize {resize:?}");
        assert!(look <= Duration::from_millis(2), "{scenario}: look {look:?}");
    }
}

/// A part's figure, once it had a value, stays in its detail at 0 while it costs nothing
/// (no figure, no chart coming and going); one it never had isn't shown.
#[test]
fn a_figure_once_reported_stays_at_zero() {
    let now = std::time::Instant::now();
    let part = |cpu: f32| {
        let mut l = Line::new("Terminal", "terminal", MODULES, None);
        l.cpu = cpu;
        l.threads = 0;
        l
    };
    let frame = |cpu| Frame { lines: vec![part(cpu)], ..Frame::default() };
    let mut history = crate::collector::history::History::new(
        std::time::Duration::from_secs(60),
        std::time::Duration::from_secs(1),
    );
    history.record(&frame(15.9), now);
    let mut view = TaskManager::default();
    view.set_all(false);
    view.history = Some(Arc::new(std::sync::Mutex::new(history)));
    view.take_frame(frame(15.9));
    let key = view.frame.lines[0].key.clone();
    view.select(&key);
    let mut pen = Pen::new();
    pen.paint(&mut view, 900.0, 600.0, 1.0);
    let metrics = |view: &TaskManager| {
        view.texts.detail.stats.iter().filter_map(|s| s.metric).collect::<Vec<_>>()
    };
    assert_eq!(metrics(&view), [Metric::Cpu]);
    // It costs nothing now: still there, at 0.
    view.history
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .record(&frame(0.0), now + std::time::Duration::from_secs(1));
    view.take_frame(frame(0.0));
    pen.paint(&mut view, 900.0, 600.0, 1.0);
    assert_eq!(metrics(&view), [Metric::Cpu], "kept");
    assert!(view.texts.detail.stats[0].now.starts_with('0'), "{}", view.texts.detail.stats[0].now);
}

/// A list longer than its rows' area: its scrollbar's thumb pressed and dragged down
/// scrolls it, through the view's own press, drag and release.
#[test]
fn the_list_s_thumb_dragged_scrolls_it() {
    let mut view = demo("split");
    let mut pen = Pen::new();
    pen.paint(&mut view, 900.0, 320.0, 1.0);
    assert!(view.max_scroll > 0.0, "more rows than the area holds");
    let rows = view.page().unwrap().rows;
    let at = |x: f32| Point::new(x, rows.min_y() + 6.0);
    let edge = (0..24).map(|dx| rows.max_x() - 1.0 - dx as f32).find(|&x| {
        let mut view = demo("split");
        pen.paint(&mut view, 900.0, 320.0, 1.0);
        view.hover(at(x));
        view.click(at(x));
        let moved = view.drag(Point::new(x, rows.min_y() + 120.0)) && view.scroll > 0.0;
        moved && view.release()
    });
    assert!(edge.is_some(), "the thumb dragged scrolls the list");
    let x = edge.unwrap_or_default();
    view.hover(at(x));
    view.click(at(x));
    view.release();
    assert!(!view.drag(Point::new(x, rows.max_y())), "let go: the moves don't reach it");
}

/// The wheel goes to the part under the mouse: over the detail it scrolls the detail,
/// elsewhere the list (also with no mouse yet).
#[test]
fn the_wheel_scrolls_the_part_under_the_mouse() {
    let mut view = demo("wide");
    let mut pen = Pen::new();
    pen.paint(&mut view, 1300.0, 700.0, 1.0);
    let page = view.page().unwrap();
    let (detail, rows) = (page.detail.expect("open"), page.rows);
    assert!(view.max_scroll > 0.0 && view.detail_max > 0.0, "both scroll");
    assert!(view.scroll(0.0, 30.0), "no mouse yet: the list");
    assert_eq!((view.scroll, view.detail_scroll), (30.0, 0.0));
    view.hover(center(detail));
    assert!(view.scroll(0.0, 20.0));
    assert_eq!((view.scroll, view.detail_scroll), (30.0, 20.0), "over the detail: its own");
    view.hover(center(rows));
    assert!(view.scroll(0.0, -30.0));
    assert_eq!((view.scroll, view.detail_scroll), (0.0, 20.0), "over the rows: the list");
    assert!(!view.scroll(0.0, -30.0), "at its top");
}
