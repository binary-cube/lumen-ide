use super::*;
use render_api::Scene;
use ui_components::{KeyReach, ViewerKey};

const FLOW: &str = include_str!("../../../../../domain/tools/diagram-model/tests/data/flow.drawio");

/// A file in a fresh scratch folder, removed at the end.
pub(crate) struct Scratch(PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.0.parent().unwrap_or(&self.0));
    }
}

fn file(text: &str) -> Scratch {
    let dir = std::env::temp_dir().join(format!(
        "diagram-view-{}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("flow.drawio");
    std::fs::write(&path, text).unwrap();
    Scratch(path)
}

pub(crate) fn paint_view(view: &mut DiagramView, text: &mut TextSystem) -> Scene {
    paint(view, text)
}

fn paint(view: &mut DiagramView, text: &mut TextSystem) -> Scene {
    let (palette, icons) = (ui_components::Palette::dark(), icons::Icons::builtin());
    let mut scene = Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1200.0, 800.0),
        scale: 1.0,
        text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    scene
}

/// Polled until the file is read (it's read on a worker).
fn loaded(view: &mut DiagramView) {
    let started = std::time::Instant::now();
    while view.working() && started.elapsed() < std::time::Duration::from_secs(10) {
        view.poll();
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

/// Opened: read on a worker, drawn fitted to the view, its pages as tabs (a click shows
/// another), its status; zoomed and panned.
#[test]
fn a_diagram_opens_fitted_with_its_pages() {
    let f = file(FLOW);
    let mut view = DiagramView::open(f.0.clone());
    let mut text = TextSystem::new();
    loaded(&mut view);
    let scene = paint(&mut view, &mut text);
    assert!(scene.quad_count() > 0);
    assert_eq!(view.status().as_deref(), Some("2 pages · 9 shapes"));
    assert!(view.zoom_level().is_some_and(|z| z > 0.5 && z <= 1.0), "fitted");
    assert_eq!(view.tabs.len(), 2, "its pages as tabs");
    let (layers_tab, _) = view.tabs[1];
    view.pointer_down(layers_tab.center(), 1, PressMods::default());
    assert_eq!(view.page, 1);
    paint(&mut view, &mut text);
    assert_eq!(view.status().as_deref(), Some("2 pages · 1 shapes"));
    let before = view.camera;
    assert!(view.zoom(2.0, Point::new(600.0, 400.0)));
    assert!(view.camera.zoom > before.zoom);
    assert!(view.reset_zoom() && (view.camera.zoom - 1.0).abs() < 1e-4);
    assert!(view.scroll(10.0, 20.0));
    // ⌥ and a drag pans the sheet.
    let at = view.camera.offset;
    let alt = PressMods { alt: true, ..PressMods::default() };
    view.pointer_down(Point::new(500.0, 300.0), 1, alt);
    assert!(view.drag(Point::new(530.0, 310.0)));
    assert!(view.release());
    assert_eq!(view.camera.offset, Point::new(at.x + 30.0, at.y + 10.0));
}

/// Changed on disk: read again (the same text: nothing to do); unreadable: why, said.
#[test]
fn a_diagram_follows_its_file() {
    let f = file(FLOW);
    let mut view = DiagramView::open(f.0.clone());
    let mut text = TextSystem::new();
    loaded(&mut view);
    paint(&mut view, &mut text);
    view.file_changed();
    loaded(&mut view);
    assert!(!view.poll(), "the same text: nothing new");
    std::fs::write(&f.0, FLOW.replace("value=\"Start\"", "value=\"Begin\"")).unwrap();
    view.file_changed();
    loaded(&mut view);
    let page = &view.diagram.as_ref().unwrap().doc.pages[0];
    assert_eq!(page.cell("start").unwrap().value, "Begin");
    let broken = file("<mxfile><diagram>%%%</diagram></mxfile>");
    let mut bad = DiagramView::open(broken.0.clone());
    loaded(&mut bad);
    paint(&mut bad, &mut text);
    assert!(bad.error.is_some() && bad.diagram.is_none());
}

/// A view of the fixture at 100%, its sheet's corner on diagram (0, 0).
pub(crate) fn editing_view(text: &mut TextSystem) -> (DiagramView, Scratch) {
    editing(text)
}

fn editing(text: &mut TextSystem) -> (DiagramView, Scratch) {
    let f = file(FLOW);
    let mut view = DiagramView::open(f.0.clone());
    loaded(&mut view);
    paint(&mut view, text);
    view.camera = Camera::default();
    paint(&mut view, text);
    (view, f)
}

/// Diagram point `(x, y)` on the screen (the camera at 100%, its corner on the sheet's).
pub(crate) fn screen(view: &DiagramView, x: f32, y: f32) -> Point {
    view.camera.to_screen(Point::new(x, y), view.sheet(), 1.0)
}

fn geometry(view: &DiagramView, id: &str) -> (f64, f64, f64, f64) {
    view.diagram.as_ref().unwrap().doc.pages[0]
        .cell(id)
        .unwrap()
        .geometry
        .as_ref()
        .unwrap()
        .bounds()
}

/// Dragged from diagram point `from` to `to`.
fn drag(view: &mut DiagramView, from: (f32, f32), to: (f32, f32), mods: PressMods) {
    let (from, to) = (screen(view, from.0, from.1), screen(view, to.0, to.1));
    view.pointer_down(from, 1, mods);
    view.drag(Point::new((from.x + to.x) / 2.0, (from.y + to.y) / 2.0));
    view.drag(to);
    view.release();
}

/// Chosen by a click (⇧ adds, again takes away) and by a band; a shape dragged moves on the
/// grid at the release, as one step undone at once.
#[test]
fn shapes_are_chosen_and_moved() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    view.pointer_down(screen(&view, 400.0, 200.0), 1, PressMods::default());
    view.release();
    assert_eq!(view.selection, ["check"]);
    let shift = PressMods { shift: true, ..PressMods::default() };
    view.pointer_down(screen(&view, 600.0, 200.0), 1, shift);
    assert_eq!(view.selection, ["check", "save"]);
    view.pointer_down(screen(&view, 600.0, 200.0), 1, shift);
    assert_eq!(view.selection, ["check"]);
    // A band from the empty sheet around the start and the decision.
    drag(&mut view, (320.0, 20.0), (480.0, 260.0), PressMods::default());
    assert!(
        view.selection.contains(&"start".to_owned()) && view.selection.contains(&"e1".to_owned())
    );
    // The decision dragged by (23, 7): on the grid (10).
    view.pointer_down(screen(&view, 2.0, 2.0), 1, PressMods::default());
    view.release();
    assert!(view.selection.is_empty(), "the empty sheet lets go");
    drag(&mut view, (400.0, 200.0), (423.0, 207.0), PressMods::default());
    assert_eq!(geometry(&view, "check"), (370.0, 170.0, 100.0, 80.0));
    assert!(view.is_dirty());
    assert!(view.command("undo"));
    assert_eq!(geometry(&view, "check"), (350.0, 160.0, 100.0, 80.0), "one step");
    assert!(view.command("redo"));
    assert_eq!(geometry(&view, "check").0, 370.0);
}

/// The one chosen shape's handle dragged resizes it (on the grid); keys delete and move it.
#[test]
fn a_shape_is_resized_moved_by_keys_and_deleted() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    view.pointer_down(screen(&view, 600.0, 200.0), 1, PressMods::default());
    view.release();
    paint(&mut view, &mut text);
    // Its bottom-right handle, at (700, 230), dragged by (38, 21).
    drag(&mut view, (700.0, 230.0), (738.0, 251.0), PressMods::default());
    assert_eq!(geometry(&view, "save"), (560.0, 170.0, 180.0, 80.0));
    assert!(view.edit_key_with(ViewerKey::Right, KeyReach::default()));
    assert_eq!(geometry(&view, "save").0, 561.0, "a point");
    assert!(view.edit_key_with(ViewerKey::Down, KeyReach { word: false, extend: true }));
    assert_eq!(geometry(&view, "save").1, 180.0, "⇧: a grid step");
    assert!(view.edit_key_with(ViewerKey::Delete, KeyReach::default()));
    assert!(view.diagram.as_ref().unwrap().doc.pages[0].cell("save").is_none());
    assert!(view.diagram.as_ref().unwrap().doc.pages[0].cell("e2").is_none(), "its edge too");
    assert!(
        view.edit_key_with(ViewerKey::Escape, KeyReach::default()) || view.selection.is_empty()
    );
}

/// Saved: written over the file (its report from the disk changes nothing), not dirty; locked:
/// chosen, never changed; changed on disk under unsaved changes: kept, the user told.
#[test]
fn saved_locked_and_kept() {
    let mut text = TextSystem::new();
    let (mut view, f) = editing(&mut text);
    view.pointer_down(screen(&view, 400.0, 200.0), 1, PressMods::default());
    view.release();
    view.edit_key_with(ViewerKey::Right, KeyReach::default());
    view.save().unwrap();
    assert!(!view.is_dirty());
    let written = std::fs::read_to_string(&f.0).unwrap();
    assert!(
        written.contains(r#"<mxGeometry x="351" y="160" width="100" height="80" as="geometry" />"#)
    );
    assert_eq!(written.lines().count(), FLOW.lines().count(), "one line changed, no other");
    view.file_changed();
    loaded(&mut view);
    assert!(!view.poll() && view.take_notifications().is_empty(), "its own save: nothing");
    view.set_locked(true);
    assert!(!view.edit_key_with(ViewerKey::Right, KeyReach::default()));
    assert!(!view.command("undo"));
    view.set_locked(false);
    view.edit_key_with(ViewerKey::Right, KeyReach::default());
    std::fs::write(&f.0, FLOW.replace("Start", "Begin")).unwrap();
    view.file_changed();
    loaded(&mut view);
    assert_eq!(geometry(&view, "check").0, 352.0, "the unsaved change kept");
    assert_eq!(view.take_notifications().len(), 1);
}

/// Copied as draw.io's XML; pasted: new cells a step away, chosen; cut: copied then gone;
/// duplicated: one step.
#[test]
fn cells_are_copied_pasted_cut_and_duplicated() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    assert_eq!(view.selected_text(), None, "nothing chosen");
    view.selection = vec!["check".into(), "save".into()];
    let xml = view.selected_text().unwrap();
    assert!(
        xml.starts_with("<mxGraphModel") && xml.contains("id=\"e2\""),
        "the edge between them too"
    );
    assert!(view.paste(&xml));
    paint(&mut view, &mut text);
    assert_eq!(view.selection.len(), 3, "the pasted ones chosen (two shapes, their edge)");
    let page = &view.diagram.as_ref().unwrap().doc.pages[0];
    let pasted = page.cell(&view.selection[0]).unwrap();
    assert_eq!(pasted.geometry.as_ref().unwrap().bounds(), (360.0, 170.0, 100.0, 80.0));
    assert!(!view.paste("not a diagram"));
    // ⌘X: the app copies, then the view cuts.
    let cut = view.selected_text().unwrap();
    assert!(view.command("cut"));
    assert!(!cut.is_empty() && view.selection.is_empty());
    assert!(
        view.diagram.as_ref().unwrap().doc.pages[0].cell("check").is_some(),
        "the originals stay"
    );
    // ⌘D: one step.
    view.selection = vec!["start".into()];
    let cells = view.diagram.as_ref().unwrap().doc.pages[0].cells.len();
    assert!(view.command("duplicate"));
    assert_eq!(view.diagram.as_ref().unwrap().doc.pages[0].cells.len(), cells + 1);
    assert!(view.command("undo"));
    assert_eq!(view.diagram.as_ref().unwrap().doc.pages[0].cells.len(), cells);
}

fn cell(view: &DiagramView, id: &str) -> diagram_model::Cell {
    view.diagram.as_ref().unwrap().doc.pages[0].cell(id).unwrap().clone()
}

/// Over a shape, its connection points; one dragged onto another shape: a new edge joining
/// them (chosen, one step); onto the sheet: its end loose there (on the grid).
#[test]
fn an_edge_is_drawn_from_a_connection_point() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    assert!(view.hover(screen(&view, 400.0, 70.0)));
    assert_eq!(view.hovered.as_deref(), Some("start"));
    // Its bottom point (400, 100) dragged onto the save shape.
    drag(&mut view, (400.0, 100.0), (630.0, 200.0), PressMods::default());
    let id = view.selection[0].clone();
    let edge = cell(&view, &id);
    assert_eq!((edge.source.as_deref(), edge.target.as_deref()), (Some("start"), Some("save")));
    assert!(edge.style.get("edgeStyle").is_some());
    assert!(view.command("undo"));
    assert!(view.diagram.as_ref().unwrap().doc.pages[0].cell(&id).is_none(), "one step");
    view.hover(screen(&view, 400.0, 70.0));
    drag(&mut view, (400.0, 100.0), (203.0, 98.0), PressMods::default());
    let loose = cell(&view, &view.selection[0].clone());
    assert_eq!(loose.target, None);
    assert_eq!(loose.geometry.unwrap().target_point, Some(diagram_model::Point::new(200.0, 100.0)));
}

