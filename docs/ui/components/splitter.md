# Splitter (`ui_components::splitter`)

**For:** the resizable separator between two panels: vertical (side by side: the tree and
the editor) or horizontal (stacked: the editor and the terminal).

**Where it's used:** the app's tool areas and previews, the split editors, the database
explorer (list and data), the bottom area.

## Anatomy

```
  panel A   ┆   panel B         a line fading toward its ends,
            ┆                   a rounded handle (three dots) in the middle,
           (⋮)                  reaching over both panels
            ┆
```

- `Splitter`: its orientation, its grab area (the strip between the panels), the drag in
  progress (the grab's offset kept), the hover's transition.
- `SplitterStyle`: line, handle, grip and active colors; line width, handle thickness and
  length (logical pixels).

## Options

| Option | Default | Effect |
|---|---|---|
| `Splitter::new(style, Orientation::Vertical)` | — | `Horizontal`: stacked panels, dragged vertically |
| `SplitterStyle::from_palette(p)` | the theme's `splitter_*` | line 1, handle 10 × 34 |
| `set_bounds(strip, scale)` | — | where it is, each layout |

## States and interaction

- Hover: the accent grows from the middle toward the ends and the handle turns the accent
  (ease-out, ~220 ms); reversed on exit. `is_animating` asks for frames meanwhile.
- Press on it: `begin_drag`; `drag_to(p)` gives where the line's center goes (its owner
  turns it into a size); `end_drag`.
- Cursor: resize, across its line.
- As a `Control` through `control::Divider { splitter, at }`: `hit`, `press` (`Held`),
  `drag` (writes `at`), `release`, `hover`, `cursor`, `animating`.

## Accessibility

None: it's told by nothing (a screen reader can't resize panels from it).

## Cost

Quads in a layer of its own; frames only while the transition runs.

## Example

```rust
splitter: Splitter::new(SplitterStyle::default(), Orientation::Vertical),
self.splitter.set_bounds(gap, scale);
// as a part
let mut splitter = Divider { splitter: &mut self.splitter, at: &mut self.split_at };
// painted after both panes
self.splitter.paint(scene);
```

## Don't

- Paint it before a pane beside it: its handle reaches over both; `is_on_top` checks a
  painted scene in its users' tests.
- Turn the line's place into a size inside the splitter: its owner does.
