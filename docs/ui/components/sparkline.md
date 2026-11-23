# Sparklines (`ui_components::charts::spark`)

**For:** a small chart with no state, hundreds a frame for nothing: a table cell's
history, a card's, a screen widget's. A chart someone reads (axes, legend, tooltips, zoom)
is `charts::Chart` ([charts.md](charts.md)).

**Where it's used:** the metric cards (`metric_card`), the Task Manager's table and
summary, the resources widget on calm screens.

## Anatomy

```
 Steps:  ▁▁▂▂▂▅▅▃▃▁▁▂  a faint fill and a 1.5 px step on top (risers when buckets are wide)
 Bars:   ▁ ▂ ▂ ▅ ▃ ▁   a bar a bucket, a pixel apart when there's room, the newest brighter
         └── newest at the right of `slots` places; an empty history leaves the left blank
```

Three pieces, each one thing:

- `Samples`: a series as kept (a ring's two slices, never copied).
- `Plot` / `PlotCache`: the values on at most one bucket per pixel column (`Reduce::Max`
  keeps a spike, `Reduce::Last` a level), built only when the generation or the room changes.
- `sparkline(…)`: draws a `Plot` in a rectangle, at most two quads per pixel column.

## Options

| Option | Default | Effect |
|---|---|---|
| `Samples::new(&[f32])` / `from_slices((a, b))` | — | the values, oldest first |
| `PlotCache::get(generation, samples, slots, columns, reduce)` | — | the plot, built again only when one of these changes |
| `slots` | the values' count | the history's places (newest at the right) |
| `columns` | — | the pixel columns (buckets, at most `slots`) |
| `Reduce::Max` / `Last` | — | a rate (CPU) / a level (memory) |
| `style: Style::Steps` / `Bars` | — | an area in steps / bars (very small ones) |
| `max` | — | the scale's top: `scale_for(peak, floor)` rounds up nicely (1, 2, 2.5, 5 × 10ⁿ) |
| `color` | — | `series_color(palette, Series::Cpu…)`: the named color in the theme's shade |

## States and interaction

- Stateless: no hover of its own. Under the mouse its owner asks
  `Plot::reading(rect, x)`: the bucket, its value, how many places back it is, and the
  bucket's middle (where to draw a guide).
- Non-finite and negative values count as 0; a value above 0 is at least a pixel tall.

## Accessibility

None of its own: it's a drawing inside its owner (a card, a cell), which names it and
says its value.

## Cost

- Building: only when the series' generation, `slots`, `columns` or `reduce` changes
  (`PlotCache::is_stale` says so before asking); `Plot::fill` reuses its buckets.
- Drawing: runs of buckets of the same height are one quad; never more than two quads
  per physical pixel column; nothing allocated.
- 900 values aggregate within the budget test (`budget_900_values_are_aggregated_in_time`).

## Example

```rust
let samples = Samples::from_slices(history.total().get(m).as_slices());
self.plots[m as usize].get(generation, samples, slots, columns, reduce);
// …in paint:
spark::sparkline(layer, ds, chart, plot, series_color(p, Series::Cpu), Style::Steps, max);
```

## Don't

- Make a `charts::Chart` per table cell: this is the one for that.
- Call `Plot::build` every frame: keep a `PlotCache` per series.
- Pick a series' color by hand: `series_color` reads on both themes.
