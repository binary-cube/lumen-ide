use super::*;

/// Two pictures compared: the pixels that differ (any channel) and by how much at most.
#[test]
fn pictures_differ_by_their_pixels() {
    let a = [10, 20, 30, 255, 0, 0, 0, 255, 5, 5, 5, 255];
    let mut b = a;
    assert_eq!(difference(&a, &b), Difference::default());
    b[1] = 23;
    b[8] = 0;
    assert_eq!(difference(&a, &b), Difference { pixels: 2, most: 5 });
}

/// A tree written as text: a node a line, under its parent, with its state.
#[test]
fn a_tree_reads_as_indented_lines() {
    use platform_api::access::{Described, Frame, Role};
    let mut frame = Frame::new();
    frame.open(Described::new(Role::Dialog, "Settings", kernel::Rect::default()));
    frame.add(Described::new(Role::Switch, "Auto-save", kernel::Rect::default()).checked(true));
    frame.close();
    assert_eq!(access_text(&frame), "Dialog \"Settings\"\n  Switch \"Auto-save\" checked\n");
}
