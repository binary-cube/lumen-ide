# Fields: text styles, search pill, number, color (`ui_components::ds::{controls, number}`)

**For:** the looks of the fields a form is made of, and the small fields that aren't text:
a text field's style (a form's, a search pill's), the search pill and its clear ×, a number
with − and +, a color's field, a color's swatch, a row of swatches.

**Where it's used:** `field_style` in about twenty forms (connection dialogs, the wizard,
the HTTP client's fields); `search_input` / `search_field` / `clear_button` in older panels
(the shared `search_field::SearchField` is the field for new ones); `number_field` and
`swatch_row` in the diagram editor's properties; `ColorField` and `ColorSwatch` in the theme
studio.

## Anatomy

```
 field_style    ┌ text being typed│          ┐   input background, border (accent focused)
 search pill    ( 🔍 search…                (×) )
 number_field   ┌ − │     12     │ + ┐
 ColorField     ┌ [■] #1e1e2e              ⟋ ┐   swatch opens the picker, pipette on the right
 swatch_row     [◩] [⟋] [■] [■] [■]           theme's default, none, colors; the chosen ringed
```

## Options

**Text styles** (both return a `ui_kit::TextInputStyle` for `TextInput::paint`):

| Function | Effect |
|---|---|
| `field_style(ctx, font, invalid)` | a form's field: input background and border, focused in the accent; `invalid`: both borders in the danger color |
| `search_input_style(ctx, height)` | the search pill: rounded by half its height, room on its left for the magnifier |

**Search pill**

| Function | Effect |
|---|---|
| `search_input(pen, rect, &input, focused, placeholder)` | a `TextInput` drawn as the pill with its magnifier |
| `search_field(layer, icons, ctx, rect, focused)` | the pill alone; returns where its text goes |
| `clear_button(layer, icons, ctx, cx, mid, hovered)` | a round × (18), brighter under the mouse; returns its area |

**Number** — `number_field(layer, text, ctx, rect, shown, typing)` returns `NumberArea {
minus, value, plus }`; while `typing`, the caller draws its `TextInput` over `value`.

**Color swatch** — `ColorSwatch::new(color)`, `.paint(layer, ctx, rect)`.

| Option | Default | Effect |
|---|---|---|
| `.hovered(bool)` | off | its edge stronger |
| `.editing(bool)` | off | the picker open on it: its edge in the accent |
| `.radius(f32)` | 6 | its corners |
| `.ringed_around(chosen)` | its own edge | no edge; a ring at a gap, in its color when chosen, muted under the mouse |

**Color field** — `ColorField::new(color, value)`, `.paint(…)` returns the swatch's area
(where the picker opens).

| Option | Default | Effect |
|---|---|---|
| `.hovered(bool)` | off | a stronger fill, the icon lit |
| `.editing(bool)` | off | tinted and bordered in the accent |
| `.icon(Option<&str>)` | `"pipette"` | the icon on its right, or none |

**Swatch row** — `swatch_row(layer, ctx, (x, mid), &[Swatch], chosen)` (`Swatch::Default`,
`None`, `Color(c)`) returns each one's area; `swatch_row_width(ctx, count)`.

## States and interaction

- All stateless: the view keeps the areas, tells `hovered`, `focused`, `editing`, `typing`.
- A number's − / + are pressed through the view's hits; its value pressed starts typing.
- A color field's swatch opens `color_picker`; its value is read-only here.

## Accessibility

- A field drawn by `TextInput` with these styles tells what `TextInput` tells.
- The search pill, the clear ×, the number's − and +, the swatches and the color field say
  nothing of their own; only their drawn texts are read.

## Cost

- The styles are made from the palette each call: plain values, nothing allocated.
- `swatch_row` returns a `Vec` of areas each paint.

## Example

```rust
let mut style = ds::field_style(&ds, font.clone(), self.naming_error);
style.border = style.border_focused;
input.paint(layer, text, field, focused, &t("http.collection_name"), &style);

let parts = ds::number_field(layer, text, &ds, rect, &shown, typing == Some(n));

ds::ColorField::new(color, &hex)
    .hovered(hovered)
    .editing(editing)
    .paint(pen.layer, pen.text, pen.icons, ds, r);
```

## Don't

- Draw a pill, a magnifier, a × and a caret by hand: `search_field::SearchField`, a part of
  the view's `Controls` (it has the keys and the clipboard).
- Draw a frame around a frameless field to add a button: the shared style with `trail` (or
  `lead`) room and the shared button there.
