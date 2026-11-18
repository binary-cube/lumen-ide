//! The grid over a fake source: what it asks for, sorting, filters, selection, copy, the
//! selection's figures — without drawing (its geometry set by hand).

use std::ops::Range;

use kernel::{Point, Rect};

use super::*;
use crate::data_grid::grid::{Field, Hit, Mark, PopRow};
use crate::tool_panel::KeyReach;
use crate::viewer::{PressMods, ViewMenuEntry, ViewerKey};

/// Rows `0..rows` of three columns (id, name, score), all there at once.
#[derive(Default)]
struct Fake {
    columns: Vec<GridColumn>,
    rows: Vec<(i64, String, Option<f64>)>,
    asked: Vec<(GridQuery, Range<u64>)>,
    sortable: bool,
    refreshed: usize,
    /// Rows can be added and deleted; how many were.
    editable: bool,
    inserted: usize,
    deleted: usize,
}

impl Fake {
    fn new(rows: usize) -> Self {
        let mut columns = vec![
            GridColumn::new("id", "INTEGER", CellClass::Integer),
            GridColumn::new("name", "TEXT", CellClass::Text),
            GridColumn::new("score", "REAL", CellClass::Decimal),
        ];
        columns[0].key = true;
        Self {
            columns,
            rows: (0..rows)
                .map(|i| (i as i64 + 1, format!("n{i}"), (i % 3 != 0).then_some(i as f64 / 2.0)))
                .collect(),
            sortable: true,
            ..Self::default()
        }
    }
}

impl GridSource for Fake {
    fn columns(&self) -> &[GridColumn] {
        &self.columns
    }
    fn total(&self) -> Total {
        Total::Known(self.rows.len() as u64)
    }
    fn row_state(&self, row: u64) -> RowState {
        if (row as usize) < self.rows.len() { RowState::Ready } else { RowState::Missing }
    }
    fn cell(&self, row: u64, column: usize) -> CellRef<'_> {
        let Some(r) = self.rows.get(row as usize) else { return CellRef::Null };
        match column {
            0 => CellRef::Int(r.0),
            1 => CellRef::Text(&r.1),
            _ => r.2.map_or(CellRef::Null, CellRef::Float),
        }
    }
    fn request(&mut self, query: &GridQuery, rows: Range<u64>) {
        self.asked.push((query.clone(), rows));
    }
    fn poll(&mut self) -> bool {
        true
    }
    fn status(&self) -> SourceStatus {
        SourceStatus::Ready
    }
    fn capabilities(&self) -> GridCaps {
        GridCaps { sort: self.sortable, filter: self.sortable, limit: None, cancel: false }
    }
    fn name(&self) -> String {
        "people".into()
    }
    fn refresh(&mut self) {
        self.refreshed += 1;
    }
    fn editing(&self) -> Editing {
        if self.editable { Editing::Allowed } else { Editing::ReadOnly("no".into()) }
    }
    fn insert(&mut self, _like: Option<u64>) -> Option<u64> {
        self.inserted += 1;
        self.editable.then_some(NEW_ROWS + self.inserted as u64)
    }
    fn delete(&mut self, rows: &[u64]) {
        self.deleted += rows.len();
    }
}

/// A grid laid out as if painted at scale 1: headers at y 100–144, rows of 28 from 144,
/// row numbers 40 wide, columns 100 wide each.
fn grid(rows: usize, page: Option<u32>) -> DataGrid<Fake> {
    let mut g = DataGrid::new(Fake::new(rows), page);
    g.poll();
    for c in 0..3 {
        g.layout.set_width(c, 100.0);
    }
    g.geometry.scale = 1.0;
    g.geometry.row_h = 28.0;
    g.geometry.gutter = 40.0;
    g.geometry.header = Rect::new(0.0, 100.0, 400.0, 44.0);
    g.geometry.body = Rect::new(0.0, 144.0, 400.0, 280.0);
    g
}

/// The middle of the cell at row place `i`, column place `col`.
fn at(i: u64, col: usize) -> Point {
    Point::new(40.0 + col as f32 * 100.0 + 50.0, 144.0 + i as f32 * 28.0 + 14.0)
}

const NONE: PressMods = PressMods { shift: false, primary: false, alt: false };
const SHIFT: PressMods = PressMods { shift: true, primary: false, alt: false };

#[test]
fn only_the_rows_in_view_are_asked_and_once() {
    let mut g = grid(10_000, Some(500));
    g.request_visible(280.0);
    g.request_visible(280.0);
    assert_eq!(g.source.asked.len(), 1, "the same rows aren't asked again");
    let (_, range) = &g.source.asked[0];
    assert_eq!(range.start, 0);
    assert!(range.end <= 30, "a screen and a bit: {range:?}");
    g.scroll(0.0, 28.0 * 100.0);
    g.request_visible(280.0);
    let (_, range) = g.source.asked.last().unwrap();
    assert!(range.start > 80 && range.end < 140, "{range:?}");
    // The next page: its own rows.
    g.paging.forward();
    g.scroll_y = 0.0;
    g.request_visible(280.0);
    assert_eq!(g.source.asked.last().unwrap().1.start, 500);
}

