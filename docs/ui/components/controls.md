# Controls (`ui_components::ds::controls`)

**For:** the small controls a form or a panel is made of: a switch, a checkbox, a bare
radio, choice pills, a segmented control, a list's check row, a wizard's stepper, a radio
card. Each draws in the place it's given and returns its area (or hands each part's area
to a closure); the view keeps the hits.

**Where it's used:** settings and properties panels (switches, checkboxes), the schema
picker (`Checkbox`), the profiler's and the console's bars (`segmented_styled`), the Task
Manager's and the calendar's menus (`check_row`), the remote connection wizard (`stepper`,
`radio_card`), filters by severity (`choice_with_dot`).

## Anatomy

```
 switch      (●──)  / (──●)        checkbox  [ ] [✓] [–]       radio  ○ ◉
 choice      ( ● Label )            segmented ┌ All │▐Dark▌│ Light ┐
 check row   │ ✓  Label ······· note │
 stepper     (✓) Account ─── (2) Server ─── (3) Done
 radio card  ┌ ◉ Title ────────────────┐
             │   a line, wrapped on two │
             └─────────────────────────┘
```

## Options

**Switch** — free functions, no spec.

| Option | Default | Effect |
|---|---|---|
| `switch(layer, ctx, x, mid, on, hovered)` | 36 × 20 | the usual toggle; returns its area |
| `switch_enabled(…, (x, mid), on, hovered, enabled)` | enabled | off limits: dimmed to `DISABLED_ALPHA`, deaf to the mouse |
| `small_switch(…, x, mid, on)` | `SMALL_SWITCH_W` × 15 | a menu row's, only telling how a setting stands |

**Checkbox** — `Checkbox::new(Check)`; `checkbox(…)` is its defaults.

| Option | Default | Effect |
|---|---|---|
| `Check::Off / On / Mixed` (`Check::of(chosen, total)`) | — | its state; `Mixed` for a group partly chosen |
| `.enabled(bool)` | on | a chosen box muted instead of in the accent |
| `.size(f32)` | `CHECKBOX` (16) | its side; corners and ✓ scale with it |
| `.hovered(bool)` | off | an empty box's outline brighter |

**Radio** — `Radio::new(selected)`, drawn by `.paint(layer, ctx, top_left)` or
`radio(layer, ctx, x, mid, spec)`.

| Option | Default | Effect |
|---|---|---|
| `.hovered(bool)` | off | the ring brighter |
| `.size(f32)` | `RADIO` (18) | its side; the dot scales with it |

**Choice pill** — `ChoiceSpec { x, mid, label, selected, hovered }`, sized by
`choice_width`; `choice_with_dot(…, spec, color)` (`choice_width_with_dot`) adds a status
dot before the label.

**Choice cell** — `choice_cell(layer, text, ctx, rect, ChoiceCell { label, selected,
hovered, active })`: a choice filling a cell of a grid of equal ones (the language
levels' versions): the one in force filled in the accent with a white bold label, one
under the mouse or the keys (`active`) tinted, its label centered and cut with `…`.

**Segmented** — `SegmentedSpec { right, mid, labels, selected, hovered }` and
`SegmentedLook`.

| Option | Default | Effect |
|---|---|---|
| `segmented(…)` | — | returns each segment's area (a `Vec`) |
| `segmented_with(…, each)` | — | the same, each area handed to `each(i, rect)`: nothing allocated |
| `segmented_styled(…, (spec, look), each)` | `SegmentedLook::default()` | the options below |
| `look.height` | `SEGMENTED_H` (32) | its height |
| `look.place` | `SegmentPlace::Right` | `Left(x)`, or `Fill { x, width }`: equal segments, labels cut with `…` |
| `look.disabled` | none | segments greyed, never lit, not handed to `each` |
| `look.icons` | none | an icon before each label (`""`: none) |
| `segmented_width(text, ctx, labels)` | — | its width, to place it from its left |

**Check row** — `CheckRowSpec::new(label, checked, hovered)`, drawn by `check_row`.

| Option | Default | Effect |
|---|---|---|
| `.active(bool)` | off | the row under the keys, lit as under the mouse |
| `.radius(f32)` | `radius::MD` | the highlight's corners |
| `.hover_fill(Rgba)` | the text, faint | the hover's fill (a token: `hover_background`) |
| `.icon(f32)` | 14 | the ✓'s side |
| `.untinted()` | tinted | the chosen one not filled (the hover shows over it) |
| `.plain_label()` | in the accent | the chosen one's label in the text color: only its ✓ says it |
| `.trailing(f32)` | none | room kept at the right (a note), the label ellipsized before it |

**Stepper** — `StepperSpec { x, width, mid, labels, current }`: done steps filled with a
✓, the current ringed with a halo and its name semibold, the next ones muted.

**Radio card** — `RadioCardSpec { rect, title, detail, selected, hovered }`: a radio, a
title cut with `…`, a line under it wrapped on two lines at most.

## States and interaction

- All are stateless: the view tells each paint whether it's hovered, chosen, enabled, and
  keeps the returned areas as its hits; a press toggles the view's own value.
- A disabled segment is never lit and never handed out, so it can't be pressed.
- None takes the keyboard by itself; the view's `Controls` move the focus and the view
  draws `ds::focus_ring` around the focused one.

## Accessibility

- Switch: a `Switch` node, `checked`, `disabled` when off limits; named by the text on
  its line.
- Checkbox: a `CheckBox` node, `checked`, `mixed`, `disabled`.
- Radio: a `Radio` node, `checked`.
- Segmented: each segment a `Radio` named by its label, `checked` when in force,
  `disabled` when it can't be chosen.
- Check row: a `ListItem` named by its label, `selected` and `checked` when chosen.
- Choice pills and the stepper tell nothing beyond their texts (read as labels).

## Cost

- No allocation in `switch`, `Checkbox`, `Radio`, `choice`, `check_row`,
  `segmented_with` / `segmented_styled` (a `Fill` label that must be cut allocates its
  cut text).
- `segmented` returns a `Vec`; `stepper` collects its shaped labels and formats each
  step's number every paint; `radio_card` allocates its cut title and wrapped lines.
  Fine for a wizard's page, not for a list drawn many times.

## Example

```rust
let state = if place.is_some() { Check::On } else { Check::Off };
ds::Checkbox::new(state).hovered(hovered).paint(pen.layer, pen.text, ds, cx, mid);

let hits = &mut self.hits;
ds::segmented_styled(layer, ctx.text, ctx.icons, &ds, (spec, look), |i, seg| {
    hits.push((seg, Hit::Mode(i)));
});

let spec = ds::CheckRowSpec::new(label, checked, self.hover == Some(hit))
    .active(self.menu_at == Some(i))
    .untinted()
    .plain_label()
    .radius(radius::SM)
    .trailing(LABEL_END);
ds::check_row(layer, text, icons, ds, row, spec);
```

## Don't

- Draw a toggle, a box, a ring or a ✓ by hand: the control, with an option added if it
  lacks one.
- Call `segmented` every frame in a busy view: `segmented_with` hands the areas out
  without a `Vec`.
- Use a context menu for one of a few choices: segments, pills or a dropdown.
