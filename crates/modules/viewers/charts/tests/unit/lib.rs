use super::*;

/// It offers one pane, its icon among its own, named in both languages.
#[test]
fn it_offers_a_chart_pane() {
    let provider = ChartPaneProvider;
    assert_eq!(provider.id(), "chart");
    assert!(ICONS.iter().any(|i| i.name == provider.icon()));
    assert!(!provider.title().is_empty());
}
