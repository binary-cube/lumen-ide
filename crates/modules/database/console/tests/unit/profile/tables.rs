//! A profile's tables: one row per table with how it was read, its rows and time, its
//! problems, the server's statistics of it and its indexes under it (used here, never used);
//! only the columns something has; SQLite's statistics present or not.

use super::*;
use database::profile::rules::Facts;
use database::profile::testing::table;

fn sample(stats: bool) -> (std::sync::Arc<ProfileReport>, Facts) {
    crate::profile::sample::report_with(crate::profile::sample::SQL, stats)
}

fn statement() -> String {
    let text = crate::profile::sample::SQL;
    text[text.find("SELECT").unwrap()..].to_owned()
}

const PG: Flavor = Flavor::Postgres { version: Some(170_002) };

fn texts(t: &TablesTable, row: usize) -> Vec<String> {
    t.rows[row].cells.iter().map(TableCell::plain).collect()
}

#[test]
fn each_table_with_its_reads_its_statistics_and_its_indexes() {
    let (report, facts) = sample(true);
    let findings: Vec<&Finding> = report.findings.iter().collect();
    let t = build(&report, &statement(), PG, &findings, Some(&facts));
    assert!(t.has_stats);
    assert_eq!(t.cols, TCol::ORDER.to_vec(), "every figure known");
    // orders first (the heaviest), its two indexes under it; then customers and its key.
    assert_eq!(
        t.kinds,
        [
            TablesRow::Table(0),
            TablesRow::Index(0, "orders_pkey".into()),
            TablesRow::Index(0, "idx_orders_created".into()),
            TablesRow::Table(1),
            TablesRow::Index(1, "customers_pkey".into()),
        ]
    );
    let orders = texts(&t, 0);
    assert!(orders[0].starts_with("public.orders"), "{orders:?}");
    assert!(orders[0].contains('o'), "its alias");
    assert_eq!(orders[1], i18n::t("profile.tables.access.scan"));
    let u = &t.usage[0];
    assert_eq!(
        orders[2],
        format!(
            "{}→{}",
            figures::count(u.rows_read.unwrap()),
            figures::count(u.rows_returned.unwrap())
        ),
        "read → kept"
    );
    assert_eq!(u.rows_returned, Some(6133.0 * 3.0), "in all its loops");
    assert_eq!(orders[4], figures::count(412_480.0));
    assert_eq!(orders[8], "2026-10-29 03:12", "its autoanalyze, shorter");
    // The worst problem's sign, the row tinted.
    assert!(t.rows[0].sign.is_some() && t.rows[0].tint.is_some());
    // An index never used, one used by the plan? (The sample reads both tables whole.)
    let never = texts(&t, 2);
    assert_eq!(never[1], i18n::t("profile.tables.never_used"));
    assert_eq!(t.rows[2].parent, Some(0));
    assert_eq!(t.rows[1].icon, Some("index-unique"), "unique");
    assert!(texts(&t, 1)[1].is_empty(), "scanned elsewhere, not here: nothing said");
}

#[test]
fn without_statistics_only_what_the_profile_knows() {
    let (report, facts) = sample(false);
    let findings: Vec<&Finding> = report.findings.iter().collect();
    let t = build(&report, &statement(), PG, &findings, Some(&facts));
    assert!(!t.has_stats);
    assert_eq!(t.cols, [TCol::Table, TCol::Access, TCol::Rows, TCol::Share]);
    // Its indexes from its structure.
    assert!(t.kinds.contains(&TablesRow::Index(0, "orders_pkey".into())));
    // No facts at all: the tables, no indexes.
    let t = build(&report, &statement(), PG, &findings, None);
    assert_eq!(t.kinds, [TablesRow::Table(0), TablesRow::Table(1)]);
}

#[test]
fn an_index_used_here_and_sqlites_statistics() {
    let (after, _) = crate::profile::sample::indexed(crate::profile::sample::SQL);
    let mut facts = Facts::default();
    let mut orders = table("public.orders", &["id:integer", "status"], &[]);
    orders.stats = Some(database::profile::TableStats {
        stats_present: Some(false),
        indexes: vec![database::profile::IndexStats {
            name: "idx_orders_status_created".into(),
            columns: vec!["status".into(), "created_at".into()],
            unique: false,
            bytes: None,
            scans: Some(0),
        }],
        ..database::profile::TableStats::new(orders.table.clone())
    });
    facts.tables.push(orders);
    let findings: Vec<&Finding> = after.findings.iter().collect();
    let t = build(&after, &statement(), PG, &findings, Some(&facts));
    let index = t.kinds.iter().position(|k| matches!(k, TablesRow::Index(..))).unwrap();
    assert_eq!(texts(&t, index)[1], i18n::t("profile.tables.used_here"), "before never used");
    let of = |t: &TablesTable, name: &str| {
        let u = t.usage.iter().position(|u| u.table.name == name).unwrap();
        t.kinds.iter().position(|k| *k == TablesRow::Table(u)).unwrap()
    };
    let orders = of(&t, "orders");
    let analyzed = t.cols.iter().position(|c| *c == TCol::Analyzed).unwrap();
    assert_eq!(texts(&t, orders)[analyzed], i18n::t("profile.tables.stats_missing"));
    let present = Facts {
        tables: vec![{
            let mut f = table("public.orders", &["id:integer"], &[]);
            f.stats = Some(database::profile::TableStats {
                stats_present: Some(true),
                ..database::profile::TableStats::new(f.table.clone())
            });
            f
        }],
        ..Facts::default()
    };
    let t = build(&after, &statement(), PG, &findings, Some(&present));
    let analyzed = t.cols.iter().position(|c| *c == TCol::Analyzed).unwrap();
    let orders = of(&t, "orders");
    assert_eq!(texts(&t, orders)[analyzed], i18n::t("profile.tables.stats_present"));
    assert!(texts(&t, orders)[1].contains("idx_orders_status_created"), "the index it read");
}

#[test]
fn a_servers_time_is_shortened() {
    assert_eq!(short_time("2026-10-29 03:12:44.123+00"), "2026-10-29 03:12");
    assert_eq!(short_time("2026-10-29T03:12:44Z"), "2026-10-29 03:12");
    assert_eq!(short_time("yesterday"), "yesterday");
}

#[test]
fn the_columns_that_dont_fit_are_left_out_by_importance() {
    let all = TCol::ORDER.to_vec();
    assert_eq!(fitting(&all, 5000.0), all);
    let narrow = fitting(&all, 920.0);
    let sum: f32 = narrow.iter().map(|c| c.width()).sum();
    assert!(sum + TABLE_MIN <= 920.0, "{narrow:?}");
    assert!(narrow.contains(&TCol::Share) && narrow.contains(&TCol::Rows));
    assert!(!narrow.contains(&TCol::SeqScans), "the least important first");
    assert_eq!(fitting(&all, 100.0), [TCol::Table, TCol::Access], "never those two");
    // What's left out is in the table's tooltip.
    let (report, facts) = sample(true);
    let findings: Vec<&Finding> = report.findings.iter().collect();
    let t = build(&report, &statement(), PG, &findings, Some(&facts));
    let (columns, rows) = t.narrowed(&narrow);
    assert_eq!(columns.len(), narrow.len());
    assert!(rows.iter().all(|r| r.cells.len() == narrow.len()));
    let tip = t.rows[0].cells[0].tip.clone().unwrap();
    assert!(tip.contains(&figures::count(9_802_113.0)), "{tip}");
}
