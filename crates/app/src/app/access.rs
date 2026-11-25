//! The window's accessibility tree (plan 93d): made with each frame while a screen reader
//! runs (the shared components put their nodes as they paint), told to the system when it
//! changed or was asked for anew; what the screen reader asks of a node goes the way the
//! mouse does.

use platform_api::access::{Frame, NodeId, Request, Role, TextPlace};
use ui_kit::access::Recording;

use super::*;

/// The tree last told to the system: for which of its asks, under which title, when.
#[derive(Debug)]
pub(super) struct Told {
    asked: u64,
    title: String,
    frame: Frame,
    at: Instant,
}

/// The least time between two trees told while the screen keeps changing (a scroll, an
/// animation): ten a second at most (a screen reader reads far slower). A moved focus is
/// told at once; what waits is told when this passes (a frame asked for then).
const TELL_EVERY: Duration = Duration::from_millis(100);

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
/// title; kept as the last. Only what changed is sent (the system asked anew: all of it);
/// while the screen keeps changing, at most every [`TELL_EVERY`] (a moved focus at once):
/// the moment the change held back is told is given back, for a frame then. A control still
/// without a name is named by its tooltip (`tip_at`: what the pointer would be told over a
/// point).
pub(super) fn end(
    begun: Option<(u64, Recording)>,
    told: &mut Option<Told>,
    window: Option<&dyn Window>,
    title: &str,
    tip_at: impl Fn(Point) -> Option<(String, Option<String>)>,
) -> Option<Instant> {
    let (asked, recording) = begun?;
    let mut frame = recording.take();
    frame.name_from_tips(|bounds| tip_at(bounds.center()));
    // The tree told last, when it's the same ask's and title's (else all is told anew).
    let before = told.as_ref().filter(|t| t.asked == asked && t.title == title);
    if before.is_some_and(|t| t.frame.same_as(&frame)) {
        return None;
    }
    let now = Instant::now();
    if let Some(t) = before
        && t.frame.focused() == frame.focused()
        && now < t.at + TELL_EVERY
    {
        return Some(t.at + TELL_EVERY);
    }
    if let Some(window) = window {
        window.update_access(&frame, before.map(|t| &t.frame));
    }
    *told = Some(Told { asked, title: title.to_owned(), frame, at: now });
    None
}

impl EditorApp {
    /// What a screen reader asked:
    /// - a press: a click in the node's middle, as the mouse makes it (never counted toward
    ///   a double click);
    /// - the focus: given to a text field by a click; anything else isn't pressed for it
    ///   (walking over buttons must not run them);
    /// - a value: a text field clicked, then, once it has the keyboard (the editor's or a
    ///   page's), its text chosen and replaced as typed (never anywhere else).
    pub(super) fn on_access(&mut self, request: Request) {
        let id = match &request {
            Request::Press(id) | Request::Focus(id) | Request::SetValue(id, _) => *id,
            Request::Select { field, anchor, focus } => {
                self.select_for_access(*field, *anchor, *focus);
                return;
            }
        };
        let Some((role, at, focused)) =
            self.frames.access.as_ref().and_then(|t| press_point(&t.frame, id))
        else {
            return;
        };
        let field = matches!(role, Role::TextField | Role::SearchField | Role::TextArea);
        if !field && !matches!(request, Request::Press(_)) {
            return;
        }
        // A field with the keyboard already keeps its caret where it is.
        if field && focused && matches!(request, Request::Focus(_)) {
            return;
        }
        self.click_for_access(at);
        if let Request::SetValue(_, value) = request {
            // A page's field the click gave the keyboard: its text chosen, then typed.
            let open = !self.workbench.overlay_open();
            if !self.web_views.set_value(&value, open) && self.workbench.text_field_focused() {
                self.with_text(|wb, text| wb.select_all(text));
                self.with_text(|wb, text| wb.type_text(text, &value));
            }
        }
        self.redraw();
    }

    /// A selection a screen reader set in a field's text (its caret moved by the user from
    /// there): the field given the keyboard if it hasn't it (a click at its caret), then the
    /// places, a line's run and a character in it, turned into the text's bytes and selected.
    fn select_for_access(&mut self, field: NodeId, anchor: TextPlace, focus: TextPlace) {
        let Some(frame) = self.frames.access.as_ref().map(|t| &t.frame) else { return };
        let Some(node) = frame.nodes().iter().find(|n| n.id == field) else { return };
        let (Some(lines), Some(value)) = (&node.lines, &node.value) else { return };
        let line_of = |run: u64| {
            (0..=lines.lines.len()).find(|&i| platform_api::access::run_id(field, i) == run)
        };
        let at = |p: TextPlace| platform_api::access::byte_of(value, line_of(p.run)?, p.character);
        let (Some(from), Some(to)) = (at(anchor), at(focus)) else { return };
        // The code editor tells its lines shown: the places as (its line told, a character).
        let told = |p: TextPlace| Some((line_of(p.run)?, p.character));
        let (Some(told_anchor), Some(told_focus)) = (told(anchor), told(focus)) else { return };
        let (focused, caret) = (node.state.focused, lines.caret.unwrap_or(node.bounds).center());
        let meant = value.clone();
        if !focused {
            self.click_for_access(caret);
        }
        // A window's field whose text it is, else the code editor with the keyboard.
        if self.workbench.select_in_field(from..to, &meant)
            || self.workbench.select_in_editor(told_anchor, told_focus)
        {
            self.redraw();
        }
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

/// Node `id`'s role, where a press on it goes (a field's caret, so it doesn't move; else
/// its middle) and whether it has the keyboard (`None`: no such node).
fn press_point(frame: &Frame, id: NodeId) -> Option<(Role, Point, bool)> {
    let node = frame.nodes().iter().find(|n| n.id == id)?;
    let caret = node.lines.as_ref().and_then(|l| l.caret);
    Some((node.role, caret.unwrap_or(node.bounds).center(), node.state.focused))
}

#[cfg(test)]
#[path = "../../tests/unit/app/access.rs"]
mod tests;
