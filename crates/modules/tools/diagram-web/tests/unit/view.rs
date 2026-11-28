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

/// While the page loads a loader turns under it; once it's drawn (or its tab hidden) it
/// stops, and a page made anew turns it again.
#[test]
fn a_loader_turns_until_the_page_is_drawn() {
    let mut view = DiagramWebView::default();
    view.path = Some(PathBuf::from("a.drawio"));
    view.page_dir = Some(Arc::from(Path::new("page")));
    view.status = Status::Installed(Arc::from(Path::new("drawio")));
    view.entry = Arc::from("index.html");
    assert!(view.is_loading(), "loading");
    view.set_shown(false);
    assert!(!view.is_loading(), "hidden: no frames for it");
    view.set_shown(true);
    view.web_event(view.page_id + 1, WebEvent::Shown);
    assert!(view.is_loading(), "another page's news changes nothing");
    view.web_event(view.page_id, WebEvent::Shown);
    assert!(!view.is_loading(), "drawn");
    view.new_page();
    assert!(view.is_loading(), "made anew: loading again");
}

/// A page that can't be made (no web engine here) stops the loader for good and says why
/// in its place; drawn later (an engine came), it's shown as usual.
#[test]
fn a_page_that_cant_be_made_stops_the_loader() {
    let mut view = DiagramWebView::default();
    view.path = Some(PathBuf::from("a.drawio"));
    view.page_dir = Some(Arc::from(Path::new("page")));
    view.status = Status::Installed(Arc::from(Path::new("drawio")));
    view.entry = Arc::from("index.html");
    view.web_event(view.page_id, WebEvent::Failed);
    assert!(!view.is_loading(), "failed: no loader turning forever");
    assert_eq!(view.page, PageState::Failed, "its explanation painted");
    view.web_event(view.page_id, WebEvent::Shown);
    assert_eq!(view.page, PageState::Shown);
    assert!(!view.is_loading());
}

/// A draw.io tab with its page drawn, as the sleeping knows it (started, quiet).
fn drawn_tab(name: &str) -> DiagramWebView {
    let file = PathBuf::from(format!("/p/{name}.drawio"));
    let mut view = DiagramWebView::in_tab(&file, Some(file.clone()));
    view.page_dir = Some(Arc::from(Path::new("page")));
    view.status = Status::Installed(Arc::from(Path::new("drawio")));
    view.entry = Arc::from("index.html");
    view.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    view.web_event(view.page_id, WebEvent::Shown);
    view.tell_veto();
    view
}

fn painted(view: &mut DiagramWebView) -> render_api::Scene {
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 1000.0, 700.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: None,
    });
    scene
}

/// Put to sleep: its page asked for its picture, then — once the picture is drawn in its
/// place (its copy made then) — let go; asleep, the moon on its tab, the picture at its own
/// size under the card; a key wakes it: a page made anew, the picture shown till it's drawn.
#[test]
fn a_diagram_falls_asleep_on_its_picture_and_wakes_as_a_new_page() {
    use ui_components::sleep::{SLEEP_ACTION, SleepWhy};
    let mut view = drawn_tab("sleepy");
    assert_eq!(view.can_sleep(), ui_components::sleep::CanSleep::Yes);
    assert!(view.command(SLEEP_ACTION));
    assert!(matches!(view.slumber, Slumber::Freezing(SleepWhy::ByHand)));
    assert!(view.web_page().is_some_and(|p| p.freeze), "its picture asked for");
    let page = view.page_id;
    // A copy of its frame (made at the renderer's next frame): its page kept till then.
    let frame = render_api::ImageData::live();
    let shot = render_api::ImageData::copy_of(&frame, 800, 600);
    view.web_event(page, WebEvent::Picture(Some(shot.clone())));
    assert_eq!(view.tab_icon(), Some(ui_components::sleep::ASLEEP_ICON));
    assert!(view.web_page().is_some_and(|p| !p.freeze), "kept till its frame is copied");
    shot.copied();
    assert_eq!(view.web_page(), None, "copied: its page let go");
    let scene = painted(&mut view);
    let images: Vec<_> = scene.layers().iter().flat_map(|l| l.images.iter()).collect();
    assert_eq!(images.len(), 1);
    assert_eq!(images[0].bounds, Rect::new(0.0, 0.0, 800.0, 600.0), "never stretched");
    // (Painting looked at the install; this test's is made up.)
    view.status = Status::Installed(Arc::from(Path::new("drawio")));
    // A key wakes it.
    let key = ui_components::KeyPress {
        key: ui_components::PressedKey::Char('x'),
        shift: false,
        ctrl: false,
        alt: false,
    };
    assert!(view.takes_keyboard());
    assert!(view.key_press(key));
    assert_ne!(view.page_id, page, "a page made anew");
    assert!(view.web_page().is_some_and(|p| !p.freeze));
    assert!(matches!(view.slumber, Slumber::Waking(Some(_))), "its picture till it's drawn");
    view.web_event(view.page_id, WebEvent::Shown);
    assert!(matches!(view.slumber, Slumber::Awake));
    assert!(shot.released(), "its picture let go");
    assert!(!view.takes_keyboard(), "the keys its page's again");
}

