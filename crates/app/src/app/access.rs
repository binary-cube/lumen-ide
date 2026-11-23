//! The window's accessibility tree (plan 93d): made with each frame while a screen reader
//! runs (the shared components put their nodes as they paint), told to the system when it
//! changed or was asked for anew; what the screen reader asks of a node goes the way the
//! mouse does.

use platform_api::access::{Frame, NodeId, Request, Role};
use ui_kit::access::Recording;

use super::*;

/// The tree last told to the system: for which of its asks, under which title.
#[derive(Debug, Default)]
pub(super) struct Told {
    asked: u64,
    title: String,
    frame: Frame,
}

/// A frame's tree, begun when the system wants one (`None` otherwise). A frame drawing only
/// the theme's decorations paints no component: it makes none, the last told stands.
pub(super) fn begin(
    window: Option<&dyn Window>,
    told: &mut Option<Told>,
    only_decorations: bool,
) -> Option<(u64, Recording)> {
    let Some(asked) = window.and_then(|w| w.access_asked()) else {
        // A screen reader gone: the next one gets the whole tree.
        *told = None;
        return None;
    };
    (!only_decorations).then(|| (asked, ui_kit::access::begin()))
}

/// The frame's tree told to the system unless it's the one told last, for the same ask and
/// title; kept as the last.
pub(super) fn end(
    begun: Option<(u64, Recording)>,
    told: &mut Option<Told>,
    window: Option<&dyn Window>,
    title: &str,
) {
    let Some((asked, recording)) = begun else { return };
    let frame = recording.take();
    let same = told
        .as_ref()
        .is_some_and(|t| t.asked == asked && t.title == title && t.frame.same_as(&frame));
    if same {
        return;
    }
    if let Some(window) = window {
        window.update_access(&frame);
    }
    *told = Some(Told { asked, title: title.to_owned(), frame });
}

impl EditorApp {
    /// What a screen reader asked:
    /// - a press: a click in the node's middle, as the mouse makes it (never counted toward
    ///   a double click);
    /// - the focus: given to a text field by a click; anything else isn't pressed for it
    ///   (walking over buttons must not run them);
    /// - a value: a text field clicked, then, once it has the keyboard, its text chosen and
    ///   replaced as typed (never anywhere else).
    pub(super) fn on_access(&mut self, request: Request) {
        let id = match &request {
            Request::Press(id) | Request::Focus(id) | Request::SetValue(id, _) => *id,
        };
        let Some((role, at)) = self.frames.access.as_ref().and_then(|t| node_middle(&t.frame, id))
        else {
            return;
        };
        let field = matches!(role, Role::TextField | Role::SearchField | Role::TextArea);
        if !field && !matches!(request, Request::Press(_)) {
            return;
        }
        self.click_for_access(at);
        if let Request::SetValue(_, value) = request
            && self.workbench.text_field_focused()
        {
            self.with_text(|wb, text| wb.select_all(text));
            self.with_text(|wb, text| wb.type_text(text, &value));
        }
        self.redraw();
    }

    /// A click at `at` through the mouse's own way (moved there, pressed, released), not
    /// counted as the first of a double click.
    fn click_for_access(&mut self, at: Point) {
        let last = self.pointer.last_click.take();
        self.event_inner(PlatformEvent::CursorMoved(at));
        let modifiers = platform_api::Modifiers::default();
        self.event_inner(PlatformEvent::MouseDown {
            button: MouseButton::Left,
            position: at,
            modifiers,
        });
        self.event_inner(PlatformEvent::MouseUp { button: MouseButton::Left, position: at });
        self.pointer.last_click = last;
    }
}

/// Node `id`'s role and middle in `frame` (`None`: no such node).
fn node_middle(frame: &Frame, id: NodeId) -> Option<(Role, Point)> {
    let node = frame.nodes().iter().find(|n| n.id == id)?;
    Some((node.role, node.bounds.center()))
}

#[cfg(test)]
#[path = "../../tests/unit/app/access.rs"]
mod tests;
