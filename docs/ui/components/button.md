# Button (`ui_components::ds::button`)

**For:** every button the app draws — a dialog's OK, a bar's icon, a toggle (`.*`), a link,
a send arrow, a filter's chip. A view describes it in a `ButtonSpec` and calls `ds::button`
in the rect it laid out, keeping the rect as its hit.

**Where it's used:** about seventy views: every module's bars and headers, the modals'
footers (through the kinds `Primary` / `Danger` / `Secondary`), the tool strip (`colors`),
the Git log's filter pills (`chip`), the top bar's project (`avatar`).

## Anatomy

```
 ┌──────────────────────────────────────────────┐
 │ [icon | spinner | avatar] label [key] ▾   (×) │   centered, or from `start`
 └──────────────────────────────────────────────┘
   back: kind's fill / border / shadow          × keeps its own place at the end
```

- `ButtonKind`: `Primary` (filled, accent), `Danger` (filled, danger), `Secondary`
  (bordered), `Toolbar` (a header's: bordered, icon then label), `Ghost` (transparent, lit
  under the mouse), `Soft` (faint fill and border), `Raised` (an opaque card with a light
  shadow, over content).
- `ButtonShape`: `Box` (the kind's corners) or `Round` (a circle, or a pill when wider).
- `button_width(text, ctx, &spec, height)`: the width for a labelled one, counting its
  icon, key cap, chevron and ×; an icon alone is square (`square_button`).
- `button_size`: the named heights — `SMALL` 22, `COMPACT` 24, `MEDIUM` 26, `LARGE` 28,
  `ROOMY` 30, `SPACIOUS` 32. The icon is about half the height (15 in 28, never under 12).
- `button_label_font(ctx)`: the light kinds' label font, to measure or cut a label before
  giving it.

## Options

| Option | Default | Effect |
|---|---|---|
| `ButtonSpec::new(kind)` | pressable, not lit, not on, not busy | the kind's look |
| `.icon(name)` / `.icon_id(id)` | none | an icon before the label (`icon_id`: already looked up) |
| `.label(&str)` | none | its text, and its name to a screen reader |
| `.round()` | `Box` | a circle or a pill |
| `.tone(Tone)` / `.tone_color(Rgba)` | the kind's | content in a tone (a Run green, a Stop red); the color wins |
| `.rest_tone(Tone)` | none | that tone only at rest: a quiet icon, muted until hovered |
| `.on(bool)` | off | a toggle's state: in the accent, stronger under the mouse |
| `.busy(bool)` / `.busy_amount(0..1)` | off | a spinner in the icon's place; the amount grows it and fades the hover out |
| `.menu(bool)` | off | a ▾ after the content, counted in the width |
| `.enabled(bool)` | on | greyed out, never lit |
| `.hovered(bool)` / `.hover_amount(0..1)` | off | lit; the amount eases it (content turns at half) |
| `.key(&str)` | none | the key that presses it in a small cap after the label |
| `.hover_tone(Tone)` | none | the back filled solid in the tone under the mouse (a close ×), content in what reads on it |
| `.hover_tint(Tone)` | none | the back faintly in the tone under the mouse, content unchanged |
| `.hover_content_tone(Tone)` | none | only the content turns to the tone under the mouse (a delete × turning red) |
| `.icon_color(Rgba)` | the content's | the icon alone in its own color |
| `.icon_size(f32)` | half the height | the icon's side (logical) |
| `.corner(f32)` | the kind's | its corners (logical) |
| `.tinted()` | off | a faint back in its tone (accent without one), stronger under the mouse |
| `.colors(ButtonColors)` | none | a theme's own icon / hover / on colors (the tool strip); `on_back: None` keeps the shared accent tint while on, eased under the mouse (the status bar's switches) |
| `.on_surface(Rgba)` | none | it stands on a fixed surface: content and light in what reads on it |
| `.strong()` | off | the label in the dialogs' font (`button_font`) |
| `.clearable(hovered)` | off | a × of its own at the end (its hit: `clear_rect`) |
| `.outlined()` | off | a faint border in its content's color |
| `.font(&Font)` | the kind's | the label in a font of the caller's |
| `.start(pad)` | centered | content from the left edge after `pad` (a slot kept for its widest value) |
| `.avatar(Avatar)` | none | a name's avatar in the icon's place |

`ds::chip(…, spec)` draws a filter's pill (`chip_spec(label, on, hovered, clear_hovered)`:
a round soft button with ▾ and ×) and returns `ChipRects { body, clear }`.

Shared pieces beside it: `button_font` / `BUTTON_WEIGHT` (every label's weight),
`primary_width(text, ctx, labels)` (a dialog button as wide as its widest label, at least
96), `BUTTON_H` (34) and `TOOLBAR_BUTTON_H` (30), `BUSY_TRANSITION` (180 ms) and
`BUSY_EXTRA` (the room a busy spinner opens), `inline_key_look` (a key inside a button),
`Press { enabled, hovered }`. The older functions the kinds are drawn with —
`primary_button`, `danger_button`, `secondary_button`, `busy_primary_button`,
`toolbar_button` / `busy_toolbar_button` (`ToolbarButton`, `toolbar_button_width`,
`toolbar_button_font`) and `primary_button_with_key` (`KeyButtonSpec`) — stay public for
their callers.

## States and interaction

- Stateless: the view keeps its hit table and tells the spec `hovered`, `on`, `busy`,
  `enabled` each paint; a press is the view's (the rect it kept).
- Hover: ghost and raised kinds are lit only under the mouse; a soft one always has its
  back; a filled one takes a little white.
- Busy: a dialog kind draws its spinner before its label and leaves out its key and ▾ (nothing
  over the spinner); a toolbar one swaps its icon for the spinner.
- Disabled: muted content, no light, its icon's own color dropped.
- The keyboard's focus ring is not part of the spec: whoever has the focus draws
  `ds::focus_ring` around the rect.

## Accessibility

A `Button` named by its label (an icon alone: by the text on its line, never the icon's
id), `disabled` while it can't be used, `selected` while `on`, its key said as its
`shortcut`, a menu button `expanded: false`. Drawn inside `access::group`, so its label is
not read a second time.

## Cost

- Nothing allocated: the spec borrows (`&'a str`, `&'a Font`), the label is shaped through
  the text system's cache.
- A key cap is measured twice per paint (`key_room`, then the cap): two cache hits.

## Example

```rust
let spec = ButtonSpec::new(ButtonKind::Ghost)
    .label(&all)
    .tone(Tone::Accent)
    .hovered(self.hover == Some(Hit::ShowAll));
let h = px(button_size::LARGE);
let w = ds::button_width(text, ds, &spec, h);
let r = Rect::new(x, (mid - h / 2.0).round(), w, h);
ds::button(layer, text, icons, ds, r, spec);
self.hits.push((r, Hit::ShowAll));
```

## Don't

- Draw a rounded quad, an icon and a hit by hand, or keep a module's own `icon_button`:
  what the button lacks is an option of `ButtonSpec`, for everyone.
- Call the older `primary_button`, `secondary_button`, `danger_button`, `toolbar_button`
  or `primary_button_with_key` in new code: they draw the same back but tell a screen reader
  nothing. `ButtonSpec::new(ButtonKind::Primary).label(…).key("↵")` is the same button,
  described.
- Give a raw height: `button_size::*` (and `BUTTON_H`, `TOOLBAR_BUTTON_H` for the dialog
  and header kinds).
