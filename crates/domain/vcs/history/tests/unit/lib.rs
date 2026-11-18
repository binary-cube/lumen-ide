use super::*;

fn rev(at: u64) -> Revision {
    Revision { at, kind: RevisionKind::Saved, size: 1, hash: at }
}

#[test]
fn old_and_surplus_versions_expire_but_never_the_newest() {
    let day = 24 * 3600 * 1000;
    let retention = Retention { max_per_file: 3, ..Retention::default() };
    let now = 100 * day;
    // Newest first: today, yesterday, 2 days ago, 3 days ago (past the count).
    let list = [rev(now), rev(now - day), rev(now - 2 * day), rev(now - 3 * day)];
    let gone: Vec<u64> = retention.expired(&list, now).iter().map(|r| r.at).collect();
    assert_eq!(gone, [now - 3 * day]);
    // Everything too old, the newest stays.
    let old = [rev(now - 40 * day), rev(now - 41 * day)];
    assert_eq!(retention.expired(&old, now).len(), 1);
}

#[test]
fn hashes_are_stable() {
    assert_eq!(text_hash(b"abc"), text_hash(b"abc"));
    assert_ne!(text_hash(b"abc"), text_hash(b"abd"));
    assert_eq!(
        RevisionKind::from_code(RevisionKind::BeforeSave.code()),
        Some(RevisionKind::BeforeSave)
    );
}
