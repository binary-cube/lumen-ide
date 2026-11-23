//! The accessibility tree put as the UI paints (`platform_api::access`): each shared
//! component tells its own node where it draws itself, so a view built from them is read by
//! the system's screen reader without a line of its own.
//!
//! The tree is made on the UI thread only while the app asks for one ([`begin`], when a
//! screen reader runs); otherwise [`node`] and [`group`] check one flag and do nothing (the
//! node's text isn't even made: it's given as a closure).

use std::cell::{Cell, RefCell};

pub use platform_api::access::{Described, Frame, Node, NodeId, Role, State};

/// How much of a text's size stands above its baseline (the rest below): its box as a
/// screen reader shows it.
const ASCENT_SHARE: f32 = 0.8;

thread_local! {
    /// A tree is being made on this thread (read before anything else).
    static ON: Cell<bool> = const { Cell::new(false) };
    /// Inside [`silent`]: texts aren't read (they're said otherwise, or not yet).
    static SILENT: Cell<u32> = const { Cell::new(0) };
    static FRAME: RefCell<Frame> = RefCell::new(Frame::new());
}

/// A frame's tree being made: the nodes painted while it lives are kept; dropped without
/// being taken (an early return, a panic in a paint), nothing is kept and nothing records.
#[derive(Debug)]
#[must_use = "the tree records until this is taken or dropped"]
pub struct Recording(());

impl Recording {
    /// The tree made, handed over (the controls without a name named by their line).
    pub fn take(self) -> Frame {
        let mut frame = FRAME.with(|f| std::mem::take(&mut *f.borrow_mut()));
        frame.name_from_labels();
        frame
    }
}

impl Drop for Recording {
    fn drop(&mut self) {
        ON.with(|on| on.set(false));
        SILENT.with(|s| s.set(0));
        FRAME.with(|f| f.borrow_mut().clear());
    }
}

/// A frame's tree begins: the nodes painted from now on are kept, until the [`Recording`]
/// is taken or dropped.
pub fn begin() -> Recording {
    FRAME.with(|f| f.borrow_mut().clear());
    SILENT.with(|s| s.set(0));
    ON.with(|on| on.set(true));
    Recording(())
}

/// A tree is being made (a component with something costly to tell asks first).
#[inline]
pub fn recording() -> bool {
    ON.with(Cell::get)
}

/// The node `describe` tells, put under the group open now (nothing while no tree is made).
#[inline]
pub fn node(describe: impl FnOnce() -> Described) -> Option<NodeId> {
    if !recording() {
        return None;
    }
    let node = describe();
    Some(FRAME.with(|f| f.borrow_mut().add(node)))
}

/// A line of text drawn from `x` on `baseline`: read as a label (a mark with no letter or
/// digit, a ✓ or an arrow, is drawing, not text; nothing inside [`silent`]).
#[inline]
pub fn text(line: &text_layout::ShapedLine, x: f32, baseline: f32) {
    if !recording() || SILENT.with(Cell::get) > 0 || !line.text.chars().any(char::is_alphanumeric) {
        return;
    }
    let top = baseline - line.size * ASCENT_SHARE;
    let bounds = kernel::Rect::new(x, top, line.width, line.size);
    let label = Described::new(Role::Label, std::sync::Arc::clone(&line.text), bounds);
    FRAME.with(|f| f.borrow_mut().add(label));
}

/// `paint` run with its texts not read as labels (a code editor's tokens, said otherwise).
#[inline]
pub fn silent<R>(paint: impl FnOnce() -> R) -> R {
    if !recording() {
        return paint();
    }
    SILENT.with(|s| s.set(s.get() + 1));
    let painted = paint();
    SILENT.with(|s| s.set(s.get().saturating_sub(1)));
    painted
}

/// `paint` run with the group `describe` tells open: what it puts goes under it.
#[inline]
pub fn group<R>(describe: impl FnOnce() -> Described, paint: impl FnOnce() -> R) -> R {
    if !recording() {
        return paint();
    }
    let group = describe();
    FRAME.with(|f| f.borrow_mut().open(group));
    let painted = paint();
    FRAME.with(|f| f.borrow_mut().close());
    painted
}

#[cfg(test)]
#[path = "../tests/unit/access.rs"]
mod tests;