#[test]
fn a_header_click_sorts_and_shift_adds() {
    let mut g = grid(10, Some(100));
    let header = |col: usize| Point::new(40.0 + col as f32 * 100.0 + 30.0, 120.0);
    g.press(header(1), 1, NONE);
    g.release();
    assert_eq!(g.query().sort, vec![SortKey { column: "name".into(), descending: false }]);
    assert_eq!(g.order_input.text(), "name");
    g.press(header(2), 1, SHIFT);
    g.release();
    g.press(header(1), 1, SHIFT);
    g.release();
    assert_eq!(g.order_input.text(), "name DESC, score");
    // A source that can't sort: a click does nothing.
    g.source.sortable = false;
    g.press(header(0), 1, NONE);
    g.release();
    assert_eq!(g.query().sort.len(), 2);
}

#[test]
fn a_header_dragged_moves_its_column() {
    let mut g = grid(10, Some(100));
    g.press(Point::new(60.0, 120.0), 1, NONE);
    g.drag(Point::new(120.0, 120.0));
    g.drag(Point::new(260.0, 120.0));
    g.release();
    assert_eq!(g.layout.visible(), vec![1, 2, 0]);
    assert!(g.query().sort.is_empty(), "a drag doesn't sort");
}

#[test]
fn an_edge_dragged_resizes_and_a_double_click_fits() {
    let mut g = grid(10, Some(100));
    g.press(Point::new(140.0, 120.0), 1, NONE);
    g.drag(Point::new(190.0, 120.0));
    g.release();
    assert_eq!(g.layout.width(0), 150.0);
    g.press(Point::new(190.0, 120.0), 2, NONE);
    assert_eq!(g.fit_pending, Some(0));
}

#[test]
fn clicks_select_cells_rows_and_shift_extends() {
    let mut g = grid(10, Some(100));
    g.press(at(1, 0), 1, NONE);
    g.release();
    g.press(at(3, 1), 1, SHIFT);
    g.release();
    assert_eq!(g.selection().count(), 6);
    assert_eq!(g.selected_text().as_deref(), Some("2\tn1\n3\tn2\n4\tn3"));
    // A row number: the whole row.
    g.press(Point::new(10.0, at(5, 0).y), 1, NONE);
    g.release();
    assert_eq!(g.copy_as("csv").as_deref(), Some("id,name,score\n6,n5,2.5"));
    // ⌘A: the page.
    assert!(g.select_all());
    assert_eq!(g.selection().count(), 30);
}

#[test]
fn keys_move_the_cursor_and_shift_extends() {
    let mut g = grid(10, Some(100));
    let plain = KeyReach::default();
    let extend = KeyReach { word: false, extend: true };
    g.edit_key(ViewerKey::Down, plain);
    assert_eq!(g.selection().cursor(), Some(CellPos::new(0, 0)));
    g.edit_key(ViewerKey::Right, plain);
    g.edit_key(ViewerKey::Down, extend);
    g.edit_key(ViewerKey::Right, extend);
    assert_eq!(g.selection().count(), 4);
    g.edit_key(ViewerKey::End, KeyReach { word: true, extend: false });
    assert_eq!(g.selection().cursor().map(|c| c.row), Some(9));
    g.edit_key(ViewerKey::Escape, plain);
    assert!(g.selection().is_empty());
}

#[test]
fn the_selection_adds_up_its_numbers() {
    let mut g = grid(10, Some(100));
    g.press(at(0, 2), 1, NONE);
    g.press(at(5, 2), 1, SHIFT);
    let a = g.aggregates().unwrap();
    // Rows 0 and 3 have no score.
    assert_eq!((a.count, a.numbers), (6, 4));
    assert_eq!(a.sum_text(), "6");
    assert_eq!(a.min_text(), "0.5");
    assert_eq!(a.max_text(), "2.5");
    // A single cell: nothing to add up.
    g.press(at(0, 2), 1, NONE);
    assert!(g.aggregates().is_none());
}

#[test]
fn the_where_field_filters_and_shows_what_it_misunderstood() {
    let mut g = grid(10, Some(100));
    g.press(at(1, 0), 1, NONE);
    g.field = Some(super::grid::Field::Where);
    g.text_input("score > 1 and name lik");
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    assert!(g.where_error.is_some());
    assert!(g.query().filter.is_none());
    assert!(g.wants_text(), "the field keeps the keys");
    g.edit_key(ViewerKey::Backspace, KeyReach { word: true, extend: false });
    g.edit_key(ViewerKey::Backspace, KeyReach { word: true, extend: false });
    g.edit_key(ViewerKey::Backspace, KeyReach { word: true, extend: false });
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    assert_eq!(g.where_error, None, "{:?}", g.where_input.text());
    assert!(matches!(g.query().filter, Some(GridFilter::Compare { .. })));
    assert!(!g.wants_text());
    assert!(g.selection().is_empty(), "another filter: another view");
}

#[test]
fn a_cells_menu_adds_quick_filters() {
    let mut g = grid(10, Some(100));
    let menu = g.context_menu(at(2, 1)).unwrap();
    assert!(menu.iter().any(|e| matches!(e, crate::ViewMenuEntry::Submenu { .. })));
    assert!(g.command("grid:filter-eq"));
    assert_eq!(g.where_input.text(), "name = 'n2'");
    g.context_menu(at(0, 2));
    g.command("grid:filter-not-null");
    assert_eq!(g.where_input.text(), "name = 'n2' AND score IS NOT NULL");
    g.command("grid:filter-clear");
    assert!(g.query().filter.is_none());
    assert_eq!(g.where_input.text(), "");
}

