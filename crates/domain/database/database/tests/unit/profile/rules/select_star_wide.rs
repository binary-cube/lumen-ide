use super::*;
use crate::profile::ProfileDepth;
use crate::profile::node::OpKind;
use crate::profile::rules::{RewriteHint, Suggestion};
use crate::profile::testing::{Check, ProfileBuilder, on, planned, table};

fn scan(width: f64) -> crate::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Estimated)
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            planned(n, 0.0, 10.0, 100.0);
            n.estimates.width = Some(width);
            on(n, "docs", None);
        })
        .build()
}

#[test]
fn star_of_wide_rows() {
    let text = "SELECT * FROM docs";
    let found = Check::postgres(text).rule(ID, scan(1_200.0));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].quantity(E::Width).unwrap().value, 1_200.0);
    assert_eq!(found[0].suggestion, Some(Suggestion::Rewrite { hint: RewriteHint::ListColumns }));
    assert_eq!(&text[found[0].sql_range.clone().unwrap()], "*");
    // At the limit.
    assert_eq!(Check::postgres(text).rule(ID, scan(500.0)).len(), 1);
    assert!(Check::postgres(text).rule(ID, scan(499.0)).is_empty());
}

#[test]
fn many_columns_count_too() {
    let names: Vec<String> = (0..21).map(|i| format!("c{i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let check = Check::postgres("SELECT * FROM docs").table(table("docs", &refs, &[]));
    let found = check.rule(ID, scan(100.0));
    assert_eq!(found[0].quantity(E::ColumnCount).unwrap().value, 21.0);
    let check = Check::postgres("SELECT * FROM docs").table(table("docs", &refs[..20], &[]));
    assert!(check.rule(ID, scan(100.0)).is_empty(), "20 columns aren't too many");
}

#[test]
fn listed_columns_or_count_star_are_fine() {
    assert!(Check::postgres("SELECT id, title FROM docs").rule(ID, scan(1_200.0)).is_empty());
    assert!(Check::postgres("SELECT count(*) FROM docs").rule(ID, scan(1_200.0)).is_empty());
}
