//! The layout built: right away or on a worker, taken when it comes, installed with its
//! palette.

use super::*;

impl MarkdownViewer {
    /// A layout built for `want`: right away when the caches say so (tests) or when there's
    /// nothing to show yet and the text is small (no empty frame on opening); otherwise on
    /// a worker, the layout there is drawn meanwhile. With the caches away (a build on its
    /// way), nothing: what's wanted is built once they're back.
    pub(super) fn build(&mut self, want: build::Want, text: &mut text_layout::TextSystem) {
        let Some(mut caches) = self.caches.take() else { return };
        self.dirty = false;
        let inline = caches.is_inline()
            || (self.layout.is_none() && self.source.len() <= build::FIRST_INLINE);
        if inline {
            let page = Page {
                source: &self.source,
                version: want.version,
                width: want.width,
                scale: want.scale,
                dir: &self.dir,
                toggled: &self.toggled,
            };
            let layout = Layout::build(page, &mut caches, text, &want.palette);
            let text = self.layout_text.take();
            self.install(build::Built { layout, caches, text }, want.palette);
            return;
        }
        self.caches_ram = caches.memory();
        let input = build::Input {
            source: self.source.clone(),
            dir: self.dir.clone(),
            toggled: self.toggled.clone(),
        };
        let text = self.layout_text.clone();
        self.building = Some(build::start(want, input, caches, text, self.waker.clone()));
    }

    /// The layout built on a worker, put in place once it's there. One that gave nothing
    /// back (it panicked) leaves the caches anew and isn't tried again for the same.
    pub(super) fn take_built(&mut self) {
        let Some(building) = &self.building else { return };
        let built = match building.rx.try_recv() {
            Ok(built) => Some(built),
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        let Some(building) = self.building.take() else { return };
        match built {
            Some(built) => self.install(built, building.want.palette),
            None => {
                tracing::warn!("a markdown layout could not be built");
                let mut caches = layout::Caches::default();
                caches.set_waker(self.waker.clone());
                self.caches = Some(caches);
                self.failed = Some(building.want);
            }
        }
    }

    /// A layout put in place (built with `palette`): the line at the top stays there (or
    /// the source line asked for comes to the top), what was selected in another text goes,
    /// the pixels of pictures it no longer shows go.
    fn install(&mut self, built: build::Built, palette: ui_components::Palette) {
        let build::Built { layout, caches, text } = built;
        self.caches = Some(caches);
        self.layout_text = text;
        self.failed = None;
        if self.layout.as_ref().is_some_and(|l| l.version != layout.version) {
            self.selection = None;
            self.selecting = None;
        }
        let anchor = self
            .layout
            .as_ref()
            .filter(|_| self.scroll > 0.0)
            .map(|l| l.lines.line_at(self.scroll));
        // Asked for this text: only a layout of it takes it (an older one keeps it waiting).
        let asked = if layout.version == self.version { self.sync_to.take() } else { None };
        if let Some(line) = asked.or(anchor) {
            self.scroll = layout.lines.y_of(line);
        }
        self.drawn.images.keep(&layout);
        self.layout = Some(layout);
        if self.built_with.as_ref() != Some(&palette) {
            self.built_with = Some(palette);
        }
    }
}