#[test]
fn a_menu_copy_comes_back_as_an_action() {
    let mut g = grid(10, Some(100));
    g.context_menu(at(0, 0));
    assert!(g.command("grid:copy:json"));
    assert_eq!(g.take_action(), Some(GridAction::Copy("[\n  {\"id\": 1}\n]".into())));
    g.command("grid:copy:sql");
    assert_eq!(
        g.take_action(),
        Some(GridAction::Copy("INSERT INTO \"people\" (\"id\") VALUES (1);".into()))
    );
    assert!(!g.command("grid:nothing"));
}

#[test]
fn hidden_columns_are_skipped_and_come_back() {
    let mut g = grid(10, Some(100));
    g.command("grid:hide:1");
    assert_eq!(g.layout.visible(), vec![0, 2]);
    g.press(at(0, 0), 1, NONE);
    g.press(at(0, 1), 1, SHIFT);
    assert_eq!(g.selected_text().as_deref(), Some("1\t"));
    g.command("grid:show-all");
    assert_eq!(g.layout.visible(), vec![0, 1, 2]);
    let saved = g.saved_layout();
    assert!(saved.hidden.is_empty());
}

#[test]
fn find_selects_matches_in_turn() {
    let mut g = grid(30, Some(100));
    g.open_find();
    g.text_input("n2");
    g.refresh_find();
    // n2, n20…n29: in the name column.
    assert_eq!(g.find.matches.len(), 11);
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    assert_eq!(g.selection().cursor(), Some(CellPos::new(2, 1)));
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    assert_eq!(g.selection().cursor(), Some(CellPos::new(20, 1)));
    g.edit_key(ViewerKey::Enter, KeyReach { word: false, extend: true });
    assert_eq!(g.selection().cursor(), Some(CellPos::new(2, 1)));
    g.edit_key(ViewerKey::Escape, KeyReach::default());
    assert!(!g.find.open);
}

#[test]
fn page_size_and_paging_move_the_rows() {
    let mut g = grid(1234, Some(500));
    assert_eq!(g.page_rows(), 500);
    g.set_page_size(Some(100));
    assert_eq!(g.page_rows(), 100);
    g.paging.last(1234);
    assert_eq!(g.page_rows(), 34);
    g.set_page_size(None);
    assert_eq!(g.page_rows(), 1234);
    g.command("grid:refresh");
    assert_eq!(g.source.refreshed, 1);
}

#[test]
fn transposed_keys_walk_the_columns() {
    let mut g = grid(5, Some(100));
    g.command("grid:transpose");
    assert!(g.transposed());
    assert_eq!(g.record_row(), Some(0));
    g.edit_key(ViewerKey::Down, KeyReach::default());
    g.edit_key(ViewerKey::Right, KeyReach::default());
    assert_eq!(g.selection().cursor(), Some(CellPos::new(1, 1)));
    g.edit_key(ViewerKey::Escape, KeyReach::default());
    assert!(!g.transposed());
}

#[test]
fn a_saved_layout_comes_back_without_the_columns_gone() {
    let mut g = grid(10, Some(100));
    g.command("grid:hide:2");
    g.command("grid:where:id > 3");
    g.command("grid:order:name DESC");
    g.choose_page_size(Some(10));
    g.layout.move_column(1, 0);
    let saved = g.saved_layout();
    assert_eq!(saved.hidden, vec!["score".to_owned()]);
    assert_eq!(saved.order, vec!["name".to_owned(), "id".to_owned(), "score".to_owned()]);
    assert_eq!(saved.filter, "id > 3");
    assert_eq!(saved.sort, vec![SortKey { column: "name".into(), descending: true }]);
    assert_eq!(saved.page, Some(Some(10)));

    // Put back on a new grid before its columns are known, and once they are.
    let mut fresh = DataGrid::new(Fake::new(10), Some(100));
    let version = fresh.layout_version();
    fresh.restore_layout(saved.clone());
    fresh.poll();
    assert_eq!(fresh.saved_layout(), saved);
    assert_ne!(fresh.layout_version(), version, "a change to keep");
    assert_eq!(fresh.paging().size, Some(10));

    // Columns gone (renamed, dropped): their widths, order, sort and filter ignored.
    let stale = SavedLayout {
        widths: vec![("gone".into(), 300.0), ("id".into(), 77.0)],
        order: vec!["gone".into(), "score".into()],
        hidden: vec!["gone".into(), "name".into()],
        sort: vec![
            SortKey { column: "gone".into(), descending: false },
            SortKey { column: "id".into(), descending: true },
        ],
        filter: "gone = 1".into(),
        page: None,
    };
    let mut other = grid(10, Some(100));
    other.restore_layout(stale);
    assert_eq!(other.layout.visible(), vec![2, 0]);
    assert_eq!(other.layout.width(0), 77.0);
    assert_eq!(other.query().sort, vec![SortKey { column: "id".into(), descending: true }]);
    assert_eq!(other.query().filter, None);
    assert_eq!(other.saved_layout().page, None, "the page size wasn't chosen");
}

#[test]
fn layout_changes_are_counted() {
    let mut g = grid(10, Some(100));
    let v = g.layout_version();
    g.command("grid:hide:1");
    assert_ne!(g.layout_version(), v);
    let v = g.layout_version();
    g.command("grid:select:0,0");
    assert_eq!(g.layout_version(), v, "a selection isn't a layout");
    g.command("grid:sort-asc:0");
    assert_ne!(g.layout_version(), v);
}

