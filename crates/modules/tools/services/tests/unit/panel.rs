//! The panel with made-up contributors: nothing asked before it's shown; the tree and a
//! node's tabs; a tab's view made when shown and let go with its node; pinning and closing;
//! the keys on the rows; the filter; two projects never seeing each other's nodes; what it
//! keeps for the next session; a contributor's reveal.

use std::sync::atomic::{AtomicUsize, Ordering};

use ui_components::{PanelMemento, ServiceNode, ViewMenuEntry, ViewerAction, ViewerPaint};

use crate::sample::{self, SampleProvider};

use super::*;

fn panel() -> ServicesPanel {
    let mut panel = ServicesPanel::with_providers(vec![Arc::new(SampleProvider)]);
    panel.set_project(Some(ProjectId::new()), EventBus::new());
    panel
}

fn labels(panel: &mut ServicesPanel) -> Vec<String> {
    panel.rows().iter().map(|r| r.label.clone()).collect()
}

fn console() -> Key {
    Key::Node(sample::ID, sample::CONSOLE)
}

/// Counts how often it's asked for a project's services.
#[derive(Debug, Default)]
struct Counting(AtomicUsize);

impl ServiceProvider for Counting {
    fn id(&self) -> &'static str {
        "counting"
    }

    fn icon(&self) -> &'static str {
        "database"
    }

    fn title(&self) -> String {
        "Counting".into()
    }

    fn open(&self, _ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        None
    }
}

#[test]
fn nothing_is_asked_before_it_s_shown_and_once_per_project() {
    let counting = Arc::new(Counting::default());
    let mut panel = ServicesPanel::with_providers(vec![Arc::clone(&counting) as _]);
    panel.set_project(Some(ProjectId::new()), EventBus::new());
    assert_eq!(counting.0.load(Ordering::SeqCst), 0, "not before it's drawn");
    panel.refresh();
    panel.refresh();
    assert_eq!(counting.0.load(Ordering::SeqCst), 1);
    assert!(panel.rows().is_empty(), "nothing given: no group");
    // Another project: asked again; the same one given again: not.
    let other = Some(ProjectId::new());
    panel.set_project(other, EventBus::new());
    panel.set_project(other, EventBus::new());
    panel.refresh();
    assert_eq!(counting.0.load(Ordering::SeqCst), 2);
}

#[test]
fn the_tree_and_a_node_s_tabs_their_views_made_when_shown() {
    let mut panel = panel();
    assert_eq!(
        labels(&mut panel),
        ["Baze de date", "shop.sqlite", "console", "console_1", "analytics@localhost", "reports"]
            .map(|s| if s == "Baze de date" {
                i18n::t("services.sample_group")
            } else {
                s.to_owned()
            })
    );
    // The first node chosen at once (it has no tabs: nothing to show).
    assert_eq!(panel.selected(), Some(Key::Node(sample::ID, sample::SHOP)));
    assert!(panel.tabs().is_empty());
    panel.select(console(), None);
    let titles: Vec<String> = panel.tabs().iter().map(|t| t.title.clone()).collect();
    assert_eq!(titles, ["Output", "main.customers", "Result 2"]);
    assert_eq!(panel.active_tab(), Some(sample::OUTPUT), "its first tab");
    assert_eq!(panel.views_made(), 0, "no view before it's shown");
    assert!(panel.active_view().is_some());
    panel.show_tab(2);
    assert!(panel.active_view().is_some());
    assert_eq!(panel.views_made(), 2);
    // Another node: the views let go (the contributor keeps what they show).
    panel.select(Key::Node(sample::ID, sample::REPORTS), None);
    assert_eq!(panel.views_made(), 0);
    // Back: the tab it had, made again.
    panel.select(console(), None);
    assert_eq!(panel.active_tab(), Some(sample::RESULT_2));
    assert!(panel.active_view().is_some());
}

