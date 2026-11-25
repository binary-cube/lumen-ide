//! The console's confirmation: "I understand" before a production run, the answer done only
//! for the question it was given to (Enter, the buttons), nothing done when it's closed
//! otherwise (Esc, ×), the transaction's three answers, the dangers in words.

use super::*;
use crate::fake::{FakeHost, Project};
use database::{Danger, DropKind, TableRef};
use std::sync::{Arc, Mutex};

fn request(kind: ConfirmKind, production: bool) -> ConfirmRequest {
    let mut r = ConfirmRequest::new(kind, "console", "shop");
    r.production = production;
    r.statements = vec!["DELETE FROM orders".into()];
    r.reasons = vec!["every row".into()];
    r
}

/// `request` asked through a host, what its answer did kept.
fn asked(host: &mut FakeHost, request: ConfirmRequest) -> (ConfirmView, Arc<Mutex<Vec<Answer>>>) {
    let answers = Arc::new(Mutex::new(Vec::new()));
    let into = Arc::clone(&answers);
    assert!(ask(host, request, Box::new(move |_, a| into.lock().unwrap().push(a))));
    let mut view = ConfirmView::default();
    let given = host.confirms.last().cloned().unwrap();
    Viewer::set_data(&mut view, &given);
    (view, answers)
}

/// What the modal's button handed to the host, done.
fn done(action: Option<ViewerAction>, host: &mut FakeHost) {
    let Some(ViewerAction::WithHost(work)) = action else { panic!("its work") };
    work.run(host);
}

#[test]
fn production_asks_for_i_understand_first() {
    let project = Project::new("confirm-tick");
    let mut host = FakeHost::new(kernel::ProjectId::new(), &project.root);
    let (mut view, answers) = asked(&mut host, request(ConfirmKind::Run, true));
    assert!(!view.can_go());
    assert!(!view.answer(Answer::Go), "not before the tick");
    assert!(!view.closed());
    assert!(view.command("understood") && view.understood());
    assert!(view.edit_key(ViewerKey::Enter));
    assert!(view.closed());
    let work = view.take_action();
    done(work.clone(), &mut host);
    assert_eq!(*answers.lock().unwrap(), [Answer::Go]);
    done(work, &mut host);
    assert_eq!(answers.lock().unwrap().len(), 1, "done once");
    // Elsewhere, no tick.
    let (view, _) = asked(&mut host, request(ConfirmKind::Run, false));
    assert!(view.can_go());
}

#[test]
fn closed_without_an_answer_nothing_is_done() {
    let project = Project::new("confirm-esc");
    let mut host = FakeHost::new(kernel::ProjectId::new(), &project.root);
    let (mut view, answers) = asked(&mut host, request(ConfirmKind::Run, false));
    assert!(view.edit_key(ViewerKey::Escape));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    drop(view);
    assert!(answers.lock().unwrap().is_empty());
}

#[test]
fn a_transaction_s_end_is_committed_rolled_back_or_kept() {
    let project = Project::new("confirm-close");
    let mut host = FakeHost::new(kernel::ProjectId::new(), &project.root);
    let close = request(ConfirmKind::CloseTransaction, true);
    assert!(!close.needs_tick(), "committing is what's asked");
    let (mut view, answers) = asked(&mut host, close.clone());
    assert!(view.edit_key(ViewerKey::Enter));
    done(view.take_action(), &mut host);
    let (mut other, _) = asked(&mut host, close);
    assert!(other.command("rollback"));
    done(other.take_action(), &mut host);
    assert_eq!(*answers.lock().unwrap(), [Answer::Commit]);
    assert!(other.closed());
}

#[test]
fn every_title_and_button_reads() {
    for kind in [
        ConfirmKind::Run,
        ConfirmKind::Analyze,
        ConfirmKind::Profile,
        ConfirmKind::CloseTransaction,
    ] {
        let r = request(kind, true);
        assert!(!r.title().is_empty() && !r.title().contains("console."), "{kind:?}");
        assert!(!r.go_label().is_empty() && !r.go_label().contains("console."), "{kind:?}");
    }
}

#[test]
fn dangers_are_said_in_words() {
    let users = TableRef::new("users");
    let texts = [
        danger_text(&Danger::UnfilteredWrite {
            kind: database::sql::WriteKind::Delete,
            table: users.clone(),
        }),
        danger_text(&Danger::UnfilteredWrite {
            kind: database::sql::WriteKind::Update,
            table: users.clone(),
        }),
        danger_text(&Danger::DropObject {
            kind: DropKind::Table,
            name: "users".into(),
            cascade: true,
        }),
        danger_text(&Danger::Truncate { table: users.clone() }),
        danger_text(&Danger::AlterDrop { table: users, column: "email".into() }),
    ];
    for text in &texts {
        assert!(text.contains("users"), "{text}");
        assert!(!text.contains("schema.lose"), "a locale's key left: {text}");
    }
    assert_ne!(texts[0], texts[1]);
}

