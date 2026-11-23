# Tree table (`ui_components::tree_table`)

**For:** a tree laid out on columns: rows with a depth, opened and folded, the first
column the tree, the others figures (numbers with their unit, shares as bars, badges).

**Where it's used:** the database console's Plan tab and its profile (tree mode, the
comparison of two plans).

## Anatomy

```
 Node                         │   Rows │ Time      │ Share ───────
 ▾ ⚙ Hash Join (id = 1)       │  1 204 │  0.42 ms  │ ████░░  38 %
 │ ▸ Seq Scan on orders  [×120 ↑]   …
 ⚠│   Index Scan …              ← gutter sign, tint, marked stripe
```

- `TreeTable`: columns, rows (parents first), what's open, the selection, the filter, the
  scroll (both ways), the columns' places and the resize in progress.
- `TableColumn`, `TableRow`, `TableCell`, `CellValue` (`Text(spans)`, `Number`, `Bar`,
  `Split`, `Badge`, `Icon`), `Span` / `SpanStyle`, `Ink` (a tone, a severity, a heat).

## Options

| Option | Default | Effect |
|---|---|---|
| `TableColumn::new(title, width)` | left | `.right()`, `.with_tip(…)`, `.with_min(…)` |
| `TableRow::new(parent, cells)` | — | `.with_icon`, `.with_sign(icon, ink, tip)`, `.tinted(ink)`, `.with_folded_note`, `.marked(true)` |
| `TableCell::text / spans / number / bar` | — | `.with_tip`, `.with_lead(icon, ink)`, `.outlined(ink)` |
| `set_rows(rows, open)` | — | `open(i)`: which start open; the selection kept when it's still a row |
| `set_filter(text)` | none | only the matches shown, with their parents |
| `set_width(i, w)` / `set_columns` | as given | a column's width; the tree's takes the rest |
| `set_empty(text)` | — | what shows with no rows |

## States and interaction

- Keys (`key`): ↑ ↓, Home / End, PgUp / PgDn move the choice; → opens (or goes to the
  first child), ← folds (or goes to the parent), ↵ opens or folds; `char_key('*')` opens
  everything under the chosen row.
- Press: a row chosen; its chevron or a double click opens / folds (`TableEvent`). A
  column's edge dragged resizes it; the scrollbar's thumb is dragged.
- Hover: the row, a column's title (its tooltip), an edge (a resize cursor).
- Tooltips: a column's says what it is; a cell's its own, or its text when cut.
- A `Control`: among a view's controls it takes the press (held while an edge or the
  thumb is dragged), the moves, the wheel, the keys, `*`, its cursor and tooltips; what a
  press did is read once with `take_event`. Its own methods stay for an owner that shows
  one table of several by its mode (the profile's tree, tables, comparison).
- Its header is the shared table's (`table::header_band`, `header_hover`, `header_edge`,
  `header_rule`), one line high (`HEADER_H` 28: a compact table).

## Accessibility

Each row drawn is a tree's `TreeItem` (`tree::item`) named by its first cell's text
(joined only while a screen reader runs), with its level, open / folded, selected.

## Cost

- An identical repaint allocates nothing (a test holds it): a cell's pieces measured,
  then drawn, none kept; its numbers written in kept texts.
- The rows shown are worked out once per change of the rows, what's open or the filter
  (`generation`); the tree column's width once per that and the scale (at most 2 000 rows
  measured).
- A frame draws only the rows in view (`TreeList`).

## Example

```rust
let mut table = TreeTable::new(built.columns.clone());
table.set_rows(built.rows.clone(), |_| true);
// paint
self.table.paint(TablePaint {
    bounds: laid.tree, scale: ctx.scale, text: ctx.text,
    scene: ctx.scene, palette: ctx.palette, icons: ctx.icons,
});
```

## Don't

- Draw a tree with figures by hand: this, or an option added to it.
- Use it for flat rows from a source: that's the data grid.
