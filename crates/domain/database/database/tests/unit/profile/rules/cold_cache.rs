use super::*;
use crate::profile::node::{Io, OpKind};
use crate::profile::testing::{Check, ProfileBuilder, ran};
use crate::profile::{ProfileDepth, ProfileNote};

fn read(hit: u64, read: u64) -> crate::profile::Profile {
    let io = Io { shared_hit: Some(hit), shared_read: Some(read), ..Io::default() };
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .child(OpKind::Aggregate(crate::profile::AggStrategy::Plain), "Aggregate", |n| {
            ran(n, 0.0, 10.0, 1.0, 1.0);
            n.io = Some(io);
        })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 9.0, 1000.0, 1.0);
            n.io = Some(io);
        })
        .with(|p| p.sum_io())
        .build()
}

#[test]
fn most_blocks_read_from_disk() {
    let found = Check::postgres("SELECT 1").rule(ID, read(50, 4_950));
    assert_eq!(found.len(), 1);
    // The root's blocks (its child's are in them), not twice.
    assert_eq!(found[0].quantity(E::BlocksTouched).unwrap().value, 5_000.0);
    assert_eq!(found[0].nodes, [crate::profile::NodeId(1)], "the scan reading them");
    let mut p = read(50, 4_950);
    p.note(ProfileNote::IoTimingOff);
    assert_eq!(Check::postgres("SELECT 1").rule(ID, p)[0].variant, Some("no_io_timing"));
}

#[test]
fn a_warm_cache_or_a_few_blocks_are_fine() {
    assert!(Check::postgres("SELECT 1").rule(ID, read(4_000, 1_000)).is_empty());
    assert!(Check::postgres("SELECT 1").rule(ID, read(0, 999)).is_empty(), "under 1 000");
    assert_eq!(Check::postgres("SELECT 1").rule(ID, read(100, 900)).len(), 1, "at the limits");
}
