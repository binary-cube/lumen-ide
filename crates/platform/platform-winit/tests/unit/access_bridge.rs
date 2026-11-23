use super::*;
use kernel::Rect;
use platform_api::access::Described;

/// The app's tree as the system takes it: the window as root over the top nodes, each
/// node's children listed, its role, name, value, state and actions kept; the focus on the
/// focused node.
#[test]
fn the_tree_reaches_the_system_whole() {
    let mut frame = Frame::new();
    let dialog =
        frame.open(Described::new(Role::Dialog, "Settings", Rect::new(0.0, 0.0, 400.0, 300.0)));
    let switch = frame.add(
        Described::new(Role::Switch, "Auto-save", Rect::new(10.0, 10.0, 36.0, 20.0))
            .checked(true)
            .focused(true),
    );
    frame.close();
    let update = tree_update(&frame, "Window");
    assert_eq!(update.tree.as_ref().map(|t| t.root), Some(KitId(ROOT.0)));
    assert_eq!(update.focus, KitId(switch.0));
    let node = |id: NodeId| &update.nodes.iter().find(|(k, _)| k.0 == id.0).expect("a node").1;
    assert_eq!(node(ROOT).children(), [KitId(dialog.0)]);
    assert_eq!(node(dialog).children(), [KitId(switch.0)]);
    let s = node(switch);
    assert_eq!(s.role(), KitRole::Switch);
    assert_eq!(s.label(), Some("Auto-save"));
    assert_eq!(s.toggled(), Some(Toggled::True));
    assert!(s.supports_action(Action::Click));
}

/// What a screen reader asks comes back as the app's request; an action it doesn't do is
/// left.
#[test]
fn the_system_s_requests_come_back() {
    let asked = |action, data| accesskit::ActionRequest {
        action,
        target_node: KitId(7),
        data,
        target_tree: accesskit::TreeId::ROOT,
    };
    assert_eq!(request(&asked(Action::Click, None)), Some(Request::Press(NodeId(7))));
    let value = Some(accesskit::ActionData::Value("php".into()));
    assert_eq!(
        request(&asked(Action::SetValue, value)),
        Some(Request::SetValue(NodeId(7), "php".into()))
    );
    assert_eq!(request(&asked(Action::ScrollDown, None)), None);
}
