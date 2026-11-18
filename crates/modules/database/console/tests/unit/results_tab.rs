use super::*;

/// Opened for no known project, it stays empty and does nothing, whatever it's asked.
#[test]
fn without_results_it_shows_nothing_and_takes_nothing() {
    let mut tab = ResultsTab::default();
    tab.set_data(&ResultsData { project: kernel::ProjectId::new() });
    assert!(!tab.poll());
    assert!(tab.take_action().is_none());
    assert!(!tab.scroll(0.0, 10.0));
    assert_eq!(
        ResultsProvider.kind(Path::new("a.view-console-results")),
        Some(ViewerKind::Replace)
    );
    assert_eq!(ResultsProvider.kind(Path::new("a.sql")), None);
}
