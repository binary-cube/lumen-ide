# Views (`ui_components::viewer`: `Viewer`, `ViewerProvider`, `ViewerPaint`)

**For:** anything a module shows in a tab, a modal or a popover: a file's viewer (an
image, a rendered markdown, a data grid) or a view opened by its extension (`view-*`).
It's the contract between a module's view and the app: the app hosts it and hands it the
input, the view paints and says what it wants as a `ViewerAction`.

**Where it's used:** every module's view: the markdown viewer, the data views, Git's
views, the Task Manager, the pickers' popovers (colors, levels), refactoring previews.

## Anatomy

```
 ViewerProvider (contributed)            the app                     Viewer
   id · priority · kind(path) ──▶ asks who shows a file ──▶ open(path) ─▶ Box<dyn Viewer>
                                  hosts it: tab / split / modal / popover
                                  each frame ──▶ paint(ViewerPaint { bounds, scale, text,
                                                  scene, palette, icons, focused, waker })
                                  input ──────▶ pointer_down · hover · drag · release ·
                                                scroll · zoom · keys · text · paste · menus
                                  ◀── ViewerAction (OpenFile, RunCommand, Ask, OpenView,
                                       Request(AppRequest), WithHost, …)
```

- `ViewerKind::Replace` (only the viewer: images) or `Preview` (the source stays
  editable; Editor / Split / Preview), its content via `set_source`.
- `ViewerPriority::Normal` (the modules' order) or `Fallback` (after every other).
- `ViewerPaint`: what it paints with; `waker` wakes the app from a worker.

## Options

Every method has a default but `paint`. The ones most views implement:

| Method | Default | Effect |
|---|---|---|
| `paint(ViewerPaint)` | — | draws in `bounds` |
| `set_data(&dyn Any)` | nothing | the data it was opened with (`OpenView`) |
| `set_shown(bool)` | — | its tab visible or hidden: pause background work when hidden |
| `set_window_focused(bool)` | — | the window has the keyboard or another program does: what moves only for the eye (a preview's effects) stands still in the background |
| `pointer_down(p, clicks, mods)` / `click(p)` | `None` | a press (⇧ / ⌘ in `mods`) |
| `hover`, `drag`, `release`, `scroll`, `zoom` | `false` | `true` when something changed |
| `cursor_at`, `tooltip_at` | `None` | the pointer's cursor, a `Tip` |
| `context_menu(p)` / `take_menu()` | `None` | a right-click's rows / a menu under its own button; rows back as `command(id)` |
| `wants_text`, `text_input`, `edit_key_with`, `takes_keyboard`, `key_press` | off | a field's text and keys; the whole keyboard (the terminal) |
| `selected_text`, `select_all`, `paste` | none | the clipboard's side |
| `poll`, `is_loading`, `is_animating`, `frame_deadline` | none | work coming back, frames only while something moves |
| `take_action`, `take_notifications` | none | what it asks for on its own; toasts |
| `memento`, `restorable` | none | kept between sessions |
| `modal_width`, `modal_height`, `modal_buttons`, `draws_footer`, `key_hints`, `modal_closable`, `modal_status` | the frame's | in a modal: its size, its footer's buttons and keys, closing off while work runs |
| `tab_icon`, `live_title`, `subtitle`, `crumbs`, `accent` | the file's | its tab |
| `memory()` | 0 | what it holds (RAM, GPU), for the Task Manager |
| `purge()` | nothing | "Clear and Rebuild Caches" |

## States and interaction

- Hidden or shown (`set_shown`); focused or not (`ViewerPaint::focused`); the window in
  front or behind (`set_window_focused`).
- Its parts are a form's controls: it keeps one `Controls` and hands its parts as a list
  (`with_controls`); the app never routes to its parts by name.
- What only the app can do (windows, tabs, the clipboard, pickers) is asked with
  `ViewerAction::Request` / `WithHost`; another module's work with `ViewerAction::Ask`.

## Accessibility

The contract has no method of its own: a view is described by the components it paints
with (`ds::button`, `Picker`, `DataGrid`, `TreeList`…). A view drawing by hand is silent.

## Cost

- Made when its tab opens, dropped when it closes; `set_shown(false)` stops its work.
- Asked each frame only for `paint` and cheap state (`is_animating`, `frame_deadline`);
  menus and `enabled` read memory only.
- A click never reads a file: `deferred::Deferred` or `host.spawn`.

## Example

```rust
impl ViewerProvider for MarkdownProvider {
    fn id(&self) -> &'static str {
        "markdown"
    }

    fn kind(&self, path: &Path) -> Option<ViewerKind> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        EXTENSIONS.contains(&ext.as_str()).then_some(ViewerKind::Preview)
    }

    fn open(&self, path: &Path) -> std::io::Result<Box<dyn Viewer>> {
        Ok(Box::new(MarkdownViewer::new(path)))
    }
}
```

## Don't

- Depend on `ui-workbench` from a view: build it from `ui-components`.
- Draw a modal's buttons in its content: `modal_buttons` / `draws_footer`.
- Read a file or run git in `pointer_down`: prepare the action off the UI thread.
- Name another module's view: open it by its extension through `ViewerAction::OpenView`.

## Moving, loading, waiting

- `is_animating`: something visibly changes every frame (a transition, a drag scrolling);
  a frame that shows nothing new is a bug, named in the log and by the snapshots.
- `is_loading`: work goes on (a worker, a measure, a program's first output): a loader's
  pace.
- `frame_deadline`: one frame at a moment (a pause after typing, a scrollbar's fade).
- A worker's result wakes the window; a worker gone (`Disconnected`) ends the wait.
