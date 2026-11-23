use super::*;

fn settled(view: &mut ChartFileView) {
    let started = std::time::Instant::now();
    while view.is_loading() {
        view.poll();
        assert!(started.elapsed() < std::time::Duration::from_secs(10), "read");
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}

/// A document's tables are found on a worker; the first is drawn, the others offered.
#[test]
fn a_document_s_tables_are_found_and_the_first_drawn() {
    let mut view = ChartFileView::default();
    let json = r#"{"a": [{"x": 1, "y": 2}, {"x": 2, "y": 3}], "b": [{"k": "u", "v": 5}, {"k": "w", "v": 6}]}"#;
    view.set_data(&ChartOf { name: "data.json".into(), text: Arc::from(json) });
    settled(&mut view);
    assert_eq!(view.tables.len(), 2);
    assert_eq!(view.shown, 0);
    assert_eq!(view.tables[0].path, "$.a");
    view.show(1);
    assert_eq!(view.shown, 1);
}

/// Text that holds no table says so, never fails.
#[test]
fn text_without_a_table_says_so() {
    let mut view = ChartFileView::default();
    view.set_data(&ChartOf { name: "notes.txt".into(), text: Arc::from("just words") });
    settled(&mut view);
    assert!(view.read && view.tables.is_empty());
}
