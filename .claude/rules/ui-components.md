# UI components: use what we have

Part of the project's rules: [index](project-conventions.md). Related: [modals](modals.md),
[writing code](code.md), [performance](performance.md), [modules](modules.md).

**A view is assembled from `ui-components`, never drawn by hand.** Before painting a
scrollbar, a list, a field, a menu or a table, look here (and in `ds`): the one component
everyone uses gives the same look, keys, hover, focus ring and accessibility for free, and a
fix in it reaches every place. What a component lacks is **added to the component** (an
option, a default method on its trait), never copied into a module. A custom drawing found
where a component exists is replaced by it (as the scrollbars were).

## What there is

| Need | Component |
|---|---|
| Tokens, surfaces, dividers, row highlight, focus ring | `ds` (`ds::surface`, `ds::divider*`, `ds::row_highlight`, `ds::focus_ring`, `space`, `radius`) |
| Buttons (primary, secondary, danger, with a key, busy), switch, checkbox, choices (pills), segmented, stepper, radio card, search field | `ds::controls` |
| A field showing a choice, its list opened by a click | `ds::dropdown` + `picker::Picker` (settings: `SettingsRow::Dropdown`) |
| A list to choose from, with search (one or many) | `picker::Picker` |
| A dropdown's list asked by an editor bar (it has no keys of its own) | `ToolbarAction::List` (the app shows the shared picker under the field) |
| Badges, a dot on a button, a key cap, a file's row | `ds::badges` |
| Text that fits (ellipsized, wrapped), a section title | `ds::text` |
| Loader, progress bar, empty state, tooltip pill, keys' footer, modal footer and its buttons | `ds::feedback`, `ds::progress` |
| Scrollbars (always this one: thumb drag, lane of its own) | `scrollbar::{Scrollbar, EdgeScrollbar}` |
| A resizable separator between panels | `splitter::Splitter` |
| The parts every table shares: header band, a column's name and subtitle, the edges between headers (lit when held), the lines between columns, alternating rows | `table` (`header_band`, `header_cell`, `header_edge`, `header_rule`, `column_lines`, `stripe`, `HEADER_H`) |
| Every tabular listing (rows from a source, lazy, filter WHERE / ORDER, paging, resizable columns, zebra rows, a cell's icon) | `data_grid` (`GridSource`) |
| A tree (project tree, a remote's) | `tree` (`TreeList`, chevrons, guides) |
| A tree on columns (rows with depth, folded / opened) | `tree_table::TreeTable` |
| Columns fitted to a width by priority | `columns` |
| Charts, sparklines, an icicle (flame graph), a ring | `chart`, `icicle`, `progress_ring` |
| A metric's card | `metric_card` |
| A text to read (release notes, details: headings, paragraphs, points), its selection and its right-click menu (`ReadText::menu` / `copied`: Copy, Copy All) | `read_text` (`Block`) |
| A log's lines (Output) | `log_view` |
| Tooltips (one manager; wide items show by the mouse) | `tooltip::{Tip, Tooltips}` |
| Toasts | `toast` (a view returns `Notification`s, never draws them) |
| A bar whose items may not fit (an "…" menu) | `overflow` |
| A slider over stops, a color picker, a date range, a multi-line field | `slider`, `color_picker`, `date_range`, `multiline` |
| Work prepared off the UI thread for a click | `deferred::Deferred` |
| Smooth transitions | `anim` |
| A settings form inside a panel | `form` |
| Side panels and their buttons | `tool_window`, `tool_panel` (registered, never hard-coded) |

Menus are the app's (`ViewMenuEntry`), modals and overlays the app's frame
([modals](modals.md)), the status bar the app's (below).

## Scrolling (it went wrong each time: check it every time)

- **A scrollbar is wired whole, never only drawn**: `paint` with the content's height and
  the offset, `hover` (it widens), and its thumb taken and followed — `press` first in the
  view's `click` (a tab and a modal both reach `click`), then `drag` and `release` (the
  `Viewer`'s own). A bar that shows but can't be dragged is a bug.
- **A test drags its thumb** (press on the bar, drag down, the offset grew), beside the one
  for the wheel.
- **An open list takes the pointer**: while a picker or a menu is open over a view, the
  cursor is the list's (`Picker::cursor_at`: a hand on its rows, the text's in its search,
  the arrow elsewhere), never what's under it.

## Dropdowns

- **A dropdown is always the field (`ds::dropdown`) and the shared picker under it**: the
  tinted surface, a search on top once there are enough rows, the ✓ on the chosen one,
  the keys' footer (`↑ ↓ · ↵ · esc`). Never a context menu opened from a dropdown's field
  (white, no keys): a view or a panel holds a `Picker` of its own; an editor bar asks the
  app for one (`ToolbarAction::List`). Rows that act instead of choosing ("Add an
  environment…") go at the end, each with its icon.
- A context menu is for a right-click and a row's "⋮" only.
- **What a user reads can be copied**: a view showing a `ReadText` offers its menu on a
  right-click over it (`ReadText::menu`, the row told to `ReadText::copied`).

## Tables

- **One look for every table**: the header with the column's name and, under it, a muted
  subtitle (its type, its meaning); thin separators between columns; an edge dragged to
  resize, double-clicked to fit; rows alternating (odd / even) with the hover and the
  selection over them; a row clicked shows its details where the view has room (under
  the table), the table's cells cut short with `…`.
- These parts are `ui_components::table`: the data grid and the Task Manager both draw
  with them. A table that has its own structure (the Task Manager's groups, its tree, an
  operation's progress) keeps it and takes the header, the edges, the column lines and
  the stripes from there; rows that span the table (a group's header, a progress) get no
  stripe nor column lines.
- **Of the same look as the cards around it**: a table standing among cards (the Task
  Manager's) has a rounded surface without an edge, its header's top corners following it
  (`table::header_band_rounded`); its rows' hover, selection, group band and keyboard ring
  keep their small rounded corners.
- **What a row shows can be copied**: a right-click on its details offers Copy (what's
  selected), the message, the line as written.

## Menus

- **Settings before Properties**: where a menu offers both (a connection's settings, an
  entry's properties), "… Settings…" comes first and "Properties…" right after it, last in
  its group — the same in every menu.
- **Menus are the app's one component** (`ViewMenuEntry` → the context menu); what a menu
  needs that it lacks is added to it (a greyed-out row, a ✓ at the end, a color dot:
  `RowLook`), never drawn by a module. A row shown greyed out is one the user should know
  exists but can't use here (an agent not hosted yet, one not installed); a row with no
  partner at all stays hidden (`presence`).

## The status bar

- **Every item has a priority** (`StatusItem::essential` / `spare`, normal by default):
  what doesn't fit goes under `…` by it, the farthest from the corner first; items that
  belong together are `joined`. Never an item the bar draws over another.
- **An item with more to say gets a window, not more items**: the bar's
  `status_popover` (label / value rows, a row running its command, graphs, a pin), floating,
  its rows made at its paint from what's already in memory.

## Settings

- A module's settings are a `SettingsContribution`; its rows are the shared ones
  (`SettingsRow::Choices`, `Segmented`, `Slider`, `Toggle`, `Text`, `Dropdown`,
  `Actions`, `Cards`…). Many values to pick from (a level for each module): a
  `Dropdown` per row, never a text to type `a=b, c=d` into.
- A settings button that runs a command returns it from `command()`; the app runs it
  whether it's a module's or its own.
