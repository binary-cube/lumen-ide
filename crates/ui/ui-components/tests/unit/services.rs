//! The contributors' registry (one per id, by their order, removed again), the nodes' and
//! tabs' identities (UUID v4: never the same twice) and their builders.

use super::*;

#[derive(Debug)]
struct Named(&'static str, u16);

impl ServiceProvider for Named {
    fn id(&self) -> &'static str {
        self.0
    }

    fn order(&self) -> u16 {
        self.1
    }

    fn icon(&self) -> &'static str {
        "database"
    }

    fn title(&self) -> String {
        self.0.to_owned()
    }

    fn open(&self, _ctx: &ServicesContext) -> Option<Box<dyn ProjectServices>> {
        None
    }
}

#[test]
fn contributors_are_registered_once_each_by_their_order() {
    register(Arc::new(Named("test.b", 20)));
    register(Arc::new(Named("test.a", 10)));
    register(Arc::new(Named("test.b", 5)));
    let ours: Vec<(&str, u16)> = providers()
        .iter()
        .filter(|p| p.id().starts_with("test."))
        .map(|p| (p.id(), p.order()))
        .collect();
    assert_eq!(ours, [("test.b", 5), ("test.a", 10)], "the same id replaced, then by order");
    unregister("test.a");
    unregister("test.b");
    assert!(providers().iter().all(|p| !p.id().starts_with("test.")));
}

#[test]
fn identities_never_repeat_and_nodes_say_what_they_are() {
    let (a, b) = (ServiceId::new(), ServiceId::new());
    assert_ne!(a, b);
    assert_eq!(uuid_version(a), 4, "a UUID v4");
    let node = ServiceNode::new(a, "database", "shop")
        .under(b)
        .with_detail("543 ms")
        .with_state(ServiceState::TxOpen)
        .with_color("green");
    assert_eq!(node.parent, Some(b));
    assert_eq!((node.detail.as_str(), node.state), ("543 ms", ServiceState::TxOpen));
    assert_eq!(node.color.as_deref(), Some("green"));
    let tab = ServiceTab::new(a, "table", "Result 1");
    assert!(tab.closable && !tab.pinned);
    let output = ServiceTab::new(b, "file-text", "Output").fixed().pinned(true);
    assert!(!output.closable && output.pinned);
}

/// The version nibble of a UUID.
fn uuid_version(id: ServiceId) -> u128 {
    (id.0 >> 76) & 0xf
}

#[test]
fn the_window_says_whether_it_is_shown_per_project() {
    let (a, b) = (ProjectId::new(), ProjectId::new());
    assert!(!shown(a));
    set_shown(a, true);
    set_shown(a, true);
    assert!(shown(a) && !shown(b));
    set_shown(a, false);
    assert!(!shown(a));
}
