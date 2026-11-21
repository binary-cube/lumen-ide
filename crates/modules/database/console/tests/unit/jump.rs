//! The consoles' popover: the filter (fuzzy over the name and the text, the line that
//! matched shown and lit, the names' matches first, "New Query Console" always first), ↵
//! opening the first match, the texts read in the background, renaming and deleting.
// The places lit in a line: often only one range.
#![allow(clippy::single_range_in_vec_init)]

use super::*;
use crate::fake::{FakeAccess, FakeHost, Manual, Project};
use workbench_model::EditorHost as _;

fn names(found: &[Found], items: &[(&str, Option<&str>)]) -> Vec<String> {
    found.iter().map(|f| items[f.index].0.to_owned()).collect()
}

#[test]
fn an_empty_filter_lists_every_console_in_its_order() {
    let items = [("console", None), ("console_1", Some("SELECT 1"))];
    let found = filter("  ", &items);
    assert_eq!(names(&found, &items), ["console", "console_1"]);
    assert!(found.iter().all(|f| f.name_lit.is_empty() && f.line.is_none()));
}

#[test]
fn the_names_match_fuzzily_before_the_texts() {
    let items = [
        ("console", Some("SELECT * FROM orders;")),
        ("orders_by_day", Some("SELECT 1;")),
        ("report", Some("-- totals\nSELECT sum(total)\nFROM Orders o")),
        ("misc", Some("SELECT id FROM x")),
    ];
    let found = filter("ord", &items);
    assert_eq!(names(&found, &items), ["orders_by_day", "console", "report"]);
    // The name's characters matched, lit.
    assert_eq!(found[0].name_lit, [0..1, 1..2, 2..3]);
    // A text's first line holding it (any case), trimmed, what matched lit.
    assert_eq!(found[1].line, Some(("SELECT * FROM orders;".to_owned(), vec![14..17])));
    assert_eq!(found[2].line, Some(("FROM Orders o".to_owned(), vec![5..8])));
    // Fuzzy in the names: `obd` is orders_by_day's words.
    assert_eq!(names(&filter("obd", &items), &items), ["orders_by_day"]);
    // A text's characters close together, not scattered over a line.
    assert!(filter("zzz", &items).is_empty());
    let found = filter("smtl", &[("x", Some("SELECT sum(total) FROM t"))]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].line.as_ref().map(|(l, _)| l.as_str()), Some("SELECT sum(total) FROM t"));
    assert!(filter("sfa", &[("x", Some("SELECT * FROM t WHERE a = 1"))]).is_empty());
}

#[test]
fn a_console_s_text_is_looked_at_only_so_far() {
    let mut text = "-- x\n".repeat(TEXT_MAX / 5 + 10);
    text.push_str("SELECT needle");
    assert!(filter("needle", &[("c", Some(&text))]).is_empty());
}

struct Setup {
    project: Project,
    manual: Manual,
    access: Arc<FakeAccess>,
    consoles: Arc<crate::Consoles>,
    host: FakeHost,
    _registered: kernel::Subscription,
}

fn setup(name: &str) -> Setup {
    let project = Project::new(name);
    let manual = Manual::new();
    let access = Arc::new(FakeAccess::new(&project));
    let consoles = crate::fake::consoles(&project, &manual, Arc::clone(&access));
    let mut host = FakeHost::new(consoles.project(), &project.root);
    host.overlays = true;
    let registered = crate::consoles::register(Arc::clone(&consoles));
    Setup { project, manual, access, consoles, host, _registered: registered }
}

fn request(s: &Setup, list: bool) -> JumpRequest {
    JumpRequest {
        project: s.consoles.project(),
        source: s.access.shop,
        schema: None,
        table: None,
        new: false,
        list,
        database: None,
    }
}

/// The popover opened for the shop's consoles, its view made with what it was given.
fn popover(s: &mut Setup, list: bool) -> JumpView {
    let request = request(s, list);
    assert!(crate::commands::open_for(&mut s.host, &s.consoles, request));
    let data = s.host.jumps.last().expect("the popover").clone();
    let mut view = JumpView::default();
    view.set_data(&data);
    view
}

