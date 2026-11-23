# Drag cards (`ui_components::drag_card`)

**For:** what's being dragged, following the mouse: a small raised card beside the
cursor with the thing's icon (or a short colored tag) and its name. One look for every
list that moves things by dragging.

**Where it's used:** the Database window's data sources, the HTTP client's request tree,
the diagram files' window; anywhere projects, files or entries are dragged into groups.

## Anatomy

```
   ↖ cursor
     ┌──────────────────────┐   12 px right of the mouse, 6 px below,
     │ ▣ users_table.sql    │   High elevation, its name cut at 280 px
     └──────────────────────┘
     Lead::Tag:  │ GET  /users/{id} │   a short word in its color (a request's method)
```

## Options

| Option | Default | Effect |
|---|---|---|
| `lead: Lead::Icon(id, color)` | — | an icon of the set, in its color |
| `Lead::Tag(word, color)` | — | a short word in its color |
| `Lead::Nothing` | — | the name alone |
| `name` | — | cut with "…" past `NAME_MAX` (280) |
| `at` | — | the mouse; the card sits beside it |

It returns the card's area.

## States and interaction

- None: drawn last, over everything in its area, while its owner's drag lasts. Its owner
  tells where it would drop (a lit folder) and does the move.

## Accessibility

None: it follows a pointer gesture. What's dragged and where it drops should be told by
the list (a move announced), not by the card.

## Cost

- One surface, one or two shaped lines: nothing kept, nothing per frame when not dragging.

## Example

```rust
// What's dragged follows the mouse, over everything.
if let Some((icon, color, name)) = self.dragged_card(p) {
    let layer = scene.push_layer(Some(b));
    let lead = ui_components::drag_card::Lead::Icon(icons.id_or_file(icon), color);
    ui_components::drag_card::paint(layer, &ds, text, icons, lead, &name, self.drag_at);
}
```

## Don't

- Draw a dragged row's copy of your own: every list drags with this card.
- Draw it under other parts: push a layer of its own, last.
