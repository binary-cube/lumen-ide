use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;

fn request(deletes: usize, confirm: bool) -> (PreviewRequest, Arc<AtomicUsize>) {
    let woken = Arc::new(AtomicUsize::new(0));
    let w = Arc::clone(&woken);
    let request = PreviewRequest {
        table: "orders".into(),
        lines: vec!["DELETE FROM orders WHERE id = 1".into()],
        sql: true,
        changes: 1,
        deletes,
        confirm,
        answer: Arc::new(Mutex::new(None)),
        wake: Arc::new(move || {
            w.fetch_add(1, Ordering::Relaxed);
        }),
    };
    (request, woken)
}

#[test]
fn submit_answers_yes_and_closes() {
    let (r, woken) = request(0, true);
    let answer = Arc::clone(&r.answer);
    let mut view = PreviewView::default();
    view.set_data(&r);
    assert!(view.submit());
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    assert_eq!(*answer.lock().unwrap(), Some(true));
    assert_eq!(woken.load(Ordering::Relaxed), 1);
    drop(view);
    assert_eq!(*answer.lock().unwrap(), Some(true), "the first answer stays");
}

#[test]
fn deleting_rows_asks_for_a_tick_when_set_so() {
    let (r, _) = request(2, true);
    let mut view = PreviewView::default();
    view.set_data(&r);
    assert!(!view.can_submit());
    assert!(!view.submit());
    view.confirmed = true;
    assert!(view.submit());
    let (r, _) = request(2, false);
    let mut view = PreviewView::default();
    view.set_data(&r);
    assert!(view.can_submit(), "not asked");
}

#[test]
fn closed_without_an_answer_is_a_no() {
    let (r, _) = request(0, true);
    let answer = Arc::clone(&r.answer);
    let mut view = PreviewView::default();
    view.set_data(&r);
    drop(view);
    assert_eq!(*answer.lock().unwrap(), Some(false));
}

#[test]
fn the_command_opens_the_waiting_request_once() {
    assert!(PreviewProvider.kind(Path::new("modal.view-data-preview")).is_some());
    assert!(PreviewProvider.kind(Path::new("a.csv")).is_none());
}

/// Its buttons are the window's footer's: Submit greyed until the deletion is ticked, each
/// one's click coming back as its command; what it is as the footer's note.
#[test]
fn the_footer_s_buttons_are_the_window_s() {
    let (r, _) = request(2, true);
    let answer = Arc::clone(&r.answer);
    let mut view = PreviewView::default();
    view.set_data(&r);
    let buttons = view.modal_buttons();
    assert_eq!(buttons.iter().map(|b| b.id).collect::<Vec<_>>(), [SUBMIT, CANCEL]);
    assert!(buttons[0].main && !buttons[0].enabled, "not ticked yet");
    assert!(matches!(view.modal_status(), Some(ui_components::ModalStatus::Note(_))));
    view.confirmed = true;
    assert!(view.modal_buttons()[0].enabled);
    assert!(view.command(CANCEL));
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    assert_eq!(*answer.lock().unwrap(), Some(false));
}

/// Many statements scroll in their card: the scrollbar's thumb, pressed and dragged down,
/// moves them (the tick isn't touched).
#[test]
fn the_statements_thumb_is_dragged() {
    let (mut r, _) = request(0, true);
    r.lines = (0..80).map(|i| format!("DELETE FROM orders WHERE id = {i}")).collect();
    r.changes = r.lines.len();
    let mut view = PreviewView::default();
    view.set_data(&r);
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    let palette = ui_components::Palette::dark();
    let icons = icons::Icons::builtin();
    let bounds = Rect::new(0.0, 0.0, 1000.0, 700.0);
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
    let on = (0..bounds.size.height as u32)
        .flat_map(|y| (0..bounds.size.width as u32).map(move |x| (x, y)))
        .map(|(x, y)| Point::new(x as f32, y as f32))
        .find(|p| view.bar.contains(*p))
        .expect("a scrollbar along the statements");
    let at = Point::new(on.x + 1.0, on.y + 2.0);
    assert_eq!(view.click(at), None);
    assert!(view.drag(Point::new(at.x, at.y + 40.0)));
    assert!(view.scroll > 0.0, "the statements moved");
    assert!(view.release());
    assert!(!view.confirmed, "the tick untouched");
}
