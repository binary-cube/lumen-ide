use super::*;

/// Sealed and opened back for the same owner; another owner, a changed byte, another key:
/// refused. Two seals of the same text differ (a fresh nonce each).
#[test]
fn sealed_data_opens_only_with_its_key_and_owner() {
    let key = RunKey::new().unwrap();
    let plain = b"history rows".repeat(100);
    let sealed = key.seal(b"owner-1", &plain).unwrap();
    assert!(!sealed.windows(12).any(|w| w == b"history rows"), "nothing readable");
    assert_eq!(key.open(b"owner-1", &sealed).as_deref(), Some(&plain[..]));
    assert_eq!(key.open(b"owner-2", &sealed), None, "another's");
    let mut changed = sealed.clone();
    changed[20] ^= 1;
    assert_eq!(key.open(b"owner-1", &changed), None, "changed");
    assert_eq!(RunKey::new().unwrap().open(b"owner-1", &sealed), None, "another run's key");
    assert_ne!(key.seal(b"owner-1", &plain).unwrap(), sealed, "a fresh nonce");
    assert_eq!(key.open(b"owner-1", &sealed[..5]), None, "too short");
    assert!(RunKey::shared().is_some());
}
