use super::*;
use platform_api::access::Described;

/// A frame's tree is kept once made (where a screen reader's press lands); nothing is kept
/// when none was begun.
#[test]
fn the_frame_s_tree_is_kept_and_a_press_lands_in_a_node_s_middle() {
    let mut told = None;
    end(None, &mut told, None, "");
    assert!(told.is_none(), "no tree asked for");
    let recording = ui_kit::access::begin();
    let save = ui_kit::access::node(|| {
        Described::new(Role::Button, "Save", kernel::Rect::new(10.0, 20.0, 40.0, 20.0))
    })
    .expect("recording");
    end(Some((1, recording)), &mut told, None, "Window");
    let frame = &told.as_ref().expect("kept").frame;
    assert_eq!(node_middle(frame, save), Some((Role::Button, Point::new(30.0, 30.0))));
    assert_eq!(node_middle(frame, NodeId(123)), None);
}

/// Without a window wanting a tree, none is begun and the last told is let go (the next
/// screen reader gets the whole tree).
#[test]
fn no_tree_is_made_while_none_is_wanted() {
    let mut told = Some(Told::default());
    assert!(begin(None, &mut told, false).is_none());
    assert!(told.is_none());
}