#[test]
fn the_where_field_has_the_usual_text_keys() {
    let mut g = grid(10, Some(100));
    g.field = Some(Field::Where);
    g.text_input("sdfsdf");
    // ⌘A: the field's text, never the cells.
    assert!(g.select_all());
    assert!(g.selection().is_empty());
    assert_eq!(g.selected_text().as_deref(), Some("sdfsdf"));
    // ⌘X takes it out; ⌘Z brings it back, ⇧⌘Z takes it again.
    assert_eq!(g.cut().as_deref(), Some("sdfsdf"));
    assert_eq!(g.where_input.text(), "");
    assert!(g.undo(false));
    assert_eq!(g.where_input.text(), "sdfsdf");
    assert!(g.undo(true));
    assert_eq!(g.where_input.text(), "");
    // Not understood: why, under the field (localized), until it's fixed.
    g.text_input("nope = 1");
    g.edit_key(ViewerKey::Enter, KeyReach::default());
    assert!(g.where_error.is_some());
    assert_eq!(g.field, Some(Field::Where), "the field keeps the keys");
}

#[test]
fn every_row_is_exported_from_the_headers_menu() {
    let mut g = grid(10, Some(100));
    let p = Point::new(90.0, 120.0);
    let menu = g.context_menu(p).unwrap_or_default();
    let ids: Vec<String> = menu
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Submenu { entries, .. } => Some(entries.clone()),
            _ => None,
        })
        .flatten()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id),
            _ => None,
        })
        .collect();
    assert!(ids.contains(&"grid:export:csv".to_owned()), "{ids:?}");
    assert!(ids.contains(&"grid:export-copy:json".to_owned()), "{ids:?}");
    assert!(g.command("grid:export:sql"));
    assert_eq!(
        g.take_action(),
        Some(GridAction::Export { format: "sql".into(), clipboard: false })
    );
    assert!(g.command("grid:export-copy:markdown"));
    assert_eq!(
        g.take_action(),
        Some(GridAction::Export { format: "markdown".into(), clipboard: true })
    );
    assert!(ids.contains(&"grid:export:xlsx".to_owned()), "{ids:?}");
    assert!(g.command("grid:export:xlsx"));
    assert_eq!(
        g.take_action(),
        Some(GridAction::Export { format: "xlsx".into(), clipboard: false })
    );
    assert!(!g.command("grid:export-copy:xlsx"), "a workbook isn't text for the clipboard");
    assert!(!g.command("grid:export:nothing"), "not a format");
}

// ─── The toolbar, painted ───

/// `g` painted `width` wide at scale 1 (a real text system: the labels' widths decide).
fn paint_at(g: &mut DataGrid<Fake>, text: &mut text_layout::TextSystem, width: f32) {
    let mut scene = render_api::Scene::default();
    let palette = crate::Palette::dark();
    let icons = icons::Icons::builtin();
    g.paint(GridPaint {
        bounds: Rect::new(0.0, 0.0, width, 600.0),
        scale: 1.0,
        text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
    });
}

/// The toolbar's buttons and texts as last painted (its height: 46).
fn toolbar_hits(g: &DataGrid<Fake>) -> Vec<(Rect, Hit)> {
    g.hits.iter().copied().filter(|(r, _)| r.max_y() <= 46.0).collect()
}

/// The keys a test reads for a command (as the app's keymap would show them).
fn test_keys(id: &str) -> Option<String> {
    match id {
        "test.refresh" => Some("F5".into()),
        "test.delete" => Some("⌘⌫".into()),
        _ => None,
    }
}

fn test_commands(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::Refresh => Some("test.refresh"),
        Tool::Delete => Some("test.delete"),
        _ => None,
    }
}

/// An editable grid of a few pages.
fn editable() -> DataGrid<Fake> {
    let mut g = grid(1200, Some(100));
    g.source.editable = true;
    g.set_commands(test_commands);
    g
}

#[test]
fn the_toolbar_never_overlaps_and_puts_away_what_doesnt_fit() {
    let mut text = text_layout::TextSystem::new();
    let mut g = editable();
    let mut before = 0;
    for width in (200..=1400).rev().step_by(40) {
        paint_at(&mut g, &mut text, width as f32);
        let hits = toolbar_hits(&g);
        for (i, (a, ha)) in hits.iter().enumerate() {
            assert!(a.min_x() >= 0.0 && a.max_x() <= width as f32, "{ha:?} out at {width}");
            for (b, hb) in &hits[i + 1..] {
                assert!(a.intersect(b).is_none(), "{ha:?} {a:?} over {hb:?} {b:?} at {width}");
            }
        }
        let collapsed = g.collapsed_tools().len();
        assert!(collapsed >= before, "narrower, fewer put away at {width}");
        before = collapsed;
        // The "…" exactly when something is in it.
        assert_eq!(hits.iter().any(|(_, h)| *h == Hit::More), collapsed > 0, "at {width}");
    }
    // Wide: everything shown; narrow: the pages' ‹ › and the menu.
    paint_at(&mut g, &mut text, 1400.0);
    assert!(g.collapsed_tools().is_empty());
    paint_at(&mut g, &mut text, 260.0);
    let shown: Vec<Hit> = toolbar_hits(&g).into_iter().map(|(_, h)| h).collect();
    assert!(shown.contains(&Hit::Forward) && shown.contains(&Hit::More), "{shown:?}");
    assert!(g.collapsed_tools().contains(&Tool::Refresh));
}