#[test]
fn typing_chooses_the_first_match_and_enter_opens_it() {
    let mut s = setup("jump-filter");
    s.consoles.create(s.access.shop, "SELECT * FROM users;").unwrap();
    let orders = s.consoles.create(s.access.shop, "-- the orders\nSELECT * FROM orders;").unwrap();
    s.consoles.create(s.access.shop, "").unwrap();
    let mut view = popover(&mut s, false);
    assert_eq!(view.selected(), 0, "New Query Console first, chosen");
    // The texts not open in a tab are read in the background.
    assert!(!view.texts_read());
    view.read_texts(None);
    s.manual.run_all();
    assert!(view.texts_read());
    assert_eq!(view.found_names(), ["console", "console_1", "console_2"]);
    assert!(view.text_input("orders"));
    assert_eq!(view.found_names(), ["console_1"]);
    let found = view.found()[0].clone();
    assert_eq!(found.line, Some(("-- the orders".to_owned(), vec![7..13])));
    assert_eq!(view.selected(), 1, "the first match chosen");
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(view.closed());
    let Some(ViewerAction::WithHost(work)) = view.take_action() else { panic!("its work") };
    work.run(&mut s.host);
    assert_eq!(s.host.opened.last(), Some(&orders.path(&s.project.root)));
}

#[test]
fn nothing_found_leaves_new_query_console_to_enter() {
    let mut s = setup("jump-none");
    s.consoles.create(s.access.shop, "SELECT 1;").unwrap();
    let mut view = popover(&mut s, false);
    view.set_filter("qqq");
    assert!(view.found().is_empty());
    assert_eq!(view.selected(), 0);
    assert!(view.edit_key(ViewerKey::Enter));
    let Some(ViewerAction::WithHost(work)) = view.take_action() else { panic!("its work") };
    work.run(&mut s.host);
    let opened = s.host.opened.last().unwrap();
    assert_eq!(opened.file_name().and_then(|n| n.to_str()), Some("console_1.sql"), "a new one");
}

#[test]
fn an_open_tab_s_text_is_matched_unsaved_changes_included() {
    let mut s = setup("jump-open");
    let spec = s.consoles.create(s.access.shop, "SELECT 1;").unwrap();
    s.host.open_file(&spec.path(&s.project.root));
    s.host.file.as_mut().unwrap().1 = "SELECT * FROM unsaved_table;".into();
    let mut view = popover(&mut s, false);
    assert!(view.texts_read(), "the open tab's text given");
    view.set_filter("unsaved");
    assert_eq!(view.found_names(), ["console"]);
}

#[test]
fn many_consoles_are_listed_a_few_rows_at_a_time() {
    let mut s = setup("jump-many");
    for i in 0..40 {
        s.consoles.create(s.access.shop, &format!("SELECT {i};")).unwrap();
    }
    let mut view = popover(&mut s, false);
    assert_eq!(view.found_names().len(), 40);
    for _ in 0..12 {
        view.edit_key(ViewerKey::Down);
    }
    assert_eq!(view.selected(), 12);
    assert_eq!(view.first, 12 + 1 - VISIBLE, "the chosen row kept in view");
    assert!(view.scroll(0.0, 10_000.0));
    assert_eq!(view.first, 41 - VISIBLE);
}

#[test]
fn database_consoles_lists_them_even_when_there_are_none() {
    let mut s = setup("jump-list");
    let view = popover(&mut s, true);
    assert!(s.host.jumps[0].consoles.is_empty());
    assert_eq!(view.rows(), 1, "only New Query Console");
    // Without `list`, none yet: one is made at once.
    let again = request(&s, false);
    assert!(crate::commands::open_for(&mut s.host, &s.consoles, again));
    assert_eq!(s.host.jumps.len(), 1);
    assert_eq!(s.consoles.consoles_of(s.access.shop).len(), 1);
}

#[test]
fn a_row_renames_or_deletes_its_console() {
    let mut s = setup("jump-rename");
    let spec = s.consoles.create(s.access.shop, "SELECT 1;").unwrap();
    let mut view = popover(&mut s, false);
    assert!(view.rename(1));
    let Some(ViewerAction::WithHost(work)) = view.take_action() else { panic!("its work") };
    work.run(&mut s.host);
    let asked = s.host.renames.last().unwrap();
    assert_eq!((asked.console, asked.name.as_str()), (spec.id, "console"));
    let mut view = popover(&mut s, false);
    assert!(!view.rename(0), "New Query Console is no console");
    assert!(!view.command("delete"), "nothing chosen to delete");
    view.edit_key(ViewerKey::Down);
    assert!(view.command("delete"));
    let Some(ViewerAction::WithHost(work)) = view.take_action() else { panic!("its work") };
    work.run(&mut s.host);
    // Asked first (the host answers yes): gone.
    assert_eq!(s.host.asked.len(), 1);
    assert!(s.consoles.spec(spec.id).is_none());
    assert!(!spec.path(&s.project.root).exists());
}

