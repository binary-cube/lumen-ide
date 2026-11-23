use super::*;
use platform_api::access::Described;

/// A frame's tree is kept once made (where a screen reader's press lands); nothing is kept
/// when none was begun.
#[test]
fn the_frame_s_tree_is_kept_and_a_press_lands_in_a_node_s_middle_or_its_caret() {
    let mut told = None;
    end(None, &mut told, None, "", |_| None);
    assert!(told.is_none(), "no tree asked for");
    let recording = ui_kit::access::begin();
    let save = ui_kit::access::node(|| {
        Described::new(Role::Button, "Save", kernel::Rect::new(10.0, 20.0, 40.0, 20.0))
    })
    .expect("recording");
    let field = ui_kit::access::node(|| {
        let lines = platform_api::access::TextLines {
            lines: vec![kernel::Rect::new(0.0, 100.0, 200.0, 20.0)],
            anchor: 2,
            focus: 2,
            caret: Some(kernel::Rect::new(40.0, 102.0, 2.0, 16.0)),
        };
        Described::new(Role::TextArea, "Code", kernel::Rect::new(0.0, 100.0, 200.0, 400.0))
            .value("ab")
            .lines(lines)
            .focused(true)
    })
    .expect("recording");
    end(Some((1, recording)), &mut told, None, "Window", |_| None);
    let frame = &told.as_ref().expect("kept").frame;
    assert_eq!(press_point(frame, save), Some((Role::Button, Point::new(30.0, 30.0), false)));
    assert_eq!(
        press_point(frame, field),
        Some((Role::TextArea, Point::new(41.0, 110.0), true)),
        "on its caret, which stays"
    );
    assert_eq!(press_point(frame, NodeId(123)), None);
}

/// Without a window wanting a tree, none is begun and the last told is let go (the next
/// screen reader gets the whole tree).
#[test]
fn no_tree_is_made_while_none_is_wanted() {
    let mut told =
        Some(Told { asked: 1, title: String::new(), frame: Frame::default(), at: Instant::now() });
    assert!(begin(None, &mut told, false).is_none());
    assert!(told.is_none());
}

/// While the screen keeps changing, a tree is told at most ten times a second (the moment
/// the change held back is due given back); a moved focus is told at once.
#[test]
fn trees_are_told_at_most_ten_a_second_but_a_focus_at_once() {
    let tree = |name: &str, focused: bool| {
        let recording = ui_kit::access::begin();
        ui_kit::access::node(|| {
            Described::new(Role::Button, name, kernel::Rect::new(0.0, 0.0, 40.0, 20.0))
                .focused(focused)
        });
        Some((1, recording))
    };
    let mut told = None;
    assert_eq!(end(tree("A", false), &mut told, None, "W", |_| None), None, "the first: at once");
    let due = end(tree("B", false), &mut told, None, "W", |_| None);
    assert!(due.is_some(), "a change right after: held back, its moment given");
    let first = told.as_ref().map(|t| t.at).expect("told");
    assert_eq!(due, Some(first + TELL_EVERY));
    assert_eq!(end(tree("A", true), &mut told, None, "W", |_| None), None, "a focus: at once");
    assert!(told.as_ref().is_some_and(|t| t.frame.focused().is_some()));
}
