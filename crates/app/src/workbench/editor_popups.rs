//! The editor's own floating cards as windows over it: the code's hover card, the
//! completion list and the parameters' hint. They float (the editor keeps the keyboard; they block nothing), take
//! the mouse on their own area (a click, the wheel, their scrollbar's thumb), and go when
//! the mouse presses elsewhere (the card also when the wheel turns elsewhere). The editor
//! draws them at its place, under the window's bars; the stack only routes their input.

use super::overlay_stack::{OverlayKind, OverlaySlot};
use super::*;
use render_api::Scene;

pub(super) struct CodeHoverSlot;

impl OverlaySlot for CodeHoverSlot {
    /// The card shows when the mouse rests; one waited for from a language server: at its
    /// grace's end instead (`served_deadline` has it).
    fn rest_deadline(&self, wb: &Workbench) -> Option<std::time::Instant> {
        wb.code_hover
            .deadline()
            .filter(|at| *at > std::time::Instant::now() || !wb.served_hover_waiting())
    }

    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.code_hover.is_shown()
    }

    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    fn floats(&self, _wb: &Workbench) -> bool {
        true
    }

    fn holds(&self, wb: &Workbench, p: Point) -> bool {
        wb.code_hover.contains(p)
    }

    fn area(&self, wb: &Workbench) -> Option<kernel::Rect> {
        wb.code_hover.bounds()
    }

    /// On its scrollbar: the content follows the thumb (a drag goes on); elsewhere on it
    /// the click stays there (the editor below doesn't move).
    fn click(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        if wb.code_hover.scrollbar_press(p) {
            return MouseResult::StartDrag;
        }
        MouseResult::None
    }

    fn press_elsewhere(&self, wb: &mut Workbench) {
        wb.code_hover.hide();
    }

    fn scroll_elsewhere(&self, wb: &mut Workbench) {
        wb.code_hover.hide();
    }

    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        (wb.code_hover.scrollbar_hover(p), CursorIcon::Default)
    }

    fn scroll(&self, wb: &mut Workbench, _: f32, dy: f32) -> Option<bool> {
        Some(wb.code_hover.scroll_by(dy))
    }

    fn held(&self, wb: &mut Workbench) -> bool {
        wb.code_hover.scrollbar_held()
    }

    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        wb.code_hover.scrollbar_held().then(|| wb.code_hover.scrollbar_drag(p))
    }

    fn release(&self, wb: &mut Workbench, _: &mut TextSystem) -> Option<bool> {
        wb.code_hover.scrollbar_release().then_some(true)
    }

    /// Drawn by the editor, at its place.
    fn paint(&self, _wb: &mut Workbench, _text: &mut TextSystem, _scene: &mut Scene) {}

    fn animating(&self, wb: &Workbench) -> bool {
        wb.code_hover.is_animating()
    }

    fn frame_deadline(&self, wb: &Workbench) -> Option<std::time::Instant> {
        wb.code_hover.frame_deadline()
    }
}

pub(super) struct CompletionSlot;

impl OverlaySlot for CompletionSlot {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.completion.is_open()
    }

    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    fn floats(&self, _wb: &Workbench) -> bool {
        true
    }

    fn holds(&self, wb: &Workbench, p: Point) -> bool {
        wb.completion.contains(p)
    }

    fn area(&self, wb: &Workbench) -> Option<kernel::Rect> {
        wb.completion.bounds()
    }

    /// On its scrollbar: the list follows the thumb (a drag goes on); on a row: that one is
    /// taken.
    fn click(&self, wb: &mut Workbench, text: &mut TextSystem, p: Point, _: u32) -> MouseResult {
        if wb.completion.scrollbar_press(p) {
            return MouseResult::StartDrag;
        }
        wb.completion.click(p);
        if wb.accept_completion(text) { MouseResult::Redraw } else { MouseResult::None }
    }

    fn press_elsewhere(&self, wb: &mut Workbench) {
        wb.completion.close();
    }

    /// Its scrollbar shows when the pointer reaches its lane, and narrows when it leaves.
    fn hover(&self, wb: &mut Workbench, p: Point) -> (bool, CursorIcon) {
        (wb.completion.hover(p), CursorIcon::Default)
    }

    fn scroll(&self, wb: &mut Workbench, _: f32, dy: f32) -> Option<bool> {
        Some(wb.completion.scroll_by(dy))
    }

    fn held(&self, wb: &mut Workbench) -> bool {
        wb.completion.scrollbar_held()
    }

    fn drag(&self, wb: &mut Workbench, _: &mut TextSystem, p: Point) -> Option<bool> {
        wb.completion.scrollbar_held().then(|| wb.completion.scrollbar_drag(p))
    }

    fn release(&self, wb: &mut Workbench, _: &mut TextSystem) -> Option<bool> {
        wb.completion.scrollbar_release().then_some(true)
    }

    /// Drawn by the editor, at its place.
    fn paint(&self, _wb: &mut Workbench, _text: &mut TextSystem, _scene: &mut Scene) {}

    fn animating(&self, _wb: &Workbench) -> bool {
        false
    }

    fn frame_deadline(&self, wb: &Workbench) -> Option<std::time::Instant> {
        wb.completion.frame_deadline()
    }
}

/// The parameters' hint over a call: only shown (nothing of it takes the mouse); it goes
/// at any press.
pub(super) struct ParamHintSlot;

impl OverlaySlot for ParamHintSlot {
    fn kind(&self, _wb: &Workbench) -> OverlayKind {
        OverlayKind::Popover
    }

    fn is_open(&self, wb: &Workbench) -> bool {
        wb.param_hint.is_open()
    }

    fn covers_home(&self, _wb: &Workbench) -> bool {
        false
    }

    fn floats(&self, _wb: &Workbench) -> bool {
        true
    }

    fn press_elsewhere(&self, wb: &mut Workbench) {
        wb.param_hint.hide();
    }

    /// Drawn by the editor, at its place.
    fn paint(&self, _wb: &mut Workbench, _text: &mut TextSystem, _scene: &mut Scene) {}

    fn animating(&self, wb: &Workbench) -> bool {
        wb.param_hint.is_animating()
    }
}
