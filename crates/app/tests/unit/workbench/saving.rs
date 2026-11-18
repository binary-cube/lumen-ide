use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::workbench::{PhysicalSize, Ports, TextSystem};

/// A workbench with a waker (as in the app: saves on the worker) and `a.txt` open in a
/// scratch folder, removed at the end.
struct Scratch {
    wb: Workbench,
    file: PathBuf,
    dir: PathBuf,
    woken: Arc<AtomicUsize>,
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn scratch(content: &str) -> Scratch {
    let (support, _) = crate::modules::load(&extensions::ModuleSelection::default());
    let mut wb = Workbench::new(Arc::new(support), Ports::local());
    wb.forget_projects();
    wb.resize(PhysicalSize::new(1200, 800), 1.0);
    let woken = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    wb.set_waker(Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    }));
    let dir = std::env::temp_dir().join(format!("saving-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("a.txt");
    std::fs::write(&file, content).unwrap();
    wb.open_file(&file).unwrap();
    Scratch { wb, file, dir, woken }
}

/// The saves told once the worker wrote them (as the app's wake would).
fn settle(s: &mut Scratch) {
    let started = std::time::Instant::now();
    while !s.wb.saving.running.is_empty() && started.elapsed() < Duration::from_secs(10) {
        std::thread::sleep(Duration::from_millis(2));
        s.wb.poll_saves();
    }
}

/// An automatic save is written on the worker and told by a poll: the file has the text,
/// the tab is saved, the app was woken.
#[test]
fn an_autosave_is_written_on_the_worker() {
    let mut s = scratch("x");
    let mut text = TextSystem::new();
    s.wb.type_text(&mut text, "y");
    assert!(s.wb.autosave(true));
    settle(&mut s);
    assert_eq!(std::fs::read_to_string(&s.file).unwrap(), "yx\n");
    assert!(!s.wb.tabs[s.wb.active].doc.is_dirty());
    assert!(s.woken.load(Ordering::SeqCst) > 0, "the app woken for it");
}

/// Typed on while the worker writes: what was written is saved, what came after isn't —
/// the tab stays unsaved, and the next pause writes the rest.
#[test]
fn typing_during_an_autosave_stays_unsaved() {
    let mut s = scratch("x");
    let mut text = TextSystem::new();
    s.wb.type_text(&mut text, "y");
    assert!(s.wb.autosave(true));
    s.wb.type_text(&mut text, "z");
    settle(&mut s);
    assert_eq!(std::fs::read_to_string(&s.file).unwrap(), "yx\n");
    assert!(s.wb.tabs[s.wb.active].doc.is_dirty(), "typed after what was written");
    assert!(s.wb.autosave(true));
    settle(&mut s);
    assert_eq!(std::fs::read_to_string(&s.file).unwrap(), "yzx\n");
    assert!(!s.wb.tabs[s.wb.active].doc.is_dirty());
}

/// The user's own save while an automatic one is under way: that one finishes first, then
/// the user's writes what's there now (never an older version last).
#[test]
fn a_save_waits_for_the_autosave_under_way() {
    let mut s = scratch("x");
    let mut text = TextSystem::new();
    s.wb.type_text(&mut text, "y");
    assert!(s.wb.autosave(true));
    s.wb.type_text(&mut text, "z");
    let active = s.wb.active;
    s.wb.save_tab(active, None).unwrap();
    assert!(s.wb.saving.running.is_empty());
    assert_eq!(std::fs::read_to_string(&s.file).unwrap(), "yzx\n");
    assert!(!s.wb.tabs[active].doc.is_dirty());
}

/// The watcher's report of our own write, heard before the worker's result: not a change on
/// disk (the tab isn't reloaded nor marked changed).
#[test]
fn the_report_of_our_own_write_is_not_a_change() {
    let mut s = scratch("x");
    let mut text = TextSystem::new();
    s.wb.type_text(&mut text, "y");
    // The tab's own path (the folder's, resolved: `/private/var/…` on macOS).
    let path = s.wb.tabs[s.wb.active].doc.path().unwrap().to_path_buf();
    assert!(s.wb.autosave(true));
    assert!(s.wb.saving.is_running(&path));
    // While it runs.
    assert!(!s.wb.reload_written(std::slice::from_ref(&path)), "nothing read again");
    settle(&mut s);
    assert!(!s.wb.reload_written(std::slice::from_ref(&path)), "its stamp is the one written");
    let tab = &s.wb.tabs[s.wb.active];
    assert!(!tab.disk_changed);
    assert_eq!(tab.doc.buffer().to_string(), "yx\n");
}
