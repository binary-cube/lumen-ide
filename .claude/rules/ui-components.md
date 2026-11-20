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

## Generic and agnostic: interaction goes through contracts

- **Code that routes doesn't name what it routes to.** Whoever passes input on (the app to
  its windows over the editor, a view to its parts, a panel to its rows) walks a list of
  parts that answer one contract — never a chain of `if self.completion… if self.dialog…
  if self.settings…`. A new part is added to the list and answers the contract; nothing
  else changes.
- **Every interaction is in the contract, not only the mouse**: a press and what follows
  it (drag, release: whoever took the press holds the pointer until it's let go), hover
  and the cursor, the wheel, the keys and the text typed (whoever has the focus), a
  selection and its copy, a tooltip, a right-click's menu. A part implements what it uses;
  the rest has defaults.
- **The windows over the editor answer `OverlaySlot`**:
  - what it answers: `click`, `drag`, `release`, `held`, `scroll`, `hover`, `key`, `text`,
    `input`, `selected_text`, `edit_command`, `paste`, `loading`, `frame_deadline`,
    `rest_deadline`, `press_elsewhere`…;
  - the stack is walked in `Overlay::ALL`'s order, and the editor's cards (hover,
    completion, the parameters' hint) are floating slots too;
  - a deadline is named by its overlay (`Overlay::name`), so a profile says whose it is.
- **A view's parts are a form's controls** (`ui_components::control`):
  - **The contract:** each part answers `Control`. The view keeps one `Controls` (who holds
    the pointer, who has the focus, what's under the mouse) and hands its parts as a list
    each time, from a `with_controls` that builds a fixed array, never a `Vec` per event.
  - **The meaning stays the view's:** a part that answered is known by its place in the
    list, or by what it wrote (`take_event`, an `action` / `answer` field). The view's own
    hit table (a rect → its action) is its meaning, not routing.
  - **Ready-made parts:**
    - fields and lists: `TextInput`, `MultilineField`, `Picker`, `DataGrid`, `ReadText`,
      `Icicle`, `changed_files::TreePart`, `date_range::RangePart`,
      `color_picker::ColorPart`;
    - bars and dividers: `Thumb` (an `EdgeScrollbar` with its offset), `ScrollThumb` (with
      a `ScrollState`), `BarThumb` (a `Scrollbar` with its own metrics), `Scrolled` (a bar
      plus the wheel over an area), `Divider` (a splitter);
    - views and wrappers: `Embedded` / `TabView` (a view inside a view), `Shown` (a part
      shown only sometimes), `Option<C>` / `Absent` (a part that may be missing),
      `OneClick`.
  - **What the contract says:**
    - `press_with` carries ⇧ / ⌘;
    - `captures_keys` is a part over everything (an open list, a picker): what it doesn't
      use (keys, text, paste, commands) goes on to the focused part;
    - `keeps_tab` / `in_tab_order` give a part its own Tab, or keep it out of the Tab
      order (the view moves the focus);
    - `Controls::sticky()` keeps the focus on a press beside the parts; `hold` holds a part
      from code.
  - **Asked from `&self`:** cursor and tooltip come from the last hover
    (`Controls::cursor_at` / `tooltip_at`: the app hovers before it asks); animation and
    the next frame come from `settle`, called while painting; the selection comes from
    `selected_text_with`, the view lending its parts read-only.
  - **A field with behavior of its own** (its key set, its Tab order, a password never
    copied) is a control of its module that does exactly that. It is never a `typing`
    bool routed by hand, and never the generic field if that would change the behavior.
- **Nothing generic names a module, a language or a view**: no `"php"`, `"terminal"`,
  `"database"` in the app or in components; what differs is said by whoever it belongs to
  (a field on its contribution, a default method it overrides).
- Found a chain that names its parts: it's a bug of the same kind as a hardcoded tool
  window — moved behind the contract.

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
