use super::*;
use crate::fake::FakeRepository;

fn dialog(repo: Arc<FakeRepository>) -> SwitchDialog {
    let mut d = SwitchDialog::default();
    let request = SwitchRequest {
        repo,
        target: SwitchTarget::Branch("feature".into()),
        label: "feature".into(),
        files: vec![InTheWay { path: "a.php".into(), state: version_control::FileState::Modified }],
    };
    d.set_data(&request);
    d
}

fn settle(d: &mut SwitchDialog) {
    for _ in 0..200 {
        d.poll();
        if d.running.is_none() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn stash_and_switch_puts_the_work_aside_then_switches() {
    let repo = Arc::new(FakeRepository::default());
    let mut d = dialog(Arc::clone(&repo));
    assert!(d.go(true));
    settle(&mut d);
    assert!(d.closed());
    let pushes = repo.stash_pushes.lock().unwrap().clone();
    assert_eq!(pushes.len(), 1);
    assert!(pushes[0].untracked, "the untracked files go too");
    assert!(!repo.switched.lock().unwrap()[0].1);
    // The notification brings it back.
    let notes = d.take_notifications();
    assert_eq!(notes[0].actions[0].command, crate::POP_SWITCH_STASH);
}

#[test]
fn throwing_the_changes_away_is_asked_twice() {
    let repo = Arc::new(FakeRepository::default());
    let mut d = dialog(Arc::clone(&repo));
    d.press_hit(Hit::Discard);
    assert!(d.armed && d.running.is_none(), "armed, nothing done yet");
    d.press_hit(Hit::Discard);
    settle(&mut d);
    assert!(repo.switched.lock().unwrap().clone()[0].1, "discarding");
    assert!(repo.stash_pushes.lock().unwrap().is_empty());
}