#[test]
fn tabs_pin_and_close_but_the_output_stays() {
    let mut panel = panel();
    panel.select(console(), Some(sample::RESULT_2));
    let pinned = |panel: &mut ServicesPanel| -> Vec<(String, bool)> {
        panel.tabs().iter().map(|t| (t.title.clone(), t.pinned)).collect()
    };
    panel.toggle_pin(2);
    assert_eq!(pinned(&mut panel)[2], ("Result 2".into(), true));
    panel.toggle_pin(2);
    panel.close_tab(0);
    assert_eq!(panel.tabs().len(), 3, "the Output can't be closed");
    // Closing the others keeps the pinned one and the Output.
    panel.close_other_tabs(0);
    assert_eq!(pinned(&mut panel), [("Output".into(), false), ("main.customers".into(), true)]);
    // A view asking to close: its tab.
    panel.select(console(), Some(sample::CUSTOMERS));
    panel.toggle_pin(1);
    assert!(panel.active_view().is_some());
    assert_eq!(panel_view_close(&mut panel), None);
    assert_eq!(panel.tabs().len(), 1);
    assert_eq!(panel.active_tab(), Some(sample::OUTPUT), "the next one shown");
}

fn panel_view_close(panel: &mut ServicesPanel) -> Option<ToolPanelAction> {
    panel.view_action_for_tests(Some(ViewerAction::Close))
}

#[test]
fn the_rows_keys_and_the_filter() {
    let mut panel = panel();
    panel.refresh();
    assert!(panel.edit_key(ViewerKey::Down));
    assert_eq!(panel.selected(), Some(console()));
    // ← on a leaf: its parent; ← on it: folded; → opens it again.
    assert!(panel.edit_key(ViewerKey::Left));
    assert_eq!(panel.selected(), Some(Key::Node(sample::ID, sample::SHOP)));
    assert!(panel.edit_key(ViewerKey::Left));
    assert_eq!(labels(&mut panel).len(), 4, "its consoles folded");
    assert!(panel.edit_key(ViewerKey::Right));
    assert_eq!(labels(&mut panel).len(), 6);
    assert!(panel.edit_key(ViewerKey::End));
    assert_eq!(panel.selected(), Some(Key::Node(sample::ID, sample::REPORTS)));
    // ↵ on a node with children folds / opens it (the sample activates nothing).
    assert!(panel.edit_key(ViewerKey::Home));
    assert!(panel.edit_key(ViewerKey::Enter));
    assert_eq!(labels(&mut panel), [i18n::t("services.sample_group")]);
    assert!(panel.tree_command("expand"));
    // Typing on the rows filters them; Esc clears it.
    assert!(panel.text_on_rows("repo"));
    assert_eq!(labels(&mut panel)[1..], ["analytics@localhost".to_owned(), "reports".to_owned()]);
    assert!(panel.edit_key(ViewerKey::Escape));
    assert_eq!(labels(&mut panel).len(), 6);
    panel.type_filter("nothing like it");
    assert!(labels(&mut panel).is_empty());
    // Collapse all from its header.
    panel.edit_key(ViewerKey::Escape);
    panel.header_action("collapse");
    assert_eq!(labels(&mut panel).len(), 1);
}

