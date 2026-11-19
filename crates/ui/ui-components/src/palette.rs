//! The UI color palette and dimensions.
//!
//! The palette is built from the active theme ([`Palette::from_theme`]); views take their
//! colors only from here, no color is hard-coded in them.

use std::collections::HashMap;

use kernel::Rgba;
use text_layout::FontFamily;
use theming::{Appearance, ResolvedTheme};

use crate::backdrop::{BackdropSpec, BackdropStyle};

#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    /// The window background, on which the sidebar and the main card sit.
    pub window_background: Rgba,
    /// The main card's border (tabs + editor).
    pub card_border: Rgba,
    pub editor_background: Rgba,
    pub panel_background: Rgba,
    pub border: Rgba,
    pub text: Rgba,
    pub text_muted: Rgba,
    pub accent: Rgba,
    pub danger: Rgba,
    /// Warnings (e.g. something deprecated).
    pub warning: Rgba,
    /// Lines changed since the last commit (gutter bars): added, modified, deleted.
    pub vcs_added: Rgba,
    pub vcs_modified: Rgba,
    pub vcs_deleted: Rgba,
    /// A changed file's badge in a list (filled with these, its sign over them).
    pub vcs_badges: VcsBadges,

    pub code_text: Rgba,
    pub gutter_text: Rgba,
    pub gutter_text_active: Rgba,
    pub current_line: Rgba,
    pub selection: Rgba,
    pub selection_inactive: Rgba,
    pub caret: Rgba,
    pub search_match: Rgba,
    pub search_match_current: Rgba,

    pub tab_hover: Rgba,
    /// How intense the project color is in the top bar (0 = none).
    pub top_bar_tint: f32,
    /// The background of the tab bar and the active tab (the tab joined to the editor).
    pub tab_bar_background: Rgba,
    pub tab_active_background: Rgba,
    /// The tab under the cursor: between the bar and the active tab, to stay subtler than it.
    pub tab_hover_background: Rgba,
    /// The thin accent line at the top of the active tab.
    pub tab_active_indicator: Rgba,
    pub input_background: Rgba,
    pub input_border: Rgba,
    pub popup_background: Rgba,
    /// The code hover card's background: set apart from the editor (`hover.background`).
    pub hover_background: Rgba,
    pub popup_border: Rgba,
    pub shadow: Rgba,
    /// The backdrop under the windows over the editor (modals, dialogs, the menu drawer).
    pub backdrop: crate::backdrop::BackdropStyle,
    /// The floating panels' glass (the menu drawer's).
    pub floating: crate::backdrop::SurfaceStyle,
    pub toggle_active: Rgba,

    pub tree_selection: Rgba,
    pub tree_hover: Rgba,
    pub indent_guide: Rgba,
    /// The editor's indent guides (the blocks' vertical lines), and the cursor's block's.
    pub editor_indent_guide: Rgba,
    pub editor_indent_guide_active: Rgba,

    /// The background of the side panels (the project card).
    pub sidebar_background: Rgba,
    /// The search field in the project panel.
    pub search_field_background: Rgba,
    pub search_field_border: Rgba,
    pub search_field_border_focused: Rgba,
    pub tool_strip_icon: Rgba,
    pub tool_strip_hover: Rgba,
    pub tool_strip_active_background: Rgba,
    pub tool_strip_active_icon: Rgba,

    /// The terminal: background, text, cursor and the 16 ANSI colors (0–7 normal, 8–15 bright).
    pub terminal_background: Rgba,
    pub terminal_foreground: Rgba,
    pub terminal_cursor: Rgba,
    pub terminal_ansi: [Rgba; 16],
    /// The theme's syntax colors, for code drawn outside the editor (hover cards, canvas,
    /// compare); `None` before a theme is applied.
    pub syntax: Option<SyntaxColors>,

    pub scrollbar_track: Rgba,
    pub scrollbar_thumb: Rgba,
    pub scrollbar_thumb_active: Rgba,

    pub splitter_line: Rgba,
    pub splitter_handle_background: Rgba,
    pub splitter_handle_border: Rgba,
    pub splitter_grip: Rgba,
    pub splitter_active: Rgba,

    pub folder_icon: Rgba,
    /// The file icon color per language (the theme's `icon.<language>` tokens).
    pub icon_colors: HashMap<String, Rgba>,

    pub ui_font: FontFamily,
    pub code_font: FontFamily,
    /// Dimensions in logical pixels.
    pub ui_font_size: f32,
    pub code_font_size: f32,
    pub code_line_height: f32,
}

