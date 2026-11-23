# Controls (`ui_components::control`)

**For:** how a view's parts are used, as a form's controls are: each part answers one
contract (`Control`), the view keeps their state of use in one `Controls` — who holds the
pointer, who has the focus, what the mouse is over. Nothing that routes names a part.

**Where it's used:** every view built from parts (the database explorer, the dialogs, the
menu drawer, the remote properties, git's views, the settings), and every stateful
component (`Picker`, `SearchField`, `TextInput`, `MultilineField`, `DataGrid`, `ReadText`,
`Chart`, `Icicle`, `TreePart`, `ColorPart`, `RangePart`).

## Anatomy

```
 view.with_controls(|controls, parts| …)
     parts: [&mut splitter, &mut list, &mut structure, &mut grid]   ← topmost first
              Divider       Scrolled    Shown<Thumb>     Shown<DataGrid>
     controls.press(parts, p, clicks) → Some((index, Press))       ← the view knows index → meaning
```

- `Control`: every method has a default; a part implements what it uses.
- `Controls`: by places in the list — held, focused, hovered; the cursor and tooltip at
  the last hover, animating / next frame at the last `settle`.
- `Parts<'a, 'b> = &'a mut [&'b mut dyn Control]`: built each time from a fixed array,
  never a `Vec` per event.

## Options

| `Control` method | Default | Says |
|---|---|---|
| `area` / `hit(p)` | none / in its area | where it is, `p` on it |
| `press(p, clicks)` / `press_with(…, mods)` | `Ignored` | `Took`, or `Held` (moves go to `drag` until `release`) |
| `drag` / `release` / `hover(Option<p>)` | `false` | what it shows changed |
| `cursor(p)` / `tooltip(p)` / `menu(p)` | none | its cursor, `Tip`, right-click rows |
| `scroll(dx, dy)` / `takes_wheel(p)` | `false` | it scrolled / keeps the wheel at its end |
| `focusable` / `set_focus` / `in_tab_order` / `keeps_tab` | `false` / — / focusable / `false` | the focus and Tab |
| `captures_keys` | `false` | while shown, the keys, text and commands are its own (an open list) |
| `key` / `char_key` / `text` / `paste` | `false` | with the focus |
| `selected_text` / `command(id)` | none / `false` | the copy; `select_all`, `cut`, `undo`, `redo` |
| `animating` / `frame_deadline` | `false` / none | it moves / its next frame |

| `Controls` | Effect |
|---|---|
| `Controls::default()` | a press beside the parts leaves the focus nowhere |
| `Controls::sticky()` | it leaves the focus where it was (a page with one field always typing) |
| `hold(i)` | part `i` holds the pointer from code (a drag begun from a menu) |
| `focus(parts, Some(i))` | gives the focus, telling both |

Ready-made parts: `Thumb { bar, offset }`, `ScrollThumb { bar, scroll }`,
`BarThumb { bar, metrics, offset }`, `Scrolled { bar, offset, area, max }` (the wheel over
the area too), `Divider { splitter, at }`, `Embedded { view, area, action, menu }` and
`TabView` (a view inside a view), `Shown { part, shown }`, `Option<C>` / `Absent`,
`OneClick` (a part pressed as one click whatever the series), and `impl Control for
TextInput`.

## States and interaction

- `press` / `press_with`: the topmost part that takes it; the focus moves to it when it
  takes the focus. `drag` / `release` go to the held part wherever the mouse is.
- `hover(parts, p)`: the part left and the one entered are told; cursor and tooltip kept
  (`cursor_at`, `tooltip_at` answer from `&self`).
- `key`: Tab / ⇧Tab move the focus among parts in the Tab order, unless the focused one
  `keeps_tab`; others go to the part capturing keys, else the focused one. `text`, `paste`,
  `command` likewise (one capturing that doesn't take them passes them on).
- Wheel: `scroll(parts, p, …)` the topmost there that scrolls; `wheel` (no point) the
  capturing part, else the hovered; `scroll_focused`.
- `settle(parts)` while painting; read back by `is_animating`, `next_frame`.
- `selected_text_with(part_of)`: the copy asked from `&self`, the view lending its parts.

## Accessibility

None of its own: each part describes itself where it paints.

## Cost

No allocation per event: a fixed array of borrows, places as `usize`; read-only questions
answered from what the last hover and settle kept.

## Example

```rust
fn with_controls<R>(&mut self, f: impl FnOnce(&mut Controls, &mut [&mut dyn Control]) -> R) -> R {
    let mut splitter = Divider { splitter: &mut self.splitter, at: &mut self.split_at };
    let mut list = Scrolled { bar: &mut self.list_bar, offset: &mut self.list_scroll,
                              area: self.list_area, max: self.list_max };
    let mut grid = Shown { part: grid, shown: data };
    let mut parts: [&mut dyn Control; 3] = [&mut splitter, &mut list, &mut grid];
    f(&mut self.controls, &mut parts)
}
```

## Don't

- Route input with a chain of `if self.picker… if self.field…`: hand the parts to
  `Controls`.
- Re-implement what a part does in its owner (its keys, its drag): the part does it.
- Keep a `typing` bool beside a field: the focus is `Controls`'.
