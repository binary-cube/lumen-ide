//! The named colors a user marks things with (data sources' folders: production in red,
//! development in green): each in a shade of its own for dark themes and one for light
//! ones, so it reads on either background.

use kernel::Rgba;

use crate::palette::Palette;

/// The names, in the order they're offered.
pub const NAMES: [&str; 8] = ["red", "orange", "yellow", "green", "teal", "blue", "purple", "gray"];

/// The theme is dark (its editor's background is).
pub fn is_dark(palette: &Palette) -> bool {
    palette.editor_background.luminance() < 0.5
}

/// Named color `name` (one of [`NAMES`]) in the shade that reads on `palette`'s
/// background, or one picked (`#rrggbb`) as it is; `None` for a name that isn't one.
pub fn named(name: &str, palette: &Palette) -> Option<Rgba> {
    if name.starts_with('#') {
        return Rgba::from_hex(name);
    }
    let dark = is_dark(palette);
    let (on_dark, on_light) = match name {
        "red" => (0xF2555A, 0xCE2C31),
        "orange" => (0xF5873D, 0xC24E00),
        "yellow" => (0xF2C94C, 0x9E6C00),
        "green" => (0x4CC38A, 0x1F8A50),
        "teal" => (0x2EC4B6, 0x0B7F72),
        "blue" => (0x5B9BF8, 0x1D64D8),
        "purple" => (0xB07CF0, 0x7B3FC4),
        "gray" => (0x9A9DA6, 0x6B6F78),
        _ => return None,
    };
    Some(Rgba::rgb(if dark { on_dark } else { on_light }))
}

/// A color for `key` (a user's name, a group's), the same each time, among the named ones
/// (not gray), in the theme's shade: avatars told apart at a glance.
pub fn for_key(key: &str, palette: &Palette) -> Rgba {
    let hash = kernel::hash::fnv32(key);
    let colorful = &NAMES[..NAMES.len() - 1];
    let name = colorful[(hash % colorful.len() as u32) as usize];
    named(name, palette).unwrap_or(palette.accent)
}

/// A project's color (its badge in the top bar, its card among the open projects): the same
/// for the same name, whatever the theme.
pub fn project(name: &str) -> Rgba {
    const COLORS: [u32; 8] =
        [0xd9774b, 0x9b6ad6, 0x5b8def, 0x4fa37a, 0xc95f8e, 0x3d9ba8, 0xb08a3e, 0x7a7fe0];
    let hash = kernel::hash::fnv32(name);
    Rgba::rgb(COLORS[(hash % COLORS.len() as u32) as usize])
}

#[cfg(test)]
#[path = "../tests/unit/tag_colors.rs"]
mod tests;
