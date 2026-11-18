// The tests make their own instants.
#![allow(clippy::disallowed_methods)]

use super::*;
use workbench_model::resources::ActivityKind;
use workbench_model::running::{StopVia, WorkProgress, grouped};
use workbench_model::{ActionTarget, PartRef};

fn upload(project: kernel::ProjectId) -> RunningWork {
    let mut work = RunningWork::new(
        WorkKind::Operation(ActivityKind::Upload),
        "index.php",
        "remote.projects",
        Some(project),
        StopImpact::LosesWork,
    );
    work.progress =
        Some(WorkProgress { done: 1, total: Some(4), unit: workbench_model::ActivityUnit::Bytes });
    work.stop = Some(StopVia {
        part: PartRef { owner: "remote.projects".into(), name: "Remote".into(), project: None },
        action: "stop-operation",
        target: ActionTarget::Activity(1),
    });
    work
}

fn prompt(count: usize) -> RunningPrompt {
    let project = kernel::ProjectId::new();
    RunningPrompt {
        closing: Closing::Window,
        groups: grouped((0..count).map(|_| upload(project)).collect()),
        projects: vec![(project, "shop".into())],
        modules: vec![("remote.projects".into(), "Remote projects".into())],
        waiting: false,
    }
}

fn opened(count: usize) -> RunningView {
    let mut view = RunningView::default();
    view.set_data(&prompt(count));
    view
}

fn told(view: &mut RunningView) -> Option<CloseAnswer> {
    match view.take_action() {
        Some(ViewerAction::Request(AppRequest::RunningAnswer(answer))) => Some(answer),
        _ => None,
    }
}

/// "Stop and close": told, and the question goes.
#[test]
fn stop_and_close_is_told_and_the_question_goes() {
    let mut view = opened(2);
    view.answer(CloseAnswer::StopAndClose);
    assert_eq!(told(&mut view), Some(CloseAnswer::StopAndClose));
    assert!(view.closed());
}

/// "Wait": told, the question stays (following the work), without its own Wait.
#[test]
fn wait_keeps_the_question_open_following_the_work() {
    let mut view = opened(2);
    assert_eq!(view.buttons(), [Button::Cancel, Button::Wait, Button::Stop]);
    view.answer(CloseAnswer::Wait);
    assert_eq!(told(&mut view), Some(CloseAnswer::Wait));
    assert!(!view.closed() && view.waiting() && view.is_loading());
    assert_eq!(view.buttons(), [Button::Cancel, Button::Stop]);
    // The app gives the work again as it goes: one left now.
    let mut fewer = prompt(1);
    fewer.waiting = true;
    view.set_data(&fewer);
    assert_eq!(view.prompt().map(RunningPrompt::count), Some(1));
    assert!(view.waiting());
}

/// "Cancel" (and Esc): told, the question goes; ↵ at first is Cancel (a stray one loses
/// nothing), Tab / arrows move to the others.
#[test]
fn cancel_esc_and_the_keys() {
    let mut view = opened(1);
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(told(&mut view), Some(CloseAnswer::Cancel));
    assert!(view.closed());
    let mut view = opened(1);
    view.edit_key(ViewerKey::Enter);
    assert_eq!(told(&mut view), Some(CloseAnswer::Cancel));
    let mut view = opened(1);
    view.edit_key(ViewerKey::Left);
    view.edit_key(ViewerKey::Enter);
    assert_eq!(told(&mut view), Some(CloseAnswer::StopAndClose), "← from the first: the last");
    let mut view = opened(1);
    view.edit_key(ViewerKey::Tab);
    view.edit_key(ViewerKey::Enter);
    assert_eq!(told(&mut view), Some(CloseAnswer::Wait));
}

/// Only so many rows: the rest counted; the window as tall as what it lists.
#[test]
fn many_items_are_counted_beyond_the_rows_shown() {
    let few = opened(2);
    assert_eq!(few.shown(), (2, 0));
    let many = opened(MAX_ROWS + 3);
    assert_eq!(many.shown(), (MAX_ROWS, 3));
    assert!(many.modal_height() > few.modal_height());
}

#[test]
fn a_row_says_what_it_is_how_far_and_what_stopping_costs() {
    let project = kernel::ProjectId::new();
    let mut work = upload(project);
    let now = Instant::now();
    work.since = now.checked_sub(Duration::from_secs(125));
    work.detail = "Remote: shop".into();
    assert!(title(&work).ends_with("· index.php"), "{}", title(&work));
    let line = detail(&work, now);
    assert!(line.starts_with("Remote: shop · "), "{line}");
    assert!(line.contains("25%"), "{line}");
    assert!(line.contains('2'), "two minutes: {line}");
    work.stop = None;
    assert_eq!(detail(&work, now).rsplit(" · ").next(), Some(t("running.not_stoppable").as_str()));
    assert_eq!(icon(work.kind), "upload");
    assert_eq!(icon(WorkKind::Program), "terminal");
    let p = prompt(1);
    assert_eq!(group_title(&p, &p.groups[0]), "shop · Remote projects");
}

/// Its buttons are the window footer's, right to left: Stop and close (danger), Wait,
/// Cancel — the keys on Cancel at first, moved by ← →; a click comes as its command.
#[test]
fn the_buttons_are_the_footers_and_the_keys_start_on_cancel() {
    let mut view = opened(2);
    let buttons = view.modal_buttons();
    let ids: Vec<&str> = buttons.iter().map(|b| b.id).collect();
    assert_eq!(ids, ["running.footer.stop", "running.footer.wait", "running.footer.cancel"]);
    assert!(buttons[0].danger && buttons[2].focused, "Cancel has the keys");
    view.edit_key(ViewerKey::Right);
    assert!(view.modal_buttons()[1].focused, "Wait");
    assert!(view.command("running.footer.cancel"));
    assert!(view.closed());
}
