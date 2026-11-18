use std::time::{Duration, Instant};

use pinned_downloads::fetch::Memory;

use super::*;

fn wait_for(what: impl Fn(&Status) -> bool) -> Status {
    let until = Instant::now() + Duration::from_secs(10);
    loop {
        let now = status();
        if what(&now) || Instant::now() > until {
            return now;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Looked for, then downloaded on the worker: offline said as such, a wrong download
/// refused; the views woken at every change.
#[test]
fn it_is_looked_for_and_a_failed_download_says_why() {
    let root = std::env::temp_dir().join(format!("diagram-web-state-{}", kernel::id::new_uuid()));
    let memory = Arc::new(Memory::default());
    isolate(root.clone(), Arc::clone(&memory) as Arc<dyn Fetch>);
    let woken = Arc::new(AtomicU64::new(0));
    let counter = Arc::clone(&woken);
    let wake: Wake = Arc::new(move || {
        counter.fetch_add(1, Ordering::Relaxed);
    });
    follow(&wake);
    assert_eq!(wait_for(|s| *s == Status::Missing), Status::Missing);
    memory.set_offline(true);
    let before = generation();
    start();
    let failed = wait_for(|s| matches!(s, Status::Failed { .. }));
    assert!(matches!(failed, Status::Failed { offline: true, .. }), "{failed:?}");
    assert!(generation() > before);
    assert!(woken.load(Ordering::Relaxed) > 0);
    // Not what's pinned: refused, nothing installed.
    memory.set_offline(false);
    memory.serve(package::URL, b"not the war".to_vec());
    start();
    let failed = wait_for(|s| matches!(s, Status::Failed { offline: false, .. }));
    assert!(matches!(failed, Status::Failed { offline: false, .. }), "{failed:?}");
    assert_eq!(package::installed(&root), None);
    let _ = std::fs::remove_dir_all(root);
}
