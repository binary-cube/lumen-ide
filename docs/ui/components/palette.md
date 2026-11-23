# The palette and named colors (`ui_components::palette`, `ui_components::tag_colors`)

**For:** every color a view draws with. The `Palette` is the active theme's colors by
role; `tag_colors` are the named colors a user marks things with (a data source's folder
in red), each in a shade that reads on the theme. No color is written in a view.

**Where it's used:** everywhere — through `Ctx` (`ctx.palette`) or a `…Look` made from
it. Named colors: data sources' folders and tabs, the database diagram, chart and
sparkline series (`spark::series_color`), avatars (`for_key`), a project's badge (`project`).

## Anatomy

```
 theme (TOML) ──▶ ResolvedTheme ──▶ Palette::from_theme ──▶ Ctx { palette, scale, opacity }
                                     │ missing tokens: Palette::dark() / light_fallback()
                                     │ backdrop, floating glass resolved (backdrop.md)
                                     └ fonts and sizes: ui_font, code_font, *_font_size

 tag_colors::named("green", palette) ─▶ #4CC38A on a dark theme · #1F8A50 on a light one
```

Roles (a theme changes them all at once): text (`text`, `text_muted`, `code_text`), the
accent, `danger`, `warning`, surfaces (`window_background`, `editor_background`,
`panel_background`, `sidebar_background`, `popup_background`, `input_background`),
lines (`border`, `card_border`, `input_border`, `popup_border`), rows (`tree_selection`,
`tree_hover`, `hover_background`), the editor's (`selection`, `caret`, `current_line`,
`search_match`…), version control (`vcs_*`, `vcs_badges`), the terminal (`terminal_*`,
16 ANSI colors), syntax (`SyntaxColors::capture(name)`), scrollbars, splitters, the tool
strip, icons (`icon_colors`, `folder_icon`).

## Options

| Option | Default | Effect |
|---|---|---|
| `Palette::from_theme(&theme)` | — | the theme's colors; missing tokens keep the fallbacks |
| `Palette::default()` / `dark()` | dark fallback | before themes load (and tests) |
| `key_hints(scale)` | — | the style of a footer's key hints |
| `tint(&Tint)` / `file_icon(lang)` / `folder_tint(key)` | `text_muted` / `folder_icon` | an icon's color |
| `tag_colors::NAMES` | 8 names | red, orange, yellow, green, teal, blue, purple, gray — in the order offered |
| `named(name, palette)` | `None` for an unknown name | the shade for the theme; a `#rrggbb` as it is |
| `for_key(key, palette)` | — | the same named color (not gray) for the same key: avatars |
| `project(name)` | — | a project's color, the same whatever the theme |
| `is_dark(palette)` | — | the editor's background is dark |

## States and interaction

None: values. A theme change makes a new palette; every view reads it the next frame.

## Accessibility

Every named color is checked to read on dark and light backgrounds
(`every_named_color_reads_on_dark_and_light_backgrounds`); a selection too vivid for its
text is made translucent (`vivid_selections_become_translucent_quiet_ones_stay`).
Meaning is never told by color alone (a sign beside it: `metric_card`'s warning).

## Cost

- Made once per theme change. `Ctx` borrows it: nothing copied per frame.
- `named` is a match; `for_key` a short hash.

## Example

```rust
let p = ctx.palette;
let fill = if card.hovered { p.text.with_alpha(0.06) } else { p.text.with_alpha(0.03) };
let border = if card.active { p.accent.with_alpha(0.75) } else { p.card_border };

// a data source's color on its tab:
fn accent(&self, palette: &Palette) -> Option<Rgba> {
    ui_components::tag_colors::named(&color.name(), palette)
}
```

## Don't

- Write a hex color or pick a gray in a view: a role, or the text with a small alpha
  (`p.text.with_alpha(0.07)`).
- Store a resolved color in settings: store its name (`"green"`), so it follows the theme.
- Clone the palette per frame (it holds the icon colors' map): borrow it.
