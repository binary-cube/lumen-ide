# Scrollbar (`ui_components::scrollbar`, `ScrollState`)

**For:** every scrollbar: a subtle thread, always the same, whose thumb thickens under the
pointer and while dragged; along an area's edge it may hide at rest. `ScrollState` keeps a
list's scroll in step with it.

**Where it's used:** every list, page, field and pane that scrolls (the picker, read text,
the log, trees, the grid, the code editor's overview as a lane).

## Anatomy

```
 content            ┃   ← thread (thin line), thumb (rounded)
                    █      hovered: widens into a rail, the thumb wide inside
                    █
                    ┃
```

- `Scrollbar`: one bar on a track, either orientation; told the `ScrollMetrics`
  (`content`, `viewport`, `offset`) at each call, it returns the new offset. Holds no
  position.
- `EdgeScrollbar`: the app's bar along an area's right edge; keeps what it was painted
  with, so press / drag / hover need only the point. Always shown, or hidden at rest
  (`AutoHide`: fades in on a scroll, stays a moment, fades out; the pointer on it keeps it).
- `ScrollbarStyle` (from the theme, shaped by the user's `ScrollbarLook`: rail or thumb
  hover, thickness, line, visibility), `LaneLook` (a lane of its own: background, tint,
  block alphas by state).
- `ScrollState`: offset, content and viewport; never past the end, at whole pixels.

## Options

| Option | Default | Effect |
|---|---|---|
| `EdgeScrollbar::default()` | always shown | — |
| `EdgeScrollbar::auto_hiding()` / `set_auto_hiding(on)` | off | hidden at rest |
| `paint(scene, palette, scale, area, content, offset)` | — | along `area`'s right edge; nothing when it all fits |
| `paint_on_line(…, (x, top, height), …)` | — | its thread on a dividing line (no lane beside it) |
| `Scrollbar::new(style, orientation)` | — | a bar of one's own metrics (across a code block) |
| `paint_lane(layer, metrics, &LaneLook::new(p, background))` | — | drawn as a lane, the part in view a block |
| `ScrollbarLook` (Settings → Appearance) | thumb grows, 5 px, line, by view | for every bar |

## States and interaction

- Press on the track: on the thumb it's held; elsewhere the thumb jumps there (and can be
  dragged on). `drag` gives the offset, `release` ends it.
- Hover: widens (or only the thumb thickens, by the look); an auto-hiding one shown stays
  while hovered.
- The cursor: over a bar (its thumb, its lane) and while its thumb is dragged anywhere,
  `scrollbar::CURSOR` (the plain arrow), never what lies under it. As a part it says so
  (`Control::owns_cursor`; a part of its own that is a bar wherever it's hit writes
  `ui_components::bar_cursor!()`): a view asks `Controls::bar_or(p, own)` (or
  `bar_cursor(p)` before its own cursor).
- Frames: `next_frame` while it fades or its hold ends; none at rest.
- As a `Control` through `control::Thumb { bar, offset }`, `ScrollThumb { bar, scroll }`,
  `Scrolled { bar, offset, area, max }` (the wheel over the area too) or
  `BarThumb { bar, metrics, offset }`.
- `ScrollState`: `scroll_by(dy)`, `scroll_to(offset)`, `set_viewport`, `measure`,
  `origin(top)`, `keep_visible(row, view)`.

## Accessibility

None: a bar isn't described; its area's content is (a list, a text).

## Cost

- In a window that fades (a modal, a menu appearing) a bar is drawn with `paint_in` /
  `paint_on_line_in` (the window's `Ctx`: it fades with it); `paint` for a view's.
Quads only; the style is built per paint from the palette and the look (no allocation);
an auto-hiding bar at rest asks for no frame.

## Example

```rust
list_bar: EdgeScrollbar::auto_hiding(),
self.list_bar.paint(scene, p, ctx.scale, area, content, self.list_scroll);
// as a part: the thumb dragged, the wheel over the list
let mut list = Scrolled { bar: &mut self.list_bar, offset: &mut self.list_scroll,
                          area: self.list_area, max: self.list_max };
```

## Don't

- Draw a scrollbar, a thumb or a lane by hand.
- Wire it half: drawn but not hovered, or its thumb not dragged. A test drags its thumb.
