//! The environments' tab: read from the files on a worker, typed in, written back to them.

use super::*;

fn wait(view: &mut EnvView, ready: impl Fn(&EnvView) -> bool) {
    let started = Instant::now();
    while !ready(view) && started.elapsed() < Duration::from_secs(10) {
        view.poll();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_variable_typed_is_written_to_its_file() {
    let dir = std::env::temp_dir().join(format!("http-envview-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(http_requests::env::PUBLIC), r#"{"dev": {"host": "e.com"}}"#).unwrap();
    let mut view = EnvView::default();
    view.set_data(&EnvData { file: dir.join("api.http"), root: Some(dir.clone()) });
    wait(&mut view, |v| v.folder.is_some());
    assert_eq!(view.names, ["dev"]);
    assert_eq!(view.lines[0].len(), 2, "host and an empty row");
    // A new private variable.
    view.focus = Some((1, false));
    view.text_input("token");
    view.focus = Some((1, true));
    view.text_input("t0k");
    view.lines[0][1].private = true;
    view.save();
    wait(&mut view, |v| v.texts.1.is_some());
    let private = std::fs::read_to_string(dir.join(http_requests::env::PRIVATE)).unwrap();
    assert!(private.contains("\"token\": \"t0k\""), "{private}");
    let public = std::fs::read_to_string(dir.join(http_requests::env::PUBLIC)).unwrap();
    assert!(public.contains("e.com") && !public.contains("t0k"), "{public}");
    // A private value isn't copied out.
    view.focus = Some((1, true));
    view.select_all();
    assert_eq!(view.selected_text(), None);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn an_environment_is_renamed_on_its_tab_and_deleted_once_confirmed() {
    let dir = std::env::temp_dir().join(format!("http-envview-{:x}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let public = r#"{"$shared": {"v": "1"}, "dev": {"host": "e.com", "SSLConfiguration": {"verifyHostCertificate": false}}, "prod": {"host": "p.com"}}"#;
    std::fs::write(dir.join(http_requests::env::PUBLIC), public).unwrap();
    std::fs::write(dir.join(http_requests::env::PRIVATE), r#"{"dev": {"token": "t"}}"#).unwrap();
    let mut view = EnvView::default();
    view.set_data(&EnvData { file: dir.join("api.http"), root: Some(dir.clone()) });
    wait(&mut view, |v| v.folder.is_some());
    assert_eq!(view.names, ["$shared", "dev", "prod"]);
    // `$shared` keeps its name; a name taken isn't one.
    view.start_rename(0);
    assert!(view.renaming.is_none());
    view.start_rename(1);
    view.text_input("prod");
    view.edit_key_with(ViewerKey::Enter, KeyReach::default());
    assert!(view.renaming.is_some() && view.error.is_some(), "taken: it stays typed");
    view.select_all();
    view.text_input("staging");
    view.edit_key_with(ViewerKey::Enter, KeyReach::default());
    assert_eq!(view.names, ["$shared", "staging", "prod"]);
    view.save();
    wait(&mut view, |v| v.texts.0.as_deref().is_some_and(|t| t.contains("staging")));
    let written = std::fs::read_to_string(dir.join(http_requests::env::PUBLIC)).unwrap();
    assert!(written.contains("verifyHostCertificate") && !written.contains("\"dev\""));
    let private = std::fs::read_to_string(dir.join(http_requests::env::PRIVATE)).unwrap();
    assert!(private.contains("staging"), "{private}");
    // Renamed again after the write: the settings still follow it.
    view.start_rename(1);
    view.text_input("stage");
    view.edit_key_with(ViewerKey::Enter, KeyReach::default());
    view.save();
    wait(&mut view, |v| v.texts.0.as_deref().is_some_and(|t| t.contains("\"stage\"")));
    let written = std::fs::read_to_string(dir.join(http_requests::env::PUBLIC)).unwrap();
    assert!(written.contains("verifyHostCertificate"), "{written}");
    // Deleted: asked first (nothing gone yet), gone once the answer is yes.
    view.menu_tab = Some(2);
    assert!(view.command("env-delete"));
    assert!(matches!(view.take_action(), Some(ViewerAction::WithHost(_))));
    assert_eq!(view.names.len(), 3, "not before the answer");
    *view.deleting.lock().unwrap() = Some("prod".into());
    view.poll();
    assert_eq!(view.names, ["$shared", "stage"]);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn the_row_tab_moves_to_is_scrolled_into_view() {
    // As last drawn: rows of 34 points, 5 of them showing.
    let mut view = EnvView {
        names: vec!["dev".into()],
        lines: vec![(0..20).map(|_| Line::empty()).collect()],
        row_px: 34.0,
        visible_rows: 5,
        ..EnvView::default()
    };
    view.focus = Some((4, true));
    view.edit_key_with(ViewerKey::Tab, KeyReach::default());
    assert_eq!(view.focus, Some((5, false)));
    assert_eq!(view.scroll, 34.0, "one row down: the sixth shows at the bottom");
    view.focus = Some((0, false));
    view.keep_visible(0);
    assert_eq!(view.scroll, 0.0, "back up to the first");
}
