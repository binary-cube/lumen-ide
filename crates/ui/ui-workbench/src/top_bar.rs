//! The window's top bar: the menu button (☰, where the app
//! draws its menus), the project badge and name (with the recent projects list), the git
//! branch, and on the right the buttons registered there
//! (search, settings — `Dock::Top`), then the window's own.
//! On macOS it sits in the title bar, next to the window buttons (`left_inset`).

use icons::{IconId, Icons};
use kernel::{Point, Rect, Rgba};
use render_api::{ImageData, Layer, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{centered_baseline, draw_icon, draw_text};

use crate::palette::Palette;

/// The bar height, in logical pixels.
const HEIGHT: f32 = 40.0;

/// What the bar displays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopBarModel<'a> {
    pub project: Option<&'a str>,
    pub branch: Option<&'a str>,
    /// The branch is a button (a module runs something on its click: a list to switch).
    pub branch_button: bool,
    /// After the branch, muted in a small frame: what a module says of this window's
    /// project (its shortcut: `⌥⌘1`).
    pub badge: Option<&'a str>,
    /// The space left free on the left (the window buttons on macOS), in logical pixels.
    pub left_inset: f32,
    /// The window's buttons drawn here, on the right (Windows, no system title bar), and
    /// whether it's maximized (the middle one restores then).
    pub controls: Option<bool>,
    /// The window is full screen: with the window's buttons drawn here, the bar comes over
    /// the content (its own background) and the middle button leaves full screen.
    pub full_screen: bool,
    /// The mouse over the maximize button, as the system tells (it has it there: Windows'
    /// snap layouts), the bar not getting the mouse there.
    pub maximize_hovered: bool,
    /// The buttons registered on the right (their id and icon), left to right.
    pub actions: &'a [(&'static str, IconId)],
    /// The menu button first on the left (☰: the app's menus in a drawer), and whether its
    /// drawer is open. `None`: none (macOS' own menu bar).
    pub menu: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopBarHit {
    /// The menu button (☰).
    Menu,
    Projects,
    /// The branch (a button when a module gave it a click).
    Branch,
    /// The badge after the branch (a module's: the project's shortcut).
    Badge,
    /// A registered button (by its id).
    Action(&'static str),
    /// The window's own buttons (when the bar draws them).
    Minimize,
    Maximize,
    Close,
    /// Empty area: moves the window (double-click: maximize).
    Empty,
}

/// A window button's width (logical pixels): smaller than Windows' own (46), to suit the
/// bar's other buttons.
const CONTROL_WIDTH: f32 = 36.0;

#[derive(Debug, Default)]
pub struct TopBar {
    bounds: Rect,
    scale: f32,
    /// The menu button (empty when not drawn).
    menu: Rect,
    project: Rect,
    /// The branch, when it's a button (empty otherwise).
    branch: Rect,
    /// The badge after the branch (empty when not drawn).
    badge: Rect,
    /// The registered buttons (from the last paint).
    actions: Vec<(Rect, &'static str)>,
    /// The window's buttons (empty when not drawn).
    minimize: Rect,
    maximize: Rect,
    close: Rect,
    hover: Option<TopBarHit>,
    /// What it showed (the project, the branch, the buttons), from the last paint: the
    /// theme's decorations hang clear of it.
    occupied: Vec<Rect>,
}

impl TopBar {
    pub fn height(scale: f32) -> f32 {
        (HEIGHT * scale).round()
    }

    pub fn set_bounds(&mut self, bounds: Rect, scale: f32) {
        self.bounds = bounds;
        self.scale = scale;
    }

    pub fn contains(&self, p: Point) -> bool {
        self.bounds.contains(p)
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// The maximize button's area (physical pixels), when the bar draws the window's
    /// buttons: the system treats it as its own (Windows' snap layouts).
    pub fn maximize_button(&self) -> Option<Rect> {
        (self.maximize.size.width > 0.0).then_some(self.maximize)
    }

    /// What it shows (the project's badge and name, the branch, the buttons), as last
    /// painted.
    pub fn occupied(&self) -> &[Rect] {
        &self.occupied
    }

    /// The window's own buttons together (minimize to close), when the bar draws them.
    pub fn window_controls(&self) -> Option<Rect> {
        (self.close.size.width > 0.0).then(|| {
            Rect::new(
                self.minimize.min_x(),
                self.minimize.min_y(),
                self.close.max_x() - self.minimize.min_x(),
                self.close.size.height,
            )
        })
    }

    /// The menu button's area, when drawn.
    pub fn menu_button(&self) -> Option<Rect> {
        (self.menu.size.width > 0.0).then_some(self.menu)
    }

    /// The branch button's area, when it's one.
    pub fn branch_anchor(&self) -> Option<Rect> {
        (self.branch.size.width > 0.0).then_some(self.branch)
    }

    /// The project button's area (the project list opens below it).
    pub fn project_anchor(&self) -> Rect {
        self.project
    }

    /// The button under `p` and its area (for the tooltip).
    pub fn hit_rect(&self, p: Point) -> Option<(Rect, TopBarHit)> {
        let hit = self.hit(p)?;
        let rect = match hit {
            TopBarHit::Menu => self.menu,
            TopBarHit::Projects => self.project,
            TopBarHit::Branch => self.branch,
            TopBarHit::Badge => self.badge,
            TopBarHit::Action(id) => self.actions.iter().find(|(_, a)| *a == id)?.0,
            TopBarHit::Minimize => self.minimize,
            TopBarHit::Maximize => self.maximize,
            TopBarHit::Close => self.close,
            TopBarHit::Empty => return None,
        };
        Some((rect, hit))
    }

    pub fn hit(&self, p: Point) -> Option<TopBarHit> {
        if !self.bounds.contains(p) {
            return None;
        }
        Some(if self.menu.contains(p) {
            TopBarHit::Menu
        } else if self.project.contains(p) {
            TopBarHit::Projects
        } else if self.branch.contains(p) {
            TopBarHit::Branch
        } else if self.badge.contains(p) {
            TopBarHit::Badge
        } else if let Some((_, id)) = self.actions.iter().find(|(r, _)| r.contains(p)) {
            TopBarHit::Action(id)
        } else if self.minimize.contains(p) {
            TopBarHit::Minimize
        } else if self.maximize.contains(p) {
            TopBarHit::Maximize
        } else if self.close.contains(p) {
            TopBarHit::Close
        } else {
            TopBarHit::Empty
        })
    }

    pub fn hover(&mut self, p: Point) -> bool {
        let hover = self.hit(p).filter(|h| *h != TopBarHit::Empty);
        let changed = hover != self.hover;
        self.hover = hover;
        changed
    }

    pub fn paint(
        &mut self,
        model: TopBarModel<'_>,
        menu_open: bool,
        icons: &Icons,
        text: &mut TextSystem,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let b = self.bounds;
        let s = self.scale;
        let px = |v: f32| (v * s).round();
        let layer = scene.push_layer(Some(b));
        self.occupied.clear();
        if model.full_screen && model.controls.is_some() {
            layer.quad(Quad::filled(b, palette.window_background));
            let line = px(1.0).max(1.0);
            layer.quad(Quad::filled(
                Rect::new(b.min_x(), b.max_y() - line, b.size.width, line),
                palette.border,
            ));
        }
        let font = Font::new(palette.ui_font.clone(), px(palette.ui_font_size));
        let bold = font.clone().with_weight(Font::SEMIBOLD);
        let metrics = text.metrics(&font);
        let baseline = centered_baseline(b.min_y(), b.size.height, &metrics);
        let pill_h = px(28.0);
        let pill_top = (b.min_y() + (b.size.height - pill_h) / 2.0).round();

        // The menu button, first (after the system's window buttons on macOS).
        let mut x = b.min_x() + px(model.left_inset.max(10.0));
        self.menu = Rect::default();
        if let Some(open) = model.menu {
            let size = px(28.0);
            self.menu = Rect::new(x, pill_top, size, pill_h);
            let active = open || self.hover == Some(TopBarHit::Menu);
            if active {
                layer.quad(Quad::filled(self.menu, palette.tab_hover).rounded(px(7.0)));
            }
            if let Some(id) = icons.id("menu") {
                let icon = px(16.0);
                let r = Rect::new(
                    (self.menu.min_x() + (size - icon) / 2.0).round(),
                    (pill_top + (pill_h - icon) / 2.0).round(),
                    icon,
                    icon,
                );
                draw_icon(
                    layer,
                    icons,
                    id,
                    r,
                    if open { palette.text } else { palette.text_muted },
                );
                self.occupied.push(r);
            }
            x = self.menu.max_x() + px(4.0);
        }

        // The project: badge, name, arrow.
        let no_project;
        let name = match model.project {
            Some(name) => name,
            None => {
                no_project = i18n::t("home.no_project");
                &no_project
            }
        };
        let label = text.shape_line(name, &bold);
        let badge = px(20.0);
        let width = px(8.0) + badge + px(8.0) + label.width + px(6.0) + px(12.0) + px(8.0);
        self.project = Rect::new(x, pill_top, width, pill_h);
        if menu_open || self.hover == Some(TopBarHit::Projects) {
            layer.quad(Quad::filled(self.project, palette.tab_hover).rounded(px(7.0)));
        }
        let badge_rect = Rect::new(x + px(8.0), pill_top + (pill_h - badge) / 2.0, badge, badge);
        match model.project {
            Some(name) => paint_badge(layer, badge_rect, name, text, palette, s),
            None => {
                if let Some(id) = icons.id("folder") {
                    draw_icon(layer, icons, id, badge_rect, palette.text_muted);
                }
            }
        }
        let label_x = badge_rect.max_x() + px(8.0);
        draw_text(layer, &label, label_x, baseline, palette.text);
        if let Some(id) = icons.id("chevron-down") {
            let size = px(12.0);
            let r = Rect::new(
                label_x + label.width + px(6.0),
                b.min_y() + (b.size.height - size) / 2.0,
                size,
                size,
            );
            draw_icon(layer, icons, id, r, palette.text_muted);
        }
        let shown_to = label_x + label.width + px(6.0) + px(12.0);
        self.occupied.push(Rect::new(
            badge_rect.min_x(),
            badge_rect.min_y(),
            shown_to - badge_rect.min_x(),
            badge_rect.size.height,
        ));
        x = self.project.max_x() + px(14.0);

        // The git branch: shown, or a button (a module's list to switch it) with a chevron.
        self.branch = Rect::default();
        if let Some(branch) = model.branch {
            let from = x;
            let shaped = text.shape_line(branch, &font);
            let icon = px(15.0);
            let chevron = if model.branch_button { px(6.0) + px(12.0) } else { 0.0 };
            let inner = icon + px(6.0) + shaped.width + chevron;
            if model.branch_button {
                let h = px(26.0);
                let r = Rect::new(
                    x - px(8.0),
                    (b.min_y() + (b.size.height - h) / 2.0).round(),
                    inner + px(16.0),
                    h,
                );
                let lit = self.hover == Some(TopBarHit::Branch);
                if lit {
                    layer.quad(Quad::filled(r, palette.text.with_alpha(0.07)).rounded(px(6.0)));
                }
                self.branch = r;
            }
            let color = if self.hover == Some(TopBarHit::Branch) {
                palette.text
            } else {
                palette.text_muted
            };
            if let Some(id) = icons.id("git-branch") {
                let r = Rect::new(x, b.min_y() + (b.size.height - icon) / 2.0, icon, icon);
                draw_icon(layer, icons, id, r, color);
            }
            x += icon + px(6.0);
            draw_text(layer, &shaped, x, baseline, color);
            x += shaped.width;
            if model.branch_button
                && let Some(id) = icons.id("chevron-down")
            {
                let size = px(12.0);
                let r =
                    Rect::new(x + px(6.0), b.min_y() + (b.size.height - size) / 2.0, size, size);
                draw_icon(layer, icons, id, r, palette.text_muted);
                x += px(6.0) + size;
            }
            let h = px(16.0);
            let top = b.min_y() + (b.size.height - h) / 2.0;
            self.occupied.push(Rect::new(from, top, x - from, h));
            x += px(12.0) + if model.branch_button { px(8.0) } else { 0.0 };
        }
        // A module's word on this project (its shortcut), in a small frame.
        self.badge = Rect::default();
        if let Some(badge) = model.badge {
            let small = Font::new(palette.ui_font.clone(), px(palette.ui_font_size - 1.5));
            let shaped = text.shape_line(badge, &small);
            let h = px(20.0);
            let r = Rect::new(
                x,
                (b.min_y() + (b.size.height - h) / 2.0).round(),
                shaped.width + px(12.0),
                h,
            );
            layer.quad(
                Quad::filled(r, palette.text.with_alpha(0.04))
                    .rounded(px(5.0))
                    .bordered(self.scale.round().max(1.0), palette.border),
            );
            let base = centered_baseline(r.min_y(), h, &text.metrics(&small));
            let hovered = self.hover == Some(TopBarHit::Badge);
            let color = if hovered { palette.text } else { palette.text_muted };
            draw_text(layer, &shaped, r.min_x() + px(6.0), base, color);
            self.occupied.push(r);
            self.badge = r;
        }

        // Far right: the window's buttons (Windows), like the bar's other buttons, thin
        // icons; closing turns red under the mouse.
        let mut right = b.max_x();
        self.minimize = Rect::default();
        self.maximize = Rect::default();
        self.close = Rect::default();
        if let Some(maximized) = model.controls {
            // As tall as the bar's other buttons, centered, a small gap between them.
            let (w, h, gap) = (px(CONTROL_WIDTH), pill_h, px(2.0));
            let place = |right: f32| Rect::new(right - w, pill_top, w, h);
            self.close = place(right - px(6.0));
            self.maximize = place(self.close.min_x() - gap);
            self.minimize = place(self.maximize.min_x() - gap);
            right = self.minimize.min_x();
            self.occupied.push(Rect::new(
                self.minimize.min_x(),
                pill_top,
                self.close.max_x() - self.minimize.min_x(),
                h,
            ));
            let glyph = px(12.0);
            let middle = match (model.full_screen, maximized) {
                (true, _) => "minimize-2",
                (false, true) => "copy",
                (false, false) => "square",
            };
            for (rect, name, hit) in [
                (self.minimize, "minus", TopBarHit::Minimize),
                (self.maximize, middle, TopBarHit::Maximize),
                (self.close, "x", TopBarHit::Close),
            ] {
                let hovered = self.hover == Some(hit)
                    || (hit == TopBarHit::Maximize && model.maximize_hovered);
                let (fill, color) = match hit {
                    TopBarHit::Close if hovered => (Some(Rgba::rgb(0xC42B1C)), Rgba::WHITE),
                    _ if hovered => (Some(palette.tab_hover), palette.text),
                    _ => (None, palette.text_muted),
                };
                if let Some(fill) = fill {
                    layer.quad(Quad::filled(rect, fill).rounded(px(7.0)));
                }
                if let Some(id) = icons.id(name) {
                    let r = Rect::new(
                        (rect.min_x() + (rect.size.width - glyph) / 2.0).round(),
                        (rect.min_y() + (rect.size.height - glyph) / 2.0).round(),
                        glyph,
                        glyph,
                    );
                    draw_icon(layer, icons, id, r, color);
                }
            }
        }

        // Right: the registered buttons (search, settings), left of the window's.
        let button = px(28.0);
        let icon = px(16.0);
        self.actions.clear();
        let mut x = right - px(10.0) - button;
        for (id, icon_id) in model.actions.iter().rev() {
            let rect = Rect::new(x, pill_top, button, button);
            if self.hover == Some(TopBarHit::Action(id)) {
                layer.quad(Quad::filled(rect, palette.tab_hover).rounded(px(7.0)));
            }
            let r = Rect::new(
                rect.min_x() + (button - icon) / 2.0,
                rect.min_y() + (button - icon) / 2.0,
                icon,
                icon,
            );
            draw_icon(layer, icons, *icon_id, r, palette.text_muted);
            self.actions.push((rect, id));
            self.occupied.push(r);
            x -= button + px(4.0);
        }
    }
}

/// A project's initials: the first letters of the first two words (`docker.acme.test` →
/// "DA", `seo-analyzer` → "SA"), otherwise the first two letters.
pub fn project_initials(name: &str) -> String {
    ui_components::ds::initials(name)
}

/// The project tint on the window background: a 2D gradient in the badge color, from the
/// top-left corner, smoothly fading to the right and downward (under the top bar and
/// beside the cards), so the bar isn't flat and the transition isn't visible.
///
/// The gradient is a small texture (64 × 32), generated once per project and scaled on the
/// GPU with bilinear filtering: a single drawn instance, however large the window.
#[derive(Debug, Default)]
pub struct ProjectTint {
    cached: Option<(String, u32, ImageData)>,
}

impl ProjectTint {
    pub fn paint(
        &mut self,
        window: Rect,
        project: Option<&str>,
        scale: f32,
        scene: &mut Scene,
        palette: &Palette,
    ) {
        let Some(name) = project else { return };
        if palette.top_bar_tint <= 0.0 {
            return;
        }
        let strength = palette.top_bar_tint.to_bits();
        let fresh = self.cached.as_ref().is_some_and(|(n, s, _)| n == name && *s == strength);
        if !fresh {
            let color = ui_components::tag_colors::project(name).to_rgba8();
            let (w, h) = (64u32, 32u32);
            let mut pixels = Vec::with_capacity((w * h * 4) as usize);
            for y in 0..h {
                for x in 0..w {
                    let fx = 1.0 - x as f32 / (w - 1) as f32;
                    let fy = 1.0 - y as f32 / (h - 1) as f32;
                    // Easing: fades fast at first, then smoothly.
                    let a = palette.top_bar_tint * fx * fx * fy.powf(1.6);
                    pixels.extend_from_slice(&[
                        color[0],
                        color[1],
                        color[2],
                        (a * 255.0).round() as u8,
                    ]);
                }
            }
            self.cached = Some((name.to_owned(), strength, ImageData::new(w, h, pixels)));
        }
        let Some((_, _, image)) = &self.cached else { return };
        let rect = Rect::new(
            window.min_x(),
            window.min_y(),
            (window.size.width * 0.6).min(980.0 * scale).round(),
            (260.0 * scale).round().min(window.size.height),
        );
        scene.push_layer(None).image(image, rect);
    }
}

/// A project's badge: a colored rounded square, with the initials.
pub fn paint_badge(
    layer: &mut Layer,
    rect: Rect,
    name: &str,
    text: &mut TextSystem,
    palette: &Palette,
    scale: f32,
) {
    layer.quad(
        Quad::filled(rect, ui_components::tag_colors::project(name)).rounded((5.0 * scale).round()),
    );
    let font = Font::new(palette.ui_font.clone(), (rect.size.height * 0.5).round())
        .with_weight(Font::BOLD);
    let initials = text.shape_line(&project_initials(name), &font);
    let metrics = text.metrics(&font);
    let baseline = centered_baseline(rect.min_y(), rect.size.height, &metrics);
    draw_text(
        layer,
        &initials,
        (rect.min_x() + (rect.size.width - initials.width) / 2.0).round(),
        baseline,
        Rgba::WHITE,
    );
}

#[cfg(test)]
#[path = "../tests/unit/top_bar.rs"]
mod tests;
