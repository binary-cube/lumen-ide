# Settings forms (`ui_components::form`)

**For:** a panel holding many settings: a setting's row, rows grouped in a card, a slider
on one line with its value in words, a foldable card (a list entry with its options under
it), a gallery of cards with thumbnails, and the navigation between a form's categories.

**Where it's used:** the settings window (`setting_row`, `value_slider`), the theme
studio (all of it: `nav_list` / `nav_tabs`, `fold_card_head`, `gallery_card`).

## Anatomy

```
 nav_list (wide)          nav_tabs (narrow)
 EDITING ──────────       [◧ Colors] [✦ Effects 3] [⌨ Keys]   chosen underlined
 [◧ Colors       ]  ← chosen: tinted pill, icon in the accent
  ✦ Effects   ● 3   ← dot (unsaved), count badge

 group_quad ┌──────────────────────────────────────────────────┐
 setting_row│ ⚙ Label                         [ control ]  ↺   │  ↺ while modified
            │   One-line description (two when long)            │
 row_divider├──────────────────────────────────────────────────┤
 value_slider│ Speed        ●━━━━━◉─────────            Slow     │
            └──────────────────────────────────────────────────┘

 fold_card_head ┌ ⠿ [▣] Title                    [switch] ⋯ ⌄ ┐
                │       Background · dense · slow              │
 fold_section   ├ SECTION ─────────────────────────────────────┤
 gallery_card   ┌──────────┐  thumbnail room (the caller draws it),
                │ ▓▓▓▓▓▓▓▓ │  title, tag, a line; selected: ringed in the accent
                │ Title tag│
```

## Options

| Option | Default | Effect |
|---|---|---|
| `RowSpec { x, y, width, label }` | — | a setting's row |
| `.detail` | `None` | a muted line under the label (wrapped to two, then cut) |
| `.control_w` / `.control_h` | — | the control's room on the right (stacked under the label when narrower than `LABEL_MIN`) |
| `.icon` | `None` | an icon before the label (a module's) |
| `.modified` / `.reset_hovered` | — | "↺" shown, a mark at the left edge |
| `.highlight` | 0 | 0..1: a search result jumped to, fading out |
| `row_height(text, ctx, &spec)` | — | its height before drawing (the group's card goes under) |
| `ValueSliderSpec { x, mid, width, stops, current, value, hovered }` | — | the slider, ticks only up to 8 stops, `value` on its right in `VALUE_W` |
| `stepped(current, stops, ±1, big)` | — | a key's step (⇧: a quarter of the stops) |
| `FoldCardSpec { …, icon, tint, title, summary, expanded, enabled, hovered }` | — | a foldable card's header; `fold_card_quad` its background |
| `fold_section(…)` | — | a section's header inside an open card |
| `Grid::new(x, y, width, min_cell, cell_h, gap)` | — | columns fitted to the width; `cell(i)`, `height(n)` |
| `GalleryCardSpec { rect, thumb_h, title, detail, badge, selected, hovered }` | — | a gallery's card; returns the thumbnail's room |
| `Nav::new(items, selected, hovered)` | — | the categories (`NavItem { icon, label, count, dot }`) |
| `NavPlace::new(x, y, width)` | rows `NAV_ROW` (36) | where a `nav_list` goes; `row_h` shorter for a long list |
| `nav_caption(pen, (x, y, w), caption, active)` | — | a group's caption, the open one's in the accent |

## States and interaction

- Stateless: each part draws what it's given and returns where its parts went
  (`RowArea`, `FoldCardArea`, `SliderArea`, the thumbnail's `Rect`); `nav_list` and
  `nav_tabs` hand each row's area to `each`. The owner keeps its hit table.
- Hover is told by the owner (`hovered`, `reset_hovered`, `FoldPart`).
- The keys are the owner's: `stepped` moves a slider.

## Accessibility

- `nav_list` / `nav_tabs`: each category is a tab, named by its label, selected while its
  page shows (its count read under it).
- The rows, the fold cards and the gallery cards don't describe themselves yet; their
  buttons (↺, the switch, ⋯) do through `ds` (see the audit).

## Cost

- No state, nothing kept. A row's description is wrapped on each call (`row_height` and
  `setting_row` both wrap it) and the count badges and captions build their strings per
  paint (see the audit).

## Example

```rust
form::nav_list(
    &mut LayerPainter::new(layer, ctx.text, ctx.icons, ds),
    form::NavPlace::new(nav.min_x() + pad, nav.min_y() + ds.px(18.0), nav.size.width - 2.0 * pad),
    form::Nav::new(&nav_items, selected, hovered),
    |i, r| nav_hits.push((r, i)),
);
let slider = form::value_slider(
    rows.layer,
    rows.text,
    ctx,
    form::ValueSliderSpec {
        x: area.control.min_x(),
        mid: area.control.min_y() + area.control.size.height / 2.0,
        width: control_w,
        stops: stops.len(),
        current: at,
        value: stops.get(at).map_or("", |(_, l)| l.as_str()),
        hovered: lit,
    },
);
```

## Don't

- Draw a settings row by hand in a module: a `SettingsContribution` with the shared rows,
  or these parts in a panel of its own.
- Type values as `a=b, c=d`: one `Dropdown` (or slider) per value.
- Put a group's card over its rows: `group_quad` goes under them, once their height is known.
