# Multi-line field (`ui_components::multiline`)

**For:** text of several lines typed in a form: a commit's message, a request's body or
variables, a diagram's label.

**Where it's used:** the git commit box, the HTTP client's GraphQL query and variables,
the diagram editor's text editing.

## Anatomy

```
 ┌──────────────────────────────────────────── 52/72 ┐
 │ First line, wrapped at words to the field's       │
 │ width│                                            ┃ auto-hiding scrollbar
 │ second paragraph…                                 │
 └───────────────────────────────────────────────────┘
```

- `MultilineField`: the text, cursor and selection, the undo history (characters typed in
  a row are one step, at most 200 kept), the visual lines and each character's x from the
  last layout, its scroll and its scrollbar.
- `MultilineSpec`: what it shows besides its text.

## Options

| Option | Default | Effect |
|---|---|---|
| `MultilineField::new(text)` | — | its text (`\r\n` made `\n`) |
| `set_text(text)` | — | all replaced, the history starts over |
| `MultilineSpec { placeholder, focused, subject_limit }` | — | `subject_limit: Some(72)`: a hint `52/72` past which the first line warns |
| `height(text, ctx, width, (min, max))` | — | its height between `min` and `max` lines of its text, with its padding |

## States and interaction

- Press: the caret placed (⇧ extends), two a word, three a line; its scrollbar first. A
  drag selects (held until let go).
- Keys while focused: ← → (by words), ↑ ↓ by visual line keeping its x, Home / End (of the
  line; with ⌘ of the text), Backspace / Delete, ↵ a line break.
- Text typed and pasted keeps its breaks; `undo` / `redo`, `select_all`, `selected_text`.
- Wheel: scrolls, never past its ends. Hover: its scrollbar shows on its lane.
- As a `Control`: `press_with`, `drag`, `release`, `hover`, `cursor` (text), `scroll`,
  `focusable`, `key`, `text`, `selected_text`, `command`, `frame_deadline`.

## Accessibility

None yet: unlike `TextInput`, it describes no node, so a screen reader doesn't find it.
It should tell a `TextField` named by its placeholder, its text and its lines.

## Cost

Laid out at paint only when the text or the width changed; the keys and the mouse use the
last layout (lines as byte ranges, characters' x).

## Example

```rust
body: MultilineField::new(&text),
// paint
form.body.paint(scene, text, &ds, q, MultilineSpec {
    placeholder: "query { … }",
    focused: focus == Some(Focus::Area(Area::Body)),
    subject_limit: None,
});
```

## Don't

- Use it for one line: `TextInput` (a field's Enter submits there).
- Route its keys by hand: it's a `Control` among its view's parts.
