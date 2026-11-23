# Changed files (`ui_components::changed_files`)

**For:** a set of changed files shown as a tree (or a list): a commit's, a file's
history's, the working tree's, a remote project's files to upload, a merge's conflicts.

**Where it's used:** git's Log, Changes and conflicts views, the remote project's
pending uploads.

## Anatomy

```
 64 files changed                 [⊞] [⊟] [Tree|List]
 ( 🔍 search names and paths                     × )
 ▾ 📁 FrontOffice/AI/Service/Chat   3     ← single-child folders folded onto one row
 │   M  Handler.php                       ← state letter, name colored by state
 │   D  Old.php  (struck through)
 │   R  New.php ← Old.php
```

- `FilesTree`: the files, the rows (rebuilt when the files, the search, what's open or the
  mode change), the chosen files, its `SearchField`, the scroll and its bar.
- `ChangedFile { path, old_path, state, note }`, `ChangeState` (added, modified, deleted,
  renamed, untracked, conflicted, failed).
- `TreePart { tree, event }`: the tree as a form's control.
- `state_mark`, `draw_lit` (a label with the found bytes lit).

## Options

| Option | Default | Effect |
|---|---|---|
| `set_files(files)` | — | the files shown; the selection (by path), what's open and the search kept |
| `count_key` | `changed_files.changed` | the header's count message (takes `{count}`) |
| `head_row: Some(HeadRow { row, total })` | `None` | the count, the search and the buttons on one row, aligned with what's beside it |
| `set_flat(true)` | tree | a plain list sorted by path |
| `expand_all(open)` | — | Expand / Collapse All |

## States and interaction

- Keys: ↑ ↓ among the rows, ← folds (or goes to the folder above), → opens (or goes in),
  ↵ the file's change (`TreeEvent`); in the search, editing.
- Press: a row chosen; ⌘ / Ctrl adds one, ⇧ a range; twice: a file's change, a folder
  opened / closed; on a chevron: opened / closed. ⌘A chooses every file shown.
- Right-click: `choose_at` keeps the chosen ones when the row is among them.
- The search: fuzzy over names and paths (as Go to File); folders with something found
  stay, what matched lit.
- `TreePart` as a `Control`: `hit`, `press_with`, `drag`, `release`, `hover`, `scroll`,
  `focusable`, `key`, `text`, `selected_text`, `command`, `cursor`, `tooltip`,
  `frame_deadline`; what it opened is written to `event`.

## Accessibility

Each row is a `TreeItem` (`tree::item`) with its level, open / folded and selected; the
header's buttons are `ds::button`s; the search is the shared field.

## Cost

Rows worked out on a change, never per frame; a frame draws only the rows in view; file
icons from `tree::FileIcons`.

## Example

```rust
let mut tree = FilesTree::default();
tree.set_files(files);
// input: one part among the view's
let mut tree = TreePart { tree: &mut self.tree, event: &mut self.opened };
// paint
self.tree.paint(ctx.scene, ctx.text, ctx.icons, &ds, (list, ctx.focused), queries.as_deref());
```

## Don't

- Draw a list of changed files of your own: this, with `count_key` / `head_row` for what
  differs.
- Find a row by its title: rows are known by a file's index or a folder's path.
