//! The parts' actions from the view: done by whoever reported the part (through the
//! window's resources, by owner — never by the Task Manager itself), those losing work asked
//! first in the module's modal (`crate::confirm`), each one's outcome a toast; from the
//! detail's buttons, a row's menu and the palette's "Stop".

use super::*;
use crate::model::Item;
use crate::view::Hit;
use crate::view::tests::{Pen, area, center, demo};
use std::sync::{Arc, Mutex};
use ui_components::{Viewer, ViewerAction, ViewerKey};
use workbench_model::{ActOutcome, NotificationKind, PartRef, ResourceSnapshot, ResourceSource};

/// A window's resources that only record what they're asked to do.
struct Recorder {
    asked: Mutex<Vec<(PartRef, String, ActionTarget)>>,
    outcome: Mutex<ActOutcome>,
}

impl ResourceSource for Recorder {
    fn snapshot(&self) -> ResourceSnapshot {
        ResourceSnapshot::default()
    }

    fn act(&self, part: &PartRef, action: &str, target: ActionTarget) -> ActOutcome {
        self.asked.lock().unwrap().push((part.clone(), action.to_owned(), target));
        self.outcome.lock().unwrap().clone()
    }
}

/// A made-up Task Manager whose actions go to a recorder.
fn with_recorder() -> (TaskManager, Arc<Recorder>) {
    let recorder =
        Arc::new(Recorder { asked: Mutex::default(), outcome: Mutex::new(ActOutcome::Done) });
    let mut view = demo("plain");
    let source: Arc<dyn ResourceSource> = recorder.clone();
    view.source = Some(Arc::downgrade(&source));
    // The recorder kept alive by the test (the view holds it weakly, as the window's).
    std::mem::forget(source);
    (view, recorder)
}

fn key_of(view: &TaskManager, name: &str) -> String {
    view.frame.lines.iter().find(|l| l.name == name).map(|l| l.key.clone()).expect("listed")
}

fn asked(recorder: &Recorder) -> Vec<(String, String, ActionTarget)> {
    recorder.asked.lock().unwrap().iter().map(|(p, a, t)| (p.name.clone(), a.clone(), *t)).collect()
}

/// The question the view asked: its modal opened (as `tasks.ask` would) and shown.
fn modal(view: &mut TaskManager, given: Option<ViewerAction>) -> crate::confirm::ConfirmView {
    assert!(
        matches!(given.or_else(|| view.take_action()), Some(ViewerAction::WithHost(_))),
        "the modal asked for"
    );
    let question = view.asking().expect("asked").0.clone();
    let mut modal = crate::confirm::ConfirmView::default();
    modal.set_data(&question);
    modal
}

/// The modal answered with `keys`; the app then tells the view (`tasks.answered`).
fn answer(view: &mut TaskManager, given: Option<ViewerAction>, keys: &[ViewerKey]) {
    let mut m = modal(view, given);
    for key in keys {
        m.edit_key(*key);
    }
    assert!(m.closed(), "answered: closed");
    if matches!(m.take_action(), Some(ViewerAction::WithHost(_))) {
        view.command(crate::confirm::CONFIRMED);
    }
}

fn toasts(view: &mut TaskManager) -> Vec<NotificationKind> {
    view.take_notifications().into_iter().map(|n| n.kind).collect()
}

#[test]
fn an_action_not_losing_work_is_done_at_once_and_said() {
    let (mut view, recorder) = with_recorder();
    let php = key_of(&view, "PHP");
    let release = view.frame.line(&php).unwrap().actions[0];
    assert_eq!(release.kind, ActionKind::Release);
    view.request(&php, release);
    assert!(view.asking().is_none(), "not asked first");
    assert_eq!(asked(&recorder), [("PHP".into(), "release".into(), ActionTarget::Part)]);
    assert_eq!(recorder.asked.lock().unwrap()[0].0.owner, "lang.php", "addressed by its owner");
    assert_eq!(toasts(&mut view), [NotificationKind::Success]);
    // What went wrong is said; what can't be done any more too.
    *recorder.outcome.lock().unwrap() = ActOutcome::Failed("busy".into());
    view.request(&php, release);
    assert_eq!(toasts(&mut view), [NotificationKind::Error]);
    *recorder.outcome.lock().unwrap() = ActOutcome::Unsupported;
    view.request(&php, release);
    assert_eq!(toasts(&mut view), [NotificationKind::Warning]);
    // A part gone: nothing asked, said.
    view.request("nobody/none", release);
    assert_eq!(recorder.asked.lock().unwrap().len(), 3);
    assert_eq!(toasts(&mut view), [NotificationKind::Warning]);
}

