use super::Channel;

/// One host only: while one holds the channel, another's claim finds it taken; once it's
/// gone, the channel is free again.
#[test]
fn only_one_host_holds_the_channel() {
    let dir = std::env::temp_dir().join(format!("wh-{}", uuid::Uuid::new_v4().simple()));
    let channel = Channel { name: "web-host-claim".into(), dir: dir.clone() };
    let claim = channel.claim().unwrap().expect("nobody holds it");
    assert!(channel.claim().unwrap().is_none(), "one host only");
    drop(claim);
    assert!(channel.claim().unwrap().is_some(), "free again");
    let _ = std::fs::remove_dir_all(dir);
}
