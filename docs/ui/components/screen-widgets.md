# Screen widgets (`ui_components::screen_widgets`)

**For:** small panes of glass on a calm screen (Relax mode's scene): the app's mark, the
resources it uses — each offered by whoever owns it, the screen knowing none of them.

**Where it's used:** the relax module's scene draws them; the Task Manager offers its
resources widget (`ResourcesWidget`, its metric cards on glass).

## Anatomy

```
  sky ……………………………………………………………………………………………………………
  ┌ widget ┐ ┌ widget ┐                         ┌ widget ┐
  │ glass  │ │        │                         │        │   WIDGET_HEIGHT (96) each,
  └────────┘ └────────┘                         └────────┘   tops lined up
  BottomLeft: from the corner, side by side      BottomRight
```

- `ScreenWidget` (contributed): `id`, `place`, `order`, `open(ctx)` — made when a screen
  opens, `None` when what it needs is missing.
- `WidgetView`: `size(text, ds, room)` (`None`: nothing to show yet) and `paint`.
- `ScreenWidgets`: the widgets of an open screen, laid out by place.
- `glass_pane`: the pane: a faint frost of the tint and its rim, or the app's night card
  (deep, tinted by the accent, deeper toward its foot, a sheen from its top, a fine rim).
- `screen_palette(palette, tint)`: the theme's palette with its words, muted words and
  edges in the glass's tint; the rest (the accent, a chart's series) the theme's.

## Options

| Option | Default | Effect |
|---|---|---|
| `register(Arc<dyn ScreenWidget>)` | — | the app, at startup (one with the same id is replaced) |
| `order()` | 0 | lower: nearer the corner |
| `WidgetContext { resources, wake }` | `None`s | what a widget is given: the parts' costs, a waker |
| `GlassLook { tint, opacity, night }` | — | the pane's tint, how much shows (fading in), the night card or the frost |
| `ScreenWidgets::paint(pen, bounds, margin)` | — | along `bounds`' bottom, `margin` (logical) from it |

## States and interaction

- None: widgets are looked at, not used. A widget that doesn't fit what's left of the
  width is skipped.
- Made when the screen opens, gone when it closes: nothing of them runs otherwise.

## Accessibility

Nothing yet: the panes don't describe themselves (a calm screen is decorative; its
widgets' figures could be a group read by name).

## Cost

- The night card is 26 quads, the frost two; nothing made again per frame.
- `paint` returns the rectangles drawn (a `Vec` each frame), and the scene builds the
  screen's palette each frame (`screen_palette` clones the theme's, icon colors included)
  — see the audit.

## Example

```rust
let widgets = self.widgets.get_or_insert_with(|| ScreenWidgets::open(context));
if !widgets.is_empty() {
    let tint = Rgba::WHITE;
    let palette = screen_palette(ctx.palette, tint);
    let ds = Ctx { opacity: 1.0 - veil, ..Ctx::new(&palette, ctx.scale) };
    let look = GlassLook { tint, opacity: 1.0 - veil, night: true };
    widgets.paint(ScreenPen { scene: ctx.scene, text: ctx.text, icons: ctx.icons, ds: &ds, look }, bounds, 28.0);
}
```

## Don't

- Start sampling or reading at `register`: only in `open`, for a screen that shows.
- Name another module's widget: they're found through the registry.
- Draw a pane of your own: `glass_pane`, so every widget matches.
