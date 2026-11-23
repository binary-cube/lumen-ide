# The visual language

Every size and color comes from here, never a number or a hex value written in a view.
Sizes are logical pixels: `ctx.px(v)` turns them into physical ones (and rounds where an
edge must be crisp).

## Space and corners (`ds::space`, `ds::radius`)

| Token | Value | For |
|---|---|---|
| `space::XS` | 4 | inside a control (icon to label) |
| `space::SM` | 8 | between controls in a row |
| `space::MD` | 12 | a panel's padding |
| `space::LG` | 16 | between groups |
| `space::XL` | 24 | between sections |
| `radius::SM` | 6 | rows, small controls |
| `radius::MD` | 9 | fields, buttons, menus |
| `radius::CARD` | 10 | the workbench's cards |
| `radius::LG` / `XL` | 12 / 16 | big surfaces, modals |

## Sizes (`ds::button_size`, the components' own constants)

| Token | Value | For |
|---|---|---|
| `SMALL` | 22 | a pill in a thin bar |
| `COMPACT` | 24 | a compact header's icon |
| `MEDIUM` | 26 | a bar's icon button, a field's height in a panel |
| `LARGE` | 28 | a header's button |
| `ROOMY` / `SPACIOUS` | 30 / 32 | a roomy toolbar, the tool strip |

A band set apart from what it's under (a footer, a bar, a group's card) is tinted with
the text at `ds::BAND_TINT` (0.022).

An icon is about half its button's height (15 in 28, never under 12). Rows, bars and
headers have their constants beside their component (`table::HEADER_H`, `picker`'s
rows, `ds::MODAL_FOOTER_H`): use them, never a height of your own.

## Type (`ctx.ui_font(Δ)`)

One family (the user's interface font), sizes relative to its base: `ui_font(0.0)` for
reading text, `-0.5` for a field's value, `-1.0` for secondary text, `-1.5` for labels
and axis ticks, `-2.5` for counts in badges; `+1.0…+3.0` for titles. Weights:
`Font::SEMIBOLD` for titles and chosen items, never bold for emphasis in a sentence.
Code is the editor's font (`Monospace`).

## Colors by role (`Palette`)

Colors are the theme's, read by role — a theme changes them all at once:

| Role | Field |
|---|---|
| Text, secondary text | `text`, `text_muted` |
| The accent (chosen, primary, focus) | `accent` |
| Danger, warning | `danger`, `warning` |
| Surfaces | `editor_background`, `panel_background`, `sidebar_background`, `popup_background`, `input_background` |
| Lines | `border`, `input_border`, `card_border` |
| Rows | `tree_selection`, `tree_hover` (and `ds::RowHighlight`'s alphas) |
| Named colors (tags, series) | `tag_colors::named(name, palette)`: a shade for dark themes, one for light |

A faint tint is the text color with a small alpha (`p.text.with_alpha(0.07)` for a grid),
never a gray picked by hand: it reads on every theme. `ctx.fade(c)` applies a surface's
appearing opacity.

## Elevation and motion

- `ds::Elevation::Low` (tooltips, pills), `Medium` (popovers, menus), `High` (dialogs,
  the palette): `ds::surface` draws the surface and its shadow.
- Motion is `anim` (eased values asked each frame only while they move); a component asks
  for the next frame (`frame_deadline`, `animating`) only while something moves.
