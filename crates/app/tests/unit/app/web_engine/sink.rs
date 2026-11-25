//! A page's sink: when it's shown, what of a frame changed.

use super::*;

/// A page is shown once, when both its content loaded and a frame came, in either order
/// (a page still after it loads draws no frame more).
#[test]
fn a_page_is_shown_once_loaded_and_drawn() {
    let frame_first = Showing::default();
    assert!(!frame_first.framed(), "the blank page's frame");
    assert!(frame_first.loaded(true), "loaded with a frame in");
    assert!(!frame_first.framed(), "once");
    let load_first = Showing::default();
    assert!(!load_first.loaded(false));
    assert!(load_first.framed());
    assert!(!load_first.loaded(true) && !load_first.framed());
}

/// What changed in a frame, as the engine tells it (untrusted): cut to the frame, empty
/// ones dropped; more than the protocol allows is all of it.
#[test]
fn what_changed_stays_within_the_frame() {
    use web_engine::protocol::{Changed, MAX_CHANGED};
    let rect = |x, y, width, height| Changed { x, y, width, height };
    let within = changed_within(&[rect(10, 10, 500, 20), rect(300, 0, 5, 5)], (100, 50));
    assert_eq!(within, [render_api::PixelRect::new(10, 10, 100, 30)], "cut, the one past it gone");
    let huge = changed_within(&[rect(u32::MAX, u32::MAX, u32::MAX, u32::MAX)], (100, 50));
    assert!(huge.is_empty(), "nothing left: all of it");
    let many = vec![rect(0, 0, 1, 1); MAX_CHANGED + 1];
    assert!(changed_within(&many, (100, 50)).is_empty(), "too many: all of it");
    assert_eq!(changed_within(&many[..MAX_CHANGED], (100, 50)).len(), MAX_CHANGED);
}