/// What it holds that isn't written, or being edited, keeps it awake (saying why); with no
/// picture to be had, it sleeps on its plain card; a theme changed while it falls asleep
/// leaves it awake.
#[test]
fn a_diagram_sleeps_only_when_nothing_would_be_lost() {
    use ui_components::sleep::{CanSleep, SLEEP_ACTION};
    let mut view = drawn_tab("careful");
    view.session.file_read(Arc::from("<a/>"));
    view.session.page_message(r#"{"event":"init"}"#);
    view.session.page_message(r#"{"event":"autosave","xml":"<b/>"}"#);
    view.tell_veto();
    assert_eq!(view.can_sleep(), CanSleep::NotNow(asleep::UNSAVED));
    assert!(view.command(SLEEP_ACTION));
    assert_eq!(view.take_notifications().len(), 1, "told why");
    assert!(matches!(view.slumber, Slumber::Awake));
    view.session.written(&Arc::from("<b/>"), true);
    let editing =
        r#"{"event":"hostState","page":0,"scale":1,"tx":0,"ty":0,"left":0,"top":0,"busy":true}"#;
    view.session.page_message(editing);
    view.tell_veto();
    assert_eq!(view.can_sleep(), CanSleep::NotNow(asleep::EDITING));
    let idle =
        r#"{"event":"hostState","page":0,"scale":1,"tx":0,"ty":0,"left":0,"top":0,"busy":false}"#;
    view.session.page_message(idle);
    view.tell_veto();
    assert_eq!(view.can_sleep(), CanSleep::Yes);
    // No picture to be had: asleep on its card alone, its page let go at once.
    assert!(view.command(SLEEP_ACTION));
    view.web_event(view.page_id, WebEvent::Picture(None));
    assert_eq!(view.web_page(), None);
    // A theme changed while falling asleep: awake (the old page's picture never comes).
    let mut view = drawn_tab("themed");
    view.command(SLEEP_ACTION);
    view.new_page();
    assert!(matches!(view.slumber, Slumber::Awake));
}

/// Used in its page (a press, a key, the wheel: told by our page), or its tab chosen, it's
/// touched: left alone counts from then.
#[test]
fn using_a_diagram_touches_it() {
    let mut view = drawn_tab("used");
    let slot = Arc::clone(view.sleep.as_ref().unwrap().slot());
    slot.lock().touched = None;
    say(&mut view, r#"{"event":"hostTouched"}"#);
    assert!(slot.lock().touched.is_some(), "used in its page");
    slot.lock().touched = None;
    view.set_active(true);
    assert!(slot.lock().touched.is_some(), "its tab chosen");
}

/// Its tab chosen before its page is drawn (as it always is) still starts it as its page
/// shows; and after a sleep its new page starts it again — it can sleep again.
#[test]
fn a_diagram_chosen_before_its_page_shows_can_sleep_and_again_after_waking() {
    use ui_components::sleep::SLEEP_ACTION;
    let file = PathBuf::from("/p/chosen.drawio");
    let mut view = DiagramWebView::in_tab(&file, Some(file.clone()));
    view.page_dir = Some(Arc::from(Path::new("page")));
    view.status = Status::Installed(Arc::from(Path::new("drawio")));
    view.entry = Arc::from("index.html");
    view.area = Rect::new(0.0, 0.0, 800.0, 600.0);
    view.set_active(true);
    view.web_event(view.page_id, WebEvent::Shown);
    let slot = Arc::clone(view.sleep.as_ref().unwrap().slot());
    assert!(slot.lock().quiet.is_some(), "how quiet it is, known");
    assert!(slot.lock().running);
    view.tell_veto();
    assert!(view.command(SLEEP_ACTION));
    view.web_event(view.page_id, WebEvent::Picture(None));
    assert!(slot.lock().quiet.is_none(), "asleep: nothing to measure");
    assert!(view.wake_by_user());
    view.web_event(view.page_id, WebEvent::Shown);
    assert!(slot.lock().quiet.is_some(), "back: measured again");
    view.tell_veto();
    assert_eq!(view.can_sleep(), ui_components::sleep::CanSleep::Yes, "it can sleep again");
}

/// Drawn, it gives the sleeping what wakes its window: an order to sleep reaches it at once.
#[test]
fn a_diagram_drawn_is_woken_for_its_orders() {
    let mut view = drawn_tab("woken");
    let woken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    let waker: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let palette = Palette::dark();
    let icons = icons::Icons::builtin();
    let mut text = text_layout::TextSystem::new();
    let mut scene = render_api::Scene::default();
    view.paint(ViewerPaint {
        bounds: Rect::new(0.0, 0.0, 800.0, 600.0),
        scale: 1.0,
        text: &mut text,
        scene: &mut scene,
        palette: &palette,
        icons: &icons,
        focused: true,
        waker: Some(waker),
    });
    let slot = Arc::clone(view.sleep.as_ref().unwrap().slot());
    // (Painting may wake it for other things: its install followed.)
    let before = woken.load(std::sync::atomic::Ordering::SeqCst);
    assert!(slot.sleep_by_hand().is_ok());
    assert_eq!(woken.load(std::sync::atomic::Ordering::SeqCst), before + 1, "its window woken");
}
