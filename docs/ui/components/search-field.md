# Search field (`ui_components::search_field`)

**For:** every place something is searched or filtered: one look (a pill, the magnifier,
× once there's text) and one behavior (⌘A, ⌘X, ⌘Z, copy, paste, one caret).

**Where it's used:** the picker's search, the changed files' search, the data grid's ⌘F,
the database explorer's and diagram's filters, the menu drawer, project search.

## Anatomy

```
 ( 🔍  text typed│                      [view's own]  × )
   ds::search_field   TextInput          trailing room  ds::clear_button
```

- `SearchField`: a `TextInput` (`input`), where it was drawn, its × (while there's text),
  whether it has the caret, and two flags its owner takes: `take_cleared`, `take_edited`.
- `SearchExtras`: what a view adds — room on the right for its own (a regex toggle, a
  count), and the field shown `invalid` (its border in the danger color).

## Options

| Option | Default | Effect |
|---|---|---|
| `SearchField::new()` | empty | — |
| `set_text(text)` | — | its text set by the owner: not an edit (`take_edited` unchanged) |
| `paint(layer, text, icons, ctx, (rect, focused), placeholder)` | — | drawn in `rect`; `focused`: the caret and the lit border |
| `paint_with(…, SearchExtras { trailing, invalid })` | `0`, `false` | room kept on the right (returned for the view to draw in); invalid border |
| `forget()` | — | not drawn now (its panel hidden): nowhere to be pressed |

## States and interaction

- Press: the caret placed, two clicks a word, three all of it; a drag selects (`Held`). On
  ×: the field emptied, `take_cleared()` and `take_edited()` say so once.
- Keys while focused: the editing keys of `TextInput` (← →, ⌥ / Ctrl by words, ⇧ extends,
  Home / End, Backspace, Delete); text typed and pasted on one line.
- Commands: `select_all`, `cut`, `undo`, `redo`; `selected_text` for the copy.
- Hover: × lit under the mouse; cursor the text's, a hand on ×.
- As a `Control`: `area`, `press`, `drag`, `hover`, `cursor`, `focusable`, `key`, `text`,
  `selected_text`, `command`. Kept among its view's parts, so the focus is the view's one
  and two fields never show a caret at once.

## Accessibility

Its `TextInput` tells a `TextField` named by the placeholder, its text as its value, its
caret and selection as `lines`, `focused` with the caret. The × is a `ds::clear_button`.

## Cost

Nothing kept beyond its text and two rectangles; its style is built per paint from the
`Ctx` without allocating.

## Example

```rust
filter: SearchField::new(),
// paint
self.filter.paint(layer, text, icons, ds, (field, typing), &placeholder);
// after input
if self.filter.take_edited() { self.search_again(); }
```

## Don't

- Draw a pill, a magnifier, a × or a caret by hand for a search: this one.
- Route its keys with a `typing` bool: put it in the view's `Controls`.
- Set its text from a search result and expect `take_edited`: `set_text` isn't an edit.
