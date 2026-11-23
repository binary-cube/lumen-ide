# Data grid (`ui_components::data_grid`)

**For:** every tabular listing of rows from a source: a database table, a query's result,
a CSV file, a log's entries. Paged or scrolled whole, sorted, filtered, edited, copied and
exported the same way everywhere.

**Where it's used:** the database explorer's Data tab, the console's result tabs, the log
viewer.

## Anatomy

```
 toolbar: ⟳ |< < 1–500 of 12 040 > >| [500 ▾] … [CSV ▾] [⋯]       (overflow → "…")
 WHERE (…)                         ORDER BY (…)                    (filter fields)
 ┌─#─┬ id 🔑  ↑1 ┬ name        ┬ created_at ┐ ← header: name + type subtitle (table::)
 │ 1 │ 12       │ Ana          │ 2026-10-31 │ ← rows in view, stripes, column lines
 │ 2 │ 13       │ NULL         │ …          │
 └───┴──────────┴──────────────┴────────────┘
 bottom bar: Σ 25 · avg 12.5 · 2 rows                (the selection's figures, an error)
```

- `GridSource` (the port): `columns`, `total`, `row_state`, `cell` (borrowed `CellRef`),
  `request(query, rows)`, `poll`, `status`, `capabilities`, `name`; optional `sql`,
  `cancel`, `refresh`, `count`, and editing (`editing`, `set`, `insert`, `delete`,
  `revert`, `pending`, `whole`, `mark`), `cell_icon`, `filter_hint`.
- `DataGrid<S>`: the paging, the query (sort, filter), the selection, the layout of the
  columns (widths, order, hidden: `SavedLayout`), ⌘F, the transposed row, the cell editor
  and the large value editor, the toolbar's overflow, the extractors' popover.
- Pure parts, tested apart: `filter` (`parse_where`, `parse_order`), `paging`,
  `selection`, `layout`, `copy` (through the data extractors), `aggregate`, `export`
  (`ExportFormat`, `ExportWriter`, `ExportSink`: a temporary file renamed over its target),
  `xlsx`.

## Options

| Option | Default | Effect |
|---|---|---|
| `DataGrid::new(source, page)` | — | `page`: rows a page (`None`: all of them scrolled) |
| `set_project(Some(id))` | none | ⌘C, "Copy As" and the export use that project's extractor |
| `set_commands(fn(Tool) -> Option<&str>)` | none | the commands behind the toolbar's tools: their keys in tooltips and the "…" menu |
| `set_page_size(size)` / `choose_page_size` | given | another page size (chosen in the grid: kept with its layout) |
| `set_sort(keys)` / `set_filter(filter)` | none | the order and the `WHERE` |
| `restore_layout(saved)` / `saved_layout()` / `layout_version()` | — | the columns' widths, order and hidden ones kept by the view |
| `GridCaps { sort, filter, limit, cancel }` (source) | — | what the grid offers; `limit` says why not |

## States and interaction

- Press: a header sorts (⇧ adds a column, numbered), a header dragged moves its column, an
  edge dragged resizes (double click fits it); cells, rows (the numbers) and columns
  selected, ⇧ extends, ⌘ adds. Always `Held` (a drag selects).
- Keys: arrows move the cursor (⇧ extends), Tab between cells (`keeps_tab`), a character
  starts editing the cell (`char_key`), ↵ / Esc in the editors; ⌘F finds in the rows.
- Commands: `select_all`, copy (through the chosen extractor), cut, undo; its menu rows
  come back as `grid:` commands (`MENU_PREFIX`).
- Right-click: a cell's menu (quick filters, copy as, edit), a header's (export, hide,
  fit). The toolbar's "…" and page sizes are the app's menu (`take_menu`).
- Tooltips: every toolbar button (with its keys), a header's key and order marks, a
  disabled button's why.
- What it asks of its view: `take_action()` → `GridAction` (`Copy(text)`, `Submit`, `Export { format, clipboard }`,
  `Command(id)`).
- As a `Control`: `area`, `press_with`, `drag`, `release`, `hover`, `cursor`, `scroll`,
  `focusable`, `keeps_tab`, `key`, `text`, `char_key`, `selected_text`, `command`,
  `tooltip`, `menu`.

## Accessibility

A `Table` (its count all the rows) opened over the rows drawn; each row a `Row` keyed and
indexed by its place, its cells' texts read under it. Toolbar buttons are `ds::button`s;
the fields are `TextInput`s / `SearchField`s.

## Cost

- An identical repaint allocates nothing (a test holds it): the columns shown and their
  edges, a row's number and a cell's text are kept buffers; the bars' texts are kept for
  what they say (the page, the total, the language: `i18n::generation`); the source is
  asked `can_edit()` (no reason written) while drawing.
- Only what's in view is asked of the source (once: asked again with the same, nothing
  happens) and drawn; a cell's text cut to 240 characters before it's shaped.
- The selection's figures once per change (at most 100 000 cells); ⌘F keeps at most
  10 000 matches; the toolbar's overflow kept for its width and state.
- An export is written a block at a time (`ExportWriter`) by the view's worker, into a
  temporary file renamed over its target once complete.

## Example

```rust
let mut grid = DataGrid::new(source, page);
grid.set_project(project);
// paint
grid.paint(GridPaint { bounds: grid_area, scale, text, scene, palette: p, icons });
// input: one of the view's parts; then
if let Some(action) = grid.take_action() { /* clipboard, submit, export */ }
```

## Don't

- Copy rows into the grid: a `GridSource` lends them (`CellRef` borrows).
- Read a page on the UI thread: `request` asks, `poll` says it came.
- Draw a table of your own for rows from a source: this, or an option added to it.

## The selection as a table

`DataGrid::picked` gives the selection as a `charts_model::Tabular`, for whoever draws from
part of the rows:
- the rows it touches, each once, in the source's order;
- the columns selected, or every column shown when whole rows are;
- values as numbers or text.

Only rows already read are taken, at most `MOST_PICKED_ROWS` (20 000); the others are
counted in `left_out`. With less than two cells there is nothing (`has_picked`).
