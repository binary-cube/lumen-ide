//! What a screen part paints with — the text system, the scene, the theme's colors and the
//! icons — handed as one, instead of four arguments repeated by every `paint`; and, for the
//! pieces drawn into one layer, the layer with the same tools and the scale ([`LayerPainter`]).

use icons::Icons;
use render_api::{Layer, Scene};
use text_layout::TextSystem;

use crate::Palette;
use crate::ds::Ctx;

/// The tools to paint with (a frame's).
pub struct Painter<'a> {
    pub text: &'a mut TextSystem,
    pub scene: &'a mut Scene,
    pub palette: &'a Palette,
    pub icons: &'a Icons,
}

impl std::fmt::Debug for Painter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Painter").finish_non_exhaustive()
    }
}

/// The tools to paint one layer with: the layer, the text system, the icons, the theme and
/// the scale (`ds::Ctx`). Handed down as one to every function drawing a piece of it.
pub struct LayerPainter<'a> {
    pub layer: &'a mut Layer,
    pub text: &'a mut TextSystem,
    pub icons: &'a Icons,
    pub ds: Ctx<'a>,
}

impl<'a> LayerPainter<'a> {
    pub fn new(
        layer: &'a mut Layer,
        text: &'a mut TextSystem,
        icons: &'a Icons,
        ds: Ctx<'a>,
    ) -> Self {
        Self { layer, text, icons, ds }
    }
}

/// The tools to paint a part making its own layers with: the scene, the text system, the
/// icons, the theme and the scale (`ds::Ctx`).
pub struct ScenePainter<'a> {
    pub scene: &'a mut Scene,
    pub text: &'a mut TextSystem,
    pub icons: &'a Icons,
    pub ds: Ctx<'a>,
}

impl<'a> ScenePainter<'a> {
    pub fn new(
        scene: &'a mut Scene,
        text: &'a mut TextSystem,
        icons: &'a Icons,
        ds: Ctx<'a>,
    ) -> Self {
        Self { scene, text, icons, ds }
    }
}

impl std::fmt::Debug for ScenePainter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScenePainter").field("ds", &self.ds).finish_non_exhaustive()
    }
}

impl std::fmt::Debug for LayerPainter<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayerPainter").field("ds", &self.ds).finish_non_exhaustive()
    }
}
