use super::*;
use std::path::PathBuf;

/// A host with the windows' projects, recording which one was brought to the front.
#[derive(Default)]
struct Host {
    projects: Vec<ProjectWindow>,
    focused: Option<u64>,
    numbered: Option<u8>,
    /// The list asked for, and what's done with a row.
    asked: Option<(workbench_model::ChoiceList, workbench_model::OnChoice)>,
    events: kernel::EventBus,
}

impl WorkbenchHost for Host {
    fn project_root(&self) -> Option<PathBuf> {
        None
    }
    fn active_file(&self) -> Option<workbench_model::ActiveFile> {
        None
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }
    fn project_storage(&self) -> Option<PathBuf> {
        None
    }
    fn relist(&mut self, _: &std::path::Path) {}
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn ask_running(&mut self, _: kernel::ProjectId, _: &str, _: workbench_model::OnYes) -> bool {
        false
    }
    fn choose(
        &mut self,
        list: workbench_model::ChoiceList,
        then: workbench_model::OnChoice,
    ) -> bool {
        self.asked = Some((list, then));
        true
    }
    fn spawn(&mut self, _: workbench_model::Job) {}
    fn notify(&mut self, _: workbench_model::Notification) {}
}

impl workbench_model::ViewHost for Host {
    fn open_view(&mut self, _: &str, _: &str, _: &dyn std::any::Any) -> bool {
        false
    }
}
impl workbench_model::EditorHost for Host {}
impl workbench_model::RefactoringHost for Host {}
impl workbench_model::ServicesHost for Host {}
impl workbench_model::SystemHost for Host {
    fn open_projects(&self) -> Vec<ProjectWindow> {
        self.projects.clone()
    }
    fn focus_project(&mut self, window: u64) -> bool {
        self.focused = Some(window);
        true
    }
    fn set_project_number(&mut self, number: u8) -> bool {
        self.numbered = Some(number);
        true
    }
}

fn project(window: u64, number: u8, current: bool) -> ProjectWindow {
    ProjectWindow {
        window,
        name: format!("project {number}"),
        path: PathBuf::from(format!("/p{number}")),
        number: Some(number),
        current,
    }
}

/// ⌥⌘N brings project N's window to the front; its own, or a number none has: nothing.
#[test]
fn a_projects_shortcut_brings_its_window_to_the_front() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(SwitcherModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let commands = contributions.take::<CommandContribution>();
    assert_eq!(commands.len(), 11, "nine projects, the list, the number");
    assert_eq!(commands[9].spec.id, SWITCHER);
    assert_eq!(commands[9].spec.shortcuts, ["CmdOrCtrl+Alt+P"]);
    assert_eq!(commands[10].spec.id, NUMBER);
    assert_eq!(commands[0].spec.shortcuts, ["CmdOrCtrl+Alt+1"]);
    let go = |n: usize| &commands[n - 1].handler;
    let mut host =
        Host { projects: vec![project(10, 1, true), project(20, 2, false)], ..Host::default() };
    assert!(!go(1).enabled(&host), "this window's own");
    assert!(go(2).enabled(&host) && go(2).run(&mut host));
    assert_eq!(host.focused, Some(20));
    assert!(!go(3).enabled(&host) && !go(3).run(&mut host), "no project 3");
    assert_eq!(contributions.take::<Arc<dyn TopBarBadge>>().len(), 1);
}

/// Its number is the user's to choose: every number listed (whose it is, or free), the one
/// picked asked of the app (it swaps there); the badge's click is that, its right click
/// the menu its button shares.
#[test]
fn a_projects_number_is_chosen_from_the_badge_and_its_menu() {
    let mut registry = extensions::ModuleRegistry::default();
    registry.register(Box::new(SwitcherModule));
    let (mut contributions, _) = registry.activate(&Default::default());
    let commands = contributions.take::<CommandContribution>();
    let number = commands.iter().find(|c| c.spec.id == NUMBER).unwrap();
    let mut host =
        Host { projects: vec![project(10, 1, true), project(20, 2, false)], ..Host::default() };
    assert!(number.handler.run(&mut host));
    let (list, then) = host.asked.take().expect("the numbers listed");
    assert_eq!(list.items.len(), 9);
    assert_eq!(list.items[1].detail, "project 2", "whose it is");
    then(4, &mut host);
    assert_eq!(host.numbered, Some(5));
    let badges = contributions.take::<Arc<dyn TopBarBadge>>();
    let shown = badges[0].badge(&project(10, 1, true));
    // No keymap in the tests: no shortcut to show, so no badge.
    assert!(
        shown.is_none() || shown.is_some_and(|b| b.command == Some(NUMBER) && b.menu == Some(TOOL))
    );
    let menus = contributions.take::<MenuContribution>();
    let menu = menus.iter().find(|m| m.id == TOOL).expect("its menu");
    let rows = menu.rows_in(MenuPlace::ToolStrip, &|_| true);
    assert_eq!(rows.len(), 2);
    let tools = contributions.take::<Arc<dyn ToolWindowProvider>>();
    assert_eq!(tools[0].id(), TOOL);
    assert!(tools[0].strip_end() && tools[0].order() < 3, "above Keep awake's");
}