/// The chosen edge's end dragged onto another shape joins it there; a middle dragged puts a
/// point in.
#[test]
fn an_edge_is_reshaped_by_its_handles() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    view.selection = vec!["e1".into()];
    paint(&mut view, &mut text);
    // e1 runs from (400, 100) to (400, 160): its end onto the save shape.
    drag(&mut view, (400.0, 160.0), (630.0, 200.0), PressMods::default());
    assert_eq!(cell(&view, "e1").target.as_deref(), Some("save"));
    paint(&mut view, &mut text);
    view.selection = vec!["loose".into()];
    paint(&mut view, &mut text);
    // The loose edge (80, 300) → (200, 300): its middle dragged down.
    drag(&mut view, (140.0, 300.0), (140.0, 342.0), PressMods::default());
    assert_eq!(
        cell(&view, "loose").geometry.unwrap().points,
        [diagram_model::Point::new(140.0, 340.0)]
    );
}

/// The menu: an edge's line and end (two keys, one step), front and back, group and ungroup.
#[test]
fn the_menu_changes_lines_ends_order_and_groups() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let on_edge = screen(&view, 400.0, 130.0);
    let entries = view.context_menu(on_edge).unwrap();
    assert_eq!(view.selection, ["e1"], "the cell under the mouse chosen");
    assert!(matches!(&entries[0], ui_components::ViewMenuEntry::Submenu { .. }), "its line first");
    assert!(view.command("line:curved"));
    let e1 = cell(&view, "e1");
    assert_eq!((e1.style.get("curved"), e1.style.get("edgeStyle")), (Some("1"), None));
    assert!(view.command("undo"));
    assert!(cell(&view, "e1").style.get("edgeStyle").is_some(), "both keys in one step");
    assert!(view.command("end:none"));
    assert_eq!(cell(&view, "e1").style.get("endArrow"), Some("none"));
    view.selection = vec!["start".into(), "check".into()];
    assert!(view.command("group"));
    let group = view.selection[0].clone();
    assert_eq!(cell(&view, "check").parent.as_deref(), Some(group.as_str()));
    assert!(view.command("ungroup"));
    assert_eq!(cell(&view, "check").parent.as_deref(), Some("1"));
    view.selection = vec!["start".into()];
    assert!(view.command("to-front"));
    let cells = &view.diagram.as_ref().unwrap().doc.pages[0].cells;
    assert_eq!(cells.last().map(|c| c.id.as_str()), Some("start"));
}

