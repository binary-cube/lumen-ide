//! The exports: the text as EXPLAIN writes it, the engine's JSON formatted (none kept: none),
//! our JSON, the Markdown for a ticket (the summary, the findings, their SQL).

use super::*;

#[test]
fn a_profile_is_exported_four_ways() {
    let report = crate::profile::sample::report(crate::profile::sample::SQL);
    assert!(text(&report).contains("Parallel Seq Scan"));
    let ours = json(&report);
    assert!(ours.trim_start().starts_with('{') && ours.contains("\"format\""), "{ours}");
    assert!(engine_json(&report).is_none(), "none kept");
    assert!(!ExportKind::EngineJson.available(&report));
    let findings: Vec<&Finding> = report.findings.iter().collect();
    let md = markdown(&report, "SELECT 1", &findings);
    assert!(md.starts_with("## ") && md.contains("```sql\nSELECT 1\n```"));
    assert!(md.contains("CREATE INDEX"), "{md}");
    for f in &findings {
        assert!(md.contains(&crate::profile::analysis::texts(f).title));
    }
    // The engine's answer formatted.
    let mut profile = (*report.profile).clone();
    profile.keep_raw(
        database::profile::RawFormat::Json,
        "[{\"Plan\":{\"Node Type\":\"Result\"}}]",
        1 << 20,
    );
    let raw = database::profile::derive(profile);
    let pretty = engine_json(&raw).unwrap();
    assert!(pretty.contains("\n") && pretty.contains("\"Node Type\": \"Result\""), "{pretty}");
    assert_eq!(file_name("Profile · orders"), "profile-orders.json");
    assert_eq!(ExportKind::from_id("markdown"), Some(ExportKind::Markdown));
}
