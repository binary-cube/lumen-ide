# Text field (`ui_kit::TextInput`)

**For:** every one-line field someone types into: a form's value, a name, a password, the
text inside a search field.

**Where it's used:** inside `SearchField`, every dialog's and wizard's field (with
`ds::field_style`), the data grid's `WHERE` / `ORDER BY` fields and cell editor, the
color picker's value, the HTTP client's URL.

## Anatomy

```
 ┌───────────────────────────────────────────┐
 │ lead │ text, sel̲e̲c̲t̲i̲o̲n̲, caret│ ……… │ trail │
 └───────────────────────────────────────────┘
```

- `TextInput`: the text, the cursor and the selection's anchor (bytes on character
  boundaries), an undo history (typing in a row is one step), where each character was
  drawn last (a press is placed against it), and whether it's a secret.
- `TextInputStyle`: font, colors, corners, border, padding, caret width; `lead` / `trail`
  keep room for what its owner draws inside (an icon, a ×, Send).

## Options

| Option | Default | Effect |
|---|---|---|
| `TextInput::new(text)` | — | its text, cursor at the end |
| `set_masked(true)` | off | a secret: dots, never copied, no history, wiped from memory |
| `set_text(text)` | — | all replaced, all selected |
| `TextInputStyle { … }` | — | from `ds::field_style(ctx, font, invalid)` (a form's), `ds::search_input_style`, or the caller's |
| `style.lead` / `style.trail` | 0 | room before / after the text for the owner's own |

## States and interaction

- Press (`press(x, clicks)`): one puts the caret, two a word, three all; `extend_to(x)`
  for ⇧-click and drag.
- Keys: ← → (by words with ⌥ / Ctrl, ⇧ extends), Home / End, Backspace / Delete (by
  words); `insert` replaces the selection and drops control characters.
- `undo` / `redo` (none for a secret); `select_all`, `selected_text`.
- As a `Control` (in `ui_components::control`): `hit`, `press` (`Held`: a drag selects),
  `drag`, `release`, `cursor` (text), `focusable`, `key`, `text` (one line: breaks made
  spaces), `selected_text`, `command` (`select_all`, `cut`, `undo`, `redo`).

## Accessibility

A `TextField` named by its placeholder, `focused` with the caret, its text as its value
and its caret, selection and line as `lines`. A secret's text (even shown) never reaches
the tree, not even its length.

## Cost

- Long text scrolls so the caret stays in view; the characters' places are found in one
  pass per paint.
- A secret reserves room up front so typing never leaves copies of it behind.

## Example

```rust
let style = ds::field_style(&ds, ds.ui_font(-0.5), invalid);
self.name.paint(layer, text, rect, focused, &placeholder, &style);
// in the view's parts: [&mut self.name, …] — the press, keys and commands are its own.
```

## Don't

- Draw a frame around a frameless field to put a button in it: use `trail` and draw the
  shared button there.
- Log or copy a secret's text: it's `masked` for that.
- Use it for several lines: that's `MultilineField`.