/// The names of the theme's ANSI tokens: `terminal.ansi.<name>`.
pub const ANSI_NAMES: [&str; 16] = [
    "black",
    "red",
    "green",
    "yellow",
    "blue",
    "magenta",
    "cyan",
    "white",
    "brightBlack",
    "brightRed",
    "brightGreen",
    "brightYellow",
    "brightBlue",
    "brightMagenta",
    "brightCyan",
    "brightWhite",
];

/// The few syntax colors code outside the editor needs (no parser there: a lexical
/// coloring).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SyntaxColors {
    pub keyword: Rgba,
    pub variable: Rgba,
    pub string: Rgba,
    pub number: Rgba,
    pub function: Rgba,
    pub comment: Rgba,
    /// What a full highlighting (a preview's code, parsed) colors too.
    pub type_name: Rgba,
    pub constant: Rgba,
    pub property: Rgba,
    pub tag: Rgba,
    pub attribute: Rgba,
    pub operator: Rgba,
    pub punctuation: Rgba,
}

impl SyntaxColors {
    fn from_theme(theme: &ResolvedTheme, text: Rgba) -> Self {
        let color = |scopes: &[&str]| {
            scopes.iter().find_map(|s| theme.syntax(s).and_then(|st| st.foreground)).unwrap_or(text)
        };
        Self {
            keyword: color(&["keyword"]),
            variable: color(&["variable.php", "variable"]),
            string: color(&["string"]),
            number: color(&["number"]),
            function: color(&["function"]),
            comment: color(&["comment"]),
            type_name: color(&["type"]),
            constant: color(&["constant", "number"]),
            property: color(&["property", "variable"]),
            tag: color(&["tag", "keyword"]),
            attribute: color(&["attribute", "property"]),
            operator: color(&["operator"]),
            punctuation: color(&["punctuation"]),
        }
    }

    /// The color of a highlighting capture (`keyword.control`, `string.special`), by its
    /// first part; `None`: the code's own color.
    pub fn capture(&self, name: &str) -> Option<Rgba> {
        let base = name.split('.').next().unwrap_or(name);
        Some(match base {
            "keyword" | "conditional" | "repeat" | "include" | "exception" | "storage" => {
                self.keyword
            }
            "variable" | "parameter" => self.variable,
            "string" | "character" | "escape" | "regex" => self.string,
            "number" | "float" => self.number,
            "boolean" | "null" | "constant" => self.constant,
            "function" | "method" | "constructor" => self.function,
            "comment" => self.comment,
            "type" | "namespace" | "module" => self.type_name,
            "property" | "field" | "label" => self.property,
            "tag" => self.tag,
            "attribute" => self.attribute,
            "operator" => self.operator,
            "punctuation" | "delimiter" => self.punctuation,
            _ => return None,
        })
    }
}

/// The colors of the changed files' badges, by state (the theme's `vcs.badge.*`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VcsBadges {
    pub modified: Rgba,
    pub added: Rgba,
    pub deleted: Rgba,
    pub renamed: Rgba,
    pub untracked: Rgba,
    pub conflicted: Rgba,
}

