# Tag colors (`ui_components::tag_colors`)

**For:** the named colors a user marks things with (a data source's folder: production in
red), a name's color of its own (an avatar), a project's color.

**Where it's used:** the database explorer's folders and sources, the picker's avatars, the
top bar's project badge, the projects list.

## Anatomy

- `NAMES`: the names offered, in order (red, orange, yellow, green, teal, blue, purple,
  gray).
- `named(name, palette)`: a name's shade for the theme (one for dark themes, one for light).
- `for_key(key, palette)`: the same color for the same key (a user's name), among the named
  ones but gray.
- `project(name)`: a project's color, the same in every theme.
- `is_dark(palette)`: the theme's background is dark.

## Options

None.

## States and interaction

None: a color chosen once, read where it's shown.

## Accessibility

A tag's color is never its only sign: its name is shown with it (a menu's row, a tooltip).

## Cost

A stable hash of a key (`kernel::hash::fnv32`) and a table: nothing allocated.

## Example

```rust
let tint = tag_colors::for_key(&user, ds.palette);
ds::Avatar::new(&user, tint).paint(layer, text, ds, rect);
```

## Don't

- Hash a name with the standard hasher (another color each run): `for_key`.
- Hand a user's tag a color of the code's: one of `NAMES`.
