//! The layer over the pages, where what floats is drawn while a page is on screen.

use super::*;

/// The room around what floats for its shadow (logical pixels): the floating layer draws
/// and takes the mouse there too.
pub(super) const FLOATING_SHADOW: f32 = 28.0;

impl EditorApp {
    /// The layer over the pages made while a page is on screen (what floats drawn there,
    /// over it), let go once none is; its renderer at the window's size. Where none can be
    /// made (or it can't be transparent), holes are cut out of the pages instead.
    pub(super) fn sync_floating(&mut self, window: &Arc<dyn Window>) {
        let wanted = self.web_views.any_shown()
            && !self.frames.floating_failed
            && self.frames.renderer.is_some();
        if !wanted {
            if let Some(floating) = self.frames.floating.take() {
                floating.layer.set_visible(false);
            }
            return;
        }
        let size = window.inner_size();
        if self.frames.floating.is_none() {
            let Some(layer) = window.floating_layer() else {
                self.frames.floating_failed = true;
                return;
            };
            let options = RenderOptions { transparent: true, ..self.shared.render_options };
            let target: Arc<dyn RenderTarget> = Arc::new(layer.handles());
            match WgpuRendererFactory.create(target, size, &options) {
                Ok(renderer) if renderer.transparent() => {
                    tracing::info!("what floats is drawn over the pages, on its own layer");
                    self.frames.floating =
                        Some(state::Floating { renderer, layer, scene: Scene::default() });
                }
                Ok(_) => {
                    tracing::info!("the floating layer can't be transparent: holes instead");
                    self.frames.floating_failed = true;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "the floating layer's renderer didn't start");
                    self.frames.floating_failed = true;
                }
            }
        }
        if let Some(floating) = &mut self.frames.floating {
            floating.renderer.resize(size);
        }
    }
}
