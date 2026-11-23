# Tree rows (`ui_components::tree`)

**For:** every tree's rows drawn alike: the chevron of a node that folds, the indentation
guides, a row's highlight, which rows of one height are in view, and a file entry's
chevron and icon.

**Where it's used:** the project tree, a remote project's folders, the changed files,
the database explorer, the Task Manager's groups, the tree table, the diagram files.

## Anatomy

```
 │ ▾ 📁 src                     ← chevron · entry icon · name
 │ │ ▸ 📁 tests
 │ │   📄 main.rs               ← guides (one per parent level)
```

Pieces, each one thing; each tree keeps its own geometry and only draws and hit-tests with
these:

- `chevron(layer, icons, at, expanded, color)`, `chevron_icon(expanded)`.
- `guides(layer, levels, x_of, (top, height), width, color)`.
- `highlight(layer, at, color, radius)`: a rounded fill.
- `item(Item { name, bounds, level, expanded, selected })`: the row told to a screen
  reader while the returned guard lives.
- `TreeList { row_height, len }`: `visible(scroll, height)`, `slot_at(y)`, `row_at(y)`.
- `EntryLayout::new(scale, row_height, indent)`: a file tree's places — `x(left, depth)`,
  `guide_x`, `guides`, `chevron_rect`, `icon_rect`, `label_x`, `pill`, `pill_radius`.
- `entry_icon(layer, icons, layout, (x, top), EntryIcon, (chevron_color, dim))`: a
  folder's chevron and icon (the icon set's), or a file's.
- `FileIcons`: files' icons and colors by name, kept per icon set and style.

## Options

| Option | Default | Effect |
|---|---|---|
| `EntryLayout::new(scale, row_height, indent)` | padding 14, a hairline | the project tree's geometry, in physical pixels |
| `EntryIcon::Folder { name, expanded, color }` | — | chevron + the set's folder icon (its own tint wins) |
| `EntryIcon::File { icon, color }` | — | the file's icon |
| `dim` | — | every color passed through it (a row cut, ignored) |
| `FileIcons::new(limit)` | — | names remembered per generation |

## States and interaction

These pieces hold no state: the tree that uses them keeps the scroll, the selection, the
hover and the keys. `TreeList` answers which row is under `y` and which are in view.

## Accessibility

- A row told whole (`item`): a `TreeItem` named by its text, its `level`, `expanded`
  (`None`: a leaf), `selected`; its texts that say its name are its own.
- A chevron drawn outside such a row is a `TreeItem` node by itself, open or folded, named
  by the text on its line; inside one it says nothing more.

## Cost

`FileIcons` is asked for every visible row every frame and answers from memory; it's
emptied when the icon set or style changes (`stamp`) or on `clear` (theme, project).

## Example

```rust
let layout = EntryLayout::new(ctx.scale, ROW_H, INDENT);
let range = TreeList::new(h, self.rows.len()).visible(self.scroll, b.size.height);
let _row = tree::item(tree::Item { name, bounds: full, level, expanded, selected });
let icon = tree::entry_icon(layer, icons, &layout, (x, top), entry, (p.text_muted, &dim));
```

## Don't

- Draw a chevron or guides by hand: these, so every tree looks the same.
- Give a row a name built per frame when no screen reader runs: build it only while
  `access::recording()`.
