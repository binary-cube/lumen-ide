# Picker (`ui_components::picker`)

**For:** every list to choose from opened over a field or a button: a dropdown's list, a
choice of several (ticked), suggestions under a text field, a small table to pick a row of.

**Where it's used:** every `ds::dropdown` (settings, forms), the remote project's
properties (owner / group as a table), the git log's filters, the toolbar lists the app
shows for an editor bar (`ToolbarAction::List`).

## Anatomy

```
 ┌ search (shared SearchField; only past SEARCH_FROM items) ┐
 ├──────────────────────────────────────────────────────────┤
 │ header (TableRows only, fixed while the rows scroll)     │
 │ (AB) Name        detail                       ↵    ✓     │
 │ (CD) Name        detail                                  │ ┃ scrollbar
 ├──────────────────────────────────────────────────────────┤
 │ ↑ ↓ move · ↵ select · esc close                          │
 └──────────────────────────────────────────────────────────┘
```

- `Picker`: what's typed, the item under the keys, the scroll, the chosen ones; placed
  under its anchor, or above when there's more room there; as wide as its anchor, never
  narrower than its rows need.
- `PickerItem`: `label`, `detail`, `avatar` (initials) or `icon`, `cells` (a table's).
- `PickerRows` (injected): how a row's content is drawn. `ItemRows` by default (the detail
  in a column after the longest name), `TableRows` for columns under a header. The list
  keeps what's common: the search, the row's highlight, `↵` and the check, the scroll.

## Options

| Option | Default | Effect |
|---|---|---|
| `open(items, chosen)` | — | one to choose, `chosen` lit and in view, drawn as `ItemRows` |
| `open_with(items, chosen, Box<dyn PickerRows>)` | — | the same, its rows drawn by the caller's (`TableRows::new(columns)`) |
| `open_many(items, chosen)` | — | several to choose: a row ticked or not, the list stays open (`PickerEvent::Toggled`) |
| `open_suggestions(items, chosen, lit_first)` | — | under a text field: no search of its own, nothing lit until ↑ / ↓ (or the first, `lit_first`) |
| `always_search()` | past 6 items | its search shown however few items there are |
| `PickerItem::person(label, detail)` | — | a row with an avatar of the name's initials |
| `PickerItem::with_cells(cells)` | none | a table's cells after the name |
| `PickerColumn { title, width, right }` | — | `width` 0: takes what the others leave; `right`: numbers |

## Paths

`open_suggestions_with(items, (chosen, lit_first), Box::new(PathRows))`: rows of paths —
the name first, the characters typed lit in the accent (`PickerItem::marks`, byte
positions in the label: for folders `workspace::search::folder_marks`), the folder after it
muted and cut in the middle (`ds::draw_ellipsized_middle`), the whole path as the row's
tooltip (`PickerRows::tooltip`, `Picker::tooltip_at`). Nothing allocated as they're drawn.
The list stays as wide as its field (`PickerRows::widens` → `false`): its paths shorten,
so it never grows and shrinks as the rows change while typing. Other rows widen it to read
their names whole (the default).

## States and interaction

- Keys (all its own while open): ↑ ↓ move, ↵ chooses, Esc closes, Tab moves in it
  (`keeps_tab`); what's typed goes to its search (one line: a paste's breaks become spaces).
- Press: a row chosen (`PickerEvent::Chosen`), or ticked (`Toggled`); outside it closes
  (`Closed`). In the search: the caret, two clicks a word, three all, a drag selects.
- Hover: the row under the mouse lit; its scrollbar widens. Cursor: a hand on a row, the
  text cursor in the search, the arrow elsewhere over it — never what's under it.
- Wheel: its rows scroll; its scrollbar's thumb is dragged (`dragging`).
- As a `Control`: `area`, `hit` (anywhere while open: a press outside is its own),
  `press`, `drag`, `release`, `hover`, `cursor`, `scroll`, `focusable`, `keeps_tab`, `key`,
  `text`, `paste`, `selected_text`, `command` (⌘A, ⌘X, ⌘Z on its search),
  `frame_deadline` (the scrollbar's fade). Its owner reads `take_event()` after each.

## Accessibility

A `List` with the count of all the rows it keeps; each row drawn is a `ListItem` named by
its label, keyed by its item, its place as `index`, `selected` when chosen, `focused` under
the keys. Its search is the shared field (a text field named by its placeholder).

## Cost

- Filtering runs on a key, never per frame. The rows' widths are measured once per open
  (`natural_width`), the columns once per width and scale (`PickerRows::prepare`).
- Only the rows in view are drawn, in a layer clipped to the list.

## Example

```rust
let columns = vec![
    PickerColumn { title: t("remote.props_account"), width: 0.0, right: false },
    PickerColumn { title: id.into(), width: 64.0, right: true },
];
self.picker.open_with(items, chosen, Box::new(TableRows::new(columns)));
// as a part: [&mut self.picker, &mut field] in the view's Controls, then
let event = self.picker.take_event();
// painted last: self.picker.paint(scene, text, icons, &ds, (anchor, bounds), &placeholder);
```

## Don't

- Open a context menu from a dropdown's field: a dropdown is the field and this list.
- Draw a list to choose from by hand (rows, a check, keys at the bottom): add a
  `PickerRows` if the rows need another look.
- Paint it before what's under it: it draws in layers of its own, last.
