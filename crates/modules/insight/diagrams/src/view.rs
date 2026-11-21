//! The diagram as a view: what it reacts to (clicks, drags, keys, scrolling, zoom).

use super::*;

impl Viewer for DiagramView {
    fn set_data(&mut self, data: &dyn std::any::Any) {
        let Some(data) = data.downcast_ref::<DiagramData>() else { return };
        self.queries = Some(Arc::clone(&data.queries));
        self.history.clear();
        self.view.zoom = 1.0;
        self.build(data.fqn.clone());
    }

    fn poll(&mut self) -> bool {
        let Some(rx) = &self.building else { return false };
        match rx.try_recv() {
            Ok(diagram) => {
                self.building = None;
                self.set_diagram(diagram);
                true
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.building = None;
                self.set_diagram(None);
                true
            }
        }
    }

    /// Its actions ([`ACTIONS`]).
    fn command(&mut self, id: &str) -> bool {
        match id {
            "fit" => {
                self.fit();
                true
            }
            "back" => self.back(),
            _ => false,
        }
    }

    fn scroll(&mut self, dx: f32, dy: f32) -> bool {
        self.view.scroll(dx, dy);
        true
    }

    fn zoom(&mut self, factor: f32, at: Point) -> bool {
        self.zoom_at(factor, at)
    }

    fn zoom_level(&self) -> Option<f32> {
        Some(self.view.zoom)
    }

    fn reset_zoom(&mut self) -> bool {
        self.view.reset_zoom(0.2, 2.5)
    }

    fn click(&mut self, p: Point) -> Option<ViewerAction> {
        // Acted on at release, unless the mouse moved (a pan).
        self.view.press(p);
        None
    }

    fn drag(&mut self, p: Point) -> bool {
        self.view.drag(p)
    }

    fn release(&mut self) -> bool {
        match self.view.release() {
            Release::None => false,
            Release::Panned => true,
            Release::Click(at) => {
                let Some(hit) = self.hit(at) else { return false };
                self.pending = self.activate(hit);
                true
            }
        }
    }

    fn take_action(&mut self) -> Option<ViewerAction> {
        self.pending.take()
    }

    fn cursor_at(&self, p: Point) -> Option<ViewerCursor> {
        self.hit(p).map(|_| ViewerCursor::Pointer)
    }

    /// Its hover says when what it shows changed.
    fn hover_tells_changes(&self) -> bool {
        true
    }

    fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    fn status(&self) -> Option<String> {
        let diagram = self.diagram.as_ref()?;
        Some(tr!("diagram.types", count = diagram.nodes.len()))
    }

    fn paint(&mut self, ctx: ViewerPaint<'_>) {
        if ctx.waker.is_some() {
            self.waker.clone_from(&ctx.waker);
        }
        self.view.bounds = ctx.bounds;
        self.view.scale = ctx.scale;
        if self.view.zoom == 0.0 {
            self.view.zoom = 1.0;
        }
        if self.view.fit_pending {
            self.fit();
        }
        self.hits.clear();
        let ds = Ctx::new(ctx.palette, ctx.scale);
        let p = ctx.palette;
        let px = |v: f32| ds.px(v);
        let b = ctx.bounds;
        let base = ctx.scene.push_layer(Some(b));
        base.quad(Quad::filled(b, p.editor_background).rounded(px(radius::CARD)));

        let area = self.area();
        if let Some(diagram) = self.diagram.clone() {
            self.paint_diagram(&diagram, &ds, ctx.text, ctx.scene, area);
        } else {
            let layer = ctx.scene.push_layer(Some(b));
            let message = if self.missing { t("diagram.missing") } else { t("diagram.building") };
            ds::empty_state(layer, ctx.text, &ds, area, &message, None);
        }
        self.paint_toolbar(&ds, (ctx.text, ctx.icons), ctx.scene, b);
    }
}
