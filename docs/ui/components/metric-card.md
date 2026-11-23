# Metric cards (`ui_components::metric_card`)

**For:** one measure someone watches over time: what it is, its value now, a line under
it, and its history along the bottom. A threshold passed shows in the warning color *and*
with a sign, never by color alone.

**Where it's used:** the Task Manager's summary (CPU, memory, GPU, network, disk), the
resources widget on calm screens (`glass_metric_card`), the profiler's page.

## Anatomy

```
 ┌ LABEL ───────────────── ● cpu ● gpu ┐   legend only with two series
 │ 3.4%   ⚠ of 12 cores · max 18%      │   value (large), its line beside it
 │ ▁▂▂▃▅▃▂▁▁▂▃▃▂▁▂▂▃▅▇▅▃▂▁ (sparkline)  │
 └─────────────────────────────────────┘
 Pill:  LABEL 3.4%  ▂▃▅          a narrow page's band: one line, a small bar sparkline
```

Short cards (under 88 logical pixels) put the label and the value on one row, the line
under them only when there's height for it (76 and up).

## Options

`MetricCard` is a plain struct (no builder yet); `CardForm` picks the drawing.

| Option | Default | Effect |
|---|---|---|
| `label` | — | what it measures, muted, cut with "…" |
| `value` | — | the value now (`3.4%`, `412 MB`), large |
| `sub` | `None` | the line beside / under the value |
| `warning` | `false` | the line in `warning`, after an `alert` icon (in a pill: the label) |
| `series: &[CardSeries]` | — | one or two `{ plot, color, label }`; two get a legend |
| `max` | — | the sparkline's scale top (`spark::scale_for`) |
| `hovered` | `false` | a stronger fill |
| `active` | `false` | the accent's edge (the table is sorted by it) |
| `focused` | `false` | the focus ring (`ds::focus_ring`) |
| `CardForm::Card` / `Pill` | — | the full card, or a one-line pill |
| `glass_metric_card(…, GlassLook)` | — | the full card on a pane of glass (`screen_widgets`) |

## States and interaction

- Stateless: it draws and returns `CardAreas { value, chart }`. Its owner keeps them in
  its hit table: the value's tooltip, the chart's reading (`Plot::reading`) under the
  mouse, a press to sort by it.
- Hover, active and focus are told by the owner each frame.

## Accessibility

Nothing yet: the card doesn't describe itself. Its owner should say what it is (a button
naming the metric, its value the value and the line) — see the audit.

## Cost

- Draws only: the texts are the caller's (kept per look), cut as they're drawn.
- The sparklines come from `spark::PlotCache`: built only for new values or another
  width; an identical repaint allocates nothing (the Task Manager's test holds it).

## Example

```rust
let spec = MetricCard {
    label: &self.texts.labels.cards[card as usize],
    value: &texts.value,
    sub: (!texts.sub.is_empty()).then_some(texts.sub.as_str()),
    warning: texts.warning,
    series,
    max,
    hovered,
    active: self.sorts_by(card),
    focused: self.shows_ring() && self.focus == Focus::Card(card),
};
let areas = metric_card(layer, text, icons, ds, rect, &spec, form);
self.hits.push((areas.value, Hit::CardValue(card)));
```

## Don't

- Tell a threshold by color alone: `warning` adds the sign.
- Format the value each frame: keep the strings per sample (the Task Manager's `texts`).
- Build a `Plot` per paint: hold a `PlotCache` per card.
