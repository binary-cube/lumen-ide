//! The notifications in the window: over a modal (the modal keeps its place and its keys),
//! × and an action clicked, nothing under a card getting the click, Esc over them, a
//! module's thread telling through the project's notifier, projects never seeing each
//! other's, the position chosen in the settings, time standing still under the mouse.

use std::time::Duration;

use workbench_model::{Notification, Notifier, ToastPosition, WorkbenchHost};

use super::*;

/// A project in a scratch folder with a file open, laid out.
fn opened(name: &str) -> (Workbench, PathBuf, TextSystem) {
    let dir = std::env::temp_dir().join(format!("toasts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.txt"), "abc\n").unwrap();
    let dir = crate::workbench::canonical(&dir);
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    wb.open_project(&dir).unwrap();
    wb.open_file(&dir.join("a.txt")).unwrap();
    let mut text = TextSystem::new();
    paint(&mut wb, &mut text);
    (wb, dir, text)
}

fn paint(wb: &mut Workbench, text: &mut TextSystem) {
    wb.paint(text, &mut render_api::Scene::default());
}

/// Painted until their transitions are over.
fn settled(wb: &mut Workbench, text: &mut TextSystem) {
    for _ in 0..20 {
        paint(wb, text);
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The × of the card drawn at `card` (scale 1).
fn close_of(card: Rect) -> Point {
    Point::new(card.max_x() - 22.0, card.min_y() + 22.0)
}

#[test]
fn a_notification_shows_over_a_modal_and_closes_with_its_x() {
    let (mut wb, dir, mut text) = opened("modal");
    wb.open_about();
    assert!(wb.modal_open());
    wb.notify(Notification::success("Saved."));
    settled(&mut wb, &mut text);
    assert!(wb.modal_open(), "the modal is still the one on top for the keys");
    assert_eq!(wb.active_overlay(), Some(Overlay::About));
    let card = wb.toasts.bounds().expect("drawn over the modal");
    // A click on the card (not its ×) does nothing, and doesn't reach the modal.
    let on_card = Point::new(card.min_x() + 30.0, card.min_y() + card.size.height / 2.0);
    assert_eq!(wb.mouse_down(&mut text, on_card, false, false, false, 1), MouseResult::None);
    assert!(wb.overlay_is_open(Overlay::About));
    // Its ×: it leaves, the modal stays.
    let result = wb.mouse_down(&mut text, close_of(card), false, false, false, 1);
    assert_eq!(result, MouseResult::Redraw);
    assert!(wb.overlay_is_open(Overlay::About));
    settled(&mut wb, &mut text);
    assert!(wb.data.notifications.is_empty());
    assert!(!wb.overlay_is_open(Overlay::Toasts));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_action_runs_its_command_and_esc_over_the_stack_closes_them() {
    let (mut wb, dir, mut text) = opened("action");
    wb.notify(
        Notification::warning("Deleted")
            .with_action("Undo", crate::commands::UNDO)
            .with_action("Show", crate::commands::REVEAL_IN_PROJECT),
    );
    settled(&mut wb, &mut text);
    let card = wb.toasts.bounds().unwrap();
    // The first action sits under the text, at the card's bottom.
    let at = Point::new(card.min_x() + 56.0 + 8.0, card.max_y() - 12.0 - 13.0);
    let (_, icon) = wb.hover(at);
    assert_eq!(icon, CursorIcon::Pointer);
    assert_eq!(
        wb.mouse_down(&mut text, at, false, false, false, 1),
        MouseResult::Command(crate::commands::UNDO.0)
    );
    settled(&mut wb, &mut text);
    assert!(wb.data.notifications.is_empty(), "closed by its action");
    // Esc: only with the mouse over them.
    wb.notify(Notification::error("Failed"));
    settled(&mut wb, &mut text);
    wb.hover(Point::new(5.0, 790.0));
    assert!(!wb.escape_toasts());
    let card = wb.toasts.bounds().unwrap();
    wb.hover(Point::new(card.min_x() + 30.0, card.min_y() + 10.0));
    assert!(wb.edit_key(&mut text, EditKey::Escape));
    settled(&mut wb, &mut text);
    assert!(wb.data.notifications.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn time_stands_still_under_the_mouse() {
    let (mut wb, dir, mut text) = opened("hover");
    wb.notify(Notification::info("Copied"));
    settled(&mut wb, &mut text);
    assert!(wb.toast_deadline().is_some(), "its ring asks for frames");
    let card = wb.toasts.bounds().unwrap();
    wb.hover(Point::new(card.min_x() + 30.0, card.min_y() + 10.0));
    let left = wb.data.notifications.shown().next().unwrap().remaining();
    settled(&mut wb, &mut text);
    assert_eq!(wb.data.notifications.shown().next().unwrap().remaining(), left);
    assert!(wb.toast_deadline().is_none(), "nothing counts down: no frames");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn projects_never_see_each_others_notifications() {
    let (mut wb, dir, mut text) = opened("projects");
    // A module's thread tells through the project's notifier: shown at the next frame.
    let notifier: Arc<dyn Notifier> = wb.data.notifier.clone();
    std::thread::spawn({
        let notifier = Arc::clone(&notifier);
        move || notifier.show(Notification::success("Indexed"))
    })
    .join()
    .unwrap();
    assert!(wb.poll_viewers());
    assert_eq!(wb.last_notification().as_deref(), Some("Indexed"));
    assert_eq!(wb.data.notifications.history().count(), 1);
    // Another project: none of them, nor what the first one's modules tell later.
    let other = dir.with_file_name(format!("toasts-other-{}", std::process::id()));
    std::fs::create_dir_all(&other).unwrap();
    wb.open_project(&other).unwrap();
    assert!(wb.data.notifications.is_empty());
    assert_eq!(wb.data.notifications.history().count(), 0);
    notifier.show(Notification::error("Late"));
    wb.poll_viewers();
    paint(&mut wb, &mut text);
    assert!(wb.data.notifications.is_empty(), "the first project's notifier reaches nothing");
    assert!(wb.show_recent_notifications(), "the history is empty: said so");
    assert_eq!(wb.last_notification(), Some(i18n::t("notifications.none")));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&other);
}

#[test]
fn the_stack_goes_to_the_edge_chosen() {
    let (mut wb, dir, mut text) = opened("position");
    // Kept until closed: an information's few seconds may run out on a slow machine while
    // every edge is tried (each one settled).
    wb.notify(
        Notification::info("Here").with_timeout(workbench_model::notifications::Timeout::Never),
    );
    let tab_bar_bottom = wb.card.min_y() + TabBar::height(1.0);
    let status_top = 800.0 - StatusBar::height(1.0);
    for position in ToastPosition::ALL {
        wb.place_toasts_at(position);
        settled(&mut wb, &mut text);
        let card = wb.toasts.bounds().unwrap();
        if position.top() {
            assert!(card.min_y() >= tab_bar_bottom, "{position:?}: under the tab bar");
            assert!(card.min_y() < tab_bar_bottom + 30.0, "{position:?}");
        } else {
            let gap = ui_workbench::ds::POPOVER_GAP;
            assert!(
                (status_top - card.max_y() - gap).abs() <= 1.0,
                "{position:?}: above the bar: {card:?} {status_top}"
            );
        }
        let mid = card.min_x() + card.size.width / 2.0;
        let card_mid = wb.card.min_x() + wb.card.size.width / 2.0;
        assert_eq!((mid - card_mid).abs() <= 1.0, position.centered(), "{position:?}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}
