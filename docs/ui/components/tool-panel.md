# Tool panel (`ui_components::tool_panel`)

**For:** a side panel's content (Structure, a module's own), made by its tool window
(`ToolWindowProvider::create`) the first time it's shown.

**Where it's used:** every panel on the window's sides that a module or the app offers.

## Anatomy

- `ToolPanel`: `update(&PanelContext)` (the editor's file, symbols, cursor, problems,
  usages), `paint(ViewerPaint)`, and the input it takes (`press`, `click`, `scroll` /
  `scroll_by`, the keys), with defaults for what it doesn't use; `memento` to be opened as
  it was.
- `ToolPanelAction`: what it asks of the app (go to a place, run a command, a menu under
  a button: `MenuUnder`).
- `PanelButton`: a button in the panel's header; `KeyReach`: how far an editing key goes in
  a field (`edit`).

## Options

Its header's buttons and its tool window's place, icon and title are its provider's
(`ToolWindowProvider`): never the app's code.

## States and interaction

Made on first show, let go when hidden long enough; asked to `update` when the editor's
context changes, never per frame.

## Accessibility

Built from components, it's told as they tell themselves; its header is the app's (a
heading with its title).

## Cost

Nothing before it's first shown; its `update` only on a change.

## Example

```rust
impl ToolPanel for Outline {
    fn update(&mut self, ctx: &PanelContext<'_>) { self.symbols = ctx.symbols.cloned(); }
    fn paint(&mut self, ctx: ViewerPaint<'_>) { /* a tree from `tree` */ }
}
```

## Don't

- Hard-code a panel in the app: a `ToolWindowProvider` registered at `register`.
- Read the disk in `update` or `paint`: a worker, its result polled.
