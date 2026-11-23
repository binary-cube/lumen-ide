# Panes over rows (`ui_components::result_pane`)

**For:** a module offering something over rows another module holds, without either
knowing the other: a chart under a query's result, under a table's data.

**Where it's used:** offered by the `charts` module; hosted by the database console's
result tab and the explorer's data view.

## Anatomy

```
 host's bar: …  [pane buttons, one per provider]  [export]
 ┌ the host's grid ─────────────────────────────┐
 ├──────── divider (dragged) ───────────────────┤
 │ the pane (paints itself, a Control)          │
 └──────────────────────────────────────────────┘
```

- `ResultPaneProvider` (contributed): `id`, `icon`, `title`, `make`.
- `ResultPane: Control + Send`: `set_rows(Lent, epoch, Held)`, `paint`, `poll`, `busy`,
  `take_request`, `saved` / `restore`, `hidden`, and `command` (a "Save As"'s path).
- `PaneHost` (the host's side): buttons, split, input, keys, kept state, lending.
- `PaneRequest`: `AllRows { limit }`, `SaveAs { name }`, `Tell { text, ok }`.

## Behavior

- No provider registered (its module off): no button, nothing else changes.
- Rows are lent anew only when their version changes; while more are on their way the
  pane updates its count and prepares once, at the end.
- A pane's state is kept per shown thing (a statement's text, a table) in the project, read
  and written on the host's pool.
- While the pane's list is open it takes the keys and the text.

## Don't

- Name a pane's module in a host, or a host's module in a pane.
- Copy rows to lend them: lend the host's chunks (`database::lent::Lent`).

## From the selection

- A right-click on the grid's rows adds the host's rows (`PaneHost::menu`):
  - "{pane}: from the selection", for every pane offered, greyed out with less than two
    cells selected;
  - "{pane}: from all the rows again", while the open pane is drawn from a selection;
  - "Close the {pane} pane".
- The view hands them back (`menu_command`) with the grid's selection as a table
  (`DataGrid::picked`, taken only when asked). Nobody names anybody: the grid knows no
  pane, the host no module, the view neither.
- While picked, the pane is lent the selection (`Held::picked`) instead of the rows.
  - It says so in its bar ("From 12 selected rows") and offers "All rows" there
    (`PaneRequest::WholeRows`, handled by the host itself).
  - Its state kept is the one set for all the rows.
