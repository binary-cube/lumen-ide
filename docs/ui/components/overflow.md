# Overflow (`ui_components::overflow`)

**For:** a bar whose items may not all fit: which are placed, which go into its "…" menu,
in an order the bar chooses, so nothing is hidden for nothing.

**Where it's used:** the data grid's toolbar, the Task Manager's toolbar.

## Anatomy

```
 [Refresh] [Group ▾] ( filter ………… )              [Sort] [Pause] [⋯]
  left items, from the bar's start                 right items, ending at its end, "…" after
```

- `OverflowItem { id, width, min_width, side, collapse, menu }`.
- `OverflowSpec { min_x, max_x, pad, gap, more_width, items }`.
- `fit(&spec) -> OverflowLayout { placed, collapsed, more, fits }`; `get(id)`, `shows(id)`.

Items go one by one by `collapse` (lowest first; `None`: never) until the rest fits; a
flexible one (a filter field) shrinks to its least first; then what went and fits again in
the room left comes back, the most wanted first (a narrow one where a wide one didn't
fit). The menu lists them by their `menu` place.

## Options

| Option | Default | Effect |
|---|---|---|
| `OverflowItem::new(id, width, side, collapse, menu)` | fixed | `Side::Left` / `Side::Right` |
| `.flexible(min_width)` | `width` | shrinks to `min_width` before anything goes |
| `collapse: None` | — | never goes (cut at worst) |
| `OverflowSpec.pad` / `.gap` / `.more_width` | — | from the bar's ends, between items, the "…" button |

## States and interaction

None: a pure function. The bar draws what's placed and opens the app's menu under "…"
with `collapsed`; `fits: false` says even what never goes is wider than the bar.

## Accessibility

None here: the bar's buttons describe themselves (`ds::button`).

## Cost

Items × steps, cheap; its user keeps the result for its width and state (`kernel::Memo`).

## Example

```rust
items.push(OverflowItem::new(Tool::Pause, icon, Side::Right, Some(2), 2));
let layout = overflow::fit(&OverflowSpec {
    min_x: bar.min_x(), max_x: bar.max_x(), pad: 0.0,
    gap: px(8.0), more_width: icon, items: &items,
});
```

## Don't

- Hide a bar's items by hand when it's narrow: this, so the "…" and its menu are the same
  everywhere.
- Call it every frame: keep its layout under its width and state.