/// The palette: a shape clicked lands in the view's middle (chosen, one step); dragged onto
/// the sheet, where it's let go (on the grid); a search finds it, ↩ adds the first found; a
/// section folds; a shape's tooltip is its name.
#[test]
fn shapes_come_from_the_palette() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let count = |v: &DiagramView| v.diagram.as_ref().unwrap().doc.pages[0].cells.len();
    let before = count(&view);
    let entry = |v: &DiagramView, s: usize, e: usize| {
        v.palette_hits_for_tests()
            .into_iter()
            .find(|(_, h)| *h == (s, e))
            .map(|(r, _)| r.center())
            .unwrap()
    };
    // The general section's ellipse (3), clicked.
    let at = entry(&view, 0, 3);
    view.pointer_down(at, 1, PressMods::default());
    view.release();
    assert_eq!(count(&view), before + 1);
    let added = cell(&view, &view.selection[0].clone());
    assert_eq!(added.style.shape(), Some("ellipse"));
    assert!(view.command("undo"));
    assert_eq!(count(&view), before, "one step");
    // The rectangle (0) dragged to diagram (203, 498): centered there, on the grid.
    let to = screen(&view, 203.0, 498.0);
    view.pointer_down(entry(&view, 0, 0), 1, PressMods::default());
    view.drag(to);
    paint(&mut view, &mut text);
    view.release();
    let added = cell(&view, &view.selection[0].clone());
    assert_eq!(added.geometry.unwrap().bounds(), (140.0, 470.0, 120.0, 60.0));
    // A search, ↩: the first shape found added.
    let field = view.palette_hits_for_search();
    view.pointer_down(field, 1, PressMods::default());
    assert!(view.wants_text());
    view.text_input("cyl");
    paint(&mut view, &mut text);
    assert_eq!(view.palette_hits_for_tests().len(), 1, "only the cylinder");
    assert!(view.edit_key_with(ViewerKey::Enter, KeyReach::default()));
    assert_eq!(cell(&view, &view.selection[0].clone()).style.shape(), Some("cylinder3"));
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(!view.wants_text(), "Esc twice: emptied, then let go");
    paint(&mut view, &mut text);
    let ellipse = entry(&view, 0, 3);
    assert!(view.tooltip_at(ellipse).is_some(), "its name");
    // The general section folded: its shapes gone; the flowchart's opened: its shown.
    view.pointer_down(view.palette_section_for_tests(0), 1, PressMods::default());
    paint(&mut view, &mut text);
    assert!(view.palette_hits_for_tests().iter().all(|(_, (s, _))| *s != 0));
    view.pointer_down(view.palette_section_for_tests(1), 1, PressMods::default());
    paint(&mut view, &mut text);
    assert!(view.palette_hits_for_tests().iter().any(|(_, (s, _))| *s == 1));
}

