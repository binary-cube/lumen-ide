//! A source's icon by its driver: the one its driver names (an installed driver's, among
//! the editor's), else the database's one, the same for every engine, server or file — told
//! from what's in memory (the drivers' registry), never the disk.

use database::IconTone;
use kernel::Rgba;
use ui_components::Palette;

/// Driver `id`'s icon and tone.
pub fn of(id: &str) -> (&'static str, IconTone) {
    database::drivers::find(id)
        .and_then(|f| f.icon)
        .map_or(("database", IconTone::Neutral), |icon| (icon.name, icon.tone))
}

/// The color icon `name` is drawn in: the theme's `icon.<name>`, else its tone's terminal
/// color, else (neutral) `neutral`.
pub fn color(name: &str, tone: IconTone, p: &Palette, neutral: Rgba) -> Rgba {
    if let Some(&color) = p.icon_colors.get(name) {
        return color;
    }
    let ansi = |i: usize| p.terminal_ansi[i];
    match tone {
        IconTone::Neutral => neutral,
        IconTone::Red => ansi(1),
        IconTone::Green => ansi(2),
        IconTone::Yellow => ansi(3),
        IconTone::Blue => ansi(4),
        IconTone::Magenta => ansi(5),
        IconTone::Cyan => ansi(6),
    }
}

#[cfg(test)]
#[path = "../tests/unit/engine_icon.rs"]
mod tests;
