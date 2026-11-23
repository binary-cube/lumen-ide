# Sliders (`ui_components::slider`)

**For:** choosing one of a few ordered values (1 · 2 · 4 · 8) by dragging. A slider on one
line with its value said in words on its right is `form::value_slider` ([form.md](form.md)).

**Where it's used:** the settings window's sliders (`SettingsRow::Slider`, through
`form::value_slider`).

## Anatomy

```
  ●━━━━━━━━━━◉─────·─────·        track filled to the current stop, a tick per stop,
  1    2     4     8     16       the thumb (a ring around it under the mouse),
             ▔ (current: accent, semibold)   the values under their stops
```

## Options

`SliderSpec` says what and where; `SliderStyle` how it looks (logical pixels for sizes).

| Option | Default | Effect |
|---|---|---|
| `SliderSpec { x, top, width }` | — | where; at most `style.max_width` wide |
| `labels: &[&str]` | — | the stops' names (their count is the stops') |
| `current` | — | the stop it's at (clamped) |
| `hovered` | — | the thumb's halo |
| `style: &SliderStyle` | `SliderStyle::from_palette(p)` | colors from the theme |
| `track_height` | 4 | the track's thickness |
| `thumb_size` | 16 | the thumb's diameter |
| `max_width` | 360 | it doesn't stretch across a wide window |
| `ticks` | `true` | a tick at each stop |
| `labels` | `true` | the values under the stops (without them: only as tall as its thumb) |

`height(style)` gives its height: `HEIGHT` (44) with labels, the thumb plus 4 without.

## States and interaction

- Stateless: it returns a `SliderArea { rect, stops }`; its owner keeps it, and on a
  press or a drag inside `rect` picks `area.nearest(x)`.
- The keys are the owner's: `form::stepped(current, stops, ±1, shift)` moves a step, or a
  quarter of the stops with ⇧, stopping at the ends.

## Accessibility

Nothing yet: it should be a slider node (its label beside it, its value the current
stop's name) — see the audit.

## Cost

- A handful of quads and one shaped line per stop.
- Each paint allocates its stops' positions (`SliderArea::stops`).

## Example

```rust
// form::value_slider: a slider with no labels, its value said after it.
let mut style = SliderStyle::from_palette(p);
style.labels = false;
style.ticks = spec.stops <= 8;
style.thumb_size = 14.0;
let mut area = slider(
    layer,
    text,
    ctx,
    SliderSpec {
        x: spec.x,
        top,
        width: track_w,
        labels: &labels[..count],
        current: spec.current.min(count - 1),
        hovered: spec.hovered,
        style: &style,
    },
);
// its owner keeps `area` and, on a press or a drag, picks area.nearest(p.x)
```

## Don't

- Draw a track and a thumb by hand: change a `SliderStyle` field instead.
- Use it for a continuous value with hundreds of steps: its labels and ticks are per stop
  (`value_slider` turns the ticks off past 8).
