# Table parts (`ui_components::table`)

**For:** the look every table shares, whatever lists it: the header band, a column's name
with its muted subtitle, the edges between headers, the lines between columns, the
alternating rows. Tables keep their own structure and draw their cells; these are the parts
that must look the same.

**Where it's used:** the data grid, the Task Manager's table, the profiler's pages.

## Anatomy

```
 ┌ header_band ────────────────────────────────────────┐
 │  Name ↑         ╎  CPU          ╎  Memory           │ ← header_cell: name (+ order arrow)
 │  process        ╎  %            ╎  MB               │              subtitle, muted
 ├ header_rule ──────────────────────────────────────── ┤   header_edge between headers
 │ row 0                 │               │             │ ← column_lines
 │▒row 1▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒│▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒│▒▒▒▒▒▒▒▒▒▒▒▒▒│ ← stripe (every second)
```

Functions, no state:

- `header_band(layer, ctx, header)`, `header_band_rounded(scene, ctx, header, radius)` (a
  table among cards: its top corners follow the table's).
- `header_cell(layer, text, ctx, &HeaderCell, start)`: hover, name (cut with `…`, the
  order's arrow in the accent), subtitle; `start` leaves room a caller drew in (a key).
- `header_edge(layer, ctx, x, header, lit)`: short and faint; taller in the accent when
  held or under the mouse.
- `header_rule`, `column_lines(layer, ctx, edges, top, height, clip)`,
  `stripe(layer, ctx, row, index)`, `stripe_color(ctx)`.
- `header_hover(layer, ctx, rect)`: a column's header under the mouse (a faint rounded
  highlight), the same in every table, with or without `header_cell`.
- `name_font(ctx)`, `subtitle_font(ctx)`, `name_baseline(ctx, top)`,
  `subtitle_baseline(ctx, top)`, `HEADER_H` (44), `CELL_PAD` (10): a table that draws its
  own headers (the data grid's key and its order's place) places them with these.

## Options

| Option | Default | Effect |
|---|---|---|
| `HeaderCell { rect, name, subtitle, … }` | — | a column's header as drawn |
| `.subtitle` | `""` | none |
| `.right` | — | numbers: name and subtitle right-aligned |
| `.hovered` | — | a faint rounded highlight (a click sorts) |
| `.sorted: Some(descending)` | `None` | `↑` / `↓` in the accent after the name |
| `.strong` | — | the name in full color (else muted) |
| `.pad` | `CELL_PAD` | room between edges and texts (physical) |

## States and interaction

None of their own: the table drawing them keeps the hover, the edge held, the sort, and
does the resize (`columns::ColumnFit::edge_at` / `dragged`).

## Accessibility

None here: the table drawing them describes itself (the data grid: a `Table` of `Row`s).
A header cell isn't told as a column header yet.

## Cost

Quads and two shaped lines a header; nothing kept.

## Example

```rust
table::header_band(layer, ds, header);
let cell = HeaderCell { rect, name: &name, subtitle: &unit, right, hovered, sorted, strong, pad };
ui_components::table::header_cell(layer, text, ds, &cell, 0.0);
table::header_edge(layer, ds, right_x, header, lit);
table::stripe(layer, ds, line, i);
```

## Don't

- Draw a header's band, its edges or stripes with alphas of your own: these.
- Put a stripe or column lines on rows that span the table (a group's header, a progress).
