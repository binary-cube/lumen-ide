use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

/// What's to tell is told once, by the first window to look; the windows are woken for it.
#[test]
fn a_tell_is_taken_once_and_wakes_the_windows() {
    let _serial = serial();
    let woken = Arc::new(AtomicUsize::new(0));
    let w = Arc::clone(&woken);
    assert!(
        add_wake(
            kernel::id::new_uuid(),
            Arc::new(move || {
                w.fetch_add(1, Ordering::Relaxed);
            })
        ),
        "the first window: the looks begin"
    );
    assert!(!add_wake(kernel::id::new_uuid(), Arc::new(|| {})), "another: they had begun");
    tell(Tell::UpToDate);
    assert!(woken.load(Ordering::Relaxed) >= 1);
    assert_eq!(take_tell(), Some(Tell::UpToDate));
    assert_eq!(take_tell(), None, "told once");
}

/// The update's notification keeps its identity (updated, not shown again) until it's
/// forgotten.
#[test]
fn the_update_s_notification_keeps_its_identity() {
    let _serial = serial();
    let (first, new) = notification();
    assert!(new);
    assert_eq!(notification(), (first, false));
    assert_eq!(forget_notification(), Some(first));
    assert_ne!(notification().0, first);
}

/// A look's result never overwrites a download begun meanwhile; asked while it downloads,
/// the user is told where it is.
#[test]
fn a_look_never_overwrites_a_download_begun_meanwhile() {
    let _serial = serial();
    set(Phase::Available(crate::tests::found()), None);
    assert!(begin_look(false));
    let (_, stop) = begin_download().expect("from a version found");
    assert!(begin_download().is_none(), "one download at a time");
    end_look(Looked::UpToDate);
    assert!(matches!(phase(), Phase::Downloading { .. }), "the download kept");
    assert!(!begin_look(true));
    assert_eq!(take_tell(), Some(Tell::Progress), "asked: told where it is");
    assert!(!stop.load(Ordering::Relaxed));
}

/// A version let go while it downloads: the download stops, and its end makes nothing
/// ready (nor says anything).
#[test]
fn a_version_let_go_while_downloading_never_becomes_ready() {
    let _serial = serial();
    set(Phase::Available(crate::tests::found()), None);
    let (available, stop) = begin_download().unwrap();
    let _ = take_tell();
    let_go();
    assert!(stop.load(Ordering::Relaxed), "the download told to stop");
    let staged = Staged { dir: "/u".into(), version: "2.0.0".into(), app: "/u/app".into() };
    end_download(&available.release.version, Ok(staged));
    assert!(matches!(phase(), Phase::Idle));
    assert_eq!(take_tell(), None);
}

/// What's New open: it shows the download, so its notifications wait; closed while the
/// download goes on, its notification is told then.
#[test]
fn what_s_new_open_holds_the_download_s_notifications() {
    let _serial = serial();
    let available = crate::tests::found();
    set(Phase::Downloading { available, received: 1, total: 4 }, None);
    notes_open(true);
    assert!(is_notes_open());
    assert_eq!(take_tell(), None, "nothing told on opening");
    notes_open(false);
    assert!(!is_notes_open());
    assert_eq!(take_tell(), Some(Tell::Progress), "its notification follows the download");
    // Nothing going on: closing tells nothing.
    set(Phase::Idle, None);
    notes_open(true);
    notes_open(false);
    assert_eq!(take_tell(), None);
}

/// A look the user asked for: its answer is told in the window showing the spinner, not
/// the first window to ask; another one takes it only once the spinner has waited long.
#[test]
fn the_answer_goes_to_the_window_showing_the_spinner() {
    let _serial = serial();
    let (spinner, _) = notification();
    spinning(spinner);
    tell(Tell::UpToDate);
    assert_eq!(take_tell_for(|_| false, true), None, "another window: not its answer");
    assert_eq!(take_tell_for(|id| id == spinner, false), Some(Tell::UpToDate), "the asking one");
    // Taken: the next tell goes to any window.
    tell(Tell::Progress);
    assert_eq!(take_tell_for(|_| false, true), Some(Tell::Progress));
}

/// Nothing asked for: the focused window tells it; an unfocused one only once it waited
/// (the app in the background).
#[test]
fn what_s_found_is_told_in_the_focused_window() {
    let _serial = serial();
    tell(Tell::Available);
    assert_eq!(take_tell_for(|_| false, false), None, "not the window the user is in");
    assert_eq!(take_tell_for(|_| false, true), Some(Tell::Available), "the focused one");
}
