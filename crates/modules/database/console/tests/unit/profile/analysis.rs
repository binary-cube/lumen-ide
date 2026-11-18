use std::collections::HashMap;
use std::path::Path;

use super::*;
use database::profile::ProfileDepth;
use database::profile::node::{CondKind, OpKind};
use database::profile::rules::EvidenceKey;
use database::profile::testing::{Check, ProfileBuilder, cond, on, ran};

/// A bundled catalog's messages, by key.
fn catalog(language: &str) -> HashMap<String, String> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../domain/core/i18n/locales")
        .join(format!("{language}.toml"));
    i18n::parse_catalog(&std::fs::read_to_string(path).unwrap()).into_iter().collect()
}

/// The `{name}`s of a message (plural and select arguments included).
fn placeholders(message: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = message;
    while let Some(open) = rest.find('{') {
        rest = &rest[open + 1..];
        let end = rest.find(['}', ',']).unwrap_or(rest.len());
        out.push(rest[..end].trim().to_owned());
        rest = &rest[end..];
    }
    out
}

/// Every text a finding may use, in both languages, with only the figures findings have.
#[test]
fn every_findings_texts_are_in_both_languages() {
    let ids: Vec<&str> = EvidenceKey::ALL.iter().map(|k| k.id()).collect();
    for language in ["en", "ro"] {
        let messages = catalog(language);
        for key in rules::text_keys() {
            let message = messages.get(&key).unwrap_or_else(|| panic!("{language}: {key} missing"));
            for name in placeholders(message) {
                assert!(ids.contains(&name.as_str()), "{language}: {key} names {{{name}}}");
            }
        }
        for key in ["info", "warning", "critical"] {
            assert!(messages.contains_key(&format!("profile.severity.{key}")), "{language}: {key}");
        }
        for key in ["profile_problems", "profile_problems_critical", "profile_finding"] {
            assert!(messages.contains_key(&format!("console.{key}")), "{language}: {key}");
        }
    }
}

fn selective() -> database::profile::Profile {
    ProfileBuilder::new("postgres", ProfileDepth::Actual { timing: true })
        .node(OpKind::SeqScan, "Seq Scan", |n| {
            ran(n, 0.0, 10.0, 18.0, 1.0);
            n.actuals.rows_removed_by_filter = Some(1_204_314.0);
            on(n, "orders", None);
            cond(n, CondKind::Filter, "(customer_id = 7)");
        })
        .with(|p| p.statement.execution_ms = Some(10.0))
        .build()
}

#[test]
fn a_findings_words_carry_its_figures() {
    let found = Check::postgres("SELECT * FROM orders WHERE customer_id = 7").analyze(selective());
    let scan = found.iter().find(|f| f.rule == "seq-scan-selective").unwrap();
    let text = texts(scan);
    assert_eq!(text.title, i18n::t("profile.finding.seq-scan-selective.title"));
    let read = grouped(1_204_332);
    assert!(text.why.contains(&read), "{} has {read}", text.why);
    assert!(!text.why.contains('{'), "every argument given: {}", text.why);
    assert!(!text.fix.is_empty() && !text.fix.contains('{'));
    assert_eq!(severity(Severity::Critical), i18n::t("profile.severity.critical"));
    let summary = problems_text(&found);
    assert!(summary.contains('1'), "{summary}");
}

#[test]
fn figures_are_written_for_people() {
    let ro = i18n::locale() == "ro";
    let sep = if ro { "\u{202f}" } else { "," };
    assert_eq!(grouped(1_204_332), format!("1{sep}204{sep}332"));
    assert_eq!(grouped(-1_000), format!("-1{sep}000"));
    assert_eq!(grouped(999), "999");
    let q = |v, u| quantity(Quantity::new(v, u));
    assert_eq!(q(120.0, Unit::Ratio), "×120 ↑");
    assert_eq!(q(0.01, Unit::Ratio), if ro { "×0,01 ↓" } else { "×0.01 ↓" });
    assert_eq!(q(42.0, Unit::Percent), "42%");
    assert_eq!(q(0.5, Unit::Percent), if ro { "0,5%" } else { "0.5%" });
    assert_eq!(q(845.24, Unit::Millis), if ro { "845,2 ms" } else { "845.2 ms" });
    assert_eq!(q(2480.0, Unit::Millis), if ro { "2,48 s" } else { "2.48 s" });
    assert_eq!(q(0.042, Unit::Millis), if ro { "0,042 ms" } else { "0.042 ms" });
    assert_eq!(q(2048.0, Unit::Bytes), i18n::bytes(2048));
}

/// The facts read on a real connection: the profile's tables' structures (an alias taken
/// back to its table), their statistics only when allowed.
#[test]
fn facts_are_gathered_on_the_connection() {
    let project = crate::fake::Project::new("facts");
    project.look("CREATE INDEX orders_user ON orders (user_id)");
    let target = database::Target {
        database: project.db().to_string_lossy().into_owned(),
        ..database::Target::default()
    };
    use database::Driver;
    let mut session =
        db_sqlite::SqliteDriver.connect(&target, &database::Secret::default()).unwrap();
    let sql = "SELECT * FROM users u JOIN orders o ON o.user_id = u.id";
    let profile = session
        .profile(sql, &database::profile::ProfileOptions::ESTIMATED, &Cancel::default())
        .unwrap();
    let facts = gather(
        &mut *session,
        &profile,
        sql,
        Flavor::Sqlite,
        false,
        RelationNotes::default(),
        &Cancel::default(),
    );
    let names: Vec<&str> = facts.tables.iter().map(|t| t.table.name.as_str()).collect();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(names.contains(&"users") && names.contains(&"orders"));
    let orders = facts.tables.iter().find(|t| t.table.name == "orders").unwrap();
    assert!(orders.index_starting_with(&["user_id".into()]).is_some());
    assert!(orders.stats.is_none(), "no auxiliary queries asked");
    let facts = gather(
        &mut *session,
        &profile,
        sql,
        Flavor::Sqlite,
        true,
        RelationNotes::default(),
        &Cancel::default(),
    );
    assert!(facts.tables.iter().all(|t| t.stats.is_some()), "the statistics read");
    // Cancelled: nothing more read.
    let cancel = Cancel::default();
    cancel.cancel();
    let facts = gather(
        &mut *session,
        &profile,
        sql,
        Flavor::Sqlite,
        true,
        RelationNotes::default(),
        &cancel,
    );
    assert!(facts.tables.is_empty());
}
