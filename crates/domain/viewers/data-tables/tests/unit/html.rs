use super::*;

/// A page of many scripts is read in one pass (each end found in place, its case ignored);
/// a table's rows and cells past what's kept are left out as they come, and counted.
#[test]
#[allow(clippy::disallowed_methods)] // The budget is measured by the test, not the domain.
fn a_hostile_page_is_read_in_bounds() {
    let scripts = "<SCRIPT>x</Script>".repeat(200_000);
    let page = format!("{scripts}<table><tr><th>a</th></tr><tr><td>1</td></tr></table>");
    let started = std::time::Instant::now();
    let found = tables(&page, Limits::default());
    assert!(started.elapsed() < std::time::Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(found.len(), 1);
    let limits = Limits { rows: 3, columns: 2, ..Limits::default() };
    let rows = "<tr><td colspan=64>1</td><td>2</td><td>3</td></tr>".repeat(10);
    let found = tables(&format!("<table><tr><th>a</th><th>b</th></tr>{rows}</table>"), limits);
    let table = found.first().expect("a table");
    assert_eq!(table.columns.len(), 2);
    assert_eq!(table.rows.len(), 3);
    assert_eq!(table.left_out, 7, "the rows past the three kept");
}

/// The end of a script found whatever its case, at its place in the text as written.
#[test]
fn an_end_tag_is_found_ignoring_case() {
    assert_eq!(find_ignoring_case("ab</ScRiPt>", "</script"), Some(2));
    assert_eq!(find_ignoring_case("ăb</style", "</style"), Some(3));
    assert_eq!(find_ignoring_case("none", "</style"), None);
}

/// Tables nested past the depth are left out whole: their ends close nothing kept, the
/// table around them keeps its rows.
#[test]
fn tables_past_the_depth_close_nothing_kept() {
    let limits = Limits { depth: 1, ..Limits::default() };
    let page = "<table><tr><td>a</td></tr><tr><td><table><tr><td>x</td></tr></table></td></tr><tr><td>b</td></tr></table>";
    let found = tables(page, limits);
    assert_eq!(found.len(), 1);
    let cells: Vec<String> = found[0].rows.iter().map(|r| format!("{:?}", r[0])).collect();
    assert_eq!(
        cells,
        ["Text(\"a\")", "Text(\"x\")", "Text(\"b\")"],
        "the inner table read into its cell"
    );
}
