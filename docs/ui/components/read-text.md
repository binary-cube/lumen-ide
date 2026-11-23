# Read text (`ui_components::read_text`)

**For:** a text to read, not edit: release notes, a description, a consent's terms, a
log entry's details. Wrapped, scrolled, selected and copied as in an editor.

**Where it's used:** the profiler's consent, the log viewer's details, release notes.

## Anatomy

```
 Heading                                        ┃
 A paragraph wrapped at words to the area's     ┃ ← EdgeScrollbar (wheel, thumb, track)
 width, s̲e̲l̲e̲c̲t̲e̲d̲ across blocks.                  ┃
   • a point of a list, indented by depth
     • a deeper one
```

- `Block { kind, text }`, `BlockKind` (`Heading`, `Paragraph`, `Point { depth }`, `Gap`).
- `ReadText`: the blocks, their layout (rows of shaped text, made when painted, again only
  when the width, the scale or the blocks change), the selection, the scroll and its bar.

## Options

| Option | Default | Effect |
|---|---|---|
| `ReadText::new(blocks)` | — | — |
| `set_blocks(blocks)` | — | laid out again, unselected, at the top |
| `menu()` | — | Copy (greyed with nothing selected), Copy All; `copied(id)` gives the text |

## States and interaction

- Press: the selection begins (⇧ extends it); two clicks a word, three a block; on the
  scrollbar its thumb is taken or the text moves there. A drag extends by its unit.
- Wheel: scrolls (`scroll_by`). Hover: its bar widens; the I-beam over its text only.
- Commands: `select_all`; the selection copied as text (a point with its `•`, indented,
  blocks on lines of their own).
- As a `Control`: `hit`, `press_with`, `drag`, `release`, `hover`, `cursor`, `scroll`,
  `focusable`, `selected_text`, `command`, `menu` (its own), `frame_deadline`.

## Accessibility

None yet: its rows aren't told to a screen reader (no `access` node), though they're what
the user reads.

## Cost

Laid out once per width, scale and blocks; a frame draws only the rows in view.

## Example

```rust
text: ReadText::new(blocks()),
// paint
self.text.paint(scene, text, &ds, inner);
// a right-click over it: its menu, the row chosen told to `copied`
```

## Don't

- Draw paragraphs and bullets by hand in a view: this.
- Offer a text to read without its menu: what a user reads can be copied.
