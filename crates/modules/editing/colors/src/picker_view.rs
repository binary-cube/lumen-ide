//! The color picker, as this module's popover: the editor opens it next to a color's swatch
//! (with the color, its format and where it is); it writes the picked color back in the
//! same form (a request for the app: one undo step), or closes on Esc / a click outside.

use std::path::Path;

use kernel::Point;
use ui_components::color_picker::ColorPart;
use ui_components::control::{Control, Controls};
use ui_components::tool_panel::KeyReach;
use ui_components::{
    ColorPick, ColorPicker, PICKER_EXTENSION, PickerAction, Viewer, ViewerAction, ViewerCursor,
    ViewerKey, ViewerKind, ViewerPaint, ViewerProvider,
};

use crate::Colors;

#[derive(Debug)]
pub(crate) struct PickerProvider;

impl ViewerProvider for PickerProvider {
    fn id(&self) -> &'static str {
        "colors.picker"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        path.extension().is_some_and(|e| e == PICKER_EXTENSION).then_some(ViewerKind::Replace)
    }

    fn open(&self, _path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(PickerView::default()))
    }
}

#[derive(Debug, Default)]
pub(crate) struct PickerView {
    picker: ColorPicker,
    /// The color's place in the active file's text.
    range: std::ops::Range<usize>,
    action: Option<ViewerAction>,
    /// The picker's state of use as a control (see [`PickerView::with_controls`]), and what
    /// it asked for at a press or a key (acted on right after).
    controls: Controls,
    asked: Option<PickerAction>,
    /// Done (applied or cancelled): the app closes it.
    done: bool,
}

impl PickerView {
    /// What the picker asked for: the color written back, given up, or the screen sampled.
    fn act(&mut self, action: PickerAction) {
        self.action = Some(match action {
            PickerAction::Eyedropper => {
                ViewerAction::Request(ui_components::AppRequest::SampleScreen)
            }
            PickerAction::Cancel => {
                self.done = true;
                ViewerAction::Close
            }
            PickerAction::Apply(color, format) => {
                self.done = true;
                let text = code_intelligence::ColorProvider::format(&Colors, color, format);
                ViewerAction::Request(ui_components::AppRequest::ReplaceInActive {
                    range: self.range.clone(),
                    text,
                })
            }
        });
    }
}

impl PickerView {
    /// Its one control: the picker, over everything while open (its keys too).
    fn with_controls<R>(
        &mut self,
        f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R,
    ) -> R {
        // Whether the keys recolored it isn't needed here: the color is read when applied.
        let mut recolored = false;
        let mut picker = ColorPart {
            picker: &mut self.picker,
            colors: &Colors,
            action: &mut self.asked,
            recolored: &mut recolored,
        };
        let mut parts: [&mut dyn Control; 1] = [&mut picker];
        f(&mut self.controls, &mut parts)
    }

    /// What the picker asked for at a press or a key, done.
    fn follow(&mut self) {
        if let Some(action) = self.asked.take() {
            self.act(action);
        }
    }
}

impl Viewer for PickerView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(pick) = data.downcast_ref::<ColorPick>() else { return };
        self.range = pick.range.clone();
        self.picker.open(pick.anchor, pick.color, pick.format, &Colors);
    }

    /// Over the whole window (it places itself next to its swatch).
    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        self.picker.set_viewport(ctx.bounds, ctx.scale);
        self.picker.paint(ctx.text, ctx.scene, ctx.icons, ctx.palette);
    }

    /// Anywhere: inside it works the picker; outside it closes, keeping a changed color.
    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        self.with_controls(|c, parts| c.press(parts, p, 1));
        self.follow();
        self.action.take()
    }

    /// In the square or on a strip: the color follows the mouse.
    fn drag(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.drag(parts, p)).is_some_and(|(_, moved)| moved)
    }

    fn release(&mut self) -> bool {
        self.with_controls(|c, parts| c.release(parts)).is_some_and(|(_, changed)| changed)
    }

    fn hover(&mut self, p: Point) -> bool {
        self.with_controls(|c, parts| c.hover(parts, Some(p)))
    }

    /// Asked from `&self` at any point (not only where the mouse last was): the picker's own.
    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.picker.over_control(p).then_some(ViewerCursor::Pointer)
    }

    fn wants_text(&self) -> bool {
        true
    }

    fn text_input(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.text(parts, text)).is_some()
    }

    fn paste(&mut self, text: &str) -> bool {
        self.with_controls(|c, parts| c.paste(parts, text)).is_some()
    }

    fn edit_key(&mut self, key: ViewerKey) -> bool {
        self.edit_key_with(key, KeyReach::default())
    }

    /// Esc gives up, ↵ applies, ⌫ erases in its field, ← → move its caret (⇧ selects).
    fn edit_key_with(&mut self, key: ViewerKey, reach: KeyReach) -> bool {
        let used = self.with_controls(|c, parts| c.key(parts, key, reach)).is_some();
        self.follow();
        used
    }

    /// The color sampled from the screen (`screen-color:#rrggbb`), ⌘A in its field.
    fn command(&mut self, id: &str) -> bool {
        if let Some(hex) = id.strip_prefix("screen-color:") {
            return match kernel::Rgba::from_hex(hex) {
                Some(color) => {
                    self.picker.set_color(color, &Colors);
                    true
                }
                None => false,
            };
        }
        self.with_controls(|c, parts| c.command(parts, id)).is_some()
    }

    /// Asked from `&self`: the picker's field's selection, read as its part reads it.
    fn selected_text(&self) -> Option<String> {
        Some(self.picker.input().selected_text().to_owned()).filter(|t| !t.is_empty())
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.action.take()
    }

    fn closed(&self) -> bool {
        self.done
    }

    fn is_animating(&self) -> bool {
        self.picker.is_animating()
    }
}

#[cfg(test)]
#[path = "../tests/unit/picker_view.rs"]
mod tests;