#[test]
fn it_draws_its_rows_and_closes_on_a_click_outside() {
    let mut s = setup("jump-paint");
    for i in 0..30 {
        s.consoles.create(s.access.shop, &format!("SELECT * FROM orders WHERE id = {i};")).unwrap();
    }
    let mut view = popover(&mut s, false);
    view.read_texts(None);
    s.manual.run_all();
    view.set_filter("orders");
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 2400.0, 1400.0),
        scale: 2.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let rows = view.hits.iter().filter(|(_, h)| matches!(h, Hit::Row(_))).count();
    assert_eq!(rows, VISIBLE, "only the rows seen");
    // The chosen row's own buttons.
    assert!(view.hits.iter().any(|(_, h)| *h == Hit::Rename(1)));
    // Inside on nothing: open; outside: closed (a popover loses nothing).
    let inside = Point::new(view.panel.min_x() + 4.0, view.panel.min_y() + 4.0);
    assert_eq!(view.click(inside), None);
    assert_eq!(view.click(Point::new(2.0, 2.0)), Some(ViewerAction::Close));
}

/// Many consoles: the app's scrollbar along the list's right edge; its thumb dragged moves
/// the list a row at a time (never past its end) and picks nothing.
#[test]
fn the_list_s_thumb_is_dragged_a_row_at_a_time() {
    let mut s = setup("jump-drag");
    for i in 0..30 {
        s.consoles.create(s.access.shop, &format!("SELECT {i};")).unwrap();
    }
    let mut view = popover(&mut s, false);
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1200.0, 900.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let first_row = view.hits.iter().find(|(_, h)| *h == Hit::Row(0)).map(|(r, _)| *r).unwrap();
    let y = first_row.min_y() + 6.0;
    let right = view.panel.max_x();
    let x = (0..14)
        .map(|dx| right - 1.0 - dx as f32)
        .find(|x| {
            let taken = Viewer::press(&mut view, Point::new(*x, y), 1).is_none() && view.first == 0;
            taken && view.bar.is_dragging()
        })
        .expect("the thumb at the list's top, along the right edge");
    assert!(view.drag(Point::new(x, y + 150.0)) && view.first > 0);
    view.drag(Point::new(x, y + 10_000.0));
    assert_eq!(view.first, view.rows() - VISIBLE, "at its end, no further");
    assert!(view.release());
    assert!(!view.closed(), "nothing picked");
}

/// The filter is a control with the keys the whole time: a press on it is its own (nothing
/// picked, nothing closed), what's typed and ⌫ go through the view's controls and filter
/// again; a click on a row leaves it the keys.
#[test]
fn the_filter_keeps_the_keys_through_its_controls() {
    let mut s = setup("jump-filter");
    s.consoles.create(s.access.shop, "SELECT * FROM orders;").unwrap();
    s.consoles.create(s.access.shop, "SELECT 1;").unwrap();
    let mut view = popover(&mut s, false);
    view.read_texts(None);
    s.manual.run_all();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let mut paint = |view: &mut JumpView| {
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1200.0, 900.0),
            scale: 1.0,
            text: &mut text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    paint(&mut view);
    let field = view.filter.area();
    let on_field = Point::new(field.min_x() + 40.0, field.min_y() + field.size.height / 2.0);
    view.hover(on_field);
    assert_eq!(view.cursor_at(on_field), Some(ViewerCursor::Text));
    assert_eq!(Viewer::press(&mut view, on_field, 1), None);
    assert!(!view.closed());
    assert_eq!(view.controls.focused(), Some(FILTER));
    assert!(view.text_input("ordersx"));
    assert!(view.found_names().is_empty());
    assert!(view.edit_key(ViewerKey::Backspace));
    assert_eq!(view.found_names(), ["console"]);
    // ⌘A selects it all; its × empties it and filters again.
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("orders"));
    paint(&mut view);
    let x = Point::new(field.max_x() - 17.0, on_field.y);
    view.hover(x);
    assert_eq!(view.cursor_at(x), Some(ViewerCursor::Pointer), "a hand on ×");
    assert_eq!(Viewer::press(&mut view, x, 1), None);
    assert_eq!(view.filter.text(), "");
    assert!(view.found_names().len() > 1, "× filtered again");
    assert!(view.text_input("orders"));
    // A row clicked: the filter still types.
    let row = view.hits.iter().find(|(_, h)| *h == Hit::Row(0)).map(|(r, _)| *r).unwrap();
    view.click(Point::new(row.min_x() + 4.0, row.min_y() + 4.0));
    assert!(view.text_input("q"));
    assert!(view.found_names().is_empty());
}
