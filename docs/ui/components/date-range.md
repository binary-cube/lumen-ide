# Date range picker (`ui_components::date_range`)

**For:** choosing a span of calendar days in a popover: a preset, or From → To typed or
picked on the months.

**Where it's used:** the Git log's Date filter, the remote's changed files.

## Anatomy

```
 ┌ presets ──────┬ [ From 01.03.2026 ] → [ To 14.03.2026 ]          ┐
 │ ✓ Today       │  ‹ March 2026            April 2026 ›             │
 │   Yesterday   │  Mo Tu We Th Fr Sa Su    Mo Tu We Th Fr Sa Su     │
 │   Last 7 days │         ●━━━━━━━━━━━━●   (range: ends solid,      │
 │   …           │   (today ringed, the end under the mouse previewed,│
 │   Any time    │    disallowed days dimmed)                         │
 ├───────────────┴────────────────────────────────────────────────────┤
 │ 1 – 14 March                                    [Cancel] [Apply ↵] │
 └────────────────────────────────────────────────────────────────────┘
```

Two months side by side, one when narrow. Below its anchor, above when it doesn't fit.

- `Date` (pure): a calendar day — `days`, `from_days`, `add_days`, `add_months`,
  `weekday`; `month_grid` gives a month's 42 days from the week's first.
- `DateLocale`: the month and weekday names, the week's first day and the date format
  from the locales (`time.*`); `format`, `parse`, `range` (a span said briefly).

## Options

| Option | Default | Effect |
|---|---|---|
| `DateRangePicker::new(today, presets, range)` | — | the caller's local day, its presets, what's chosen now |
| `standard_presets(today, any_time)` | — | Today, Yesterday, Last 7 / 30 days, This / Last month, This year (and Any time) |
| `max` | `None` | the last day that may be picked (commits: today) |
| `paint(scene, text, icons, ds, anchor, view)` | — | under `anchor`, within `view` |

## States and interaction

- As a form's control it is `RangePart { picker, answer }`: hit anywhere; a press outside
  answers `RangeAnswer::Outside` (its owner closes it, as a popover does); a preset, an
  applied range or Cancel answer `Chose(PickerEvent)`.
- Picking: the first day, then the last (in either order); the fields follow, and typing
  in them moves the range (red until it's a day).
- Keys (`keeps_tab`): ← → ↑ ↓ move the day, Page Up / Down the month, Home / End the
  month's ends, Space / ↵ pick, ↵ applies a whole range, Tab between the fields and the
  days, Esc cancels. In a field: the line-editing keys (`KeyReach`), ⌘A, copy, paste.
- The cursor is a hand over what takes a press, the arrow elsewhere — never what's under it.

## Accessibility

Nothing yet: the days, presets and fields don't describe themselves; the footer's
buttons do through `ds` (see the audit).

## Cost

- No work while closed: the picker exists only while open (its owner holds an `Option`).
- The locale's names are read once when it's made.
- Each paint shapes and formats its day numbers, its month titles and the footer's words
  again (see the audit).

## Example

```rust
let presets = ui_components::date_range::standard_presets(today, false);
let mut picker = DateRangePicker::new(today, presets, range);
picker.max = Some(today);
self.picker = Some(picker);
// in with_controls: RangePart { picker, answer: &mut answer }
// in paint:
picker.paint(ctx.scene, ctx.text, ctx.icons, &ds, self.pick_rect, bounds);
```

## Don't

- Turn the range into moments inside it: it works in calendar days; the caller converts
  with its own zone.
- Write dates with a format of the view's own: `DateLocale` follows the user's language.
- Keep it made while closed.
