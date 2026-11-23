# The app's own look (`ui_components::ds::brand`)

**For:** the pieces that make the app recognisable where it shows itself: the "About" card,
the splash, the calm screens' panes — the night they sit on, a soft wide shadow, the name in
widely spaced capitals, the version on a pill of glass.

**Where it's used:** the splash and its "About" card (`ui-workbench`), the screen's mark
(`app`).

## Anatomy

```
        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░       soft_shadow: 16 near-clear layers, lower
        ░  N A M E                     ░      wordmark: capitals, tracked, an accent shadow
        ░  ( 1.4.0 )                   ░      version_pill: white glass, a thin edge
        ░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░
```

## Options

| Item | Effect |
|---|---|
| `NIGHT` | the deep night under the app's card and its panes |
| `soft_shadow(layer, rect, radius, scale, strength)` | a wide shadow with no visible edge; `strength` 1 is the "About" card's |
| `Wordmark { family, size, tracking, accent, opacity }` | how the name is written (sizes in physical pixels); `.width(text, name)` |
| `wordmark(layer, text, &mark, name, (x, baseline))` | the name in capitals, white, an accent shadow under each letter; returns where it ends |
| `version_pill(layer, text, family, version, (x, y), (scale, opacity))` | the version on a pill of white glass; returns the pill |

The name is always `kernel::brand::NAME`, never written in code.

## States and interaction

None: drawn, faded in by `opacity`.

## Accessibility

The wordmark is drawn letter by letter, each letter twice (its shadow, then itself), and
each is read as a label of its own; the version is read as a label.

## Cost

- Fixed colors on a fixed night: they don't follow the theme, by design.
- `wordmark` and `Wordmark::width` upper-case the name and make a `String` per letter each
  call; they're drawn on a splash and a few calm screens, not in a working view.

## Example

```rust
let mark = crate::ds::Wordmark {
    family: &palette.ui_font,
    size: px(40.0),
    tracking: px(12.0),
    accent: palette.accent,
    opacity,
};
crate::ds::wordmark(layer, text, &mark, kernel::brand::NAME, (card.min_x() + pad, baseline));
```

## Don't

- Write the app's name in code or in a label: `kernel::brand`.
- Use these looks in a working view: its surfaces are the theme's (`ds::surface`).
