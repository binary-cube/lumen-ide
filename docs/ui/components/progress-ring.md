# Progress rings (`ui_components::progress_ring`)

**For:** a smooth ring showing how much of something is done or left: an arc with round
caps over a faint track.

**Where it's used:** a notification's time left around its × (`toast`).

## Anatomy

```
    ╭──╮      the track: the whole ring, faint (optional)
   │    ●     the arc: clockwise from `start` (turns from the top), `sweep` turns long,
    ╰──╯       round caps at both ends
```

The renderers draw only quads, glyphs and images, so the ring is rasterized here on the
CPU into a small image at the device pixel size (drawn 1:1, never scaled), anti-aliased
by each pixel's coverage, and kept until what it shows changes.

## Options

`RingSpec` (device pixels) is a plain struct:

| Option | Default | Effect |
|---|---|---|
| `size` | — | the image's side; the ring is centered in it (0: nothing drawn) |
| `radius` | — | the radius of the stroke's middle |
| `stroke` | — | the stroke's width |
| `color` | — | the arc's color |
| `track` | — | `Some(color)`: the full ring under the arc; `None`: only the arc |
| `paint(…, (start, sweep), opacity)` | — | the arc, in turns (quantized to `STEPS` = 360 a turn) |

## States and interaction

- None: a drawing. Its owner animates it by changing `sweep` (the toast by its time
  left, in steps of `RING_STEPS`).

## Accessibility

None of its own: its owner says what it means (a notification closing in N seconds).

## Cost

- Rasterized again only when its size, radius, stroke (to 1/64 px), colors or arc (to one
  degree) change (`kernel::Memo`); a frame at the toasts' pace moves it less than the eye sees.
- An unchanged frame draws the image it has: nothing allocated, nothing uploaded
  (`ImageData` is shared). `memory()` says what it holds; `Purge` lets it go.

## Example

```rust
let spec = RingSpec {
    size: close.size.width.round().max(0.0) as u32,
    radius: close.size.width / 2.0 - ctx.px(1.0),
    stroke: (ctx.scale * 1.6).max(1.5),
    color: color.with_alpha(0.9),
    track: Some(ctx.palette.text.with_alpha(0.08)),
};
let sweep = f32::from(steps) / f32::from(RING_STEPS);
cache.paint(layer, center, &spec, (0.0, sweep), ctx.opacity);
```

## Don't

- Draw a ring from many small quads: it's one image.
- Pass an unquantized value that changes every frame and expect it cached: the arc is
  kept per degree, so pass the steps you mean.
- Make a `ProgressRing` per frame: keep one per thing shown (the toasts keep one per card).