impl Palette {
    /// Fallback values for dark themes (and the palette used before themes are
    /// loaded).
    pub fn dark() -> Self {
        Self {
            window_background: Rgba::rgb(0x151618),
            card_border: Rgba::rgb(0x2c2e33),
            editor_background: Rgba::rgb(0x1e1f22),
            panel_background: Rgba::rgb(0x2b2d30),
            border: Rgba::rgb(0x393b40),
            text: Rgba::rgb(0xdfe1e5),
            text_muted: Rgba::rgb(0x8a8e95),
            accent: Rgba::rgb(0x3574f0),
            danger: Rgba::rgb(0xf75464),
            warning: Rgba::rgb(0xe0a840),
            vcs_added: Rgba::rgb(0x5a9e60),
            vcs_modified: Rgba::rgb(0x4d86c9),
            vcs_deleted: Rgba::rgb(0x868a91),
            vcs_badges: VcsBadges {
                modified: Rgba::rgb(0xd9a53b),
                added: Rgba::rgb(0x4f9c5a),
                deleted: Rgba::rgb(0xd5525f),
                renamed: Rgba::rgb(0x4a82c4),
                untracked: Rgba::rgb(0x6d58a8),
                conflicted: Rgba::rgb(0xdb7434),
            },

            code_text: Rgba::rgb(0xbcbec4),
            gutter_text: Rgba::rgb(0x4b5059),
            gutter_text_active: Rgba::rgb(0xa1a3ab),
            current_line: Rgba::rgb(0x26282e),
            selection: Rgba::rgb(0x214283),
            selection_inactive: Rgba::rgb(0x2f3542),
            caret: Rgba::rgb(0xced0d6),
            search_match: Rgba::rgb(0xd9a343).with_alpha(0.22),
            search_match_current: Rgba::rgb(0xd9a343).with_alpha(0.55),

            tab_hover: Rgba::rgb(0x2b2d30),
            top_bar_tint: 0.22,
            tab_bar_background: Rgba::rgb(0x18191b),
            tab_active_background: Rgba::rgb(0x1e1f22),
            tab_hover_background: Rgba::rgb(0x232427),
            tab_active_indicator: Rgba::rgb(0x3574f0),
            input_background: Rgba::rgb(0x1e1f22),
            input_border: Rgba::rgb(0x4e5157),
            popup_background: Rgba::rgb(0x2b2d30),
            hover_background: Rgba::rgb(0x2b2d30),
            popup_border: Rgba::rgb(0x43454a),
            shadow: Rgba::rgb(0x000000).with_alpha(0.35),
            backdrop: BackdropStyle::resolve(BackdropSpec::default(), Rgba::rgb(0x151618)),
            floating: crate::backdrop::SurfaceStyle::default(),
            toggle_active: Rgba::rgb(0x3574f0).with_alpha(0.35),

            tree_selection: Rgba::rgb(0x2a2d33),
            tree_hover: Rgba::rgb(0x202227),
            indent_guide: Rgba::rgb(0x2a2c31),
            editor_indent_guide: Rgba::rgb(0x2a2c31),
            editor_indent_guide_active: Rgba::rgb(0x45484f),

            sidebar_background: Rgba::rgb(0x1b1c1f),
            search_field_background: Rgba::rgb(0x161719),
            search_field_border: Rgba::rgb(0x34363c),
            search_field_border_focused: Rgba::rgb(0x4a5a8a),
            tool_strip_icon: Rgba::rgb(0xa8abb2),
            tool_strip_hover: Rgba::rgb(0x26282d),
            tool_strip_active_background: Rgba::rgb(0x3574f0),
            tool_strip_active_icon: Rgba::WHITE,

            terminal_background: Rgba::rgb(0x1e1f22),
            terminal_foreground: Rgba::rgb(0xbcbec4),
            terminal_cursor: Rgba::rgb(0xced0d6),
            terminal_ansi: [
                Rgba::rgb(0x1e1f22),
                Rgba::rgb(0xf0524f),
                Rgba::rgb(0x5c962c),
                Rgba::rgb(0xa68a0d),
                Rgba::rgb(0x3993d4),
                Rgba::rgb(0xa771bf),
                Rgba::rgb(0x00a3a3),
                Rgba::rgb(0xbcbec4),
                Rgba::rgb(0x595b5f),
                Rgba::rgb(0xff4050),
                Rgba::rgb(0x4fc414),
                Rgba::rgb(0xe5bf00),
                Rgba::rgb(0x1fb0ff),
                Rgba::rgb(0xed7eed),
                Rgba::rgb(0x00e5e5),
                Rgba::rgb(0xffffff),
            ],

            syntax: None,
            scrollbar_track: Rgba::rgb(0x2a2c31),
            scrollbar_thumb: Rgba::rgb(0x4a4d55),
            scrollbar_thumb_active: Rgba::rgb(0x6b6f78),

            splitter_line: Rgba::rgb(0x2a2c31),
            splitter_handle_background: Rgba::rgb(0x1b1c1f),
            splitter_handle_border: Rgba::rgb(0x45484f),
            splitter_grip: Rgba::rgb(0x8a8e95),
            splitter_active: Rgba::rgb(0x7b7ff0),

            folder_icon: Rgba::rgb(0x8c96a6),
            icon_colors: HashMap::new(),

            ui_font: FontFamily::SansSerif,
            code_font: FontFamily::Monospace,
            ui_font_size: 13.0,
            code_font_size: 13.0,
            code_line_height: 1.6,
        }
    }
}

