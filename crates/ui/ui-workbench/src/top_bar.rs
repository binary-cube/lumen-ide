//! The window's top bar: the menu button (☰, where the app
//! draws its menus), the project badge and name (with the recent projects list), the git
//! branch, and on the right the buttons registered there
//! (search, settings, the window kept on top — `Dock::Top`; a short line between two of
//! different groups), then the window's own. The window's own act on their release over
//! the button pressed (the app arms them on the press).
//! On macOS it sits in the title bar, next to the window buttons (`left_inset`).

use icons::{IconId, Icons};
use kernel::{Point, Rect};
use render_api::{ImageData, Quad, Scene};
use text_layout::{Font, TextSystem};
use ui_kit::{centered_baseline, draw_icon, draw_text};

use crate::ds;
use crate::palette::Palette;
use ui_components::tag_colors;

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
    /// The buttons registered on the right, left to right.
    pub actions: &'a [TopBarAction],
    /// The menu button first on the left (☰: the app's menus in a drawer), and whether its
    /// drawer is open. `None`: none (macOS' own menu bar).
    pub menu: Option<bool>,
}

/// A button registered on the bar's right side, as drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TopBarAction {
    pub id: &'static str,
    pub icon: IconId,
    /// Its group (what it's about): between neighbors of different groups a short line.
    pub group: &'static str,
    /// A switch (its state: lit while on, told to a screen reader as a toggle); `None`: a
    /// plain button.
    pub on: Option<bool>,
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
/// A window button's glyph and corners (logical pixels).
const CONTROL_GLYPH: f32 = 12.0;
const CONTROL_CORNER: f32 = 7.0;
/// The branch button's icon, and the height of the badge after it (logical pixels).
const BRANCH_ICON: f32 = 15.0;
const BADGE_H: f32 = 20.0;

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
    /// The window's button pressed and not yet let go (drawn lit while the mouse is on it).
    armed: Option<TopBarHit>,
    /// The lines between the registered buttons' groups (from the last paint).
    separators: Vec<Rect>,
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

    /// The window's button `hit` pressed (`Some`), or let go (`None`): it acts on its
    /// release over it, and looks pressed meanwhile.
    pub fn arm(&mut self, hit: Option<TopBarHit>) {
        self.armed = hit;
    }

    /// The window's button pressed and not let go yet.
    pub fn armed(&self) -> Option<TopBarHit> {
        self.armed
    }

    /// The lines drawn between the registered buttons' groups, as last painted.
    pub fn separators(&self) -> &[Rect] {
        &self.separators
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
        let ctx = ds::Ctx::new(palette, s);
        self.occupied.clear();
        if model.full_screen && model.controls.is_some() {
            layer.quad(Quad::filled(b, palette.window_background));
            ds::divider(layer, &ctx, b.min_x(), b.max_x(), b.max_y() - ctx.line());
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
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon("menu")
                .rest_tone(ds::Tone::Neutral)
                .hovered(open || self.hover == Some(TopBarHit::Menu));
            ds::button(layer, text, icons, &ctx, self.menu, spec);
            self.occupied.push(icon_room(self.menu, px(ICON)));
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
        // A ghost button: its avatar (or a folder without a project), its name, its ▾.
        let mut spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
            .label(name)
            .font(&bold)
            .icon_size(PROJECT_AVATAR)
            .menu(true)
            .corner(PROJECT_RADIUS)
            .start(PROJECT_PAD)
            .colors(ds::ButtonColors {
                icon: palette.text,
                hover: palette.tab_hover,
                on_back: Some(palette.tab_hover),
                on_icon: palette.text,
            })
            .hovered(menu_open || self.hover == Some(TopBarHit::Projects));
        spec = match model.project {
            Some(name) => spec.avatar(ds::Avatar::new(name, tag_colors::project(name)).solid()),
            None => spec.icon("folder").icon_color(palette.text_muted),
        };
        let width = ds::button_width(text, &ctx, &spec, pill_h);
        self.project = Rect::new(x, pill_top, width, pill_h);
        ds::button(layer, text, icons, &ctx, self.project, spec);
        let avatar = px(PROJECT_AVATAR);
        self.occupied.push(Rect::new(
            x + px(PROJECT_PAD),
            (pill_top + (pill_h - avatar) / 2.0).round(),
            width - 2.0 * px(PROJECT_PAD),
            avatar,
        ));
        x = self.project.max_x() + px(14.0);

        // The git branch: shown, or a button (a module's list to switch it) with a chevron.
        self.branch = Rect::default();
        if let Some(branch) = model.branch {
            let from = x;
            let icon = px(15.0);
            if model.branch_button {
                let h = px(ds::button_size::MEDIUM);
                let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                    .icon("git-branch")
                    .label(branch)
                    .icon_size(BRANCH_ICON)
                    .menu(true)
                    .rest_tone(ds::Tone::Neutral)
                    .hovered(self.hover == Some(TopBarHit::Branch));
                let w = ds::button_width(text, &ctx, &spec, h);
                let r =
                    Rect::new(x - px(8.0), (b.min_y() + (b.size.height - h) / 2.0).round(), w, h);
                ds::button(layer, text, icons, &ctx, r, spec);
                self.branch = r;
                x = r.max_x() - px(8.0);
            } else {
                let shaped = text.shape_line(branch, &font);
                if let Some(id) = icons.id("git-branch") {
                    let r = Rect::new(x, b.min_y() + (b.size.height - icon) / 2.0, icon, icon);
                    draw_icon(layer, icons, id, r, palette.text_muted);
                }
                x += icon + px(6.0);
                draw_text(layer, &shaped, x, baseline, palette.text_muted);
                x += shaped.width;
            }
            let h = px(16.0);
            let top = b.min_y() + (b.size.height - h) / 2.0;
            self.occupied.push(Rect::new(from, top, x - from, h));
            x += px(12.0) + if model.branch_button { px(8.0) } else { 0.0 };
        }
        // A module's word on this project (its shortcut), in a small frame.
        self.badge = Rect::default();
        if let Some(badge) = model.badge {
            let h = px(BADGE_H);
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Soft)
                .label(badge)
                .rest_tone(ds::Tone::Neutral)
                .hovered(self.hover == Some(TopBarHit::Badge));
            let w = ds::button_width(text, &ctx, &spec, h);
            let r = Rect::new(x, (b.min_y() + (b.size.height - h) / 2.0).round(), w, h);
            ds::button(layer, text, icons, &ctx, r, spec);
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
                // Pressed: lit while the mouse stays on it (its release acts).
                let hovered = (self.hover == Some(hit) && self.armed.is_none_or(|a| a == hit))
                    || (hit == TopBarHit::Maximize && model.maximize_hovered);
                let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                    .icon(name)
                    .icon_size(CONTROL_GLYPH)
                    .corner(CONTROL_CORNER)
                    .rest_tone(ds::Tone::Neutral)
                    .hovered(hovered);
                let spec =
                    if hit == TopBarHit::Close { spec.hover_tone(ds::Tone::Danger) } else { spec };
                ds::button(layer, text, icons, &ctx, rect, spec);
            }
        }

        // Right: the registered buttons (search, settings, on top), left of the window's;
        // a short line between two of different groups.
        let button = px(ds::button_size::LARGE);
        let (gap, room) = (px(ACTION_GAP), px(GROUP_ROOM));
        let (line_h, line_w) = (px(SEPARATOR_H), ctx.line());
        self.actions.clear();
        self.separators.clear();
        let mut x = right - px(10.0) - button;
        let mut after: Option<&str> = None;
        for action in model.actions.iter().rev() {
            if after.is_some_and(|g| g != action.group) {
                // Centered in the room between this one and the one after it.
                let next = x + button + gap;
                let line = Rect::new(
                    (next - (room + line_w) / 2.0).round(),
                    (b.min_y() + (b.size.height - line_h) / 2.0).round(),
                    line_w,
                    line_h,
                );
                ds::divider_vertical(layer, &ctx, line.min_x(), line.min_y(), line.max_y());
                self.separators.push(line);
                x -= room - gap;
            }
            after = Some(action.group);
            let rect = Rect::new(x, pill_top, button, button);
            let spec = ds::ButtonSpec::new(ds::ButtonKind::Ghost)
                .icon_id(action.icon)
                .rest_tone(ds::Tone::Neutral)
                .hovered(self.hover == Some(TopBarHit::Action(action.id)));
            let spec = match action.on {
                Some(on) => spec.toggle(on),
                None => spec,
            };
            ds::button(layer, text, icons, &ctx, rect, spec);
            let r = icon_room(rect, px(ICON));
            self.actions.push((rect, action.id));
            self.occupied.push(r);
            x -= button + gap;
        }
    }
}

/// The room between two registered buttons, and between two of different groups (where
/// their line goes), logical pixels.
const ACTION_GAP: f32 = 4.0;
const GROUP_ROOM: f32 = 13.0;
/// The line between two groups: as tall as an icon.
const SEPARATOR_H: f32 = 16.0;

/// The room a bar button's icon takes (what the theme's decorations keep clear), logical
/// pixels.
const ICON: f32 = 16.0;

/// The icon's room, `size` across, centered in a button at `rect`.
fn icon_room(rect: Rect, size: f32) -> Rect {
    Rect::new(
        (rect.min_x() + (rect.size.width - size) / 2.0).round(),
        (rect.min_y() + (rect.size.height - size) / 2.0).round(),
        size,
        size,
    )
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
            let color = tag_colors::project(name).to_rgba8();
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

/// The project's button in the bar: its avatar's side, its corners, the room before its
/// avatar (logical pixels).
const PROJECT_AVATAR: f32 = 20.0;
const PROJECT_RADIUS: f32 = 7.0;
const PROJECT_PAD: f32 = 8.0;

#[cfg(test)]
#[path = "../tests/unit/top_bar.rs"]
mod tests;