/// Nothing chosen, the page's settings: its grid off and on, its step, its background — each
/// a step undone.
#[test]
fn the_page_s_properties() {
    use super::properties::{Number, Prop};
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let page = |v: &DiagramView| v.diagram.as_ref().unwrap().doc.pages[0].clone();
    let press = |v: &mut DiagramView, prop: Prop| {
        let at = v.prop_for_tests(prop).unwrap_or_else(|| panic!("{prop:?} shown"));
        v.pointer_down(at, 1, PressMods::default());
        v.release();
    };
    press(&mut view, Prop::Grid);
    assert!(!page(&view).grid());
    paint(&mut view, &mut text);
    press(&mut view, Prop::Step(Number::GridSize, 1.0));
    assert_eq!(page(&view).grid_size(), 15.0);
    paint(&mut view, &mut text);
    press(
        &mut view,
        Prop::Swatch("background", ui_components::ds::Swatch::Color(kernel::Rgba::rgb(0xdae8fc))),
    );
    assert_eq!(page(&view).background(), Some("#DAE8FC"));
    assert!(view.command("undo"));
    assert_eq!(page(&view).background(), None, "one step");
}

/// A shape chosen: its fill, line, width (−/+ and typed), bold, alignment, place — each
/// shown as it is and set as one step.
#[test]
fn a_shape_s_properties() {
    use super::properties::{Number, Prop};
    use ui_components::ds::Swatch;
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    view.selection = vec!["save".into()];
    paint(&mut view, &mut text);
    let press = |v: &mut DiagramView, text: &mut TextSystem, prop: Prop| {
        let at = v.prop_for_tests(prop).unwrap_or_else(|| panic!("{prop:?} shown"));
        v.pointer_down(at, 1, PressMods::default());
        v.release();
        paint(v, text);
    };
    press(
        &mut view,
        &mut text,
        Prop::Swatch("fillColor", Swatch::Color(kernel::Rgba::rgb(0xfff2cc))),
    );
    assert_eq!(cell(&view, "save").style.get("fillColor"), Some("#FFF2CC"));
    press(&mut view, &mut text, Prop::Swatch("strokeColor", Swatch::None));
    assert_eq!(cell(&view, "save").style.get("strokeColor"), Some("none"));
    press(&mut view, &mut text, Prop::Swatch("strokeColor", Swatch::Default));
    assert_eq!(cell(&view, "save").style.get("strokeColor"), None, "the theme's: taken away");
    press(&mut view, &mut text, Prop::Step(Number::LineWidth, 1.0));
    assert_eq!(cell(&view, "save").style.get("strokeWidth"), Some("2"));
    // Typed: 4, ↩.
    press(&mut view, &mut text, Prop::Type(Number::LineWidth));
    assert!(view.wants_text());
    view.text_input("4");
    assert!(view.edit_key_with(ViewerKey::Enter, KeyReach::default()));
    assert_eq!(cell(&view, "save").style.get("strokeWidth"), Some("4"));
    // Typed, then Esc: nothing.
    press(&mut view, &mut text, Prop::Type(Number::LineWidth));
    view.text_input("9");
    view.edit_key_with(ViewerKey::Escape, KeyReach::default());
    assert_eq!(cell(&view, "save").style.get("strokeWidth"), Some("4"));
    press(&mut view, &mut text, Prop::FontBit(1));
    assert_eq!(cell(&view, "save").style.get("fontStyle"), Some("1"));
    press(&mut view, &mut text, Prop::Align("left"));
    assert_eq!(cell(&view, "save").style.get("align"), Some("left"));
    // Its place: typed.
    press(&mut view, &mut text, Prop::Type(Number::X));
    view.text_input("600");
    view.edit_key_with(ViewerKey::Enter, KeyReach::default());
    assert_eq!(geometry(&view, "save").0, 600.0);
}

