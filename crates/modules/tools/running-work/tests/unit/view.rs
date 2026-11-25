// The tests make their own instants.
#![allow(clippy::disallowed_methods)]

use super::*;
use workbench_model::resources::ActivityKind;
use workbench_model::running::ProgramState;
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
        stopping: None,
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

fn stopping(states: &[ProgramState]) -> RunningPrompt {
    use workbench_model::running::{Stopping, StoppingProgram};
    let programs = states
        .iter()
        .enumerate()
        .map(|(i, state)| StoppingProgram {
            name: format!("program-{i}"),
            pid: 100 + u32::try_from(i).unwrap(),
            state: *state,
        })
        .collect();
    RunningPrompt { closing: Closing::App, stopping: Some(Stopping { programs }), ..prompt(0) }
}

/// While the programs are being stopped: no buttons, no `esc`, × and every key change
/// nothing, it keeps turning until they've all ended.
#[test]
fn while_stopping_nothing_closes_or_answers_it() {
    let mut view = RunningView::default();
    view.set_data(&stopping(&[ProgramState::Asked, ProgramState::Ended]));
    assert!(view.modal_buttons().is_empty());
    assert!(!view.modal_closable() && !view.may_close());
    for key in [ViewerKey::Escape, ViewerKey::Enter, ViewerKey::Tab] {
        assert!(view.edit_key(key), "{key:?} kept");
    }
    assert!(!view.command("running.footer.cancel"));
    assert_eq!(view.take_action(), None);
    assert!(!view.closed());
    assert!(view.is_loading());
    assert_eq!(view.modal_icon(), Some("log-out"));
    // All ended: nothing turns any more (the app goes on by itself).
    view.set_data(&stopping(&[ProgramState::Forced, ProgramState::Ended]));
    assert!(!view.is_loading());
    assert!(!view.modal_closable());
}

/// The window as tall as the list (up to its visible rows); a longer list scrolls by the
/// wheel, only as far as it goes.
#[test]
fn a_long_list_of_programs_scrolls_as_far_as_it_goes() {
    let mut view = RunningView::default();
    view.set_data(&stopping(&[ProgramState::Asked; 3]));
    let three = view.modal_height().unwrap();
    assert!(!view.scroll(0.0, 40.0), "all shown: nothing to scroll");
    view.set_data(&stopping(&[ProgramState::Asked; 20]));
    let twenty = view.modal_height().unwrap();
    view.set_data(&stopping(&[ProgramState::Asked; 30]));
    assert_eq!(view.modal_height().unwrap(), twenty, "past the visible rows, no taller");
    assert!(twenty > three);
    assert!(view.scroll(0.0, 1e6));
    assert!(!view.scroll(0.0, 10.0), "at the end");
    assert!(view.scroll(0.0, -1e6));
    // The question's own view is as before.
    let mut question = opened(1);
    assert!(question.modal_closable());
    assert!(!question.scroll(0.0, 10.0));
}

/// A list longer than its box: the app's scrollbar along its right edge, its thumb dragged
/// scrolls it; a shorter one has none to take.
#[test]
fn the_list_s_scrollbar_thumb_is_dragged() {
    use kernel::{Point, Rect};
    use render_api::Scene;
    use text_layout::TextSystem;
    let icons = icons::Icons::builtin();
    let palette = ui_components::Palette::dark();
    let mut text = TextSystem::new();
    let bounds = Rect::new(0.0, 0.0, 540.0, 420.0);
    let paint = |view: &mut RunningView, text: &mut TextSystem| {
        let mut scene = Scene::default();
        view.paint(ViewerPaint {
            bounds,
            scale: 1.0,
            text,
            scene: &mut scene,
            palette: &palette,
            icons: &icons,
            focused: true,
            waker: None,
        });
    };
    let opened = |count: usize, text: &mut TextSystem| {
        let mut view = RunningView::default();
        view.set_data(&stopping(&vec![ProgramState::Asked; count]));
        paint(&mut view, text);
        view
    };
    // Along the box's right edge, near its top (the thumb at rest there).
    let grab = |count: usize, asked: bool, text: &mut TextSystem| {
        (100..200).step_by(4).any(|y| {
            (0..40).any(|dx| {
                let mut view = opened(count, text);
                if asked {
                    // Back to the question: the bar drawn before isn't shown any more.
                    view.set_data(&prompt(1));
                }
                let p = Point::new(bounds.max_x() - 24.0 - dx as f32, y as f32);
                view.hover(p);
                Viewer::press(&mut view, p, 1);
                let moved = view.drag(Point::new(p.x, p.y + 120.0)) && view.scroll > 0.0;
                view.release();
                moved
            })
        })
    };
    assert!(grab(30, false, &mut text), "the thumb dragged scrolls the list");
    // Over the thumb, the bar's cursor; held and dragged anywhere, still the bar's; let go
    // away from it, the modal's own again.
    let bar = Some(ui_components::scrollbar::CURSOR);
    let mut view = opened(30, &mut text);
    let thumb = (100..200)
        .step_by(4)
        .flat_map(|y| {
            (0..40).map(move |dx| Point::new(bounds.max_x() - 24.0 - dx as f32, y as f32))
        })
        .find(|&p| {
            view.hover(p);
            view.cursor_at(p) == bar
        })
        .expect("the thumb");
    Viewer::press(&mut view, thumb, 1);
    let away = Point::new(40.0, 40.0);
    view.drag(away);
    view.hover(away);
    assert_eq!(view.cursor_at(away), bar, "held: the bar's wherever it goes");
    view.release();
    assert_eq!(view.cursor_at(away), None, "let go: the modal's own");
    assert!(!grab(3, false, &mut text), "all shown: no scrollbar to take");
    assert!(!grab(30, true, &mut text), "the question has no list: its bar isn't hit");
}
