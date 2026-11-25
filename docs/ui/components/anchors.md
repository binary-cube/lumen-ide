# Anchors (`ui_components::anchors`)

**For:** saying where a part of the window was drawn, by an id, so that something drawn over
it later in the same frame can point at it (a [hint card](hint-card.md)).

**Where it's used:** the app reports the editor's active tab
(`workbench_model::hints::ANCHOR_ACTIVE_TAB`) as it draws the tabs; the hint overlay finds
it. Any view, panel or bar may report its own parts the same way.

## Anatomy

- `begin()`: a window's frame starts (the app, before it draws anything): what was reported
  before is let go.
- `report(id, rect)`: part `id` (a `&'static str`) is at `rect` (physical pixels) this frame;
  the latest report wins.
- `find(id)`: where it was drawn this frame; `None` if it wasn't (hidden, scrolled away).

## States and interaction

None of its own: a list of ids and rects, rebuilt each frame on the drawing thread.

## Accessibility

Nothing: it's where parts are, not a part.

## Cost

One small list per drawing thread, cleared each frame and reused (no allocation once it has
room); a report is a short scan of it. Nothing is reported unless a part says so.

## Example

```rust
// A part, as it's drawn:
ui_components::anchors::report("feature.button", rect);
// Drawn over it later in the frame:
let at = ui_components::anchors::find("feature.button");
```

## Don't

- Keep a rect found for another frame: the layout may have changed.
- Name an anchor after the user's data (a file, a project): ids are the code's own words.
