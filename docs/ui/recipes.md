# Recipes

## A new component

1. Look first: `ds`, `ui-components` (the table in `.claude/rules/ui-components.md`). What
   exists gets an option instead.
2. Pure work (layout, parsing, scales) in a domain crate or a pure module, tested alone.
3. In `ui-components`: its spec with today's defaults, its look from `Ctx`, its paint,
   `Control` if it has state, its accessibility node ([anatomy.md](anatomy.md)).
4. Its tests (`tests/unit/<file>.rs`), its page in `docs/ui/components/`, its row in the
   rules' table.

## An option for an existing component

1. A field in its spec (or look), a method setting it; the default keeps every caller's
   pixels as they are.
2. A test for the option, and the existing tests unchanged.
3. Its page: the option in its table.

## A module's view

Built from `ui-components` only (never `ui-workbench`): a `Viewer` whose parts are
`Control`s in one `Controls` handed as a fixed array (`with_controls`), its hits for its own
meaning, its work on its pool (`Deferred`, `host.spawn`, a `kernel::Pool`), its texts in
both locales.

## A pane over another module's data

The way a module offers something over rows another holds, without knowing it (a chart
under a query's result, under a table's data):

1. **The pane** (the offering module): a `ResultPane` (a `Control` that paints, told its
   rows as `result_pane::Lent` — `charts_model::Tabular`, readable on a worker), contributed
   as a `ResultPaneProvider` (id, icon, title, `make`). What it needs of the host is a
   `PaneRequest` (all the rows up to a limit, a "Save As", a message); what it is, a text
   it gives (`saved`) and gets back (`restore`).
2. **The host** (the module holding rows): a `result_pane::PaneHost` — its buttons in the
   host's bar (`paint_buttons`), the split (`split`), the input (`press`, `drag`, `release`,
   `hover`, `scroll`, `key`, `text`, `tooltip`, `cursor`, `command`), the state kept
   (`new(folder, shown, spawn)`). The host writes only how it lends its rows (`lend`, with a
   version) and how it fetches all of them.
3. **The app** registers the providers from the contributions; a hidden module's
   provider goes with it. No provider: no button.

## A chart

- **Something someone reads** (hovered, zoomed, exported): `charts::Chart` with a
  `ChartLook::new(ctx, background)`, its data a `charts_model::Prepared` — from rows
  (`charts_model::prepare(spec, &tabular)`, on a worker for many) or made directly
  (`Prepared::over_time` for a history). Its value's unit through `set_format`, a fixed
  axis through `set_y_range`, which parts show through `ChartParts`.
- **A small one in a cell, a card, a widget** (hundreds a frame, no state):
  `charts::spark::sparkline` over a `PlotCache`.
- **From any data** (a JSON, an XML, an HTML table, a CSV): `data_tables::tables(name,
  text, Limits)` gives `DataTable`s, each a `Tabular`.
