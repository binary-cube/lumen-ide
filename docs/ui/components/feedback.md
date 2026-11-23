# Feedback: loader, empty state, tooltip, footers (`ui_components::ds::feedback`)

**For:** what tells the user how things stand: a spinner while work runs, an empty
state where there's nothing to show, a tooltip pill, and the footers at the bottom of a
dialog or a card — its keys on the left, a modal's buttons on the right.

**Where it's used:** `loader` in about two dozen views (a grid loading, a busy button's
spinner through `ButtonSpec::busy`); `empty_state` in about fifty; `tooltip` by the app's
one tooltip manager (`tooltip::Tooltips`); `footer_in_card` in the views drawn inside the
editor's card; `modal_footer_bar` + `footer_buttons` in the modals that draw their own
footer (`Viewer::draws_footer`: the profiler's consent, the remote wizard).

## Anatomy

```
 loader        ⁘ Loading tables…                 (8 dots, the brightest turning)
 empty_state         No results
                  try another word                 (centered in its rect)
 tooltip              ▲
               ┌ Label line one         ⌈⌘K⌉ ┐     below the anchor, above if it
               │ value line two              │     doesn't fit; stays in bounds
               └─────────────────────────────┘
 modal footer  ├───────────────────────────────────────────────┤
               │ ↑ ↓ move · ↵ open · esc close   [Cancel] | [Start ↵] │
```

## Options

**Loader** — `Loader::default()`, `Loader::with_text(&str)`, then fields or builders.

| Option | Default | Effect |
|---|---|---|
| `text` | none | a muted label after the spinner |
| `spinner` | on | the dots (off: the text alone) |
| `size` | 16 | the spinner's diameter (logical) |
| `color` / `text_color` | accent / muted | their colors |
| `.elapsed(Duration)` | the wall clock | turned by the caller's clock (a notification's age) |
| `.width(text, ctx)` | — | its width, to center it |
| `loader(layer, text, ctx, x, mid, &l)` / `loader_spinner(layer, ctx, (x, mid), &l)` | — | drawn with its text, or the spinner alone (no text system) |

**Empty state** — `empty_state(layer, text, ctx, rect, title, detail)`: a muted title and
an optional fainter line, centered.

**Tooltip** — `tooltip(layer, text, ctx, anchor, label, shortcut, bounds)`: lines split by
`\n`, each wrapped past `TOOLTIP_MAX_W` (440), at most 12; the shortcut in a key cap on
the first line; an arrow pointing at the anchor. `tooltip_lines` gives the lines alone.

**Footers**

| Function | Effect |
|---|---|
| `footer(layer, text, ctx, rect, hints)` | a dialog's footer: faint fill, top line, keys on the left; returns the free right edge for actions |
| `footer_in_card(scene, text, ctx, rect, hints, radius)` | the same at a card's bottom, its corners following the card's; keys only, the last left out when narrow |
| `modal_footer_bar(scene, ctx, foot)` | a modal's bar (`MODAL_FOOTER_H` = 60): its top line and tint; returns the layer for its keys and buttons |
| `footer_buttons(layer, text, ctx, right, mid, &[FooterButton])` | the buttons right to left; returns each area (in the order given) and where the room on their left ends |

`FooterButton` (fields, `..Default::default()`):

| Field | Effect |
|---|---|
| `label`, `enabled`, `hovered` | what it says and how it stands |
| `main` | in the accent with its `↵` |
| `danger` | what can't be undone: in the danger color |
| `keyless` | a main one pressed only by a click: no `↵` |
| `separated` | a `|` between it and the buttons on its right |
| `focused` | the keys are on it: ringed (`focus_ring`) |

## States and interaction

- The loader turns by the clock: its view asks for frames while it shows
  (`Viewer::is_animating` / `is_loading`) and stops when the work is done.
- Footer buttons are stateless: the modal keeps the returned areas, tells `hovered` and
  `focused`, and runs the action on a press or on `↵` / Esc.
- A tooltip is placed, not interacted with; the manager shows it after the shared delay.

## Accessibility

- Every text drawn (the loader's, the empty state's, the tooltip's lines, the footer's keys)
  is read as a label.
- The loader's spinner and busy state say nothing.
- `footer_buttons` draws through the older dialog-button functions: its buttons are read as
  their labels, not as buttons.

## Cost

- `loader_spinner`: eight quads, nothing allocated.
- `tooltip` collects its lines in a `Vec` (and wrapped lines as `String`s) each paint; it
  is one pill on screen at a time.
- `footer_buttons` returns a `Vec` of areas each paint.

## Example

```rust
let loader = ds::Loader::with_text(&label);
let w = loader.width(text, &ds);
ds::loader(layer, text, &ds, area.min_x() + (area.size.width - w) / 2.0, area.center().y, &loader);

ds::empty_state(layer, ctx.text, &ds, area, &title, detail.as_deref());

let buttons = [
    ds::FooterButton {
        label: &start,
        main: true,
        enabled: true,
        hovered: self.hover == Some(Hit::Start),
        ..ds::FooterButton::default()
    },
    ds::FooterButton {
        label: &cancel,
        enabled: true,
        hovered: self.hover == Some(Hit::Cancel),
        ..ds::FooterButton::default()
    },
];
let (areas, left) = ds::footer_buttons(layer, text, &ds, foot.max_x() - px(PAD), mid, &buttons);
self.hits.extend(areas.into_iter().zip([Hit::Start, Hit::Cancel]));
```

## Don't

- Float a modal's buttons in its content: they're in its footer, with the keys it answers
  to on the left.
- Draw a tooltip from a view: return a `Tip` from `tooltip(p)` and let the manager show it.
- Keep asking for frames once the loader is gone.
