# Transitions (`ui_components::anim`)

**For:** a short eased change of a value — a hover's fill, a window appearing, a bar
widening — moving smoothly toward its target instead of jumping.

**Where it's used:** the color picker, the level picker and tooltips appearing; About and
the context menu (`Appear`); the menu drawer's slides; the editor's floating cards; a tab
drawing the eye while a hint is about it (`Attention`).

## Anatomy

```
 value ▲            ╭──────── to
       │         ╭──╯
       │      ╭──╯          ease-out cubic over `duration`
       │ ╭────╯
  from ┼─╯────────────────▶ time
```

- `Tween`: a value moving from where it is toward a target.
- `Appear`: a window's appearance, from 0 to 1 once it opens (1 before it ever opened).
- `Attention`: something drawing the eye a moment: it fades in, pulses a few times, then
  holds still (no frames while it does); let go, it fades out from where it is and is gone.
  Its times are given (`value(now)`), so it's tested without waiting.

## Options

| Option | Default | Effect |
|---|---|---|
| `Tween::new(value, duration)` | at rest on `value` | a value that will move over `duration` |
| `retarget(to)` | — | starts toward `to` from the current value (no jump mid-way); `true` if the target changed |
| `set(value)` | — | jumps there, no transition |
| `value()` / `target()` | — | where it is now / where it's going |
| `animating()` | — | still moving: ask for the next frame only while true |
| `Appear::start(duration)` | 1 (shown) | from 0 toward 1 |
| `Appear::value()` / `animating()` | — | how far it appeared |
| `Attention::new(look, now)` | — | starts drawing the eye at `now` |
| `AttentionLook` | 220 ms fade, 2 pulses of 650 ms, dipping to 0.4 | how it fades and pulses |
| `release(now)` / `released()` / `gone(now)` | — | let go (fades out once); faded out |
| `value(now)` / `animating(now)` | — | how strongly it shows (0..=1); changing (fading, pulsing) |

## States and interaction

- At rest, or moving until `duration` has passed since the last `retarget`.
- The time is the wall clock's: no per-frame step to call.

## Accessibility

None: it's a value. What it moves is described by its owner.

## Cost

- `Copy`, no allocation; `value()` is a few float operations.
- A view asks for frames only while `animating()` (`frame_deadline`, `is_animating`).

## Example

```rust
// About appearing:
self.appear.start(Duration::from_millis(160));
let t = self.appear.value();          // in paint: its opacity, its lift
self.appear.animating()               // in is_animating

// The menu drawer widening while a menu is open:
self.widen.retarget(if open_menu.is_some() && self.open { 1.0 } else { 0.0 });
```

## Don't

- Ask for frames while nothing moves.
- Build an appearance by hand from a `Tween` (`new(0.0)` then `retarget(1.0)`): that's
  `Appear::start`.
- Pick durations above a few hundred milliseconds for a hover or an appearance.
