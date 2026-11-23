# Backdrops and floating glass (`ui_components::backdrop`, `ds::backdrop`)

**For:** what lies under every window over the editor (modals, dialogs, About, the
project search, the menu drawer): one style for all, resolved once from the theme and
the user's own choice over it, kept in the `Palette`. Also the glass of a floating panel.

**Where it's used:** every modal and overlay (`ds::backdrop`, `ds::backdrop_faded`), the
menu drawer (`BackdropRole::Drawer`, `SurfaceStyle`).

## Anatomy

```
 ┌ the window behind ─────────────────────────┐
 │ ░░░░░░░ blurred (Glass) ░░░░░░░░░░░░░░░░░░ │
 │ ▓▓▓▓ the color laid over it, at its opacity │
 │        ┌ the modal ┐                         │
 │        └───────────┘          [hole: the     │
 └───────────────────────────────window's own buttons on Windows]
```

- `BackdropStyle`: `Simple` dims with a color; `Glass` blurs a little (the renderer's
  backdrop blur) and lays the color over it.
- `SurfaceStyle`: a floating panel opaque (`Simple`) or its color at `opacity` over a
  blurred copy of what's under it (`Glass`).
- `ds::backdrop(layer, ctx, viewport)` / `ds::backdrop_faded(layer, ctx, area, role,
  shown, hole)` draw it, first in the overlay's layer.

## Options

| Option | Default | Effect |
|---|---|---|
| `BackdropStyle::resolve(spec, window)` | Simple, black, `DEFAULT_BLUR` | the theme's `BackdropSpec`; `window`'s brightness sets the drawer's strength |
| `.with_user(spec)` | the theme's | the user's choice over it (what it doesn't say stays the theme's) |
| `.without_blur()` | — | never blurred (a renderer that can't) |
| `opacity(role)` | Dialog 16 %; Drawer 25–55 % by brightness | unless the theme or user says |
| `tint(role)` / `blur_radius(scale)` | — | the color with its opacity / the blur in physical pixels (`None`: not blurred) |
| `SurfaceStyle::resolve(spec)` | Simple, `PANEL_OPACITY` | a floating panel's glass (`MIN_PANEL_OPACITY`–1) |
| `fill(base)` | `base` | the panel's color, translucent when Glass |
| `ds::backdrop_faded(…, shown, hole)` | — | `shown` 0..1 (its animation), `hole` left clear |

## States and interaction

- None of its own: it only covers. The modal over it is what stops input reaching what's
  behind (`Workbench::modal_open`); a press on the backdrop never closes a modal.

## Accessibility

None: it's decoration. The modal over it is a modal dialog to a screen reader.

## Cost

- Resolved once per theme or user change, read from the palette each frame.
- Drawn as at most four quads around the hole (`ds::around`, a small `Vec` each call);
  the blur is the renderer's, only when Glass.

## Example

```rust
// Under the menu drawer, faded in with it:
ds::backdrop_faded(back, &ctx, a, BackdropRole::Drawer, shown, None);
// The app: the theme's style, the user's over it, never blurred where the renderer can't:
let style = theme.with_user(own());
if blur { style } else { style.without_blur() }
```

## Don't

- Dim the window behind with a quad of your own: `ds::backdrop`.
- Close a modal on a press on its backdrop.
- Blur where the renderer can't: `without_blur` keeps the color.
