use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::rules::Suggestion;
use crate::profile::testing::{Check, ProfileBuilder, ran};

fn bitmap(lossy: f64, work_mem: Option<&str>) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::BitmapHeapScan, "Bitmap Heap Scan", |n| {
            ran(n, 0.0, 30.0, 50_000.0, 1.0);
            n.actuals.exact_blocks = Some(2_000.0);
            n.actuals.lossy_blocks = Some(lossy);
        })
        .with(|p| {
            if let Some(w) = work_mem {
                p.statement.settings.push(crate::profile::Setting {
                    name: "work_mem".into(),
                    value: format!("'{w}'"),
                });
            }
        })
        .build()
}

#[test]
fn lossy_blocks_want_more_memory() {
    let found = Check::postgres("SELECT 1").rule(ID, bitmap(4_500.0, None));
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].suggestion.as_ref().and_then(Suggestion::sql),
        Some("SET work_mem = '8MB';"),
        "twice the default"
    );
    let found = Check::postgres("SELECT 1").rule(ID, bitmap(1.0, Some("64MB")));
    assert_eq!(found[0].text_of(E::Value), Some("128MB"));
}

#[test]
fn exact_blocks_only_are_fine() {
    assert!(Check::postgres("SELECT 1").rule(ID, bitmap(0.0, None)).is_empty());
    assert_eq!(lossy_bitmap_bytes("4096kB"), Some(4 << 20));
    assert_eq!(lossy_bitmap_bytes("1GB"), Some(1 << 30));
    assert_eq!(lossy_bitmap_bytes("lots"), None);
}

fn lossy_bitmap_bytes(text: &str) -> Option<u64> {
    memory_bytes(text)
}