/// The bar: the palette and the properties hidden and shown back (the sheet wider), undo and
/// redo, the zoom.
#[test]
fn the_bar_s_buttons() {
    use super::toolbar::Tool;
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let press = |v: &mut DiagramView, text: &mut TextSystem, tool: Tool| {
        let at = v.tool_for_tests(tool).unwrap();
        v.pointer_down(at, 1, PressMods::default());
        v.release();
        paint(v, text);
    };
    let wide = view.sheet().size.width;
    press(&mut view, &mut text, Tool::Palette);
    press(&mut view, &mut text, Tool::Properties);
    assert!(view.sheet().size.width > wide + 300.0, "both panels hidden");
    press(&mut view, &mut text, Tool::Palette);
    press(&mut view, &mut text, Tool::Properties);
    assert_eq!(view.sheet().size.width, wide);
    let zoom = view.camera.zoom;
    press(&mut view, &mut text, Tool::ZoomIn);
    assert!(view.camera.zoom > zoom);
    view.selection = vec!["start".into()];
    paint(&mut view, &mut text);
    press(&mut view, &mut text, Tool::Delete);
    assert!(view.diagram.as_ref().unwrap().doc.pages[0].cell("start").is_none());
    press(&mut view, &mut text, Tool::Undo);
    assert!(view.diagram.as_ref().unwrap().doc.pages[0].cell("start").is_some());
    press(&mut view, &mut text, Tool::Redo);
    assert!(view.diagram.as_ref().unwrap().doc.pages[0].cell("start").is_none());
}

