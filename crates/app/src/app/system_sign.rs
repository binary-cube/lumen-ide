//! The modules' sign outside the windows (the computer kept awake): handed to the platform
//! (an icon by the system's clock, a mark on the app's icon) when it changes — one for the
//! whole app, whichever window sees the change first. Its icon is drawn once per change.

use std::sync::Mutex;

use icons::Icons;

use super::*;

/// The sign the platform shows now (the app's: shared by its windows).
static SHOWN: Mutex<Option<workbench_model::SystemSign>> = Mutex::new(None);

/// The icon's side, in pixels: macOS' menu bar takes 18 points at 2×; Windows' notification
/// area 16 pixels at 100%, 32 at 200% (it scales it to the screen).
const SIDE: u16 = if cfg!(target_os = "macos") { 36 } else { 32 };

/// The mark on the app's icon, in pixels: macOS' Dock shows its tile at up to 128 points
/// (256 pixels at 2×), the mark about two fifths of it; Windows' overlay is 16 pixels at
/// 100%, 32 at 200%.
const BADGE: u16 = if cfg!(target_os = "macos") { 112 } else { 32 };

impl EditorApp {
    /// The modules' sign read again (cheap); changed, it goes to the platform.
    pub(super) fn sync_system_sign(&mut self) {
        let sign = self.workbench.system_sign();
        {
            let mut shown = SHOWN.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if *shown == sign {
                return;
            }
            shown.clone_from(&sign);
        }
        let Some(window) = &self.window else { return };
        let accent = self.workbench.palette().accent;
        window
            .set_system_sign(sign.and_then(|s| platform_sign(self.workbench.icons(), accent, &s)));
    }
}

/// `sign` as the platform takes it: its icon drawn (macOS: black, a template the system
/// draws in the menu bar's color; elsewhere in `accent`, seen on a light or a dark taskbar),
/// its menu (its action, then the row bringing the app to the front).
fn platform_sign(
    icons: &Icons,
    accent: kernel::Rgba,
    sign: &workbench_model::SystemSign,
) -> Option<platform_api::SystemSign> {
    let id = icons.id(sign.icon)?;
    let mask = icons.rasterize_key(icons.key(id, SIDE))?;
    let color = if cfg!(target_os = "macos") {
        [0, 0, 0]
    } else {
        [accent.r, accent.g, accent.b].map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8)
    };
    let rgba = mask.data.iter().flat_map(|&a| [color[0], color[1], color[2], a]).collect();
    let mut menu = Vec::new();
    if let Some((label, command)) = &sign.action {
        menu.push(platform_api::MenuItem::command(command.0, label, None));
    }
    let show = kernel::brand::named(&i18n::t("system_sign.show"));
    menu.push(platform_api::MenuItem::command(platform_api::SHOW_APP, &show, None));
    Some(platform_api::SystemSign {
        tooltip: sign.tooltip.clone(),
        icon: platform_api::MenuIcon { rgba, width: mask.width, height: mask.height },
        badge: badge(icons, id, accent)?,
        menu,
    })
}

/// The mark on the app's icon: a disc in `accent`, the icon `id` in white on it (the same
/// icon as everywhere else in the app).
fn badge(icons: &Icons, id: icons::IconId, accent: kernel::Rgba) -> Option<platform_api::MenuIcon> {
    let side = usize::from(BADGE);
    let glyph_side = BADGE * 3 / 5;
    let glyph = icons.rasterize_key(icons.key(id, glyph_side))?;
    let (gw, gh) = (glyph.width as usize, glyph.height as usize);
    let (ox, oy) = ((side - gw) / 2, (side - gh) / 2);
    let fill = [accent.r, accent.g, accent.b].map(|c| c.clamp(0.0, 1.0));
    let radius = side as f32 / 2.0;
    let mut rgba = vec![0u8; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            // The disc, its edge smoothed over a pixel.
            let (dx, dy) = (x as f32 + 0.5 - radius, y as f32 + 0.5 - radius);
            let disc = (radius - (dx * dx + dy * dy).sqrt() + 0.5).clamp(0.0, 1.0);
            let ink = match (x.checked_sub(ox), y.checked_sub(oy)) {
                (Some(gx), Some(gy)) if gx < gw && gy < gh => {
                    f32::from(glyph.data[gy * gw + gx]) / 255.0
                }
                _ => 0.0,
            };
            // White over the accent, inside the disc.
            let color = fill.map(|c| c + (1.0 - c) * ink);
            let at = (y * side + x) * 4;
            for (i, c) in color.iter().enumerate() {
                rgba[at + i] = (c * 255.0).round() as u8;
            }
            rgba[at + 3] = (disc * 255.0).round() as u8;
        }
    }
    Some(platform_api::MenuIcon { rgba, width: u32::from(BADGE), height: u32::from(BADGE) })
}

#[cfg(test)]
#[path = "../../tests/unit/app/system_sign.rs"]
mod tests;
