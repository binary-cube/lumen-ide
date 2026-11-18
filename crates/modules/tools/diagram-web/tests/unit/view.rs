use std::time::{Duration, Instant};

use super::*;

/// Polls `view` until `done` holds (the workers' results come back), at most a few seconds.
fn poll_until(view: &mut DiagramWebView, done: impl Fn(&DiagramWebView) -> bool) {
    let until = Instant::now() + Duration::from_secs(10);
    while !done(view) && Instant::now() < until {
        view.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(done(view), "timed out");
}

fn messages(view: &mut DiagramWebView) -> Vec<serde_json::Value> {
    view.take_web_sends().iter().map(|(_, s)| serde_json::from_str(s).unwrap()).collect()
}

fn say(view: &mut DiagramWebView, text: &str) {
    let id = view.page_id;
    view.web_event(id, WebEvent::Message(text.to_owned()));
}

/// A fake page: configured, given the file, its autosaves written to disk, a change made
/// by someone else merged in, its links opened, its exit closing the tab.
#[test]
fn the_view_talks_to_the_page_and_keeps_the_file() {
    let dir = std::env::temp_dir().join(format!("diagram-web-view-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("flow.drawio");
    std::fs::write(&path, "<mxfile>one</mxfile>").unwrap();
    let events = kernel::EventBus::new();
    let mut view = DiagramWebView::default();
    view.follow_theme(&Palette::dark());
    view.set_data(&OpenData { path: path.clone(), events: Some(events.clone()) });
    say(&mut view, r#"{"event":"configure"}"#);
    let configure = messages(&mut view);
    assert_eq!(configure[0]["action"], "configure");
    assert_eq!(configure[0]["config"]["suppressNewWindows"], true);
    say(&mut view, r#"{"event":"init"}"#);
    poll_until(&mut view, |v| v.read_once);
    let load = messages(&mut view);
    assert_eq!(load[0]["action"], "load");
    assert_eq!(load[0]["xml"], "<mxfile>one</mxfile>");
    // An autosave: written through a temporary file, the page told.
    say(&mut view, r#"{"event":"autosave","xml":"<mxfile>two</mxfile>"}"#);
    assert!(view.is_dirty());
    poll_until(&mut view, |v| !v.is_dirty());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "<mxfile>two</mxfile>");
    assert_eq!(
        messages(&mut view),
        vec![serde_json::json!({"action": "status", "modified": false})]
    );
    // Our own write heard back: nothing sent.
    events.publish(&workspace::events::FilesWritten { paths: vec![path.clone()] });
    let until = Instant::now() + Duration::from_millis(300);
    while Instant::now() < until {
        view.poll();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(messages(&mut view).is_empty());
    // Someone else's: loaded again.
    std::fs::write(&path, "<mxfile>three</mxfile>").unwrap();
    events.publish(&workspace::events::FilesWritten { paths: vec![path.clone()] });
    let mut merged = Vec::new();
    let until = Instant::now() + Duration::from_secs(10);
    while merged.is_empty() && Instant::now() < until {
        view.poll();
        merged = messages(&mut view);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(merged[0]["action"], "load");
    assert_eq!(merged[0]["xml"], "<mxfile>three</mxfile>");
    // A message for another page is let go.
    view.web_event(1, WebEvent::Message(r#"{"event":"exit"}"#.into()));
    assert_eq!(view.take_action(), None);
    // Links: the web's only.
    view.web_event(view.page_id, WebEvent::OpenLink("https://example.com".into()));
    view.web_event(view.page_id, WebEvent::OpenLink("file:///etc/passwd".into()));
    assert_eq!(view.take_action(), Some(ViewerAction::OpenLink("https://example.com".into())));
    assert_eq!(view.take_action(), None);
    say(&mut view, r#"{"event":"exit"}"#);
    assert_eq!(view.take_action(), Some(ViewerAction::Close));
    // Renamed: it follows.
    let moved = dir.join("moved.drawio");
    events.publish(&workspace::events::FileRenamed { from: path, to: moved.clone() });
    poll_until(&mut view, |v| v.path.as_ref() == Some(&moved));
    std::fs::remove_dir_all(dir).unwrap();
}

/// A theme changed: the page made anew with the new colors, unless something's unsaved
/// (then after the write).
#[test]
fn a_theme_change_makes_the_page_anew() {
    let mut view = DiagramWebView::default();
    view.follow_theme(&Palette::dark());
    let first = view.page_id;
    let entry = view.entry.clone();
    assert!(entry.contains("dark=1"), "{entry}");
    view.follow_theme(&Palette::dark());
    assert_eq!(view.page_id, first, "the same palette changes nothing");
    let mut light = Palette::dark();
    light.editor_background = Rgba::rgb(0xffffff);
    light.text = Rgba::rgb(0x111111);
    view.follow_theme(&light);
    assert_ne!(view.page_id, first);
    assert!(view.entry.contains("dark=0"), "{}", view.entry);
    // Loaded and saved (what a page is most of the time): made anew at once, not after a
    // change the user may never make.
    view.session.file_read(Arc::from("a"));
    view.session.page_message(r#"{"event":"init"}"#);
    assert!(view.session.ready() && !view.session.dirty());
    let loaded = view.page_id;
    view.follow_theme(&Palette::dark());
    assert_ne!(view.page_id, loaded, "at once");
    assert!(!view.reload_after_save);
    view.follow_theme(&light);
    // Something unsaved: after the write.
    view.session.file_read(Arc::from("a"));
    view.session.page_message(r#"{"event":"autosave","xml":"b"}"#);
    let second = view.page_id;
    view.follow_theme(&Palette::dark());
    assert_eq!(view.page_id, second);
    assert!(view.reload_after_save);
}

#[test]
fn nothing_is_shown_before_it_is_installed() {
    let mut view = DiagramWebView::default();
    assert_eq!(view.web_page(), None);
    assert!(!view.page_ready());
    view.status = Status::Missing;
    assert_eq!(view.button_list(), vec![Button::Download]);
    view.status = Status::Failed { reason: "x".into(), offline: true };
    assert_eq!(view.button_list(), vec![Button::Retry]);
    view.status = Status::Downloading { received: 1, total: 2 };
    assert_eq!(view.button_list(), vec![Button::Stop]);
}

#[test]
fn words_are_wrapped_on_a_few_lines() {
    let fits = |s: &str| s.len() <= 10;
    assert_eq!(wrap("one two three four", 4, fits), vec!["one two", "three four"]);
    assert_eq!(wrap("aaaaaaaaaaaaaa b", 4, fits), vec!["aaaaaaaaaaaaaa", "b"]);
    assert_eq!(wrap("a b c d e f g h i j k l m n", 1, fits).len(), 1);
}

/// An export: the Save dialog asked, beside the diagram; written where it says.
#[test]
fn an_export_is_saved_where_the_user_chooses() {
    let dir = std::env::temp_dir().join(format!("diagram-web-export-{}", kernel::id::new_uuid()));
    std::fs::create_dir_all(&dir).unwrap();
    let mut view = DiagramWebView::default();
    view.follow_theme(&Palette::dark());
    view.set_data(&OpenData { path: dir.join("flow.drawio"), events: None });
    say(&mut view, r#"{"event":"export","format":"svg","filename":"flow.svg","data":"<svg/>"}"#);
    assert_eq!(
        view.take_action(),
        Some(ViewerAction::Request(AppRequest::PickSavePath {
            start: Some(dir.clone()),
            name: "flow.svg".into()
        }))
    );
    let target = dir.join("out.svg");
    assert!(view.command(&format!("picked-save:{}", target.display())));
    poll_until(&mut view, |_| target.exists());
    poll_until(&mut view, |v| !v.notifications.is_empty());
    assert_eq!(std::fs::read(&target).unwrap(), b"<svg/>");
    assert!(!view.command("picked-save:/nowhere"), "nothing waits now");
    std::fs::remove_dir_all(dir).unwrap();
}

/// Its tab hidden: no frame asked for the next look at the file (it happens as the tab
/// paints: a hidden one's would be due forever, the app drawing without end); seen again,
/// it's due again.
#[test]
fn a_hidden_tab_asks_for_no_frames() {
    let mut view = DiagramWebView::default();
    view.read_once = true;
    view.looked_at = Some(Instant::now() - Duration::from_secs(60));
    assert!(view.frame_deadline().is_some_and(|due| due <= Instant::now()));
    view.set_shown(false);
    assert_eq!(view.frame_deadline(), None);
    view.set_shown(true);
    assert!(view.frame_deadline().is_some());
}