impl Palette {
    /// The style of the key hints in popup footers.
    pub fn key_hints(&self, scale: f32) -> ui_kit::KeyHintsStyle {
        let font = text_layout::Font::new(
            self.ui_font.clone(),
            (self.ui_font_size - 1.5).max(9.0) * scale,
        );
        let key_font = text_layout::Font::new(
            self.ui_font.clone(),
            (self.ui_font_size - 2.5).max(8.0) * scale,
        );
        ui_kit::KeyHintsStyle {
            font,
            key_font,
            text: self.text_muted,
            key_text: self.text_muted,
            key_background: self.popup_background,
            key_border: self.popup_border,
            scale,
        }
    }

    /// A theme's palette. Tokens missing from the theme keep the fallback values from
    /// [`Palette::dark`] / [`Palette::light_fallback`], so an incomplete theme doesn't break the
    /// UI.
    pub fn from_theme(theme: &ResolvedTheme) -> Self {
        let base = match theme.appearance {
            Appearance::Dark => Self::dark(),
            Appearance::Light => Self::light_fallback(),
        };
        let c = |token: &str, fallback: Rgba| theme.color_or(token, fallback);
        Self {
            window_background: c("window.background", base.window_background),
            card_border: c("card.border", base.card_border),
            editor_background: c("editor.background", base.editor_background),
            panel_background: c("panel.background", base.panel_background),
            border: c("border", base.border),
            text: c("text", base.text),
            text_muted: c("text.muted", base.text_muted),
            accent: c("accent", base.accent),
            danger: c("danger", base.danger),
            warning: c("warning", base.warning),
            // From the theme's own green / blue (its ANSI colors), unless it sets them.
            vcs_added: c("vcs.added", c("terminal.ansi.green", base.vcs_added)),
            vcs_modified: c("vcs.modified", c("terminal.ansi.blue", base.vcs_modified)),
            vcs_deleted: c("vcs.deleted", c("text.muted", base.vcs_deleted)),
            vcs_badges: VcsBadges {
                modified: c("vcs.badge.modified", base.vcs_badges.modified),
                added: c("vcs.badge.added", base.vcs_badges.added),
                deleted: c("vcs.badge.deleted", base.vcs_badges.deleted),
                renamed: c("vcs.badge.renamed", base.vcs_badges.renamed),
                untracked: c("vcs.badge.untracked", base.vcs_badges.untracked),
                conflicted: c("vcs.badge.conflicted", base.vcs_badges.conflicted),
            },
            code_text: c("editor.foreground", base.code_text),
            gutter_text: c("editor.gutter.foreground", base.gutter_text),
            gutter_text_active: c("editor.gutter.foreground.active", base.gutter_text_active),
            current_line: c("editor.line.current", base.current_line),
            selection: readable_selection(
                c("editor.selection", base.selection),
                theme.appearance,
                0.34,
            ),
            selection_inactive: readable_selection(
                c("editor.selection.inactive", base.selection_inactive),
                theme.appearance,
                0.22,
            ),
            caret: c("editor.cursor", base.caret),
            search_match: c("editor.match", base.search_match),
            search_match_current: c("editor.match.current", base.search_match_current),
            tab_hover: c("tab.hover", base.tab_hover),
            top_bar_tint: base.top_bar_tint,
            tab_bar_background: c("tabBar.background", base.tab_bar_background),
            tab_active_background: c(
                "tab.active.background",
                c("editor.background", base.tab_active_background),
            ),
            tab_hover_background: c("tab.hover.background", {
                let bar = c("tabBar.background", base.tab_bar_background);
                let active =
                    c("tab.active.background", c("editor.background", base.tab_active_background));
                let mix = |a: f32, b: f32| a + (b - a) * 0.5;
                Rgba::new(mix(bar.r, active.r), mix(bar.g, active.g), mix(bar.b, active.b), 1.0)
            }),
            tab_active_indicator: c("tab.active.indicator", c("accent", base.tab_active_indicator)),
            input_background: c("input.background", base.input_background),
            input_border: c("input.border", base.input_border),
            popup_background: c("popup.background", base.popup_background),
            // A popup as light as the editor would read as "white on white": tinted.
            hover_background: c("hover.background", {
                let popup = c("popup.background", base.popup_background);
                let editor = c("editor.background", base.editor_background);
                let text = c("text", base.text);
                let same = (popup.r - editor.r).abs()
                    + (popup.g - editor.g).abs()
                    + (popup.b - editor.b).abs()
                    < 0.04;
                if same { popup.mix(text, 0.035) } else { popup }
            }),
            popup_border: c("popup.border", base.popup_border),
            shadow: c("shadow", base.shadow),
            backdrop: BackdropStyle::resolve(
                theme.backdrop(),
                c("window.background", base.window_background),
            ),
            floating: crate::backdrop::SurfaceStyle::resolve(theme.floating()),
            toggle_active: c("toggle.active", base.toggle_active),
            tree_selection: c("tree.selection", base.tree_selection),
            tree_hover: c("tree.hover", base.tree_hover),
            indent_guide: c("tree.indentGuide", base.indent_guide),
            // From the theme's editor colors (subtle, any theme), unless it sets them.
            editor_indent_guide: c("editor.indentGuide", {
                let editor = c("editor.background", base.editor_background);
                editor.mix(c("editor.foreground", base.code_text), 0.08)
            }),
            editor_indent_guide_active: c("editor.indentGuide.active", {
                let editor = c("editor.background", base.editor_background);
                editor.mix(c("editor.foreground", base.code_text), 0.24)
            }),
            sidebar_background: c("sidebar.background", base.sidebar_background),
            search_field_background: c("searchField.background", base.search_field_background),
            search_field_border: c("searchField.border", base.search_field_border),
            search_field_border_focused: c(
                "searchField.border.focused",
                base.search_field_border_focused,
            ),
            tool_strip_icon: c("toolStrip.foreground", base.tool_strip_icon),
            tool_strip_hover: c("toolStrip.hover", base.tool_strip_hover),
            tool_strip_active_background: c(
                "toolStrip.active.background",
                base.tool_strip_active_background,
            ),
            tool_strip_active_icon: c("toolStrip.active.foreground", base.tool_strip_active_icon),
            terminal_background: c("terminal.background", base.terminal_background),
            terminal_foreground: c("terminal.foreground", base.terminal_foreground),
            terminal_cursor: c("terminal.cursor", base.terminal_cursor),
            terminal_ansi: std::array::from_fn(|i| {
                c(&format!("terminal.ansi.{}", ANSI_NAMES[i]), base.terminal_ansi[i])
            }),
            syntax: Some(SyntaxColors::from_theme(theme, c("editor.foreground", base.code_text))),
            scrollbar_track: c("scrollbar.track", base.scrollbar_track),
            scrollbar_thumb: c("scrollbar.thumb", base.scrollbar_thumb),
            scrollbar_thumb_active: c("scrollbar.thumb.active", base.scrollbar_thumb_active),
            splitter_line: c("splitter.line", base.splitter_line),
            splitter_handle_background: c(
                "splitter.handle.background",
                base.splitter_handle_background,
            ),
            splitter_handle_border: c("splitter.handle.border", base.splitter_handle_border),
            splitter_grip: c("splitter.grip", base.splitter_grip),
            splitter_active: c("splitter.active", base.splitter_active),
            folder_icon: c("icon.folder", base.folder_icon),
            icon_colors: theme
                .colors_with_prefix("icon.")
                .map(|(k, v)| (k.to_owned(), v))
                .collect(),
            ..base
        }
    }

