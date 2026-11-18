//! The theme's colors over draw.io's (82g): its own light or dark look chosen by the
//! theme's, then our configuration on top — the page's and the grid's colors, the colors it
//! offers, new shapes' and edges' colors (`default`: they follow the look without a fixed
//! color written in the file) and CSS for its interface (bars, panels, borders, text, the
//! accent) from the palette. Existing files keep their colors: a file looks the same to
//! everyone. Pure: made once per palette, not per frame.

use kernel::Rgba;
use serde_json::{Value, json};
use ui_components::Palette;

/// The colors taken from a palette, as `#rrggbb` (opaque: a translucent one is laid on what
/// it's drawn over).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Theme {
    pub dark: bool,
    /// The diagram's page.
    pub page: String,
    /// Around the page.
    pub workspace: String,
    /// Bars and panels.
    pub panel: String,
    pub border: String,
    pub text: String,
    pub muted: String,
    pub accent: String,
    /// A selected row, a pressed button: the accent, soft.
    pub selected: String,
    pub hover: String,
    /// Fields.
    pub field: String,
    pub field_border: String,
    /// Dialogs, menus, cards.
    pub popup: String,
    pub grid: String,
    /// The theme's own colors offered first in the color pickers (hex without `#`).
    pub offered: Vec<String>,
}

/// `color` laid on `under` (its alpha used up).
fn opaque(color: Rgba, under: Rgba) -> Rgba {
    under.mix(color.with_alpha(1.0), color.a.clamp(0.0, 1.0)).with_alpha(1.0)
}

impl Theme {
    pub fn from_palette(p: &Palette) -> Self {
        let bg = opaque(p.editor_background, Rgba::BLACK);
        let panel = opaque(p.panel_background, bg);
        let hex = |c: Rgba, under: Rgba| opaque(c, under).to_hex();
        let mut offered: Vec<String> = Vec::new();
        let mut offer = |c: Rgba| {
            let h = hex(c, bg).trim_start_matches('#').to_ascii_uppercase();
            if !offered.contains(&h) {
                offered.push(h);
            }
        };
        offer(p.accent);
        if let Some(s) = &p.syntax {
            for c in [s.keyword, s.string, s.function, s.type_name, s.number, s.constant] {
                offer(c);
            }
        }
        for c in [p.vcs_added, p.vcs_modified, p.warning, p.danger] {
            offer(c);
        }
        Self {
            dark: ui_components::tag_colors::is_dark(p),
            page: bg.to_hex(),
            workspace: hex(p.window_background, bg),
            panel: panel.to_hex(),
            border: hex(p.border, panel),
            text: hex(p.text, bg),
            muted: hex(p.text_muted, bg),
            accent: hex(p.accent, bg),
            selected: opaque(p.accent.with_alpha(0.22), panel).to_hex(),
            hover: hex(p.hover_background, panel),
            field: hex(p.input_background, panel),
            field_border: hex(p.input_border, panel),
            popup: hex(p.popup_background, bg),
            grid: bg.mix(opaque(p.text, bg), 0.12).to_hex(),
            offered,
        }
    }

    /// draw.io's look under ours: `ui` and `dark` in the page's address.
    pub fn look(&self) -> (&'static str, &'static str) {
        ("kennedy", if self.dark { "1" } else { "0" })
    }
}

/// draw.io's usual preset colors, after the theme's.
const PRESETS: [&str; 24] = [
    "E6D0DE", "CDA2BE", "B5739D", "E1D5E7", "C3ABD0", "A680B8", "D4E1F5", "A9C4EB", "7EA6E0",
    "D5E8D4", "9AC7BF", "67AB9F", "D5E8D4", "B9E0A5", "97D077", "FFF2CC", "FFE599", "FFD966",
    "FFF4C3", "FFCE9F", "FFB570", "F8CECC", "F19C99", "EA6B66",
];

/// What draw.io's menus offer that can't work here, hidden (`hideMenuItems`, its own
/// configuration): importing from the browser's storage or a picker the page can't open,
/// printing (a new window), exports made by its servers (PDF, VSDX, links, embedding, the
/// advanced export), its pages opened in a new window, files of its own (new, open, save
/// as, libraries, recent ones), plugins and its configuration editor, the Exit button.
/// Save, the exports it makes itself (PNG, SVG, HTML, XML…: sent to us), Edit, View,
/// Arrange stay.
pub const HIDDEN_MENU_ITEMS: [&str; 26] = [
    "importFrom",
    "import",
    "print",
    "exportPdf",
    "exportVsdx",
    "exportUrl",
    "publishLink",
    "embed",
    "export",
    "keyboardShortcuts",
    "plugins",
    "configuration",
    "new",
    "open",
    "openFrom",
    "openRecent",
    "saveAs",
    "makeCopy",
    "share",
    "revisionHistory",
    "synchronize",
    "properties",
    "close",
    "newLibrary",
    "openLibraryFrom",
    "exit",
];