/// The filter is the app's search field: pressed, it has the keys; ⌘A selects it (⌘C copies
/// it), ⌘X cuts it (every row again), ⌘Z brings it back; its × empties it and lists every
/// row; a press in a node's view takes the keys (the filter's caret goes).
#[test]
fn the_filter_edits_and_empties_as_the_app_s_field() {
    let mut panel = panel();
    panel.refresh();
    panel.field = Rect::new(0.0, 0.0, 280.0, 30.0);
    let paint = |panel: &mut ServicesPanel| {
        let palette = ui_components::palette::Palette::dark();
        let ctx = ui_components::ds::Ctx::new(&palette, 1.0);
        let mut layer = render_api::Layer::default();
        let mut text = text_layout::TextSystem::new();
        let icons = icons::Icons::builtin();
        let (field, typing) = (panel.field, panel.typing);
        panel.filter.paint(&mut layer, &mut text, &icons, &ctx, (field, typing), "");
    };
    paint(&mut panel);
    panel.press(Point::new(40.0, 15.0), 1);
    assert!(panel.typing && panel.wants_text(), "pressed: the filter's");
    assert!(panel.text_input("repo"));
    assert_eq!(labels(&mut panel).len(), 3);
    assert!(panel.command("select_all"));
    assert_eq!(panel.selected_text().as_deref(), Some("repo"));
    assert!(panel.command("cut"));
    assert_eq!(labels(&mut panel).len(), 6, "⌘X: every row");
    assert!(panel.command("undo"));
    assert_eq!(labels(&mut panel).len(), 3, "⌘Z: filtered again");
    // A line break typed or pasted: a space (the shared field's one rule).
    assert!(panel.command("select_all"));
    assert!(panel.text_input("re\npo"));
    assert_eq!(panel.filter.text(), "re po");
    assert!(panel.command("undo"));
    assert_eq!(panel.filter.text(), "repo");
    // × sits at the field's right end (the shared field's room for it).
    paint(&mut panel);
    let clear = Point::new(280.0 - 17.0, 15.0);
    assert_eq!(panel.cursor_at(clear), Some(ViewerCursor::Pointer));
    panel.press(clear, 1);
    assert!(panel.filter.text().is_empty());
    assert_eq!(labels(&mut panel).len(), 6, "× empties it: every row");
    // Pressed in a node's view, the keys are the view's.
    panel.view_area = Rect::new(300.0, 0.0, 400.0, 400.0);
    panel.press(Point::new(400.0, 100.0), 1);
    assert!(!panel.typing, "one field with the keys");
}

/// One node named after its project.
#[derive(Debug)]
struct PerProject;

struct Named(Vec<ServiceNode>);

impl ProjectServices for Named {
    fn nodes(&self) -> &[ServiceNode] {
        &self.0
    }

    fn generation(&self) -> u64 {
        1
    }

    fn tabs(&self, _node: ServiceId) -> Vec<ServiceTab> {
        Vec::new()
    }

    fn open_tab(&mut self, _tab: ServiceId) -> Option<Box<dyn Viewer>> {
        None
    }
}

impl ServiceProvider for PerProject {
    fn id(&self) -> &'static str {
        "per-project"
    }

    fn icon(&self) -> &'static str {
        "database"
    }

    fn title(&self) -> String {
        "Database".into()
    }

    fn open(&self, ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>> {
        let name = format!("{}", ctx.project?);
        Some(Box::new(Named(vec![ServiceNode::new(ServiceId::new(), "database", name)])))
    }
}

#[test]
fn two_projects_never_see_each_other_s_nodes() {
    let (one, two) = (ProjectId::new(), ProjectId::new());
    let mut a = ServicesPanel::with_providers(vec![Arc::new(PerProject)]);
    let mut b = ServicesPanel::with_providers(vec![Arc::new(PerProject)]);
    a.set_project(Some(one), EventBus::new());
    b.set_project(Some(two), EventBus::new());
    assert_eq!(labels(&mut a), ["Database".to_owned(), one.to_string()]);
    assert_eq!(labels(&mut b), ["Database".to_owned(), two.to_string()]);
    // The window shows another project: the previous one's nodes gone.
    a.set_project(Some(two), EventBus::new());
    assert_eq!(labels(&mut a), ["Database".to_owned(), two.to_string()]);
    // No project: nothing.
    a.set_project(None, EventBus::new());
    assert!(labels(&mut a).is_empty());
}

#[test]
fn its_tree_s_width_is_kept_for_the_next_session() {
    let mut panel = panel();
    assert_eq!(panel.memento().as_deref(), Some("tree=300"));
    panel.set_data(&PanelMemento("tree=420".into()));
    assert_eq!(panel.memento().as_deref(), Some("tree=420"));
    // Too narrow, or not a width: kept within its bounds / ignored.
    panel.set_data(&PanelMemento("tree=12".into()));
    assert_eq!(panel.memento().as_deref(), Some("tree=180"));
    panel.set_data(&PanelMemento("tree=wide".into()));
    assert_eq!(panel.memento().as_deref(), Some("tree=180"));
}

