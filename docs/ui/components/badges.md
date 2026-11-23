# Badges, avatars, dots and key caps (`ui_components::ds::badges`)

**For:** the small marks beside a name: a pill with a short label or a count, a name's
avatar, a dot (a state, a branch, something waiting on a button), a changed file's sign, a
file's row, a key in a field's corner, a pane of glass in a corner of the screen.

**Where it's used:** about fifty views. `Badge` / `badge`: counts beside rows, tags on
cards, the relation diagram's labels; `Avatar`: the projects picker and the top bar's
project; `dot` / `ringed_dot`: the Git log's lanes, tool strip states; `dot_badge`: a
button with something to upload; `change_badge` / `file_row`: changed files; `keycap`:
`esc` in a modal's or a field's corner; `glass_badge`: the calm screens.

## Anatomy

```
 badge       ( [icon] lead  Label )        dot_badge    icon◖●  (ringed in what's behind)
 avatar      (DA)  tinted circle / ▣ solid  change_badge [+]  (a rounded square, its sign)
 keycap      ⌈ esc ⌉                        file_row     M  Name.php  src/app/…   (tag)
 glass_badge ┌──────────────────────┐
             │ [▢] Title            │  in a corner, frosted
             │     what it's about  │
             └──────────────────────┘
```

## Options

**Badge** — `Badge::new(label)`; `badge(…, BadgeSpec { label, x, mid, align_right, tone })`
is its defaults.

| Option | Default | Effect |
|---|---|---|
| `.tone(Tone)` | `Neutral` | the label and a faint fill in the tone |
| `.color(Rgba)` | the tone's | the label's color, the fill made from it |
| `.fill(Rgba)` / `.fill_alpha(f32)` | 0.12 of the color (0.05 of the text in neutral) | the fill's own color, or its alpha |
| `.size(BadgeSize)` | `Regular` (20) | `Tiny` 15, `Small` 16, `Medium` 18, `Custom(h)` |
| `.pad(f32)` | 7 | room each side of the label |
| `.min_width(f32)` | 0 | the narrowest it gets (a round count) |
| `.radius(f32)` | half the height | its corners |
| `.centered()` | from the left | the label centered (a count) |
| `.bold()` | off | the label bold |
| `.font_delta(f32)` / `.font(&Font)` | `-1.5` | the label's size, or a font of the caller's |
| `.baseline(BadgeBaseline)` | `Optical` | `Metrics`, or `At(y)`: on a row's baseline |
| `.zoom(f32)` | 1 | every size times the zoom (a bar zoomed apart) |
| `.border(Rgba)` | none | an outline a line wide |
| `.icon(icons, name)` then `.icon_size` / `.icon_color` | none | an icon before the text (size and color only after `icon`) |
| `.lead(&str)` / `.lead_font(&Font)` | none | a muted run before the label (`rows 1 204`) |
| `.width(text, ctx)` | — | its width, to place it |
| `.paint(…, left, mid)` / `.paint_right(…, right, mid)` | — | drawn from either side; returns its rect |

**Avatar** — `Avatar::new(name, color)`, `.solid()` for a rounded square with white bold
letters (default: a tinted circle); `.paint(layer, text, ctx, square)`. The letters are
`initials(name)`.

**Dots**

| Function | Effect |
|---|---|
| `dot(layer, ctx, center, diameter, color)` | a round dot on whole pixels |
| `ringed_dot(…, (diameter, ring), color, behind)` | a dot cut out of what it stands on: a ring per color in `behind`, in order |
| `dot_badge(layer, ctx, at, DotBadge { kind, lit, outline })` | a dot on an icon's corner, colored by `BadgeKind` (`Count`, `Warning`, `Error`), white on a lit button |

**Changed files** — `change_badge(layer, icons, ctx, rect, state)` (its colors and sign:
`change_badge_look`, the sign's color: `sign_on`); `file_row(…, FileRow { path, mark,
color, tag })`: the mark, the name, the folder cut in the middle, a tag badge on the right.

**Key caps** — `Keycap::new(key)`, `.hovered(bool)`, `.size(KeycapSize::Regular | Small)`
(26 / 20 high), `.paint` / `.paint_right`, `.width`, `.look(ctx)`; `keycap(…)` and
`keycap_width(…)` are its defaults. `overlay_key(…)` is a key over a picture (a dark
translucent chip in `OVERLAY_KEY_FILL`, its label in `overlay_key_font`, fixed colors).

**Glass badge** — `GlassBadge::new(icon, title)` with `image`, `detail`, `corner`,
`margin`, `tint`, `opacity` set as fields; `glass_badge_size`, `glass_badge`; `in_corner`
places any pane in a `Corner`.

## States and interaction

- Stateless and not pressable, except a key cap that's a button (`esc`): its owner keeps
  the rect and tells `hovered`.
- A badge's look changes only through its options; nothing animates.

## Accessibility

- Badges, key caps, file rows and glass badges: their texts are read as labels; nothing
  more.
- An avatar's letters are drawn inside `access::silent`: the name is said beside it.
- `dot`, `ringed_dot`, `dot_badge` and `change_badge` say nothing: what they mean (a count
  waiting, a file's state) must be in the text beside them or in the button's tooltip.

## Cost

- `Badge` borrows everything; a repaint shapes from the cache and allocates nothing.
- `Avatar::paint` makes its initials (a `String`) each paint; `file_row` cuts its name,
  folder and tag with `ellipsize*`, one `String` each.

## Example

```rust
ds::Badge::new(label)
    .font(font)
    .color(p.text)
    .fill(p.popup_background)
    .border(color.with_alpha(if strong { LABEL_BORDER_STRONG } else { LABEL_BORDER }))
    .size(ds::BadgeSize::Custom(logical(r.size.height)))
    .min_width(logical(r.size.width))
    .pad(0.0)
    .centered()
    .paint(layer, text, ds, r.min_x(), r.min_y() + r.size.height / 2.0);

let esc = ds::keycap(layer, text, &ds, "esc", panel.max_x() - px(space::LG), mid, hovered);
```

## Don't

- Draw a pill, a count or a dot by hand: `Badge` or `dot`, with the option it lacks added.
- Call `.icon_size` or `.icon_color` before `.icon`: they change nothing then.
- Let a dot or a badge's color be the only thing that says a state: say it in words too.
