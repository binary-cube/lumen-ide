use super::*;
use crate::control::Control;

#[derive(Debug)]
struct Pane;

impl Control for Pane {}

impl ResultPane for Pane {
    fn set_rows(&mut self, _rows: Lent, _epoch: u64, _held: Held) {}
    fn paint(&mut self, _paint: ViewerPaint<'_>) {}
}

#[derive(Debug)]
struct Provider(&'static str);

impl ResultPaneProvider for Provider {
    fn id(&self) -> &'static str {
        self.0
    }
    fn icon(&self) -> &'static str {
        "chart"
    }
    fn title(&self) -> String {
        self.0.to_owned()
    }
    fn make(&self) -> Box<dyn ResultPane> {
        Box::new(Pane)
    }
}

/// Providers are found by id once registered, one per id, and gone once unregistered.
#[test]
fn providers_are_registered_once_per_id() {
    register(Arc::new(Provider("test-pane")));
    register(Arc::new(Provider("test-pane")));
    assert_eq!(providers().iter().filter(|p| p.id() == "test-pane").count(), 1);
    assert!(provider("test-pane").is_some());
    unregister("test-pane");
    assert!(provider("test-pane").is_none());
}
