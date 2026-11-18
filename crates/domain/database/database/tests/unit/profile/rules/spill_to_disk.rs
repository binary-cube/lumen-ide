use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::{HashStats, Memory, NodeId, OpKind, Spill, SpillKind, Storage};
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, ran};

fn one(edit: impl FnOnce(&mut crate::profile::ProfileNode)) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::Sort, "Sort", |n| {
            ran(n, 0.0, 10.0, 1000.0, 1.0);
            edit(n);
        })
        .with(|p| p.statement.execution_ms = Some(10.0))
        .build()
}

#[test]
fn a_sort_on_disk_wants_work_mem_twice_its_spill() {
    let p = one(|n| {
        n.spill = Some(Spill { kind: SpillKind::Sort, disk_bytes: Some(2_760_704), batches: None })
    });
    let found = Check::postgres("SELECT 1").rule(ID, p);
    let f = &found[0];
    assert_eq!(f.nodes, [NodeId(0)]);
    assert_eq!(f.text_of(E::Spill), Some("sort"));
    assert_eq!(f.quantity(E::Disk).unwrap().value, 2_760_704.0);
    // 2.6 MB × 2 → 8 MB.
    assert_eq!(f.suggestion.as_ref().and_then(Suggestion::sql), Some("SET work_mem = '8MB';"));
    let Some(Suggestion::Setting { name, value, .. }) = &f.suggestion else { panic!() };
    assert_eq!((name.as_str(), value.as_str()), ("work_mem", "8MB"));
}

#[test]
fn a_hash_in_batches_by_its_memory() {
    let p = one(|n| {
        n.op = OpKind::Hash;
        n.memory = Some(Memory {
            peak_bytes: Some(3_156_992),
            hash: Some(HashStats { batches: Some(16), ..HashStats::default() }),
            ..Memory::default()
        });
    });
    let found = Check::postgres("SELECT 1").rule(ID, p);
    assert_eq!(found[0].text_of(E::Spill), Some("hash"));
    assert_eq!(found[0].quantity(E::Batches).unwrap().value, 16.0);
    // 3 MB × 16 × 2 ≈ 96 MB → 128 MB.
    assert_eq!(found[0].text_of(E::Value), Some("128MB"));
}

#[test]
fn what_fit_in_memory_is_fine() {
    let p = one(|n| {
        n.memory = Some(Memory {
            sort_method: Some("quicksort".into()),
            sort_space: Some((26_000, Storage::Memory)),
            hash: Some(HashStats { batches: Some(1), ..HashStats::default() }),
            ..Memory::default()
        })
    });
    assert!(Check::postgres("SELECT 1").rule(ID, p).is_empty());
    // An external sort the driver didn't sum up: still on disk; SQLite has no work_mem.
    let p = one(|n| {
        n.memory = Some(Memory {
            sort_method: Some("external merge".into()),
            sort_space: Some((45_015_040, Storage::Disk)),
            ..Memory::default()
        })
    });
    let found = Check::postgres("SELECT 1").rule(ID, p.clone());
    assert_eq!(found[0].text_of(E::Value), Some("128MB"));
    let mut sqlite = p;
    sqlite.engine.driver = "sqlite".into();
    let found = Check::sqlite("SELECT 1").rule(ID, sqlite);
    assert!(found[0].suggestion.is_none());
}
