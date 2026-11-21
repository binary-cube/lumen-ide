//! The app's search field, one look and one behavior wherever something is searched: a pill
//! with the magnifier, the text typed, × once there is text (it empties the field), the
//! caret only while it has the focus. As a form's control it takes a press (the caret put,
//! two a word, three all of it, a drag selecting), the editing keys, the text typed (on one
//! line), paste, ⌘A / ⌘X / ⌘Z and the copy of what's selected; × is a hand.
//!
//! The view keeps it among its parts ([`crate::control::Controls`]): the focus is the
//! view's one, so two fields never show a caret at once.

use kernel::{Point, Rect, Rgba};
use render_api::Layer;
use text_layout::TextSystem;
use ui_kit::{TextInput, TextInputStyle};

use crate::control::{Control, Press};
use crate::ds::{self, Ctx};
use crate::tool_panel::KeyReach;
use crate::{ViewerCursor, ViewerKey};

/// The room kept on the right for × (logical pixels: its size and its margin).
const CLEAR_ROOM: f32 = 17.0;

/// The app's search field (see the module's doc).
#[derive(Debug, Default)]
pub struct SearchField {
    pub input: TextInput,
    /// Where it was drawn last, and its × (while it has text).
    area: Rect,
    clear: Option<Rect>,
    clear_hover: bool,
    /// It was drawn last with the focus (its caret shown).
    focused: bool,
    /// × was pressed since the owner last asked ([`Self::take_cleared`]).
    cleared: bool,
    /// Its text changed since the owner last asked ([`Self::take_edited`]).
    edited: bool,
}

/// What a view adds to its search field: room on the right for what's its own (a regex and
/// a case toggle, a key cap, a count of results), drawn by the view where
/// [`SearchField::paint_with`] says; and the field shown invalid (what's typed can't be used:
/// its border in the danger color).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SearchExtras {
    /// The room kept on the right (logical pixels; 0: none).
    pub trailing: f32,
    pub invalid: bool,
}

impl SearchField {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn text(&self) -> &str {
        self.input.text()
    }

    /// Its text set by its owner: not an edit ([`Self::take_edited`] stays as it was).
    pub fn set_text(&mut self, text: &str) {
        self.input.set_text(text);
    }

    /// × was pressed (the field emptied): told once, for the owner to search again.
    pub fn take_cleared(&mut self) -> bool {
        std::mem::take(&mut self.cleared)
    }

    /// Its text changed (typed, deleted, pasted, cut, undone, emptied by ×): told once, for
    /// the owner to search again. An owner that hasn't asked so far gets one `true` left
    /// from before when it first does (an edit it never followed): it searches once more.
    pub fn take_edited(&mut self) -> bool {
        std::mem::take(&mut self.edited)
    }

    /// Not drawn now (its page, its panel hidden): nowhere to be pressed until drawn again.
    pub fn forget(&mut self) {
        self.area = Rect::default();
        self.clear = None;
    }

    /// Where it was drawn last.
    pub fn area(&self) -> Rect {
        self.area
    }

    /// Its × as drawn last (`None`: empty, no ×).
    pub fn clear_area(&self) -> Option<Rect> {
        self.clear
    }

    /// It was drawn last with the focus: the caret is in it (only one field's at a time).
    pub fn shows_caret(&self) -> bool {
        self.focused
    }

    /// Draws it in `rect` (`focused`: the caret, the lit border), `placeholder` while empty.
    pub fn paint(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ctx: &Ctx<'_>,
        (rect, focused): (Rect, bool),
        placeholder: &str,
    ) {
        self.paint_with(
            layer,
            text,
            icons,
            ctx,
            (rect, focused),
            placeholder,
            SearchExtras::default(),
        );
    }

