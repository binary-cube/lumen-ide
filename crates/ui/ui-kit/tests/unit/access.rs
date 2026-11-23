use super::*;
use kernel::Rect;
use platform_api::access::Role;

/// Nothing is made while no tree is asked for (the node isn't even described); asked, the
/// nodes painted are handed over once, under the groups open as they were put.
#[test]
fn nodes_are_kept_only_while_a_tree_is_made() {
    let mut described = false;
    assert!(
        node(|| {
            described = true;
            Described::new(Role::Button, "Save", Rect::default())
        })
        .is_none()
    );
    assert!(!described, "not described while off");

    let tree = begin();
    let inner = group(
        || Described::new(Role::Dialog, "Settings", Rect::default()),
        || node(|| Described::new(Role::Button, "Close", Rect::default())),
    );
    node(|| Described::new(Role::Label, "Done", Rect::default()));
    let frame = tree.take();
    let nodes = frame.nodes();
    assert_eq!(nodes.len(), 3);
    assert_eq!(nodes[1].parent, nodes[0].id, "the button in the dialog");
    assert_eq!(Some(nodes[1].id), inner);
    assert_eq!(nodes[2].parent, platform_api::access::ROOT, "the label after it, at the root");
    assert!(!recording(), "off again once taken");
    // Begun and dropped without being taken (an early return): nothing records after.
    drop(begin());
    assert!(!recording());
}

/// A field tells its text as its value, named by its placeholder; a secret's text never
/// reaches the tree, not even its length.
#[test]
fn a_field_tells_its_text_but_never_a_secret() {
    let mut text = text_layout::TextSystem::new();
    let white = kernel::Rgba::WHITE;
    let style = crate::TextInputStyle {
        font: text_layout::Font::new(text_layout::FontFamily::SansSerif, 13.0),
        background: kernel::Rgba::TRANSPARENT,
        border: kernel::Rgba::TRANSPARENT,
        border_focused: kernel::Rgba::TRANSPARENT,
        text: white,
        placeholder: white,
        selection: white,
        caret: white,
        radius: 0.0,
        border_width: 0.0,
        padding: 0.0,
        caret_width: 1.0,
        lead: 0.0,
        trail: 0.0,
    };
    let mut layer = render_api::Layer::default();
    let mut field = crate::TextInput::default();
    field.set_text("php");
    let mut secret = crate::TextInput::default();
    secret.set_masked(true);
    secret.set_text("hunter2");
    let recording = begin();
    field.paint(&mut layer, &mut text, Rect::new(0.0, 0.0, 100.0, 20.0), true, "Search", &style);
    secret.paint(
        &mut layer,
        &mut text,
        Rect::new(0.0, 30.0, 100.0, 20.0),
        false,
        "Password",
        &style,
    );
    let frame = recording.take();
    let nodes = frame.nodes();
    assert_eq!((nodes[0].role, &*nodes[0].name), (Role::TextField, "Search"));
    assert_eq!(nodes[0].value.as_deref(), Some("php"));
    assert!(nodes[0].state.focused);
    assert_eq!(nodes[1].value, None);
    assert!(!format!("{nodes:?}").contains("hunter2"), "the secret never in the tree");
}
