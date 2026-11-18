//! What the views' painting methods share: the drawing tools and the content's column.

use render_api::Layer;
use text_layout::TextSystem;
use ui_components::ds::Ctx;

/// The tools a part of a view draws with: its layer, the text, the icons, the design system.
pub(crate) struct Pen<'a, 'b> {
    pub layer: &'a mut Layer,
    pub text: &'a mut TextSystem,
    pub icons: &'a icons::Icons,
    pub ds: &'a Ctx<'b>,
}

/// The content's column: where it starts and how wide it is.
#[derive(Clone, Copy)]
pub(crate) struct Column {
    pub x0: f32,
    pub width: f32,
}

impl<'b> Pen<'_, 'b> {
    /// The same tools, lent to one part of the view (the pen stays usable after it).
    pub fn lend(&mut self) -> Pen<'_, 'b> {
        Pen { layer: self.layer, text: self.text, icons: self.icons, ds: self.ds }
    }
}