/// The statements' box fits them: a one-line statement gets two lines' height, a long
/// script twelve (the rest scrolls); the window asks for what it shows, no more.
#[test]
fn the_statements_box_fits_them() {
    let mut r = request(ConfirmKind::Run, false);
    let two = 2.0 * 21.0 + 20.0;
    assert_eq!(code_box_height(&r), two, "one line: the least");
    r.statements = vec!["UPDATE a SET b = 1\nWHERE c = 2".into(), "DELETE FROM d".into()];
    assert_eq!(code_box_height(&r), 4.0 * 21.0 + 20.0, "three lines and the gap between");
    r.statements = vec![(0..40).map(|i| format!("SELECT {i};")).collect::<Vec<_>>().join("\n")];
    assert_eq!(code_box_height(&r), 12.0 * 21.0 + 20.0, "the most; the rest scrolls");
    // The window: one reason, a short statement, no tick, compact.
    let short = request(ConfirmKind::Run, false);
    let base = content_height(&short);
    assert_eq!(base, 42.0 + 24.0 + 8.0 + two + 8.0 + 64.0);
    // Production adds its tick; a note its line.
    let prod = request(ConfirmKind::Run, true);
    assert_eq!(content_height(&prod), base + 44.0);
    let mut noted = short;
    noted.note = Some("and 3 more".into());
    assert_eq!(content_height(&noted), base + 28.0);
    let mut view = ConfirmView::default();
    assert_eq!(Viewer::modal_height(&view), None, "nothing asked yet");
    Viewer::set_data(&mut view, &prod);
    assert_eq!(Viewer::modal_height(&view), Some(content_height(&prod)));
}

/// A long script scrolls in its box: the scrollbar's thumb, pressed and dragged down, moves
/// the statements (a click there presses no button).
#[test]
fn the_statements_thumb_is_dragged() {
    let project = Project::new("confirm-thumb");
    let mut host = FakeHost::new(kernel::ProjectId::new(), &project.root);
    let mut r = request(ConfirmKind::Run, false);
    r.statements = vec![(0..80).map(|i| format!("SELECT {i};")).collect::<Vec<_>>().join("\n")];
    let (mut view, answers) = asked(&mut host, r);
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let bounds = Rect::new(0.0, 0.0, 1000.0, 1200.0);
    view.paint(ViewerPaint {
        bounds,
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    // The thumb: the bar's topmost point (the box shows its first lines).
    let on = (0..bounds.size.height as u32)
        .flat_map(|y| (0..bounds.size.width as u32).map(move |x| (x, y)))
        .map(|(x, y)| Point::new(x as f32, y as f32))
        .find(|p| view.bar.contains(*p))
        .expect("a scrollbar along the statements");
    let at = Point::new(on.x + 1.0, on.y + 2.0);
    view.hover(at);
    assert_eq!(view.cursor_at(at), Some(ui_components::scrollbar::CURSOR), "the bar's");
    assert_eq!(view.click(at), None);
    assert!(view.bar.is_dragging(), "the thumb is held");
    assert!(view.drag(Point::new(at.x, at.y + 40.0)));
    assert!(view.scroll > 0.0, "the statements moved");
    // Dragged over a button: the bar's cursor still, not the button's hand.
    let button = view.hits.first().map(|(r, _)| r.center()).expect("a button");
    view.drag(button);
    view.hover(button);
    assert_eq!(view.cursor_at(button), Some(ui_components::scrollbar::CURSOR), "held");
    assert!(view.release());
    assert_eq!(view.cursor_at(button), Some(ViewerCursor::Pointer), "let go: the button's");
    assert!(!view.closed() && answers.lock().unwrap().is_empty(), "nothing answered");
}

/// Its own buttons show the hand; beside them, the modal's cursor.
#[test]
fn its_buttons_show_the_hand() {
    let project = Project::new("confirm-cursor");
    let mut host = FakeHost::new(kernel::ProjectId::new(), &project.root);
    let (mut view, _) = asked(&mut host, request(ConfirmKind::Run, true));
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1000.0, 800.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    let button = view.hits.first().map(|(r, _)| r.center()).expect("a button");
    view.hover(button);
    assert_eq!(view.cursor_at(button), Some(ViewerCursor::Pointer));
    let beside = Point::new(1.0, 1.0);
    view.hover(beside);
    assert_eq!(view.cursor_at(beside), None, "the modal's own");
}
