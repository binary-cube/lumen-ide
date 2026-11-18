use super::*;

fn result(path: &str) -> FileMatch {
    FileMatch { path: path.into(), score: 0, name_positions: vec![0] }
}

#[test]
fn selection_moves_within_results_and_rows_hit() {
    let mut search = FileSearch::new(FileSearchStyle::default());
    search.set_bounds(Rect::new(0.0, 0.0, 300.0, 44.0), Rect::new(0.0, 44.0, 300.0, 60.0), 1.0);
    search.input.insert("a");
    search.set_results(PathResults {
        folders: Vec::new(),
        files: vec![result("a.php"), result("src/b.php"), result("c.php")],
    });
    search.move_selection(5);
    assert_eq!(search.selected().map(|m| m.path.as_str()), Some("c.php"));
    assert!(search.scroll_y > 0.0, "the selection must stay visible");
    search.move_selection(-10);
    assert_eq!(search.selected().map(|m| m.name()), Some("a.php"));
    assert_eq!(search.result_at(Point::new(50.0, 50.0)), Some(0));
    assert_eq!(search.result_at(Point::new(50.0, 10.0)), None);
    search.clear();
    assert!(!search.is_active());
}