/// The sheet's menu with nothing chosen: the exports. Each asks where (the file's name and
/// the page's, as the file has two pages), then writes there and says so.
#[test]
fn a_page_is_exported() {
    let mut text = TextSystem::new();
    let (mut view, f) = editing(&mut text);
    view.selection.clear();
    let sheet = view.sheet();
    let empty = Point::new(sheet.min_x() + 4.0, sheet.max_y() - 4.0);
    let menu = view.menu_at(empty).expect("the page's menu");
    let ids: Vec<&str> = menu
        .iter()
        .filter_map(|e| match e {
            ui_components::ViewMenuEntry::Item { id, .. } => Some(id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, ["export:svg", "export:png"]);
    for (id, ext) in [("export:svg", "svg"), ("export:png", "png")] {
        assert!(view.command(id));
        let Some(ViewerAction::Request(ui_components::AppRequest::PickSavePath { start, name })) =
            view.take_action()
        else {
            panic!("asked where")
        };
        assert_eq!(start.as_deref(), f.0.parent());
        let stem = f.0.file_stem().unwrap().to_string_lossy().into_owned();
        assert_eq!(name, format!("{stem}-Flow.{ext}"));
        let to = f.0.with_file_name(&name);
        assert!(view.command(&format!("picked-save:{}", to.display())));
        assert!(view.poll());
        let written = std::fs::read(&to).unwrap();
        assert!(written.starts_with(if ext == "svg" {
            b"<svg".as_slice()
        } else {
            b"\x89PNG".as_slice()
        }));
        let notices = view.take_notifications();
        assert_eq!(notices.len(), 1);
        assert!(notices[0].title.contains(&name), "{}", notices[0].title);
        let _ = std::fs::remove_file(&to);
    }
    // A path chosen with no export asked: nothing.
    assert!(!view.command("picked-save:/nowhere.svg"));
}

/// Renamed (the tab follows): saved where it is now, never at its old path again.
#[test]
fn a_moved_diagram_saves_where_it_is() {
    let mut text = TextSystem::new();
    let (mut view, f) = editing(&mut text);
    let to = f.0.with_file_name("renamed.drawio");
    std::fs::rename(&f.0, &to).unwrap();
    view.moved(&to);
    view.selection = vec!["start".into()];
    assert!(view.command("cut"));
    view.save().unwrap();
    assert!(!f.0.exists(), "not made again at its old path");
    assert!(!std::fs::read_to_string(&to).unwrap().contains(r#"id="start""#));
}

/// A number being typed in the panel has the keys' commands: ⌘X, ⌘A, ⌘Z act on it, never
/// on the shape chosen; what's copied is the field's selection.
#[test]
fn the_panel_s_field_keeps_its_commands() {
    use super::properties::{Number, Prop};
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    view.selection = vec!["save".into()];
    paint(&mut view, &mut text);
    let at = view.prop_for_tests(Prop::Type(Number::W)).unwrap();
    view.pointer_down(at, 1, PressMods::default());
    view.release();
    assert!(view.wants_text());
    view.text_input("120");
    assert!(view.command("select_all"));
    assert_eq!(view.selected_text().as_deref(), Some("120"));
    assert!(view.command("cut"));
    assert!(view.diagram.as_ref().unwrap().doc.pages[0].cell("save").is_some(), "the shape stays");
    assert!(view.command("undo"));
    assert!(view.wants_text(), "still typing");
}

/// An older read arriving after a newer one (or after a save) is let go.
#[test]
fn an_old_read_is_let_go() {
    let mut text = TextSystem::new();
    let (mut view, f) = editing(&mut text);
    let old = Document::read(&std::fs::read_to_string(&f.0).unwrap()).unwrap();
    view.selection = vec!["start".into()];
    view.command("cut");
    view.save().unwrap();
    // What a read started before the save would bring: the shape still there.
    *view.reading.lock().unwrap() = Some((view.reads - 1, Ok((fingerprint(FLOW), old))));
    assert!(!view.poll());
    assert!(view.diagram.as_ref().unwrap().doc.pages[0].cell("start").is_none());
}

/// Esc during a drag ends it and keeps what's chosen; again, nothing chosen.
#[test]
fn escape_ends_a_drag_first() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let from = screen(&view, 400.0, 200.0);
    view.pointer_down(from, 1, PressMods::default());
    view.drag(Point::new(from.x + 30.0, from.y));
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert_eq!(view.selection, ["check"]);
    assert!(view.edit_key_with(ViewerKey::Escape, KeyReach::default()));
    assert!(view.selection.is_empty());
}

/// A measured frame: its time, the glyphs, quads and images it drew.
type Frame<'a> =
    dyn FnMut(&mut DiagramView, &mut TextSystem) -> (std::time::Duration, usize, usize, usize) + 'a;

/// 2 000 shapes and 2 400 edges: what opening, a frame, a pan, a zoom step, a mouse move
/// and a drag's step cost (plan 80's measure: `--release -- --ignored --nocapture`).
#[test]
#[ignore]
fn measure_a_large_page() {
    use diagram_model::{CellKind, Geometry, Style};
    use std::time::Instant;
    let mut doc = Document::new("Big", "big");
    for i in 0..2000u32 {
        let mut c = diagram_model::Cell::new(format!("v{i}"), CellKind::Vertex, "1");
        c.value = format!("Shape number {i} with a longer label");
        c.style = Style::parse(match i % 4 {
            0 => "ellipse;whiteSpace=wrap;html=1;",
            1 => "rhombus;whiteSpace=wrap;html=1;fillColor=#fff2cc;strokeColor=#d6b656;",
            _ => "rounded=1;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;",
        });
        c.geometry = Some(Geometry {
            x: f64::from(i % 50) * 160.0,
            y: f64::from(i / 50) * 100.0,
            width: 120.0,
            height: 60.0,
            ..Default::default()
        });
        doc.pages[0].cells.push(c);
    }
    for i in 0..2400u32 {
        let mut e = diagram_model::Cell::new(format!("e{i}"), CellKind::Edge, "1");
        e.style = Style::parse("edgeStyle=orthogonalEdgeStyle;html=1;");
        e.source = Some(format!("v{}", i % 2000));
        e.target = Some(format!("v{}", (i * 7 + 1) % 2000));
        e.geometry = Some(Geometry { relative: true, ..Default::default() });
        doc.pages[0].cells.push(e);
    }
    let f = file(&doc.write());
    let (palette, icons) = (ui_components::Palette::dark(), icons::Icons::builtin());
    // A waker, as in the app: the tiles are made on workers, a frame is the UI thread's work.
    let waker: std::sync::Arc<dyn Fn() + Send + Sync> = std::sync::Arc::new(|| {});
    let mut text = TextSystem::new();
    let mut frame = |view: &mut DiagramView, text: &mut TextSystem| {
        let mut scene = Scene::default();
        let started = Instant::now();
        view.paint(ViewerPaint {
            bounds: Rect::new(0.0, 0.0, 1600.0, 1000.0),
            scale: 2.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: Some(std::sync::Arc::clone(&waker)),
        });
        let took = started.elapsed();
        let glyphs: usize = scene.layers().iter().map(|l| l.glyphs.len()).sum();
        let quads: usize = scene.layers().iter().map(|l| l.quads.len()).sum();
        let images: usize = scene.layers().iter().map(|l| l.images.len()).sum();
        (took, glyphs, quads, images)
    };
    let started = Instant::now();
    let mut view = DiagramView::open(f.0.clone());
    loaded(&mut view);
    let read = started.elapsed();
    // Until every tile in view is there (they come from the workers; a frame takes them).
    let settle = |view: &mut DiagramView, text: &mut TextSystem, frame: &mut Frame<'_>| {
        let started = Instant::now();
        while view.working() && started.elapsed() < std::time::Duration::from_secs(10) {
            std::thread::sleep(std::time::Duration::from_millis(1));
            view.poll();
            frame(view, text);
        }
        started.elapsed()
    };
    let first = frame(&mut view, &mut text);
    let tiles_wait = settle(&mut view, &mut text, &mut frame);
    let second = frame(&mut view, &mut text);
    // A zoom step with the whole page in view (every label laid out at the new size).
    view.zoom(1.05, Point::new(800.0, 500.0));
    let fit_zoom = frame(&mut view, &mut text).0;
    // Zoomed in to 100% around the middle, then panned.
    view.camera = Camera::default();
    let zoomed = frame(&mut view, &mut text);
    let zoomed_tiles = settle(&mut view, &mut text, &mut frame);
    let again = frame(&mut view, &mut text);
    view.scroll(0.0, 40.0);
    let panned = frame(&mut view, &mut text);
    view.zoom(1.1, Point::new(800.0, 500.0));
    let zoom_step = frame(&mut view, &mut text);
    // A pinch: 20 frames of 1% each, then back.
    let mut pinch = std::time::Duration::ZERO;
    for factor in [1.01f32; 20].into_iter().chain([1.0 / 1.01; 20]) {
        view.zoom(factor, Point::new(800.0, 500.0));
        pinch += frame(&mut view, &mut text).0;
    }
    let pinch = pinch / 40;
    let started = Instant::now();
    for i in 0..100 {
        view.hover(Point::new(400.0 + i as f32 * 3.0, 300.0));
    }
    let hover = started.elapsed() / 100;
    // A drag of the first shape: pressed, ten steps (each with its frame), released.
    // The shape under the sheet's middle (shapes are 160 × 100 apart, 120 × 60 big).
    let middle = view.camera.to_diagram(view.sheet().center(), view.sheet(), 2.0);
    let (mut col, row) = ((middle.x / 160.0).floor() as u32, (middle.y / 100.0).floor() as u32);
    // A plain box (i % 4 == 2), pressed near its top left (edges meet its sides' middles).
    while (row * 50 + col) % 4 != 2 {
        col += 1;
    }
    let id = format!("v{}", row * 50 + col);
    let inside = Point::new(col as f32 * 160.0 + 20.0, row as f32 * 100.0 + 12.0);
    let at = view.camera.to_screen(inside, view.sheet(), 2.0);
    view.selection = vec![id.clone()];
    let before = geometry(&view, &id);
    view.pointer_down(at, 1, PressMods::default());
    let started = Instant::now();
    let mut drag_frames = std::time::Duration::ZERO;
    for i in 1..=10 {
        view.drag(Point::new(at.x + i as f32 * 15.0, at.y + i as f32 * 7.0));
        drag_frames += frame(&mut view, &mut text).0;
    }
    let dragging = started.elapsed();
    view.release();
    assert_ne!(geometry(&view, &id), before, "the shape was dragged");
    // One shape chosen (the panel shows its values): a frame.
    view.selection = vec!["v1999".into()];
    frame(&mut view, &mut text);
    let one_frame = frame(&mut view, &mut text).0;
    // Everything chosen: a frame, a mouse move.
    view.select_all();
    let all_frame = frame(&mut view, &mut text).0;
    let started = Instant::now();
    for i in 0..20 {
        view.hover(Point::new(400.0 + i as f32 * 3.0, 300.0));
        view.cursor_at(Point::new(400.0 + i as f32 * 3.0, 300.0));
    }
    let all_hover = started.elapsed() / 20;
    let tiles_bytes = view.tiles.bytes();
    let drawing = view.drawing.as_ref().map_or(0, |(d, _)| d.items.len());
    eprintln!(
        "MEASURE read {read:?}; frame 1 {first:?}; tiles in view made in {tiles_wait:?}; frame 2 {second:?}; zoom step at fit {fit_zoom:?}; at 100% {zoomed:?} (its tiles {zoomed_tiles:?}) then {again:?}; \
         pan {panned:?}; zoom step {zoom_step:?}; pinch {pinch:?}/frame; hover {hover:?}/move; 10 drag steps {dragging:?} \
         (their frames {drag_frames:?}); tiles {tiles_bytes} B; {drawing} items; one chosen: frame {one_frame:?}; all chosen: frame {all_frame:?}, move {all_hover:?}"
    );
}

/// A zoom lays texts out again only when it crosses a step: within one, the same layout;
/// back to a step seen, its layout kept; never more than a few steps kept.
#[test]
fn a_zoom_lays_texts_out_by_steps() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let laid = |v: &DiagramView| {
        let (step, texts) = &v.laid.1[0];
        let first = texts.iter().flatten().next().map(Arc::clone).expect("a text laid out");
        (*step, first)
    };
    let center = view.sheet().center();
    let (step, before) = laid(&view);
    view.zoom(1.01, center);
    paint(&mut view, &mut text);
    let (same, again) = laid(&view);
    assert_eq!(same, step);
    assert!(Arc::ptr_eq(&before, &again), "within a step: not laid out again");
    view.zoom(1.5, center);
    paint(&mut view, &mut text);
    assert_ne!(laid(&view).0, step, "another step");
    view.zoom(1.0 / 1.5, center);
    paint(&mut view, &mut text);
    let (back, kept) = laid(&view);
    assert_eq!(back, step);
    assert!(Arc::ptr_eq(&before, &kept), "a step seen: its layout kept");
    for factor in [1.2, 1.2, 1.2, 1.2, 1.2] {
        view.zoom(factor, center);
        paint(&mut view, &mut text);
    }
    assert!(view.laid.1.len() <= LAID_STEPS);
}

/// How many cells the first page holds.
fn cell_count(view: &DiagramView) -> usize {
    view.diagram.as_ref().unwrap().doc.pages[0].cells.len()
}

/// A click on a shape's arrow (outside its side) makes a shape like it beside it, joined to
/// it and chosen — one step, undone at once.
#[test]
fn a_shape_s_arrow_makes_a_joined_shape_beside_it() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let before = cell_count(&view);
    // Over the start shape (340, 40, 120 × 60): its right arrow, just out of its side.
    view.hover(screen(&view, 400.0, 70.0));
    paint(&mut view, &mut text);
    let arrow = screen(&view, 475.0, 70.0);
    assert!(view.hover(arrow) || view.hovered.as_deref() == Some("start"), "still its shape");
    view.pointer_down(arrow, 1, PressMods::default());
    view.release();
    let made = view.selection[0].clone();
    assert_eq!(geometry(&view, &made), (500.0, 40.0, 120.0, 60.0), "beside it, as large");
    assert_eq!(
        cell(&view, &made).style.get("ellipse").is_some(),
        cell(&view, "start").style.get("ellipse").is_some()
    );
    let page = &view.diagram.as_ref().unwrap().doc.pages[0];
    assert!(
        page.cells
            .iter()
            .any(|c| c.source.as_deref() == Some("start")
                && c.target.as_deref() == Some(made.as_str())),
        "joined to it"
    );
    assert_eq!(cell_count(&view), before + 2);
    assert!(view.command("undo"));
    assert_eq!(cell_count(&view), before, "one step");
}

