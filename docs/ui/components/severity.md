# Severity (`ui_components::severity`)

**For:** how bad something is, shown the same everywhere: a profile's findings, problems,
diagnostics, a status's sign.

**Where it's used:** the database console's profile (findings, the plan's signs), the
tree table's signs and inks, the debug overlay.

## Anatomy

- `Level`: `Info`, `Warning`, `Critical` — each an icon (`Level::icon`: a circle with an
  `i`, a triangle, an octagon with a `!`) and a color (`Level::color(palette)`).
- `Palette::severity(level)`: the theme's shade (Okabe–Ito, lighter on a dark background,
  deeper on a light one); `Palette::heat(t)`: one warm hue from faint to intense for a share
  `t` (0–1).
- `contrast(a, b)`, `readable(color, toward, background, ratio)`: a color moved until it
  reads on a background.

## Options

None: a level and the palette say everything.

## States and interaction

None of its own: whoever shows a level draws its icon and its color together.

## Accessibility

Never a color alone: the icon says the level for an eye that can't tell the colors apart;
a screen reader is told the level's word by whoever shows it.

## Cost

Constants and arithmetic: nothing kept, nothing allocated.

## Example

```rust
let color = Level::Warning.color(ds.palette);
ui_kit::draw_icon(layer, icons, icons.id(Level::Warning.icon())?, at, color);
```

## Don't

- Show a level by its color only.
- Pick a red or an orange of your own for a problem: `Palette::severity`.
