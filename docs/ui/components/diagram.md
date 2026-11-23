# Diagram canvases (`ui_components::diagram`)

**For:** a view of boxes and the lines between them, whatever the boxes stand for (a type
and its relatives, a table and its keys, a plan's steps): where the cards and edges go,
how the canvas is looked at, and how its pieces are drawn.

**Where it's used:** the type diagrams (`insight/diagrams`), the database diagram, the
HTTP client's request chains and the query profile's graph (`tidy`), the markdown
viewer's diagrams (`layered` for graphs, `tidy` for mind maps).

## Anatomy

```
 · · · · · · · · · · · · · · · ·    dot grid: fixed spacing at any zoom, moves with the pan
 · ┌──────────────────┐ · · · · ·
 · │ [C] Order        │ ← header: badge (its letter, color), title, subtitle
 · │   app/Models     │
 · ├──────────────────┤ ← header_divider
 · │ + id      int    │ ← rows: mark, label, detail on the right
 · │ + total   float  │
 · │   3 more…        │ ← more_row
 · └────────┬─────────┘
 ·          │ axis-aligned edge, dashed or not
 ·          ▼ arrow (filled or hollow) at the target's border
```

Four layers, each one thing:

- Layout (pure): `tidy` (a tree, Reingold–Tilford, O(n)) + `links`; `layered::layout` (any
  graph, in layers: cycles broken, medians, ordered least squares). Both give a `Layout`
  of `Card`s and `Edge`s in diagram space (logical pixels before pan and zoom).
- `Viewport`: pan, zoom, fit, center, screen ↔ diagram, a press told from a drag
  (`Release::Click` / `Panned`), `Insets` for what covers its sides.
- Drawing: `card_frame`, `card_header`, `header_divider`, `card_row`, `more_row`, `edge`,
  `legend_sample`, `dot_grid` — quads and text only.
- Labels: `label_spots` beside an edge's longest segment, `place_labels` clear of cards
  and of one another.

## Options

| Option | Default | Effect |
|---|---|---|
| `Viewport::new(top)` | zoom 1 | under a toolbar `top` logical pixels high |
| `fit(bounds, focus, min, max)` | — | zoomed to fit within limits, a focus centered when too large |
| `zoom_at(factor, at, min, max)` / `reset_zoom` | — | around the mouse |
| `insets` | none | what the view draws over the area's sides |
| `CardLook { focus, hovered, muted }` | all `false` | raised and outlined in the accent / lit / only named |
| `CardFonts::new(palette, k)` | — | title, subtitle, rows at the zoom |
| `Header { glyph, color, title, subtitle, muted }` | — | a card's header |
| `Row { mark, mark_color, label, color, detail, hovered }` | — | a card's row |
| `EdgeStyle { color, dashed, head, width }` | `width: EdgeStyle::LINE` (1.5) | `Head::None` / `Arrow { hollow }` |
| `card_height(shown, more)` | — | `HEADER_H` + rows × `ROW_H` + `PAD` |
| `History<T>` | empty | where it was before (back in order) |

## States and interaction

- Stateless drawing; the view keeps the `Viewport` and its hit table (`card_header` and
  `card_row` return their rectangles).
- The wheel pans (`scroll`), ⌘ + wheel zooms around the mouse; a press then a move pans;
  a press without a move is a click (`release` → `Release::Click(p)`).
- `Edge::distance(p)` finds the edge under the mouse.

## Accessibility

Nothing yet: cards and edges don't describe themselves. A card should be a group named
by its title, its rows its items (see the audit).

## Cost

- Layout: pure, once per model, where its view runs it (a worker for a big one); `tidy`
  is linear.
- Drawing: only cards meeting the area; quads for lines (a dash a quad). `edge` copies its
  points to shorten the line before the arrow (a `Vec` per edge per frame).

## Example

```rust
let fonts = CardFonts::new(p, k);
let look = CardLook { focus, hovered, muted: node.path.is_none() };
diagram::card_frame(layer, ds, r, look, k, self.view.zoom, self.view.scale);
let header = diagram::card_header(pen, fonts, header, r, k);
self.hits.push((header, Hit::Card(card.node)));
diagram::edge(edges, &points, edge.dir, k, edge_style(edge.kind, p));
// input: self.view.press(p); self.view.drag(p); match self.view.release() { Release::Click(p) => …, _ => … }
```

## Don't

- Lay out on the UI thread for a big model: `tidy` / `layered::layout` on a worker.
- Draw lines as anything but axis-aligned segments here; curves are rasterized elsewhere
  (`vector-tiles`).
- Zoom the dot grid: its spacing stays the same at any zoom.
