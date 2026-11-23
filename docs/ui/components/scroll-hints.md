# Scroll hints (`ui_components::ds::scroll`)

**For:** telling that a list or a strip has more beyond its edge: a soft fade into the
background at each edge that hides content, with a small translucent chip pointing there.
It is not the scrollbar (that's `scrollbar::{Scrollbar, EdgeScrollbar}`): a hint drawn over
the content, beside or without one.

**Where it's used:** the remote connection's review list and wizard, the data grid's
popups.

## Anatomy

```
 ┌───────────────────────────────┐
 │▓▓▓▓▓▓▓▓▓▓▓▓( ^ )▓▓▓▓▓▓▓▓▓▓▓▓▓▓│  fade from the edge (eased), up to 44 deep
 │   rows…                       │
 │▓▓▓▓▓▓▓▓▓▓▓▓( v )▓▓▓▓▓▓▓▓▓▓▓▓▓▓│  a chip with a chevron near the edge's middle
 └───────────────────────────────┘
```

## Options

`ScrollHintsSpec`, a plain struct:

| Field | Effect |
|---|---|
| `area` | the scrolled area; drawn inside its border, so a card's edge stays |
| `axis` | `Axis::Vertical` (top / bottom) or `Axis::Horizontal` (left / right) |
| `before` | content hidden above / on the left |
| `after` | content hidden below / on the right |
| `background` | what the fade goes into (the area's background token) |

## States and interaction

Stateless and not pressable: the owner says `before` / `after` from its scroll offset each
paint; the wheel and the bar stay the scrolled view's.

## Accessibility

Nothing: a hint for the eye; the list's rows say where they are.

## Cost

Up to 24 strips and a chip per edge shown; nothing allocated.

## Example

```rust
ds::scroll_hints(
    hints,
    icons,
    ds,
    ds::ScrollHintsSpec {
        area: list,
        axis: ds::Axis::Vertical,
        before: self.scroll > 0.5,
        after: self.scroll < self.max_scroll - 0.5,
        background: p.editor_background,
    },
);
```

## Don't

- Use it instead of a scrollbar where the content is long: the shared scrollbar, wired
  whole, and the hint only where a bar would crowd a small strip.
