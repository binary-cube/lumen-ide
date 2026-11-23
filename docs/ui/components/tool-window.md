# Tool windows and panels (`ui_components::tool_window`, `ui_components::tool_panel`)

**For:** anything with a button on the window's sides (left top, left bottom / the bottom
area, right, the top bar's right): a module's or the app's own. It's registered once,
never hard-coded; the app discovers and lays them out — one dock per side, the same
buttons, tooltips and headers for all.

**Where it's used:** every side window: the project tree, Structure, Git, the terminal,
the Database window, the Task Manager's button, Focus mode's button, the canvas.

## Anatomy

```
 top bar ……………………………………… [🔍][⚙]        Dock::Top: a command, no window
 ┌──┬──────────────────────────────┬──┐
 │▣ │ ┌ Header: icon Title  [+][⟳]┐│ ▤│  Dock::Left (top group) / Dock::Right
 │▣ │ │ the panel (ToolPanel)      ││ ▤│  buttons by `order`, a short line between groups
 │  │ └────────────────────────────┘│  │
 │▤ │ bottom area: views as tabs    │ ⚙│  Dock::Bottom (left strip's bottom group);
 └──┴──────────────────────────────┴──┘  `strip_end`: the right strip's bottom group
```

- `ToolWindowProvider` (contributed at `register`): its button and what showing it does.
- `ToolContent`: `Panel` (made on first show), `Views { command, new }` (the module's
  views as the window's tabs), `Command { command, tab }` (no window: the button runs it).
- `ToolPanel`: a panel's content, made by `create()` when first shown.
- Its right-click menu is a `MenuContribution` with the same id.

## Options

| Option | Default | Effect |
|---|---|---|
| `id`, `icon`, `title()`, `dock()`, `content()` | — | what it is, its button, its header's title and tooltip |
| `order()` | 100 | its place among its side's buttons (smaller first) |
| `strip_end()` | `false` | its button in its strip's bottom group |
| `toggle_command()` | `None` | the command showing / hiding it (its shortcut in the tooltip) |
| `group()` | `""` | `code`, `services`, `documents`: a line between neighbors of different groups |
| `active_tone()` | `None` (accent) | the color it's lit in (a recording's red) |
| `active()` | `false` | a command's button lit (a mode is on) |
| `available()` | `true` | offered now (only while its partner exists): read from memory |
| `create()` | `None` | the panel, for `ToolContent::Panel` |
| `ToolWindow { id, icon, title_key, dock, order, strip_end, group, toggle, content }` | — | one described by data alone (no panel of its own) |

A `ToolPanel` implements what it uses: `update(&PanelContext)` (the editor's file,
symbols, cursor, problems), `paint(ViewerPaint)`, `press` / `press_with`, `hover`, `drag`
/ `release`, `scroll`, the text and edit keys, `tooltip_at`, `cursor_at`, `header()`
(its header's `PanelButton`s) and `header_action`, `menu_at` / `menu_command`, `poll`,
`frame_deadline`, `memento` (kept between sessions), `purge`.

## States and interaction

- Nothing exists before it's first shown; its button is drawn from the description.
- A press on its button shows or hides it (or runs its command); it's lit while its
  window shows, or while `active()`.
- A panel asks for things with `ToolPanelAction`: `Reveal`, `Open`, `View(ViewerAction)`,
  `Move`, `Rename`, `Menu` (under its header's button), `MenuUnder { anchor }`.
- `KeyReach::edit(input, key)`: the line-editing keys of a field in a panel, by word (⌥ /
  Ctrl), extending the selection (⇧).

## Accessibility

- The strips' buttons are the shared `ds::button` (a button to a screen reader). They show
  only an icon and `ButtonSpec` has no name apart from its label, so today they're read
  without their title (see the audit).
- A panel describes its content through the components it's built from.

## Cost

- Registering costs only its description; `available()` and `active()` are asked as the
  strip is drawn, so they read memory only.
- `PanelContext` is built only when something changed in the editor.

## Example

```rust
ctx.contribute::<Arc<dyn ToolWindowProvider>>(Arc::new(ToolWindow {
    id: TOOL,
    icon: "cpu",
    title_key: "tasks.title",
    dock: Dock::Right,
    order: 20,
    strip_end: true,
    group: "",
    toggle: Some(OPEN.0),
    content: ToolContent::Command { command: OPEN.0, tab: Some(EXTENSION) },
}));
// A window with a panel of its own implements the trait:
fn content(&self) -> ToolContent { ToolContent::Panel }
fn create(&self) -> Option<Box<dyn ToolPanel>> { Some(Box::new(window::DatabaseWindow::new())) }
```

## Don't

- Hard-code a button on a side in the app: register a provider.
- Make the panel at `register`: `create` is called on first show.
- Read the disk in `available()`: it's asked as the strip is drawn.
- Name a module's tool window from another module: open it through its command.