/// The configuration draw.io is given (`{action: 'configure', config}`).
pub fn config(theme: &Theme) -> Value {
    let mut presets: Vec<String> = theme.offered.clone();
    for p in PRESETS {
        if !presets.iter().any(|q| q == p) {
            presets.push(p.to_owned());
        }
    }
    presets.truncate(36);
    let mut config = json!({
        // Links come to us ({event: 'openLink'}), never a window of its own.
        "suppressNewWindows": true,
        "hideMenuItems": HIDDEN_MENU_ITEMS,
        "presetColors": presets,
        // New shapes and edges follow the look (light or dark) without a color of their own.
        "defaultVertexStyle": {
            "fillColor": "default",
            "strokeColor": "default",
            "fontColor": "default",
        },
        "defaultEdgeStyle": {
            "edgeStyle": "orthogonalEdgeStyle",
            "rounded": "0",
            "jettySize": "auto",
            "orthogonalLoop": "1",
            "strokeColor": "default",
            "fontColor": "default",
        },
        "css": css(theme),
    });
    if let Value::Object(map) = &mut config {
        if theme.dark {
            // The dark look's page, and what `default` means on it.
            map.insert("darkColor".into(), json!(theme.page));
            map.insert("lightColor".into(), json!(theme.text));
            map.insert("defaultDarkPageBackgroundColor".into(), json!(theme.page));
            map.insert("defaultDarkGridColor".into(), json!(theme.grid));
        } else {
            map.insert("defaultPageBackgroundColor".into(), json!(theme.page));
            map.insert("defaultGridColor".into(), json!(theme.grid));
        }
    }
    config
}

/// CSS over draw.io's interface: its variables (both looks') set from the theme.
pub fn css(theme: &Theme) -> String {
    let both = [
        ("ge-panel-color", "ge-dark-panel-color", &theme.panel),
        ("toolbar-color", "dark-toolbar-color", &theme.panel),
        ("border-color", "dark-border-color", &theme.border),
        ("text-color", "dark-text-color", &theme.text),
        ("strong-text-color", "dark-strong-text-color", &theme.text),
        ("secondary-text-color", "dark-secondary-text-color", &theme.muted),
        ("faint-text-color", "dark-faint-text-color", &theme.muted),
        ("accent-color", "dark-accent-color", &theme.selected),
        ("accent-text-color", "dark-accent-text-color", &theme.accent),
        ("focus-color", "dark-focus-color", &theme.accent),
        ("highlight-color", "dark-highlight-color", &theme.hover),
        ("soft-hover-color", "dark-soft-hover-color", &theme.hover),
        ("dialog-color", "dark-dialog-color", &theme.popup),
        ("card-color", "dark-card-color", &theme.popup),
        ("field-color", "dark-field-color", &theme.field),
        ("field-border-color", "dark-field-border-color", &theme.field_border),
        ("strong-border-color", "dark-strong-border-color", &theme.field_border),
        ("scrollbar-color", "dark-scrollbar-color", &theme.border),
    ];
    let mut out = format!(":root{{--workspace-color:{};", theme.workspace);
    for (light, dark, value) in both {
        out.push_str(&format!("--{light}:{value};--{dark}:{value};"));
    }
    out.push_str(&format!(
        "--dark-header-color:{panel};--inactive-color:{panel};--active-color:{sel};\
         --primary-color:{sel};--dark-active-accent-color:{sel};}}",
        panel = theme.panel,
        sel = theme.selected,
    ));
    // The bars and panels, and the text they draw.
    out.push_str(&format!(
        ".geMenubarContainer,.geToolbarContainer,.geSidebarContainer,.geFormatContainer,\
         .geFooterContainer,.geTabContainer,.geHsplit,.geVsplit{{background-color:{panel};\
         color:{text};border-color:{border};}}",
        panel = theme.panel,
        text = theme.text,
        border = theme.border,
    ));
    out
}

#[cfg(test)]
#[path = "../tests/unit/theme.rs"]
mod tests;
