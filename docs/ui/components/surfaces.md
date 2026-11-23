# Context, surfaces, dividers, row highlight (`ui_components::ds`)

**For:** what every other component stands on: the drawing context (`Ctx`: the palette,
the scale, the appearing opacity), the tokens, a floating surface and its shadow, the
backdrop under a modal, the keyboard's focus ring, a list row's highlight, the
separators.

**Where it's used:** everywhere. `surface` / `surface_with` by every popover, menu, card
and dialog; `RowHighlight` by about forty lists (pickers, trees, menus, the Task Manager);
`divider_card` under panel headers; `focus_ring` by the views that move the keys between
their parts; `backdrop*` by the overlay stack.

## Anatomy

```
 surface       ░░┌───────────────┐░░   shadow: 10 concentric faint layers, a little lower
               ░░│ popup_background│░   border: popup_border, a line wide
               ░░└───────────────┘░░
 focus_ring    ┏━━━━━━━━━━━━━━━━━━┓     2 px accent (60%) just outside the rect
 row highlight │▓▓ chosen ▓▓▓▓▓▓▓▓│     accent 10% · hovered: text 4.5% · inactive: text 7%
 dividers      ───────  │  ╌╌─╌╌  ──╌╌·    plain, vertical, faded both ends, faded right
```

## Options

**Ctx** — `Ctx::new(palette, scale)`, `.with_opacity(o)`; `px(v)` (logical → physical,
rounded), `line()` (a crisp line, at least 1 physical), `fade(c)` (the appearing opacity),
`ui_font(Δ)`, `code_font(Δ)`. `Pen { layer, text, ds }` lends the three together to a
part's helper (`.lend()`). `optical_baseline(top, height, font)` centers text by its cap
height. Tokens: `space::*`, `radius::*`, `POPOVER_GAP`, `Elevation`, `Tone` (see
[tokens.md](../tokens.md)).

**Surfaces**

| Function | Effect |
|---|---|
| `surface(layer, ctx, rect, Elevation, r)` | the popup background with its shadow and border |
| `surface_with(…, background)` | the same on a background of the caller's (the hover card's) |
| `focus_ring(layer, ctx, rect, r)` | the keys' ring around a control (`r` physical) |
| `backdrop(layer, ctx, viewport)` | a dialog's backdrop, fading with `ctx.opacity` |
| `backdrop_faded(layer, ctx, area, BackdropRole, shown, hole)` | a window's backdrop (glass blur when the theme asks), `shown` of it, leaving `hole` clear |
| `around(area, hole)` | the parts of `area` around a hole, at most four |

**RowHighlight** — `RowHighlight::new(selected, hovered)`, `.paint(layer, ctx, r)`;
`row_highlight(…)` is its defaults.

| Option | Default | Effect |
|---|---|---|
| `.inactive(bool)` | off | the chosen one's list doesn't have the keys: grey (`ROW_INACTIVE`) |
| `.radius(f32)` | `radius::MD` | its corners |
| `.hover(HoverFill)` | `Faint(ROW_HOVER)` | `Background` (`hover_background`), `Tree` (`tree_hover`), `Color(c)` |
| `.selected_alpha(f32)` | `ROW_SELECTED` (0.10) | the chosen one's accent alpha |
| `.selected_fill(Rgba)` | the accent's | the chosen one in a token (`tree_selection`) |
| `.outline(Rgba)` / `.outline_width(f32)` | none / a line | an outline wherever it's lit (a drop target) |
| `.fill(palette)` | — | its fill, if any, without drawing |

**Dividers** — all a line thick, faded with `ctx`:

| Function | Effect |
|---|---|
| `divider(layer, ctx, x0, x1, y)` | horizontal, `border` |
| `divider_card(…)` | horizontal, `card_border` (under a panel's header) |
| `divider_in(layer, ctx, (x0, x1, y), color)` | horizontal, a token of the caller's |
| `divider_vertical(layer, ctx, x, y0, y1)` / `divider_vertical_in(…, color)` | vertical |
| `divider_vertical_faded(…)` / `divider_horizontal_faded(…)` | strongest in the middle, clear at both ends |
| `divider_faded_right(…, color)` | strongest at its start (after a caption), clear at its end |

## States and interaction

None of their own: a row's highlight is told `selected`, `hovered`, `inactive` by its list;
a surface is placed by its owner; the focus ring is drawn by whoever has the focus.

## Accessibility

They draw no text and tell nothing: what stands on them describes itself (the modal is a
`Dialog`, a row a `ListItem`).

## Cost

- `surface`: eleven quads; `RowHighlight`: one; a faded divider: up to 24 strips.
- `backdrop_faded` collects the parts around its hole in a `Vec` (`around`) each paint.

## Example

```rust
let layer = scene.push_layer(Some(area));
ds::surface(layer, ds, panel, ds::Elevation::Medium, radius::LG);

ds::RowHighlight::new(false, row_hit.is_some() && self.hover == row_hit)
    .hover(ds::HoverFill::Faint(ROW_HOVER))
    .radius(radius::SM)
    .paint(rows, ds, r);

ds::divider_card(layer, &ds, b.min_x(), b.max_x(), y + px(38.0));
```

## Don't

- Pick a gray by hand: a role of the palette, or the text at a small alpha.
- Draw a shadow with a few hard quads: `surface` (or `brand::soft_shadow` for the app's own
  card).
- Light a row with a quad of its own: `RowHighlight`, its look an option.