/// One node with one tab, whose view says whether it was given the project's sizes.
#[derive(Debug, Default)]
struct Sized(Arc<std::sync::Mutex<Option<workbench_model::LayoutSizes>>>);

struct SizedServices {
    nodes: Vec<ServiceNode>,
    tab: ServiceId,
    got: Arc<std::sync::Mutex<Option<workbench_model::LayoutSizes>>>,
}

struct SizedView(Arc<std::sync::Mutex<Option<workbench_model::LayoutSizes>>>);

impl Viewer for SizedView {
    fn paint(&mut self, _ctx: ViewerPaint<'_>) {}

    fn set_data(&mut self, data: &dyn std::any::Any) {
        if let Some(sizes) = data.downcast_ref::<workbench_model::LayoutSizes>() {
            *self.0.lock().unwrap() = Some(sizes.clone());
        }
    }
}

impl ProjectServices for SizedServices {
    fn nodes(&self) -> &[ServiceNode] {
        &self.nodes
    }

    fn generation(&self) -> u64 {
        1
    }

    fn tabs(&self, _node: ServiceId) -> Vec<ServiceTab> {
        vec![ServiceTab::new(self.tab, "table", "result")]
    }

    fn open_tab(&mut self, _tab: ServiceId) -> Option<Box<dyn Viewer>> {
        Some(Box::new(SizedView(Arc::clone(&self.got))))
    }
}

const SIZED_NODE: ServiceId = ServiceId(0x5eed);

impl ServiceProvider for Sized {
    fn id(&self) -> &'static str {
        "sized"
    }

    fn icon(&self) -> &'static str {
        "database"
    }

    fn title(&self) -> String {
        "Sized".into()
    }

    fn open(&self, _ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>> {
        Some(Box::new(SizedServices {
            nodes: vec![ServiceNode::new(SIZED_NODE, "database", "node")],
            tab: ServiceId::new(),
            got: Arc::clone(&self.0),
        }))
    }
}

/// The views of its tabs get the project's sizes when they're made (their splitters'
/// places are the project's).
#[test]
fn its_tabs_views_get_the_project_s_sizes() {
    let provider = Arc::new(Sized::default());
    let got = Arc::clone(&provider.0);
    let mut panel = ServicesPanel::with_providers(vec![provider as _]);
    panel.set_project(Some(ProjectId::new()), EventBus::new());
    let sizes = workbench_model::LayoutSizes::new();
    sizes.set("x", 1.0);
    panel.set_data(&sizes);
    panel.refresh();
    panel.select(Key::Node("sized", SIZED_NODE), None);
    panel.refresh();
    assert!(panel.active_view().is_some());
    let given = got.lock().unwrap().clone().expect("given when made");
    assert_eq!(given.get("x"), Some(1.0));
}

#[test]
fn a_contributor_or_the_app_reveals_a_node_and_its_tab() {
    let mut panel = panel();
    panel.refresh();
    panel.collapse_all();
    assert_eq!(labels(&mut panel).len(), 1);
    // Told by the app (`tell_panel`): the node shown, its parents opened, the tab chosen.
    panel.set_data(&Reveal {
        provider: sample::ID,
        node: sample::CONSOLE,
        tab: Some(sample::CUSTOMERS),
    });
    assert_eq!(panel.selected(), Some(console()));
    assert_eq!(panel.active_tab(), Some(sample::CUSTOMERS));
    assert!(labels(&mut panel).contains(&"console".to_owned()));
    // Its button's menu: the window's own commands.
    panel.set_data(&crate::PanelCommand("collapse"));
    assert_eq!(labels(&mut panel).len(), 1);
}

