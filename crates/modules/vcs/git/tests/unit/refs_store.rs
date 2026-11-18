//! A repository's references, read once for all its views and read again when they move.

use std::sync::atomic::AtomicUsize;

use super::*;
use crate::fake::FakeRepository;
use version_control::SyncState;

fn wait(what: &dyn Fn() -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while !what() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// Two views of a repository hold the same copy, read once in the background; its branches'
/// moving (a push) reads it again, every view woken.
#[test]
fn every_view_of_a_repository_shares_its_references() {
    let root = std::env::temp_dir().join(format!("refs-store-{}", kernel::id::new_uuid()));
    let state = SyncState {
        branch: "main".into(),
        upstream: Some("origin/main".into()),
        ahead: 1,
        behind: 0,
    };
    let repo: Arc<dyn Repository> = Arc::new(FakeRepository {
        root: Some(root.clone()),
        branch_states: vec![state.clone()],
        ..FakeRepository::default()
    });
    let woken = Arc::new(AtomicUsize::new(0));
    let count = Arc::clone(&woken);
    let wake: Wake = Arc::new(move || {
        count.fetch_add(1, Ordering::SeqCst);
    });
    let log = of(&repo, Some(Arc::clone(&wake)));
    let commit = of(&repo, None);
    assert!(Arc::ptr_eq(&log, &commit), "one copy for both");
    wait(&|| log.current().0 > 0);
    let (number, read) = log.current();
    assert_eq!(read.unwrap().states, [state]);
    let before = woken.load(Ordering::SeqCst);
    moved(&root);
    wait(&|| woken.load(Ordering::SeqCst) > before);
    assert!(woken.load(Ordering::SeqCst) > before, "read again, the views woken");
    assert_eq!(log.current().0, number, "the same as before: nothing new to take");
    drop((log, commit));
    let again = of(&repo, None);
    assert!(again.wakes.lock().unwrap().is_empty(), "let go with its views: a new copy");
}