#[test]
fn the_more_menu_holds_the_tools_put_away_and_runs_them() {
    let mut text = text_layout::TextSystem::new();
    let mut g = editable();
    paint_at(&mut g, &mut text, 260.0);
    let more = toolbar_hits(&g).into_iter().find(|(_, h)| *h == Hit::More).unwrap().0;
    let middle =
        |r: Rect| Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0);
    g.press(middle(more), 1, NONE);
    g.release();
    assert!(g.more_open());
    paint_at(&mut g, &mut text, 260.0);
    let rows: Vec<(Rect, Tool)> = g
        .hits
        .iter()
        .filter_map(|(r, h)| match h {
            Hit::MoreChoice(t) => Some((*r, *t)),
            _ => None,
        })
        .collect();
    // Every tool put away that can be used now has its row (the first page: ‹‹ can't).
    for tool in g.collapsed_tools() {
        let usable = g.tool_enabled(*tool);
        assert_eq!(rows.iter().any(|(_, t)| t == tool), usable, "{tool:?}");
    }
    // The menu stays in the grid, under the toolbar.
    for (r, _) in &rows {
        assert!(r.min_x() >= 0.0 && r.max_x() <= 260.0 && r.min_y() > 46.0);
    }
    // Refresh from the menu: run, the menu closed.
    let refresh = rows.iter().find(|(_, t)| *t == Tool::Refresh).unwrap().0;
    let before = g.source.refreshed;
    g.press(middle(refresh), 1, NONE);
    g.release();
    assert!(!g.more_open());
    assert_eq!(g.source.refreshed, before + 1);
    // Find too (what's put away depends on the texts: after the refresh the total is read
    // again).
    g.open_more();
    paint_at(&mut g, &mut text, 260.0);
    let find = g.hits.iter().find(|(_, h)| *h == Hit::MoreChoice(Tool::Find)).unwrap().0;
    g.press(middle(find), 1, NONE);
    assert!(g.find.open);
    g.find.open = false;
    // Esc closes it; a press elsewhere closes it without running anything.
    g.open_more();
    assert!(g.edit_key(ViewerKey::Escape, KeyReach::default()));
    assert!(!g.more_open());
    g.open_more();
    g.press(Point::new(130.0, 400.0), 1, NONE);
    assert!(!g.more_open());
    assert!(!g.find.open);
    // The page size from the menu opens its choices.
    g.open_more();
    paint_at(&mut g, &mut text, 260.0);
    let size = g.hits.iter().find(|(_, h)| *h == Hit::MoreChoice(Tool::PageSize)).unwrap().0;
    g.press(middle(size), 1, NONE);
    assert!(g.sizes_open);
}

#[test]
fn keys_keep_working_for_tools_put_away() {
    let mut text = text_layout::TextSystem::new();
    let mut g = editable();
    paint_at(&mut g, &mut text, 240.0);
    assert!(g.collapsed_tools().contains(&Tool::Refresh));
    assert!(g.collapsed_tools().contains(&Tool::Find));
    let before = g.source.refreshed;
    assert!(g.command("refresh"));
    assert_eq!(g.source.refreshed, before + 1);
    assert!(g.command("find"));
    assert!(g.find.open);
    assert!(g.command("grid:add-row"));
    assert_eq!(g.source.inserted, 1);
}

#[test]
fn every_toolbar_button_has_a_tooltip_with_its_keys() {
    crate::shortcuts::set_reader(test_keys);
    let mut text = text_layout::TextSystem::new();
    for width in [1400.0, 600.0, 260.0] {
        let mut g = editable();
        paint_at(&mut g, &mut text, width);
        for (r, hit) in toolbar_hits(&g) {
            let at = Point::new(r.min_x() + 2.0, r.min_y() + r.size.height / 2.0);
            let tip = g.tooltip_at(at).unwrap_or_else(|| panic!("{hit:?} has no tooltip"));
            assert!(!tip.label.trim().is_empty(), "{hit:?}");
            match hit {
                Hit::Refresh => assert_eq!(tip.shortcut.as_deref(), Some("F5")),
                Hit::DeleteRows => assert_eq!(tip.shortcut.as_deref(), Some("⌘⌫")),
                _ => {}
            }
        }
        // The buttons that can't be used now (the first page's ‹‹ and ‹) have theirs too.
        for (r, _) in g.marks.iter().filter(|(r, _)| r.max_y() <= 46.0) {
            let at = Point::new(r.min_x() + 2.0, r.min_y() + 4.0);
            assert!(g.tooltip_at(at).is_some_and(|t| !t.label.is_empty()));
        }
    }
    // The fields say how to write them; a sorted header's arrow which way it orders.
    let mut g = editable();
    g.set_sort(vec![SortKey { column: "name".into(), descending: true }]);
    paint_at(&mut g, &mut text, 1000.0);
    let field = g.hits.iter().find(|(_, h)| *h == Hit::Where).unwrap().0;
    let tip = g.tooltip_at(Point::new(field.min_x() + 4.0, field.min_y() + 4.0)).unwrap();
    assert!(tip.label.contains("price > 10"), "{}", tip.label);
    let (mark, _) = g.marks.iter().find(|(_, m)| matches!(m, Mark::Sort { .. })).unwrap();
    let tip = g.tooltip_at(Point::new(mark.min_x() + 4.0, mark.min_y() + 4.0)).unwrap();
    assert!(!tip.label.is_empty());
    let (key, _) = g.marks.iter().find(|(_, m)| *m == Mark::Key).unwrap();
    assert!(g.tooltip_at(Point::new(key.min_x() + 4.0, key.min_y() + 4.0)).is_some());
}