/// `panel` painted at scale 1, its scene given back.
fn painted(panel: &mut ServicesPanel) -> render_api::Scene {
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    panel.paint_panel(ViewerPaint {
        bounds: kernel::Rect::new(0.0, 0.0, 900.0, 400.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    scene
}

#[test]
fn the_splitter_s_handle_is_drawn_over_both_panes() {
    let mut panel = panel();
    // A node without tabs (the right pane's empty state), then one with its tabs and a view.
    let _ = painted(&mut panel);
    assert!(panel.tabs().is_empty());
    let scene = painted(&mut panel);
    assert!(panel.splitter.is_on_top(&scene), "over the empty state");
    panel.select(console(), None);
    assert!(panel.active_view().is_some());
    let scene = painted(&mut panel);
    assert!(panel.splitter.is_on_top(&scene), "over the tabs and the view");
}

/// A tab's view clicked into: its actions' keys first (its kind), its single keys and the
/// commands meant for it reach it; a link to another tab of the node shows that tab.
#[test]
fn a_tab_s_view_gets_its_keys_and_links_to_its_siblings() {
    let mut p = panel();
    p.open();
    p.select(console(), Some(sample::CUSTOMERS));
    assert_eq!(p.keyed_view(), None, "the tree has the keys");
    p.in_view = true;
    assert!(p.active_view().is_some());
    assert_eq!(p.keyed_view(), Some(sample::NOTE_VIEW));
    // A single key typed, and a command for the tab's view.
    assert!(p.text_on_rows("*"));
    p.set_data(&TabCommand("note.mark".into()));
    assert!(p.selected_text().unwrap().ends_with("*!"), "{:?}", p.selected_text());
    // The Output's link to a result's tab: that tab shown.
    assert_eq!(
        p.view_action_for_tests(Some(ViewerAction::OpenLink(sample::RESULT_2.link()))),
        None
    );
    assert_eq!(p.active_tab(), Some(sample::RESULT_2));
    // Another link goes on to the app.
    let other = ViewerAction::OpenLink("https://example.com".into());
    assert_eq!(p.view_action_for_tests(Some(other.clone())), Some(ToolPanelAction::View(other)));
}

/// What the view asked for and was sent: its menu's rows chosen.
type Chosen = Arc<std::sync::Mutex<Vec<String>>>;

/// One node with one tab, whose view asks for a menu under its button when pressed (and on
/// its "more" command), and keeps the commands it's sent.
#[derive(Debug, Default)]
struct Menus(Chosen);

struct MenusServices {
    nodes: Vec<ServiceNode>,
    tab: ServiceId,
    chosen: Chosen,
}

struct MenuView {
    chosen: Chosen,
    asked: Option<(kernel::Rect, Vec<ViewMenuEntry>)>,
}

impl MenuView {
    fn ask(&mut self, at: Point) {
        let button = kernel::Rect::new(at.x, at.y, 20.0, 20.0);
        self.asked = Some((button, vec![ViewMenuEntry::item("export.json", "JSON")]));
    }
}

impl Viewer for MenuView {
    fn paint(&mut self, _ctx: ViewerPaint<'_>) {}

    fn press(&mut self, p: Point, _clicks: u32) -> Option<ViewerAction> {
        self.ask(p);
        None
    }

    fn take_menu(&mut self) -> Option<(kernel::Rect, Vec<ViewMenuEntry>)> {
        self.asked.take()
    }

    fn command(&mut self, id: &str) -> bool {
        if id == "more" {
            self.ask(Point::new(1.0, 2.0));
        } else {
            self.chosen.lock().unwrap().push(id.to_owned());
        }
        true
    }
}

impl ProjectServices for MenusServices {
    fn nodes(&self) -> &[ServiceNode] {
        &self.nodes
    }

    fn generation(&self) -> u64 {
        1
    }

    fn tabs(&self, _node: ServiceId) -> Vec<ServiceTab> {
        vec![ServiceTab::new(self.tab, "table", "profile")]
    }

    fn open_tab(&mut self, _tab: ServiceId) -> Option<Box<dyn Viewer>> {
        Some(Box::new(MenuView { chosen: Arc::clone(&self.chosen), asked: None }))
    }
}

const MENUS_NODE: ServiceId = ServiceId(0x3e2);

impl ServiceProvider for Menus {
    fn id(&self) -> &'static str {
        "menus"
    }

    fn icon(&self) -> &'static str {
        "database"
    }

    fn title(&self) -> String {
        "Menus".into()
    }

    fn open(&self, _ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>> {
        Some(Box::new(MenusServices {
            nodes: vec![ServiceNode::new(MENUS_NODE, "database", "node")],
            tab: ServiceId::new(),
            chosen: Arc::clone(&self.0),
        }))
    }
}

/// A menu a tab's view asks for under a button of its own (its "Export ▾") reaches the app
/// as this panel's menu under that button, after a press or a command; the row chosen goes
/// back to the view.
#[test]
fn a_view_s_asked_menu_goes_to_the_app_under_its_button() {
    let provider = Arc::new(Menus::default());
    let chosen = Arc::clone(&provider.0);
    let mut panel = ServicesPanel::with_providers(vec![provider as _]);
    panel.set_project(Some(ProjectId::new()), EventBus::new());
    panel.refresh();
    panel.select(Key::Node("menus", MENUS_NODE), None);
    panel.refresh();
    let _ = painted(&mut panel);
    let at = panel.view_area.center();
    match panel.press(at, 1) {
        Some(ToolPanelAction::MenuUnder { anchor, entries }) => {
            assert_eq!(anchor.origin, at, "under its own button");
            assert_eq!(entries.len(), 1);
        }
        other => panic!("the view's menu, not {other:?}"),
    }
    assert!(panel.menu_command("export.json"), "the row chosen goes to the view");
    assert_eq!(*chosen.lock().unwrap(), ["export.json"]);
    // Asked for by a command (its keys, the palette): the same menu, taken by the app.
    assert!(panel.command("more"));
    assert!(matches!(panel.take_action(), Some(ToolPanelAction::MenuUnder { .. })));
    assert!(panel.take_action().is_none(), "taken once");
}

/// A menu asked for by a command sent to the tab (the app's `TabCommand`: a profile's
/// "export", "compare") is offered then, never left to pop up on a later press.
#[test]
fn a_tab_command_s_asked_menu_is_offered_at_once() {
    let provider = Arc::new(Menus::default());
    let mut panel = ServicesPanel::with_providers(vec![provider as _]);
    panel.set_project(Some(ProjectId::new()), EventBus::new());
    panel.refresh();
    panel.select(Key::Node("menus", MENUS_NODE), None);
    panel.refresh();
    let _ = painted(&mut panel);
    panel.set_data(&TabCommand("more".into()));
    assert!(matches!(panel.take_action(), Some(ToolPanelAction::MenuUnder { .. })));
    assert!(panel.active_view().and_then(|v| v.take_menu()).is_none(), "nothing left asked");
}

/// To a screen reader: each row of its tree is a tree's item, with its name and its depth.
#[test]
fn its_rows_are_a_tree_s_items() {
    use ui_kit::access::Role;
    let mut panel = panel();
    // Its rows made (as the panel asks for them), then told as they're drawn.
    assert!(!labels(&mut panel).is_empty());
    let recording = ui_kit::access::begin();
    let _ = painted(&mut panel);
    let frame = recording.take();
    let items: Vec<_> = frame.nodes().iter().filter(|n| n.role == Role::TreeItem).collect();
    assert!(!items.is_empty(), "its rows told");
    assert!(items.iter().all(|n| !n.name.is_empty() && n.place.level.is_some()));
}

/// How text is shown reaches its views: one made later (the Output's log) wraps as told.
#[test]
fn its_views_are_told_how_text_is_shown() {
    let mut panel = panel();
    panel.set_text_options(&ui_components::TextOptions { word_wrap: true });
    panel.select(console(), None);
    let view = panel.active_view().expect("the Output's view");
    let menu = view.context_menu(kernel::Point::new(10.0, 10.0)).unwrap_or_default();
    let unwrap = i18n::t("log.unwrap");
    let labels: Vec<String> = menu
        .iter()
        .filter_map(|e| match e {
            ui_components::ViewMenuEntry::Item { label, .. } => Some(label.clone()),
            _ => None,
        })
        .collect();
    assert!(labels.contains(&unwrap), "wrapped: {labels:?}");
}