    /// Fallback values for incomplete light themes.
    fn light_fallback() -> Self {
        Self {
            window_background: Rgba::rgb(0xf2f2f4),
            card_border: Rgba::rgb(0xe3e3e8),
            editor_background: Rgba::WHITE,
            panel_background: Rgba::rgb(0xf8f8fa),
            border: Rgba::rgb(0xe7e7ec),
            text: Rgba::rgb(0x1f2329),
            text_muted: Rgba::rgb(0x6b7079),
            warning: Rgba::rgb(0xb07d12),
            vcs_added: Rgba::rgb(0x5aa35f),
            vcs_modified: Rgba::rgb(0x3f7fd1),
            vcs_deleted: Rgba::rgb(0x9aa0a8),
            vcs_badges: VcsBadges {
                modified: Rgba::rgb(0xedbd4f),
                added: Rgba::rgb(0x3f9a4c),
                deleted: Rgba::rgb(0xd8434f),
                renamed: Rgba::rgb(0x3f7fd1),
                untracked: Rgba::rgb(0x5d4b8c),
                conflicted: Rgba::rgb(0xe07028),
            },
            code_text: Rgba::rgb(0x24292f),
            gutter_text: Rgba::rgb(0xb0b4bb),
            gutter_text_active: Rgba::rgb(0x50555e),
            current_line: Rgba::rgb(0xf6f7fb),
            selection: Rgba::rgb(0xdde3ff),
            selection_inactive: Rgba::rgb(0xeceef4),
            caret: Rgba::rgb(0x1f2329),
            tab_hover: Rgba::rgb(0xebebef),
            top_bar_tint: 0.16,
            tab_bar_background: Rgba::rgb(0xf6f6f8),
            tab_active_background: Rgba::WHITE,
            tab_hover_background: Rgba::rgb(0xebebef),
            tab_active_indicator: Rgba::rgb(0x4f46e5),
            input_background: Rgba::WHITE,
            input_border: Rgba::rgb(0xd9dbe1),
            popup_background: Rgba::WHITE,
            hover_background: Rgba::rgb(0xf7f8fa),
            popup_border: Rgba::rgb(0xe3e3e8),
            shadow: Rgba::BLACK.with_alpha(0.08),
            backdrop: BackdropStyle::resolve(BackdropSpec::default(), Rgba::rgb(0xf2f2f4)),
            floating: crate::backdrop::SurfaceStyle::default(),
            tree_selection: Rgba::rgb(0xe6e6eb),
            tree_hover: Rgba::rgb(0xebebef),
            indent_guide: Rgba::rgb(0xdedee4),
            editor_indent_guide: Rgba::rgb(0xe6e6ec),
            editor_indent_guide_active: Rgba::rgb(0xc4c5ce),
            sidebar_background: Rgba::rgb(0xfafafb),
            search_field_background: Rgba::WHITE,
            search_field_border: Rgba::rgb(0xdcdde3),
            search_field_border_focused: Rgba::rgb(0x9aa5e8),
            tool_strip_icon: Rgba::rgb(0x5e636d),
            tool_strip_hover: Rgba::rgb(0xe6e6eb),
            tool_strip_active_background: Rgba::rgb(0x3574f0),
            tool_strip_active_icon: Rgba::WHITE,
            terminal_background: Rgba::WHITE,
            terminal_foreground: Rgba::rgb(0x24292f),
            terminal_cursor: Rgba::rgb(0x1f2329),
            terminal_ansi: [
                Rgba::rgb(0x000000),
                Rgba::rgb(0xc91b00),
                Rgba::rgb(0x00a600),
                Rgba::rgb(0xa68a00),
                Rgba::rgb(0x0225c7),
                Rgba::rgb(0xc930c7),
                Rgba::rgb(0x00a5b2),
                Rgba::rgb(0xc7c7c7),
                Rgba::rgb(0x686868),
                Rgba::rgb(0xff6e67),
                Rgba::rgb(0x2ec92e),
                Rgba::rgb(0xb8a000),
                Rgba::rgb(0x5c6dff),
                Rgba::rgb(0xff77ff),
                Rgba::rgb(0x00b8c2),
                Rgba::rgb(0xffffff),
            ],
            syntax: None,
            scrollbar_track: Rgba::rgb(0xececf0),
            scrollbar_thumb: Rgba::rgb(0xc6c8cf),
            scrollbar_thumb_active: Rgba::rgb(0x9fa3ad),
            splitter_line: Rgba::rgb(0xe3e3e8),
            splitter_handle_background: Rgba::WHITE,
            splitter_handle_border: Rgba::rgb(0xd5d6dc),
            splitter_grip: Rgba::rgb(0x9a9ea7),
            splitter_active: Rgba::rgb(0x5b5bd6),
            folder_icon: Rgba::rgb(0x8a8f99),
            ..Self::dark()
        }
    }

