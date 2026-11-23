# Progress bar and meter (`ui_components::ds::progress`)

**For:** how far something is: a download, a transfer, a notification's work, a modal's
progress — a thin rounded track filled as far as it went, or, not known yet, a short piece
sweeping across. `Meter` is its small vertical sibling: a row of bars standing on a line.

**Where it's used:** `ProgressBar` in about fifteen views (running work, the updates'
download, notifications, the Task Manager's shares split in parts, the splash's gliding
sweep); `Meter` in the status bar.

## Anatomy

```
 Downloading update ……………………… 12 MB of 33 MB     text row (optional): label left, value right
 ████████████░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░     the bar: track, fill (or a sweeping piece)

 meter   ▁▃▇▂   bars filled from the bottom over their own color, faint
```

## Options

**ProgressBar** — made by `of(f)`, `unknown()`, `from(Option<f32>)`, `counted(done,
total)` or `split(&[ProgressSegment])`.

| Option | Default | Effect |
|---|---|---|
| `.tone(Tone)` | `Accent` | the fill's color |
| `.color(Rgba)` | the tone's | the fill's own color |
| `.fill_alpha(f32)` | 1 | the fill's opacity |
| `.thickness(f32)` | 4 | the bar's height in `paint` (logical) |
| `.track(ProgressTrack)` | `Faint(0.08)` | `Color(c)`, or `None` |
| `.track_alpha(f32)` | 0.08 | the usual track at another alpha |
| `.fill_only()` | track shown | no track |
| `.on_dark()` | — | a faint white track (over a picture, a dark card) |
| `.min_fill(f32)` | the bar's height | the narrowest a non-empty fill shows |
| `.radius(f32)` | half the height | its corners |
| `.snapped()` | off | fills on whole pixels |
| `.sweep_at(t)` | by the clock | the unknown piece at `t` of a pass (the caller's clock) |
| `.glide(share)` | linear, 0.3 | the piece eases in and out, `share` of the track wide |
| `.label(&str)` | none | the text row's left (muted, cut with `…`) |
| `.percent()` / `.value(&str)` | none | the row's right: `40%`, or the caller's text |
| `.height(ctx)` | — | the bar, plus the row when there's one |
| `.animating()` | — | it sweeps by the clock: frames needed while it shows |
| `.paint(…, rect)` / `.paint_bar(layer, ctx, track)` | — | with its text row, or the bar alone |

**Meter** — `Meter::default()` (fields: `bar_width` 5, `gap` 3, `height` 13, `radius`
1.5, `min_fill` 2, `track_alpha` 0.16); `.width(ctx, bars)`, `.bar_height(ctx)`,
`.paint(layer, ctx, (x, bottom), bars)` with `(level, color)` per bar.

## States and interaction

- Not pressable. Known: drawn as far as it went. Not known: the piece sweeps one pass every
  1.4 s by the wall clock, or where `sweep_at` puts it.
- Its view asks for frames only while `animating()` is true.

## Accessibility

Its label and value are read as labels. The bar itself says nothing: no `ProgressBar`
node with its fraction is told.

## Cost

- `paint_bar`: one or two quads (one per part when split), nothing allocated.
- `paint` with a text row formats the value (`value_text` returns a `String`) and cuts the
  label with `ellipsize` each paint.
- `Meter::paint`: two quads a bar, nothing allocated.

## Example

```rust
if let Some(fraction) = work.progress.and_then(|w| w.fraction()) {
    let bar = Rect::new(tx, row.max_y() - px(5.0), room, px(3.0));
    ds::ProgressBar::of(fraction).paint_bar(layer, &ds, bar);
}

let meter = ds::Meter::default();
let w = meter.width(&ds, item.meter.len());
```

## Don't

- Draw a track and a fill by hand: an option of `ProgressBar`.
- Keep asking for frames for a known fraction or a `sweep_at` one: `animating()` is false.