/// An arrow dragged onto another shape joins them; dragged onto the sheet, a shape like it
/// is made there, joined.
#[test]
fn a_shape_s_arrow_dragged_joins_or_makes_a_shape() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let before = cell_count(&view);
    view.hover(screen(&view, 400.0, 70.0));
    drag(&mut view, (475.0, 70.0), (630.0, 200.0), PressMods::default());
    let edge = cell(&view, &view.selection[0].clone());
    assert_eq!((edge.source.as_deref(), edge.target.as_deref()), (Some("start"), Some("save")));
    assert_eq!(cell_count(&view), before + 1, "only the edge");
    paint(&mut view, &mut text);
    view.hover(screen(&view, 400.0, 70.0));
    drag(&mut view, (475.0, 70.0), (480.0, 320.0), PressMods::default());
    let made = view.selection[0].clone();
    assert_eq!(geometry(&view, &made), (420.0, 290.0, 120.0, 60.0), "centered where let go");
    assert_eq!(cell_count(&view), before + 3);
}

/// A press in the band just inside a shape's outline draws an edge (no connection point to
/// find); inside it, the shape moves as before.
#[test]
fn an_edge_is_drawn_from_a_shape_s_outline() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    view.hover(screen(&view, 400.0, 70.0));
    drag(&mut view, (342.0, 48.0), (630.0, 200.0), PressMods::default());
    let edge = cell(&view, &view.selection[0].clone());
    assert_eq!((edge.source.as_deref(), edge.target.as_deref()), (Some("start"), Some("save")));
    paint(&mut view, &mut text);
    view.hover(screen(&view, 400.0, 70.0));
    let before = geometry(&view, "start");
    drag(&mut view, (380.0, 60.0), (420.0, 60.0), PressMods::default());
    assert_ne!(geometry(&view, "start"), before, "moved from inside");
}

