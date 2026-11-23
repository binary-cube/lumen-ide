# Charts (`ui_components::charts`)

**For:** every chart someone reads — a query's result, a table's rows, any data file, the
Task Manager's history. Small charts with no state (a table cell's, a card's, a screen
widget's) are `charts::spark`.

**Where it's used:** the `charts` module (result panes, "Open as Chart"), the Task
Manager's detail, the profiler's timeline (lanes), the folder sizes' ring (a pie with a
hole, its list beside it as its legend).

## Anatomy

```
 ┌ legend (2+ series, a pie's slices) ───────────────┐
 │ values' ticks │ grid ─────────────────────────────│
 │               │ marks: bars · lines · areas ·      │
 │               │        points · bubbles · slices   │
 │               │ ┆ guide and dots under the mouse   │
 │               └─ categories' / X's ticks ──────────│
 └────────────────────────────────────────────────────┘
```

Three layers, each one thing:

- `charts_model` (domain): `prepare` (rows → categories, series, points, slices, bins),
  `scale` (nice ticks, time, log), `geometry` (`lay_out` → marks; `Geometry::hit`), `svg`.
- `charts::Chart` (this component): lays out when its fingerprint changes, rasterizes
  lines, areas and slices on the `chart-raster` pool, draws bars and points from quads, its
  labels as text.
- `ChartLook`: colors, fonts and parts from the theme.

## Options

| Option | Default | Effect |
|---|---|---|
| `ChartLook::new(ctx, background)` | the theme's | named series colors, grid, labels |
| `.colors(&[…])` | named colors | a caller's series colors first |
| `.parts(ChartParts)` | all | `x_axis`, `y_axis`, `grid`, `legend`, `hover`, `zoom`; `ChartParts::bare()` for a widget |
| `set_options(Options)` | none | `stepped`, `points`, `percent` (stacked to 100%), `log`, `hole` (a pie as a ring: its hole, percent of the radius) |
| `set_lit(Some(i))` | none | a pie's slice lit from outside (its row under the mouse in a list beside it) |
| `slice_at(p)` | — | the pie's slice under `p` (what a click on it is about) |
| `Prepared::slices(named, rest)` | — | a pie of values already counted (sizes), the smaller ones as one |
| `set_format(ValueFormat)` | four significant digits | values in their unit (tooltip, values' axis) |
| `set_y_range(Some((lo, hi)))` | from the values | a fixed axis (a share 0–100) |
| `set_lanes(true)` | off | each series in its own band, on its own scale, along one X, one tooltip for all |
| `set_x_format(ValueFormat)` | numbers, dates, categories | X's ticks written as the caller says (durations `1:20`) |
| `set_title` | — | its name to a screen reader |

## States and interaction

- Hover: the bar / point / slice lit (a slice comes out of the pie), or a category's
  column (guide, a dot on each line); its tooltip.
- Legend: a press hides or shows its series.
- Measured X: wheel zoom around the mouse, drag to move, double click for all.
- Loading: the last masks stay drawn while new ones are made.

## Accessibility

An image named by its title, its value a summary (`bar, 12 × 2, max 4 210`).

## Cost

- Layout: only when its fingerprint changes (data generation, area, scale, hidden,
  zoom, options, parts, axis); tick labels formatted and cut then.
- Lines: at most two points per pixel column (`geometry::thin`); masks per layout and
  theme, made on a worker.
- An identical repaint allocates nothing (the Task Manager's test holds it).

## Example

```rust
let look = ChartLook::new(&ctx, palette.editor_background).parts(ChartParts::bare());
chart.set_data(Some(Arc::new(prepared)), generation);
chart.set_format(Some(ValueFormat(Arc::new(|_, v| format!("{v:.1}%")))));
chart.paint(layer, text, &ctx, &look, rect);
// and its owner: chart.poll() in poll, chart.hover / press / scroll / tooltip as a Control.
```

## Don't

- Draw a chart's axes, grid or tooltip by hand: an option of the component instead.
- Prepare rows on the UI thread for more than a handful: `prepare` on a pool.
- Make a `Chart` per table cell: that's `spark::sparkline`.
