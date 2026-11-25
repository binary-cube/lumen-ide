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

/// A text cut to its room (scrolled in a field, cut with "…") is read whole, where it shows.
#[test]
fn a_text_cut_short_is_read_whole() {
    let mut text = text_layout::TextSystem::new();
    let font = text_layout::Font::new(text_layout::FontFamily::SansSerif, 13.0);
    let line = text.shape_line("a rather long name", &font);
    let mut layer = render_api::Layer::default();
    let tree = begin();
    crate::draw_text_within(&mut layer, &line, 10.0, 20.0, kernel::Rgba::WHITE, (10.0, 40.0));
    let frame = tree.take();
    let label = &frame.nodes()[0];
    assert_eq!(&*label.name, "a rather long name");
    assert!((label.bounds.size.width - 30.0).abs() < 0.5, "where it shows");
}

/// Nodes naming their parents, walked depth first, nest as they say: a node after a deeper
/// one goes back up to its parent; one with no parent among them is under the group open.
#[test]
fn nested_nodes_follow_their_parents() {
    let tree = begin();
    let label = |name: &str| Described::new(Role::Label, name, Rect::default());
    group(
        || Described::new(Role::Group, "page", Rect::default()),
        || {
            nested([
                (1, None, label("list")),
                (2, Some(1), label("item")),
                (3, Some(2), label("text")),
                (4, Some(1), label("item 2")),
                (5, None, label("after")),
            ]);
        },
    );
    node(|| label("outside"));
    let frame = tree.take();
    let nodes = frame.nodes();
    let id = |name: &str| nodes.iter().find(|n| &*n.name == name).map(|n| n.id).unwrap();
    let parent = |name: &str| nodes.iter().find(|n| &*n.name == name).map(|n| n.parent).unwrap();
    assert_eq!(parent("list"), id("page"));
    assert_eq!(parent("item"), id("list"));
    assert_eq!(parent("text"), id("item"));
    assert_eq!(parent("item 2"), id("list"), "back up after the deeper one");
    assert_eq!(parent("after"), id("page"));
    assert_eq!(parent("outside"), platform_api::access::ROOT, "every group closed");
}

/// A node naming a parent not among the nodes open now (one never put, or one already
/// closed by a node not under it) goes under the group open, as a node with none does.
#[test]
fn a_node_whose_parent_is_not_open_goes_under_the_group() {
    let tree = begin();
    // Groups, not labels: labels side by side on a line are joined as one text.
    let part = |name: &str| Described::new(Role::Group, name, Rect::default());
    group(
        || Described::new(Role::Group, "page", Rect::default()),
        || {
            nested([
                (1, None, part("list")),
                (2, Some(1), part("item")),
                (3, Some(99), part("orphan")),
                (4, Some(1), part("late")),
                (5, Some(4), part("inner")),
            ]);
        },
    );
    let frame = tree.take();
    let nodes = frame.nodes();
    let id = |name: &str| nodes.iter().find(|n| &*n.name == name).map(|n| n.id).unwrap();
    let parent = |name: &str| nodes.iter().find(|n| &*n.name == name).map(|n| n.parent).unwrap();
    assert_eq!(parent("item"), id("list"));
    assert_eq!(parent("orphan"), id("page"), "its parent never put: under the group");
    assert_eq!(parent("late"), id("page"), "its parent closed by the orphan: under the group");
    assert_eq!(parent("inner"), id("late"), "the nodes after it nest under it again");
}