/// A double click where nothing is on the sheet opens a text's field there, its placeholder
/// chosen: what's typed makes the text — one step, undone at once; nothing typed, nothing
/// made (no invisible cell left).
#[test]
fn a_double_click_on_the_sheet_makes_a_text() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let before = cell_count(&view);
    let at = screen(&view, 480.0, 320.0);
    view.pointer_down(at, 2, PressMods::default());
    view.release();
    assert!(view.editing.is_some(), "its field open");
    assert_eq!(cell_count(&view), before, "made once something's typed");
    assert!(view.text_input("Hello"));
    assert!(view.edit_key_with(ui_components::ViewerKey::Escape, Default::default()));
    let id = view.selection[0].clone();
    assert_eq!(cell(&view, &id).value, "Hello");
    assert_eq!(geometry(&view, &id), (450.0, 310.0, 60.0, 30.0), "centered there, on the grid");
    assert_eq!(cell_count(&view), before + 1);
    assert!(view.command("undo"));
    assert_eq!(cell_count(&view), before, "one step");
    // Emptied, then left: nothing made.
    view.pointer_down(at, 2, PressMods::default());
    view.release();
    assert!(view.edit_key_with(ui_components::ViewerKey::Backspace, Default::default()));
    assert!(view.edit_key_with(ui_components::ViewerKey::Escape, Default::default()));
    assert_eq!(cell_count(&view), before);
}

/// A click on a shape's outline (no drag) chooses it: no edge of no length made.
#[test]
fn a_click_on_the_outline_chooses_the_shape() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let before = cell_count(&view);
    view.hover(screen(&view, 400.0, 70.0));
    let p = screen(&view, 342.0, 48.0);
    view.pointer_down(p, 1, PressMods::default());
    view.release();
    assert_eq!(cell_count(&view), before);
    assert_eq!(view.selection, ["start"]);
}

/// A double click on a shape's arrow makes one shape beside it, not a text as well.
#[test]
fn a_double_click_on_an_arrow_makes_one_shape() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    let before = cell_count(&view);
    view.hover(screen(&view, 400.0, 70.0));
    paint(&mut view, &mut text);
    let arrow = screen(&view, 475.0, 70.0);
    view.pointer_down(arrow, 1, PressMods::default());
    view.release();
    view.hover(screen(&view, 400.0, 70.0));
    view.pointer_down(arrow, 2, PressMods::default());
    view.release();
    assert!(view.editing.is_none(), "no text");
    assert_eq!(cell_count(&view), before + 2, "a shape and its edge");
}

/// A chosen shape's side handle is the handle's, not a connection point's: dragged, it
/// resizes.
#[test]
fn a_side_handle_resizes_rather_than_connects() {
    let mut text = TextSystem::new();
    let (mut view, _f) = editing(&mut text);
    view.selection = vec!["start".into()];
    paint(&mut view, &mut text);
    view.hover(screen(&view, 400.0, 70.0));
    let before = cell_count(&view);
    drag(&mut view, (460.0, 70.0), (500.0, 70.0), PressMods::default());
    assert_eq!(cell_count(&view), before, "no edge");
    assert_eq!(geometry(&view, "start").2, 160.0, "wider");
}