#[test]
fn stopping_a_program_is_asked_first_in_a_modal_and_done_only_on_yes() {
    let (mut view, recorder) = with_recorder();
    let mut pen = Pen::new();
    let formatters = key_of(&view, &i18n::t("tasks.quality"));
    view.select(&formatters);
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    // Its program's button in the detail.
    let button =
        area(&view, |h| matches!(h, Hit::Action(a) if a.target == ActionTarget::Process(5200)));
    let action = view.click(center(button.expect("its program's Stop")));
    assert!(
        matches!(action.or_else(|| view.take_action()), Some(ViewerAction::WithHost(_))),
        "asked first, in the module's modal"
    );
    let (question, key, asked_action) = view.asking().expect("asked");
    assert_eq!((key, asked_action.target), (formatters.as_str(), ActionTarget::Process(5200)));
    assert_eq!(question.title, i18n::tr!("tasks.confirm_stop", name = "php-cs-fixer · 5200"));
    assert_eq!(question.icon, "stop");
    assert!(asked(&recorder).is_empty());
    // Esc, or ↵ on "Cancel" (the keys' default): nothing done.
    for keys in [&[ViewerKey::Escape][..], &[ViewerKey::Enter]] {
        let given = view.click(center(button.unwrap()));
        answer(&mut view, given, keys);
        assert!(asked(&recorder).is_empty(), "{keys:?}");
    }
    // Tab to the action, ↵: done by its owner, said.
    let given = view.click(center(button.unwrap()));
    answer(&mut view, given, &[ViewerKey::Tab, ViewerKey::Enter]);
    assert_eq!(
        asked(&recorder),
        [(i18n::t("tasks.quality"), "stop-program".into(), ActionTarget::Process(5200))]
    );
    assert_eq!(toasts(&mut view), [NotificationKind::Success]);
    // A "yes" is done once, and only by the view that asked (its next poll would too).
    assert!(!view.command(crate::confirm::CONFIRMED));
    let (mut other, other_recorder) = with_recorder();
    let given = view.click(center(button.unwrap()));
    answer(&mut view, given, &[ViewerKey::Right, ViewerKey::Enter]);
    assert!(!other.command(crate::confirm::CONFIRMED));
    assert!(asked(&other_recorder).is_empty());
    assert_eq!(asked(&recorder).len(), 2);
}

#[test]
fn an_operation_stops_from_the_detail_without_asking() {
    let (mut view, recorder) = with_recorder();
    let mut pen = Pen::new();
    let remote = key_of(&view, &i18n::tr!("tasks.remote", name = "shop"));
    view.select(&remote);
    pen.paint(&mut view, 1300.0, 1400.0, 1.0);
    let stop = area(
        &view,
        |h| matches!(h, Hit::Action(a) if matches!(a.target, ActionTarget::Activity(_))),
    );
    view.click(center(stop.expect("the upload's Stop")));
    assert!(view.asking().is_none(), "an operation can be run again: not asked");
    assert_eq!(asked(&recorder)[0].1, workbench_model::resources::STOP_OPERATION);
    // The part's own buttons: the idle sessions closed at once.
    let sessions = key_of(&view, &i18n::t("tasks.database"));
    view.select(&sessions);
    pen.paint(&mut view, 1300.0, 900.0, 1.0);
    let close = area(&view, |h| matches!(h, Hit::Action(a) if a.target == ActionTarget::Part));
    view.click(center(close.expect("its button")));
    assert_eq!(asked(&recorder)[1].1, "close-idle");
}

#[test]
fn a_rows_menu_and_the_palette_offer_its_actions() {
    let (mut view, recorder) = with_recorder();
    let mut pen = Pen::new();
    pen.paint(&mut view, 1300.0, 1400.0, 1.0);
    let terminal = i18n::t("tasks.terminal");
    let i = view
        .model()
        .items
        .iter()
        .position(
            |i| matches!(i, Item::Part { index, .. } if view.frame.lines[*index].name == terminal),
        )
        .unwrap();
    let row = area(&view, |h| *h == Hit::Row(i)).unwrap();
    let entries = view.context_menu(center(row)).unwrap();
    let ids: Vec<String> = entries
        .iter()
        .filter_map(|e| match e {
            ui_components::ViewMenuEntry::Item { id, .. } => Some(id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(ids[0], "act:close-session:p4242", "its actions first: {ids:?}");
    assert!(view.command(&ids[0]));
    assert!(view.asking().is_some(), "closing a session is asked first");
    answer(&mut view, None, &[ViewerKey::Escape]);
    // The palette's "Stop" on the keys' row: the same, asked.
    assert!(view.command("stop"));
    assert_eq!(view.asking().map(|(_, _, a)| a.target), Some(ActionTarget::Process(4242)));
    answer(&mut view, None, &[ViewerKey::Tab, ViewerKey::Enter]);
    assert_eq!(asked(&recorder), [(terminal, "close-session".into(), ActionTarget::Process(4242))]);
    // A row offering nothing: "Stop" does nothing.
    let model = view.model();
    let interface = model
        .items
        .iter()
        .position(|i| matches!(i, Item::Part { index, .. } if model.frame.lines[*index].name == i18n::t("tasks.ui")))
        .unwrap();
    view.go_to(interface);
    assert!(!view.command("stop"));
    // An action gone since the menu was shown: said, nothing asked.
    assert!(view.command("act:stop-program:p1"));
    assert_eq!(toasts(&mut view).last(), Some(&NotificationKind::Warning));
    assert_eq!(recorder.asked.lock().unwrap().len(), 1);
}

#[test]
fn actions_are_written_into_menu_ids_and_read_back() {
    for target in [ActionTarget::Part, ActionTarget::Process(4242), ActionTarget::Activity(0x7a51)]
    {
        let action = PartAction::new("stop-x", ActionKind::Stop, target);
        assert_eq!(decode(&encode(&action)), Some(("stop-x", target)));
    }
    assert_eq!(decode("act:x:q1"), None);
    assert_eq!(decode("copy-row"), None);
}
