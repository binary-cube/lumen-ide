# Columns (`ui_components::columns`)

**For:** a table's columns fitted to its width by priority: which show, how wide, which
left for want of room, and the range an edge may be dragged in.

**Where it's used:** the Task Manager's table (its column chooser says "doesn't fit").

## Anatomy

```
 | Name (priority 0: never leaves) | CPU | Memory | Disk (leaves first) |
   ←— flexible, takes the rest —→    fixed, never under their widest text
```

- `ColumnSpec<K>`: its id, priority (0: never leaves; higher: sooner), width (a flexible
  one's least), most, `reserve` (its widest text), the user's `Choice` (`Auto`, `Pinned`,
  `Hidden`), the least it may be dragged to, the width the user gave it.
- `fit(&specs, x0, width, gap, sorted) -> ColumnFit<K>`: `shown` (id, left, width),
  `dropped`, `hidden`, `cramped`, `limits` (each shown one's drag range), `gap`.
- `ColumnFit::edges()`, `edge_at(x, grip)`, `dragged(id, from, dx)`, `get`, `shows`, `end`.

## Options

| Option | Default | Effect |
|---|---|---|
| `ColumnSpec::new(id, priority, width)` | fixed, auto | — |
| `.flexible(max_width)` | `width` | grows up to `max_width` with what's left (`f32::INFINITY`: any) |
| `.reserving(widest)` | 0 | never narrower than its widest text (numbers don't dance) |
| `.chosen(Choice::Pinned / Hidden)` | `Auto` | stays and pushes others out / never shown |
| `.min(w)` | `width` | the least a drag may make it |
| `.sized(Some(w))` | `None` | the width the user dragged it to: kept, no longer grows |
| `sorted: Some(id)` | — | the sorted column never leaves (takes the last shown's place) |

## States and interaction

None: a pure function. Its user hit-tests an edge with `edge_at` and turns a drag into a
width with `dragged` (between the column's floor and what the others leave).

## Accessibility

None here: the table drawing the columns describes itself.

## Cost

Pure and cheap; its user keeps the result for its width and state (`kernel::Memo`).

## Example

```rust
let specs: Vec<ColumnSpec<ColumnId>> = present(spec, class)
    .map(|c| ColumnSpec::new(c, priority, px(least)).flexible(most).reserving(reserve)
        .chosen(spec.choices[c.index()]).min(px(c.min_width())).sized(set))
    .collect();
columns::fit(&specs, table.min_x() + inset, width, px(12.0), spec.sorted_by)
```

## Don't

- Drop columns by hand when a table is narrow: this, so the order and the comeback rules
  are the same.
- Recompute it every frame: keep it under its width and state.