    /// [`Self::paint`] with what the view adds (`extras`); returns the room kept on the right
    /// for the view to draw its own in (empty without one).
    #[allow(clippy::too_many_arguments)]
    pub fn paint_with(
        &mut self,
        layer: &mut Layer,
        text: &mut TextSystem,
        icons: &icons::Icons,
        ctx: &Ctx<'_>,
        (rect, focused): (Rect, bool),
        placeholder: &str,
        extras: SearchExtras,
    ) -> Rect {
        let p = ctx.palette;
        self.area = rect;
        self.focused = focused;
        let mut input_rect = ds::search_field(layer, icons, ctx, rect, focused);
        // What can't be used: the pill's border in the danger color over the usual one.
        if extras.invalid {
            layer.quad(
                render_api::Quad::filled(rect, Rgba::TRANSPARENT)
                    .rounded(rect.size.height / 2.0)
                    .bordered(ctx.line(), ctx.fade(p.danger)),
            );
        }
        let trailing_w = ctx.px(extras.trailing);
        let trailing = Rect::new(
            rect.max_x() - ctx.px(8.0) - trailing_w,
            rect.min_y(),
            trailing_w,
            rect.size.height,
        );
        let end = if extras.trailing > 0.0 { trailing.min_x() } else { rect.max_x() };
        input_rect.size.width = (end - ctx.px(6.0) - input_rect.min_x()).max(0.0);
        // With text: × on the right (left of the view's own) empties the field.
        self.clear = (!self.input.text().is_empty()).then(|| {
            let mid = rect.min_y() + rect.size.height / 2.0;
            let cx = end - ctx.px(CLEAR_ROOM);
            ds::clear_button(layer, icons, ctx, cx, mid, self.clear_hover)
        });
        if let Some(clear) = self.clear {
            input_rect.size.width = (clear.min_x() - ctx.px(6.0) - input_rect.min_x()).max(0.0);
        }
        let style = TextInputStyle {
            font: ctx.ui_font(-0.5),
            background: Rgba::TRANSPARENT,
            border: Rgba::TRANSPARENT,
            border_focused: Rgba::TRANSPARENT,
            text: ctx.fade(p.text),
            placeholder: ctx.fade(p.text_muted),
            selection: ctx.fade(p.selection),
            caret: ctx.fade(p.caret),
            radius: 0.0,
            border_width: 0.0,
            padding: 0.0,
            caret_width: ctx.px(1.5).max(1.0),
            lead: 0.0,
            trail: 0.0,
        };
        self.input.paint(layer, text, input_rect, focused, placeholder, &style);
        if extras.trailing > 0.0 { trailing } else { Rect::default() }
    }

    /// `edit` done on its text, its change noted ([`Self::take_edited`]).
    fn followed(&mut self, edit: impl FnOnce(&mut TextInput) -> bool) -> bool {
        // Its text's fingerprint before and after (nothing copied).
        let print = |input: &TextInput| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            input.text().hash(&mut h);
            h.finish()
        };
        let before = print(&self.input);
        let used = edit(&mut self.input);
        if print(&self.input) != before {
            self.edited = true;
        }
        used
    }

    /// `p` is on its ×.
    fn on_clear(&self, p: Point) -> bool {
        self.clear.is_some_and(|r| r.contains(p))
    }
}

impl Control for SearchField {
    fn area(&self) -> Option<Rect> {
        Some(self.area).filter(|a| a.size.width > 0.0)
    }

    fn press(&mut self, p: Point, clicks: u32) -> Press {
        if self.on_clear(p) {
            self.input.set_text("");
            self.cleared = true;
            self.edited = true;
            return Press::Took;
        }
        self.input.press(p.x, clicks);
        Press::Held
    }

    fn drag(&mut self, p: Point) -> bool {
        self.input.extend_to(p.x);
        true
    }

    fn hover(&mut self, p: Option<Point>) -> bool {
        let over = p.is_some_and(|p| self.on_clear(p));
        std::mem::replace(&mut self.clear_hover, over) != over
    }

    fn cursor(&self, p: Point) -> Option<ViewerCursor> {
        Some(if self.on_clear(p) { ViewerCursor::Pointer } else { ViewerCursor::Text })
    }

    fn focusable(&self) -> bool {
        true
    }

    fn key(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        self.followed(|input| Control::key(input, key, reach))
    }

    fn text(&mut self, typed: &str) -> bool {
        self.followed(|input| Control::text(input, typed))
    }

    fn selected_text(&self) -> Option<String> {
        Control::selected_text(&self.input)
    }

    fn command(&mut self, id: &str) -> bool {
        self.followed(|input| Control::command(input, id))
    }
}

#[cfg(test)]
#[path = "../tests/unit/search_field.rs"]
mod tests;
