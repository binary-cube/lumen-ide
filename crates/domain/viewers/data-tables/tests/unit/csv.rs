use super::*;

/// A record of more fields than a table keeps holds only those (the rest read past, never
/// kept); a field past the most bytes stops growing.
#[test]
fn a_hostile_record_keeps_only_what_a_table_shows() {
    let limits = Limits { columns: 4, ..Limits::default() };
    let line = ",".repeat(100_000);
    let (wide, _) = records(&format!("{line}\n1,2,3,4,5\n"), ',', limits);
    assert!(
        wide.iter().all(|r| r.len() <= 4),
        "{:?}",
        wide.iter().map(Vec::len).collect::<Vec<_>>()
    );
    let huge = "x".repeat(MOST_FIELD * 2);
    let (long, _) = records(&format!("{huge},y\n"), ',', Limits::default());
    assert_eq!(long[0][0].len(), MOST_FIELD);
    assert_eq!(long[0][1], "y");
}
