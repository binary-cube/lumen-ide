use super::*;

#[test]
fn the_manifest_round_trips_as_text() {
    let mut m = Manifest::default();
    let s = Synced {
        remote_size: 10,
        remote_modified: 1_700_000_000,
        local_size: 10,
        local_modified: 1_700_000_001,
        local_hash: 0xabc,
    };
    m.set("app/a b.php", s);
    let back = Manifest::from_text(&m.to_text());
    assert_eq!(back, m);
    assert_eq!(Manifest::from_text("broken\tline\n").len(), 0);
}
