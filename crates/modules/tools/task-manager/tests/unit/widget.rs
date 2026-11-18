use super::*;
use workbench_model::{ResourceSnapshot, ResourceSource, ResourceUse};

/// A window's picture: the app using some memory, no GPU known.
struct Source;

impl ResourceSource for Source {
    fn snapshot(&self) -> ResourceSnapshot {
        ResourceSnapshot {
            memory: 64 * 1024 * 1024,
            parts: vec![ResourceUse::new("editor", "app")],
            ..ResourceSnapshot::default()
        }
    }
}

/// The widget: nothing without the app's resources; nothing to show before a look; then its
/// cards on glass (the GPU's only when known), each as wide as there's room for, within its
/// limits.
#[test]
fn the_resources_show_as_cards_once_looked_at() {
    assert!(ResourcesWidget.open(&WidgetContext::default()).is_none(), "no source: nothing");
    let source: Arc<dyn ResourceSource> = Arc::new(Source);
    let ctx = WidgetContext { resources: Some(Arc::clone(&source)), wake: None };
    let mut view = ResourcesWidget.open(&ctx).expect("made");
    let palette = ui_components::Palette::dark();
    let ds = Ctx::new(&palette, 1.0);
    let mut text = TextSystem::new();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let size = loop {
        if let Some(size) = view.size(&mut text, &ds, 2000.0) {
            break size;
        }
        assert!(std::time::Instant::now() < deadline, "nothing was looked at");
        std::thread::sleep(std::time::Duration::from_millis(10));
    };
    assert_eq!(size.1, WIDGET_HEIGHT);
    // What the source said, as the Task Manager writes it.
    let mut resources = Resources::open(&ctx).unwrap();
    assert!(until(|| resources.write()), "nothing was looked at");
    let values: Vec<&str> = resources.cards.iter().map(|c| c.value.as_str()).collect();
    assert_eq!(values[1], i18n::bytes(64 * 1024 * 1024), "{values:?}");
    // CPU, memory and I/O: the GPU isn't known here.
    let n = 3.0;
    assert_eq!(size.0, n * CARD_MAX + (n - 1.0) * GAP, "as wide as allowed");
    // Narrow: each card at its narrowest.
    let narrow = view.size(&mut text, &ds, 300.0).unwrap();
    assert_eq!(narrow.0, n * CARD_MIN + (n - 1.0) * GAP);
}

fn until(mut f: impl FnMut() -> bool) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        if f() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    false
}
