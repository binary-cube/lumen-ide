//! How the window looks: its palette (the theme's colors), the backdrop behind the cards,
//! the code's font, the background, the renderer it says it draws with.

use super::*;

impl Workbench {
    pub fn palette(&self) -> &Palette {
        &self.palette
    }

    /// Where the diagnostics overlay may go: the work area between the tool strips, below
    /// the top bar and above the status bar, inset by the popover gap (never over chrome).
    pub fn overlay_area(&self) -> Rect {
        let s = self.scale;
        let gap = (ui_workbench::ds::POPOVER_GAP * s).round();
        let (w, h) = (self.size.width as f32, self.size.height as f32);
        let top = TopBar::height(s) + gap;
        let bottom = h - StatusBar::height(s) - gap;
        let left = self.tools.strip.bounds().max_x() + gap;
        let right_strip = self.tools.right_strip.bounds();
        let right = if right_strip.size.width > 0.0 { right_strip.min_x() } else { w } - gap;
        Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
    }

    pub fn set_palette(&mut self, palette: Palette) {
        self.tools.set_palette(&palette);
        // The code font is the one found on the system (not the theme's generic family).
        let code_font = self.code_font.clone();
        let backdrop = self.backdrop_style(&palette.backdrop);
        let floating = crate::floating::style(&palette.floating, self.blur);
        self.palette = Palette { code_font, backdrop, floating, ..palette };
        // The icons' colors come from the palette.
        self.file_icons.purge();
    }

    /// What draws the window, as its driver says.
    pub fn set_renderer(&mut self, description: String) {
        self.renderer = Some(description);
    }

    /// Whether the window's renderer draws backdrop blurs.
    pub fn set_backdrop_blur(&mut self, blur: bool) {
        self.blur = blur;
        self.refresh_backdrop();
    }

    /// The backdrop and the floating panels' glass again (the user's choice changed).
    pub fn refresh_backdrop(&mut self) {
        self.palette.backdrop = self.backdrop_style(&self.palette.backdrop);
        self.palette.floating = crate::floating::style(&self.palette.floating, self.blur);
    }

    /// The backdrop a window draws: the theme's (what `style` was resolved from), never
    /// blurred where the renderer can't.
    pub(super) fn backdrop_style(
        &self,
        style: &ui_workbench::BackdropStyle,
    ) -> ui_workbench::BackdropStyle {
        crate::backdrop::style(style, self.blur)
    }

    pub fn set_font_family(&mut self, family: FontFamily) {
        // Also in the palette: the terminal and the other components use the same code font.
        self.palette.code_font = family.clone();
        self.code_font = family.clone();
        for tab in &mut self.tabs {
            tab.view.set_font_family(family.clone());
        }
    }

    pub fn background(&self) -> Rgba {
        self.palette.window_background
    }
}