#[test]
fn narrow_fields_stack_and_the_wide_ones_sit_side_by_side() {
    let mut text = text_layout::TextSystem::new();
    let mut g = editable();
    paint_at(&mut g, &mut text, 1000.0);
    let field = |g: &DataGrid<Fake>, hit: Hit| g.hits.iter().find(|(_, h)| *h == hit).unwrap().0;
    let (w, o) = (field(&g, Hit::Where), field(&g, Hit::Order));
    assert_eq!(w.min_y(), o.min_y());
    assert!(w.max_x() < o.min_x());
    paint_at(&mut g, &mut text, 300.0);
    let (w, o) = (field(&g, Hit::Where), field(&g, Hit::Order));
    assert!(w.max_y() < o.min_y(), "stacked: {w:?} {o:?}");
    assert_eq!(w.size.width, o.size.width);
    assert!(o.max_x() <= 300.0);
}

#[test]
fn a_cells_menu_has_no_empty_section_and_an_icon_on_every_row() {
    for editable_grid in [false, true] {
        let mut g = if editable_grid { editable() } else { grid(10, Some(100)) };
        let menu = g.context_menu(at(1, 1)).unwrap();
        assert!(!matches!(menu.first(), Some(ViewMenuEntry::Separator)));
        assert!(!matches!(menu.last(), Some(ViewMenuEntry::Separator)));
        assert!(
            !menu
                .windows(2)
                .any(|w| matches!(w, [ViewMenuEntry::Separator, ViewMenuEntry::Separator])),
            "no two separators in a row (editable: {editable_grid}): {menu:?}"
        );
        for entry in &menu {
            match entry {
                ViewMenuEntry::Item { id, icon, .. } => assert!(icon.is_some(), "{id} has no icon"),
                ViewMenuEntry::Submenu { label, icon, .. } => {
                    assert!(icon.is_some(), "{label} has no icon")
                }
                ViewMenuEntry::Separator => {}
            }
        }
        let has_edit =
            menu.iter().any(|e| matches!(e, ViewMenuEntry::Item { id, .. } if id == "grid:edit"));
        assert_eq!(has_edit, editable_grid);
    }
}

#[test]
fn the_find_field_takes_the_bar_and_its_buttons_end_it() {
    let mut text = text_layout::TextSystem::new();
    for width in [600.0, 1200.0] {
        let mut g = grid(30, Some(100));
        g.open_find();
        g.text_input("n2");
        paint_at(&mut g, &mut text, width);
        let hit = |h: Hit| g.hits.iter().find(|(_, x)| *x == h).unwrap().0;
        let (field, close) = (hit(Hit::Find), hit(Hit::FindClose));
        let (prev, next) = (hit(Hit::FindPrev), hit(Hit::FindNext));
        // × at the right end (the bar's padding), ↑ ↓ before it, the field up to the count.
        assert!((close.max_x() - (width - 10.0)).abs() <= 1.0, "at {width}: {close:?}");
        assert!(prev.max_x() <= next.min_x() && next.max_x() <= close.min_x());
        assert!(field.max_x() + 64.0 + 18.0 <= prev.min_x() + 1.0, "at {width}");
        assert!(field.max_x() >= prev.min_x() - 120.0, "at {width}: the field stops short");
        assert_eq!(field.min_x(), 10.0);
    }
}

// ─── The data extractors ───

/// A grid of a project of its own (its choice of extractor is its own: tests run together).
fn of_a_project() -> DataGrid<Fake> {
    let mut g = grid(10, Some(100));
    g.set_project(Some(kernel::ProjectId::new()));
    g
}

fn middle(r: Rect) -> Point {
    Point::new(r.min_x() + r.size.width / 2.0, r.min_y() + r.size.height / 2.0)
}

#[test]
fn copy_follows_the_extractor_chosen() {
    let mut g = of_a_project();
    g.press(at(0, 0), 1, NONE);
    g.release();
    g.press(at(1, 1), 1, SHIFT);
    g.release();
    // TSV until one is chosen (pasted into a spreadsheet as cells).
    assert_eq!(g.extractor().id(), "tsv");
    assert_eq!(g.selected_text().as_deref(), Some("1\tn0\n2\tn1"));
    assert!(g.command("grid:extractor:sql"));
    assert_eq!(g.take_action(), Some(GridAction::Command(extractors::SAVE_COMMAND.into())));
    assert_eq!(
        g.selected_text().as_deref(),
        Some(
            "INSERT INTO \"people\" (\"id\", \"name\") VALUES (1, 'n0');\nINSERT INTO \"people\" (\"id\", \"name\") VALUES (2, 'n1');"
        )
    );
    // "Copy As" any of them, the choice kept.
    assert!(g.command("grid:copy:json"));
    assert!(
        matches!(g.take_action(), Some(GridAction::Copy(t)) if t.starts_with("[\n  {\"id\": 1"))
    );
    assert_eq!(g.extractor().id(), "sql");
    assert!(!g.command("grid:extractor:nothing"));
    // Another project's grid has its own choice.
    let other = of_a_project();
    assert_eq!(other.extractor().id(), "tsv");
}

