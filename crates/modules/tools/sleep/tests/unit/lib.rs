use super::*;
use ui_components::sleep::{SleepKind, SleepPart, SleepTexts, SleepWhy};

static TEXTS: SleepTexts = SleepTexts {
    asleep_title: "t",
    resuming: "t",
    by_hand: "t",
    idle: "t",
    freed: "t",
    wake_up: "t",
    how: "t",
    sleep_now: "t",
    going_to_sleep: "t",
    waking_up: "t",
    cant: "t",
    cant_sleeping: "t",
    cant_waking: "t",
};
static KIND: SleepKind = SleepKind {
    id: "test-timer-view",
    viewer: "test.viewer",
    hint: Some("test.hint"),
    texts: &TEXTS,
};

/// A host recording which tab it was asked to show (and whether it can), and its hints.
struct Shows {
    events: kernel::EventBus,
    shown: Vec<(Option<String>, String)>,
    hints: Vec<(String, String)>,
    can: bool,
    project: Option<kernel::ProjectId>,
}

impl Shows {
    fn new() -> Self {
        Self {
            events: kernel::EventBus::new(),
            shown: Vec::new(),
            hints: Vec::new(),
            can: true,
            project: None,
        }
    }
}

impl WorkbenchHost for Shows {
    fn project_root(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn project_id(&self) -> Option<kernel::ProjectId> {
        self.project
    }
    fn active_file(&self) -> Option<workbench_model::ActiveFile> {
        None
    }
    fn repository(&self) -> Option<Arc<dyn version_control::Repository>> {
        None
    }
    fn project_storage(&self) -> Option<std::path::PathBuf> {
        None
    }
    fn events(&self) -> &kernel::EventBus {
        &self.events
    }
    fn choose(&mut self, _: workbench_model::ChoiceList, _: workbench_model::OnChoice) -> bool {
        false
    }
    fn spawn(&mut self, _: workbench_model::Job) {}
    fn notify(&mut self, _: workbench_model::Notification) {}
}

impl workbench_model::ViewHost for Shows {
    fn open_view(&mut self, title: &str, _: &str, _: &dyn std::any::Any) -> bool {
        self.shown.push((None, title.to_owned()));
        self.can
    }
    fn show_in_panel(&mut self, tool: &str, title: &str, _: &str, _: &dyn std::any::Any) -> bool {
        self.shown.push((Some(tool.to_owned()), title.to_owned()));
        self.can
    }
    fn show_hint_by(&mut self, id: &str, anchor: workbench_model::HintAnchor) -> bool {
        let workbench_model::HintAnchor::Tab(tab) = anchor;
        self.hints.push((id.to_owned(), tab.title));
        true
    }
}
impl workbench_model::EditorHost for Shows {}
impl workbench_model::RefactoringHost for Shows {}
impl workbench_model::ServicesHost for Shows {}
impl workbench_model::SystemHost for Shows {}

/// A started view of [`KIND`] known by `key`, its tab at `place`.
fn view(key: &str, place: SleepPlace, project: Option<kernel::ProjectId>) -> SleepPart {
    let part = SleepPart::new(&KIND, key.into(), project, place);
    part.started(None);
    part
}

fn editor(name: &str) -> SleepPlace {
    SleepPlace::Editor { name: name.into(), extension: "view-test" }
}

/// Woken from elsewhere, a sleeping view's tab is shown where it is — a tool window's, or the
/// editor's — so it's drawn and comes back; one that can't be shown, or isn't asleep,
/// answers no and is left as it was.
#[test]
fn a_woken_view_s_tab_is_shown_where_it_is() {
    let asleep = |key: &str, place| {
        let mut part = view(key, place, None);
        part.fell_asleep(SleepWhy::ByHand);
        part
    };
    let mut host = Shows::new();
    let place =
        SleepPlace::Panel { tool: "tool", name: "wake-bottom".into(), extension: "view-test" };
    let bottom = asleep("wake bottom", place);
    assert!(wake_shown(bottom.slot(), &mut host));
    assert_eq!(host.shown, [(Some("tool".to_owned()), "wake-bottom".to_owned())]);
    assert!(bottom.take_wake(), "shown: woken");
    let editor_view = asleep("wake-editor", editor("wake-editor"));
    assert!(wake_shown(editor_view.slot(), &mut host));
    assert_eq!(host.shown[1], (None, "wake-editor".to_owned()));
    let hidden = asleep("wake-nowhere", editor("wake-nowhere"));
    host.can = false;
    assert!(!wake_shown(hidden.slot(), &mut host), "its tab can't be shown");
    assert!(!hidden.take_wake(), "not shown: left asleep as it was");
    let awake = view("wake-awake", editor("wake-awake"), None);
    assert!(!wake_shown(awake.slot(), &mut host), "not asleep");
}

/// A view opened in the editor while its kind's sleeping is off: its hint asked once, at
/// the window's poll, for its own project's window only; never for a tool window's tab.
#[test]
fn a_kind_s_hint_is_asked_once_for_a_view_opened_while_it_s_off() {
    let project = Some(kernel::ProjectId::new());
    let _view = view("hint-key", editor("hint-tab"), project);
    let _bottom = view(
        "hint-bottom",
        SleepPlace::Panel { tool: "tool", name: "x".into(), extension: "view-test" },
        project,
    );
    let mut other = Shows { project: Some(kernel::ProjectId::new()), ..Shows::new() };
    workbench_model::ProjectService::poll(&Sleeper, &mut other);
    assert!(!other.hints.iter().any(|(_, tab)| tab == "hint-tab"), "another project's window");
    let mut host = Shows { project, ..Shows::new() };
    workbench_model::ProjectService::poll(&Sleeper, &mut host);
    let ours: Vec<_> = host.hints.iter().filter(|(_, tab)| tab == "hint-tab").collect();
    assert_eq!(ours, [&("test.hint".to_owned(), "hint-tab".to_owned())]);
    workbench_model::ProjectService::poll(&Sleeper, &mut host);
    assert_eq!(host.hints.iter().filter(|(_, tab)| tab == "hint-tab").count(), 1, "once");
}