    /// The file icon color for a language (defaults to the secondary text).
    /// The color an icon is drawn with (`Own`: its own colors — white, the alpha only).
    pub fn tint(&self, tint: &icons::Tint) -> Rgba {
        match tint {
            icons::Tint::Language(language) => self.file_icon(language),
            icons::Tint::Folder => self.folder_icon,
            icons::Tint::Own => Rgba::WHITE,
        }
    }

    pub fn file_icon(&self, language: &str) -> Rgba {
        self.icon_colors.get(language).copied().unwrap_or(self.text_muted)
    }

    /// A folder's color by its rule's key: the theme's `icon.<key>`, else the role's (sources
    /// in the accent, tests in green, libraries faded), else the folder's.
    pub fn folder_tint(&self, key: &str) -> Rgba {
        if let Some(&color) = self.icon_colors.get(key) {
            return color;
        }
        match key {
            "source" => self.accent,
            "test" => self.vcs_added,
            "library" => self.folder_icon.with_alpha(self.folder_icon.a * 0.55),
            _ => self.folder_icon,
        }
    }
}

impl Default for Palette {
    fn default() -> Self {
        Self::dark()
    }
}
/// A selection the text stays readable on: an opaque, vivid one (bright on a dark theme,
/// dark or saturated on a light one) is made translucent, keeping its hue; a quiet one is
/// left as the theme has it.
fn readable_selection(color: Rgba, appearance: Appearance, alpha: f32) -> Rgba {
    if color.a < 0.99 {
        return color;
    }
    // Relative luminance (sRGB, roughly linearized).
    let lin = |v: f32| v.powf(2.2);
    let luminance = 0.2126 * lin(color.r) + 0.7152 * lin(color.g) + 0.0722 * lin(color.b);
    let vivid = match appearance {
        Appearance::Dark => luminance > 0.12,
        Appearance::Light => luminance < 0.55,
    };
    if vivid { color.with_alpha(alpha) } else { color }
}

#[cfg(test)]
#[path = "../tests/unit/palette.rs"]
mod tests;