#[test]
fn the_cell_menu_copies_with_the_choice_and_lists_every_extractor() {
    let mut g = of_a_project();
    g.command("grid:extractor:markdown");
    let menu = g.context_menu(at(0, 0)).unwrap();
    let ViewMenuEntry::Item { id, label, .. } = &menu[0] else { panic!("{menu:?}") };
    assert_eq!(id, "grid:copy:markdown");
    // (Its label names the extractor: "Copy as Markdown" — the locales aren't loaded here.)
    assert!(!label.is_empty());
    let ViewMenuEntry::Submenu { entries, .. } = &menu[1] else { panic!("{menu:?}") };
    let ids: Vec<&str> = entries
        .iter()
        .filter_map(|e| match e {
            ViewMenuEntry::Item { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    for want in [
        "grid:copy:sql",
        "grid:copy:sql-where",
        "grid:copy:tsv-header",
        "grid:copy:xml",
        "grid:copy:pretty",
    ] {
        assert!(ids.contains(&want), "{want}: {ids:?}");
    }
    assert!(!ids.contains(&"grid:copy:xlsx"), "a workbook isn't copied");
    // The groups apart.
    assert!(entries.iter().filter(|e| matches!(e, ViewMenuEntry::Separator)).count() >= 2);
}

/// The pill's name is drawn whole (never "TS…" for TSV, whatever the scale rounds its width
/// to): the glyphs inside it are the name's own.
#[test]
fn the_toolbar_s_pill_shows_the_whole_name() {
    let mut text = text_layout::TextSystem::new();
    let palette = crate::Palette::dark();
    let icons = icons::Icons::builtin();
    for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
        let mut g = of_a_project();
        let mut scene = render_api::Scene::default();
        g.paint(GridPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0 * scale, 600.0 * scale),
            scale,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
        });
        let pill = g.hits.iter().find(|(_, h)| *h == Hit::Extractor).unwrap().0;
        let font = crate::ds::Ctx::new(&palette, scale).ui_font(-1.0);
        let want: Vec<_> =
            text.shape_line(&g.extractor_label(), &font).glyphs.iter().map(|g| g.key).collect();
        let drawn: Vec<_> = scene
            .layers()
            .iter()
            .flat_map(|l| l.glyphs.iter())
            // The name's, not the chevron's (an icon, 8 + 12 from the right).
            .filter(|gl| pill.contains(gl.origin) && gl.origin.x < pill.max_x() - 20.0 * scale)
            .map(|gl| gl.key)
            .collect();
        assert_eq!(drawn, want, "at scale {scale}");
    }
}

/// The extractors' popover taller than the room below its pill scrolls, and says so: faded
/// edges over its rows (a layer clipped to it after its rows'), only where there's more.
#[test]
fn the_extractors_popover_shows_there_is_more_to_scroll_to() {
    let mut text = text_layout::TextSystem::new();
    let palette = crate::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut paint = |g: &mut DataGrid<Fake>, height: f32| {
        let mut scene = render_api::Scene::default();
        g.paint(GridPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0, height),
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
        });
        let area = g.extract_area.unwrap();
        scene
            .layers()
            .iter()
            .filter(|l| l.clip == Some(area))
            .map(|l| l.quads.len())
            .collect::<Vec<usize>>()
    };
    let mut g = of_a_project();
    g.open_extractors();
    // Room for every row: its rows' layer only.
    assert_eq!(paint(&mut g, 2000.0).len(), 1);
    // Short: the rows, then the hints over them.
    let short = paint(&mut g, 400.0);
    assert_eq!(short.len(), 2, "{short:?}");
    assert!(short[1] > 0, "the fade below drawn");
}

#[test]
fn the_toolbar_s_pill_opens_the_popover_and_chooses() {
    let mut text = text_layout::TextSystem::new();
    let mut g = of_a_project();
    paint_at(&mut g, &mut text, 1200.0);
    let pill = toolbar_hits(&g).into_iter().find(|(_, h)| *h == Hit::Extractor).unwrap().0;
    assert!(g.tooltip_at(middle(pill)).is_some());
    g.press(middle(pill), 1, NONE);
    g.release();
    assert!(g.extractors_open());
    paint_at(&mut g, &mut text, 1200.0);
    let area = g.extract_area.unwrap();
    assert!(area.min_y() > 36.0 && area.max_x() <= 1200.0, "{area:?}");
    // Its rows: the groups' headings, the skips, "Configure CSV Formats…", the note.
    let rows = g.extract_rows.clone();
    assert!(matches!(rows.first(), Some(PopRow::Heading(_))));
    assert!(rows.iter().any(|r| matches!(r, PopRow::Toggle { computed: true, .. })));
    assert!(rows.iter().any(|r| matches!(r, PopRow::Action { command, .. } if *command == extractors::CONFIGURE_CSV_COMMAND)));
    assert!(matches!(rows.last(), Some(PopRow::Note(_))));
    let row_of = |g: &DataGrid<Fake>, want: &dyn Fn(&PopRow) -> bool| {
        let at = g.extract_rows.iter().position(want).unwrap();
        g.hits.iter().find(|(_, h)| *h == Hit::ExtractorRow(at)).map(|(r, _)| *r).unwrap()
    };
    // A switch keeps it open.
    let skip = row_of(&g, &|r| matches!(r, PopRow::Toggle { computed: false, .. }));
    let before = extractors::catalog().options().skip_generated;
    g.press(middle(skip), 1, NONE);
    assert!(g.extractors_open());
    assert_eq!(extractors::catalog().options().skip_generated, !before);
    assert_eq!(g.take_action(), Some(GridAction::Command(extractors::SAVE_COMMAND.into())));
    g.toggle_skip(false);
    g.take_action();
    // A choice closes it.
    paint_at(&mut g, &mut text, 1200.0);
    let json = row_of(&g, &|r| matches!(r, PopRow::Choice { id, .. } if id == "json"));
    g.press(middle(json), 1, NONE);
    assert!(!g.extractors_open());
    assert_eq!(g.extractor().id(), "json");
    // The command closes it too, and asks for it to be run.
    g.open_extractors();
    paint_at(&mut g, &mut text, 1200.0);
    let configure = row_of(&g, &|r| matches!(r, PopRow::Action { .. }));
    g.press(middle(configure), 1, NONE);
    assert!(!g.extractors_open());
    assert_eq!(
        g.take_action(),
        Some(GridAction::Command(extractors::CONFIGURE_CSV_COMMAND.into()))
    );
    // Esc, or a press elsewhere, closes it.
    g.open_extractors();
    assert!(g.edit_key(ViewerKey::Escape, KeyReach::default()));
    assert!(!g.extractors_open());
    g.open_extractors();
    paint_at(&mut g, &mut text, 1200.0);
    g.press(Point::new(5.0, 590.0), 1, NONE);
    assert!(!g.extractors_open());
}

