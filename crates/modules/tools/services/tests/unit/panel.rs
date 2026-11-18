//! The panel with made-up contributors: nothing asked before it's shown; the tree and a
//! node's tabs; a tab's view made when shown and let go with its node; pinning and closing;
//! the keys on the rows; the filter; two projects never seeing each other's nodes; what it
//! keeps for the next session; a contributor's reveal.

use std::sync::atomic::{AtomicUsize, Ordering};

use ui_components::ServiceNode;

use super::*;
use crate::sample::{self, SampleProvider};

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
