# Editor bars (`ui_components::editor_toolbar`)

**For:** a bar a module puts above the editor of the files it knows. The file stays a
text file in the editor — undo, find, multiple cursors, saving — and the bar adds what
its module does with it.

**Where it's used:** the database console (Run, Stop, the transaction, the database and
schema), the HTTP client's `.http` files, the language server download offer (a bar with
no height once there's nothing to say).

## Anatomy

```
 tab:  [● console.sql TX]           the bar's accent as a dot, its badge after the title
 ┌ bar ───────────────────────────────────────────────────────────────┐  accent strip on top
 │ [▶ Run] [■ Stop]   [db ▾] [schema ▾]                    [Tx ▾]     │  HEIGHT (40) unless it says
 └────────────────────────────────────────────────────────────────────┘
 ▶│ SELECT * FROM users          ← gutter marks (Runnable, Running, Done, Failed),
 ┃│ WHERE id = 1;                  the statement Run would run highlighted / barred
```

- `EditorToolbarProvider` (contributed): `id`, `open(&ToolbarContext)` — asked when a
  file's tab opens: a look at the path and language, nothing connects.
- `EditorToolbar`: the bar. Everything but `paint` has a default.

## Options

| Option | Default | Effect |
|---|---|---|
| `height()` | `HEIGHT` (40) | asked at each layout; 0 hides it |
| `title()` / `icon()` / `crumbs()` | the file's | the tab's title, icon, the breadcrumb's start |
| `accent(palette)` / `tab_badge(palette)` | `None` | the tab's color dot and bar strip; a mark after its title (`TX`) |
| `may_close()` | `true` | `false` keeps the tab open (the bar asks first, then closes it) |
| `moved(path)` | `false` | `true`: still the file's bar, it follows; `false`: providers asked again |
| `follows_editor()` / `sync_editor(&EditorState)` | `false` | the editor's text, version, cursor, selection while it asks |
| `marks()` / `mark_clicked(at)` | none | the gutter's marks and a version; a ▶ clicked |
| `highlight()` / `current()` / `focus()` | `None` | the statement Run would run, the one the cursor is in, a finding's place |
| `grammar_hints()` | `false` | a permissive grammar's errors shown, faint |
| `purge()` | nothing | "Clear and Rebuild Caches" |

## States and interaction

- `pointer_down`, `hover`, `cursor_at`, `tooltip_at`: the app hands it the mouse over
  the bar.
- What it asks for is a `ToolbarAction`: `View(ViewerAction)`, `Menu { anchor, entries }`
  (a button's dropdown), `List { anchor, rows, chosen }` (the shared picker under a
  field); the row chosen comes back as `command(id)`. `take_action` after a poll or a command.
- `poll` after a wake (a statement ended); `is_loading` / `is_animating` for frames;
  `take_notifications` for toasts.
- `text_hash` (FNV-1a): a mark shows only while the statement's text is the same.

## Accessibility

Nothing of its own: the contract has no description hook; a bar built from `ds::button`
and `ds::dropdown` describes its buttons through them (see the audit).

## Cost

- `open` is cheap: nothing starts until the bar is used.
- `accent` and `tab_badge` are asked at each paint: kept, never worked out.
- `EditorState` is handed only when the editor changes, and only to a bar that follows.

## Example

```rust
impl EditorToolbarProvider for OfferProvider {
    fn id(&self) -> &'static str {
        "language-server-download"
    }

    fn open(&self, context: &ToolbarContext<'_>) -> Option<Box<dyn EditorToolbar>> {
        let language = context.language?;
        offer(language)?;
        Some(Box::new(DownloadBar::new(language, Arc::clone(&context.wake))))
    }
}
```

## Don't

- Connect, read or start anything in `open`: it's asked for every tab that opens.
- Draw a dropdown's list or a menu yourself: return `ToolbarAction::List` / `Menu`.
- Work out `accent` or `tab_badge` when asked: keep them.