#[test]
fn a_tall_popover_scrolls_in_a_short_grid() {
    let mut text = text_layout::TextSystem::new();
    let mut g = of_a_project();
    g.open_extractors();
    let mut scene = render_api::Scene::default();
    let palette = crate::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut paint = |g: &mut DataGrid<Fake>, text: &mut text_layout::TextSystem| {
        g.paint(GridPaint {
            bounds: Rect::new(0.0, 0.0, 900.0, 260.0),
            scale: 1.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
        });
    };
    paint(&mut g, &mut text);
    let area = g.extract_area.unwrap();
    assert!(area.size.height < 260.0);
    assert!(g.scroll(0.0, 10_000.0));
    paint(&mut g, &mut text);
    // Scrolled to its end: the last choice is in view.
    assert!(g.extract_scroll > 0.0);
    let last_choice =
        g.extract_rows.iter().rposition(|r| matches!(r, PopRow::Choice { .. })).unwrap();
    let r = g.hits.iter().find(|(_, h)| *h == Hit::ExtractorRow(last_choice)).unwrap().0;
    assert!(r.max_y() <= area.max_y() + 0.5);
}

#[test]
fn a_narrow_toolbar_puts_the_pill_in_the_more_menu() {
    let mut text = text_layout::TextSystem::new();
    let mut g = editable();
    g.set_project(Some(kernel::ProjectId::new()));
    paint_at(&mut g, &mut text, 300.0);
    assert!(g.collapsed_tools().contains(&Tool::Extractor));
    g.open_more();
    paint_at(&mut g, &mut text, 300.0);
    let row = g.hits.iter().find(|(_, h)| *h == Hit::MoreChoice(Tool::Extractor)).unwrap().0;
    g.press(middle(row), 1, NONE);
    assert!(g.extractors_open());
    paint_at(&mut g, &mut text, 300.0);
    assert!(g.extract_area.is_some(), "anchored at the \"…\"");
}

#[test]
fn a_partial_selection_is_copied_as_it_is() {
    const PRIMARY: PressMods = PressMods { shift: false, primary: true, alt: false };
    let mut g = of_a_project();
    // The first row's id and the second's name: a block of two rows and two columns, the
    // cells not chosen NULL.
    g.press(at(0, 0), 1, NONE);
    g.release();
    g.press(at(1, 1), 1, PRIMARY);
    g.release();
    assert_eq!(
        g.copy_as("sql").as_deref(),
        Some(
            "INSERT INTO \"people\" (\"id\", \"name\") VALUES (1, NULL);\nINSERT INTO \"people\" (\"id\", \"name\") VALUES (NULL, 'n1');"
        )
    );
    // Only names chosen: the WHERE clause is on them (the key isn't among them).
    g.press(at(0, 1), 1, NONE);
    g.release();
    g.press(at(2, 1), 1, SHIFT);
    g.release();
    assert_eq!(g.copy_as("sql-where").as_deref(), Some("\"name\" IN ('n0', 'n1', 'n2')"));
    // The whole rows: by the key.
    g.press(Point::new(10.0, at(0, 0).y), 1, NONE);
    g.release();
    assert_eq!(g.copy_as("sql-where").as_deref(), Some("\"id\" IN (1)"));
    assert_eq!(
        g.copy_as("sql-update").as_deref(),
        Some("UPDATE \"people\" SET \"name\" = 'n0', \"score\" = NULL WHERE \"id\" = 1;")
    );
}

/// A column found to be a key after it was fitted (the structure read after the rows) is
/// fitted again with room for the key's mark: its width never depends on which came first.
#[test]
fn a_column_that_becomes_a_key_is_fitted_again() {
    let mut text = text_layout::TextSystem::new();
    let fitted = |key_first: bool, text: &mut text_layout::TextSystem| {
        let mut source = Fake::new(10);
        source.columns[0].key = key_first;
        let mut g = DataGrid::new(source, Some(100));
        g.poll();
        paint_at(&mut g, text, 900.0);
        if !key_first {
            g.source.columns[0].key = true;
            g.poll();
            paint_at(&mut g, text, 900.0);
        }
        g.layout.width(0)
    };
    let after = fitted(false, &mut text);
    let before = fitted(true, &mut text);
    assert_eq!(after, before, "the key's room whichever came first");
}
