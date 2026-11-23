# Color picker, color field, swatch (`ui_components::color_picker`, `ds::ColorField`, `ds::ColorSwatch`)

**For:** choosing a color. The picker is a popover card that edits one color; the field
and the swatch are what opens it.

**Where it's used:** the picker — the editor's color swatches in the gutter (the `colors`
module's `view-color-picker` popover), the theme studio. `ColorField` — the theme
studio's color rows. `ColorSwatch` — a list of colors, a color to choose among a few
(data sources' folder colors).

## Anatomy

```
 ColorField:  ┌[■] #1e1e2e                 ✎┐   swatch, value as written, pipette
 ColorSwatch: [■]  edge ring · or ringed around at a gap when chosen

 ColorPicker (popover under its swatch, above when it doesn't fit):
 ┌──────────────────────────────┐
 │ saturation / brightness      │  a texture of the hue, stretched on the GPU
 │                      ○       │
 ├──────────────────────────────┤
 │ [old|new]  ═══ hue ═════○══  │
 │            ▒▒▒ alpha ▒▒○▒▒▒  │  checkerboard under what can be transparent
 │ [ HEX | RGBA | HSLA ]  [💧]  │  formats (segmented), the eyedropper when available
 │ [ #1e1e2eff              ]   │  the text, editable (red until it's a color)
 │              [Cancel][Apply] │
 └──────────────────────────────┘
```

## Options

| Option | Default | Effect |
|---|---|---|
| `ColorPicker::open(anchor, color, format, colors)` | closed | opens beside `anchor` with the code's color and format |
| `set_viewport(rect, scale)` | — | where it may place itself |
| `ColorPicker::enable_eyedropper(bool)` | off | set once by the app: every picker shows the eyedropper |
| `set_color(color, colors)` | — | a color from elsewhere (the screen's eyedropper) |
| `ColorField::new(color, "#1e1e2e")` | — | a field showing a color |
| `.hovered(bool)` / `.editing(bool)` | `false` | lit / in the accent while the picker is open on it |
| `.icon(Option<&str>)` | `Some("pipette")` | the icon on its right |
| `ColorSwatch::new(color)` | edge ring, `radius` 6 | a swatch alone |
| `.hovered` / `.editing` / `.radius(r)` | — | its ring's color, its corners |
| `.ringed_around(chosen)` | — | a ring at a gap, in its color when chosen |

`ColorField::paint` returns its swatch's area (where the picker opens).

## States and interaction

- The picker as a form's control is `ColorPart { picker, colors, action, recolored }`:
  hit anywhere while open, its square and bands held while dragged, the keys taken while
  open (`captures_keys`): the field's text, ⇧ ← →, ⌫, ⌘A; ↵ applies, Esc cancels.
- What it asked for comes back in `action`: `Apply(color, format)`, `Cancel`,
  `Eyedropper` (the app starts it and calls `set_color`).
- A press outside the card keeps what was chosen (applied) — a stray click never loses
  it; with nothing changed it just closes. Only Cancel or Esc go back to the original.
- It appears over 130 ms (opacity and a 6 px lift).

## Accessibility

Nothing yet: the picker, the field and the swatch don't describe themselves; the picker's
segmented formats and buttons do through `ds` (see the audit).

## Cost

- Gradients are small textures (the square at 64², made once per hue; the hue band once;
  the alpha band once per color), stretched on the GPU: a few quads, not thousands.
- They're let go when the picker closes.

## Example

```rust
// The view's control:
let mut picker = ColorPart {
    picker: &mut self.picker,
    colors: &Colors,
    action: &mut self.asked,
    recolored: &mut recolored,
};
// Opened with its data, drawn over the window:
self.picker.open(pick.anchor, pick.color, pick.format, &Colors);
self.picker.set_viewport(ctx.bounds, ctx.scale);
self.picker.paint(ctx.text, ctx.scene, ctx.icons, ctx.palette);

// A row of a form:
ds::ColorField::new(color, &hex).hovered(hovered).editing(editing).paint(layer, text, icons, ds, r);
```

## Don't

- Draw a swatch as a bare quad: `ColorSwatch` has the ring that reads on every color.
- Close the picker on a click outside by throwing the choice away: it applies.
- Open a picker of a module's own: it's the one popover view (`PICKER_EXTENSION`).
