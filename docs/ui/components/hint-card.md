# Hint card (`ui_components::hint_card`) and anchors (`ui_components::anchors`)

**For:** a hint (a coach mark): a short card pointing at a part of the window, telling the
user of something they may not know, with a quick action — or a tour of several steps
("click here, then there"), Back / Next between them.

**Where it's used:** the app's hint overlay (`workbench::hints`, `Overlay::Hint`). Modules
never draw it: they contribute a `workbench_model::HintContribution` and ask for it with
`host.show_hint(id)` (the terminal's "idle sessions can sleep").

## Anatomy

```
              ▲                                  ← the notch, toward the anchor
 ┌────────────────────────────────────────┐
 │ (☾)  TIP                       2 / 4 ×  │  ← icon in a soft accent circle, eyebrow
 │      Idle sessions can sleep            │    (accent, capitals), counter (a tour), ×
 │      A session left alone for a while   │  ← the text, under the title (≤ 6 lines)
 │      stops and frees its memory…        │
 │      ⚙ Settings › Terminal › Sleeping…  │  ← where it lives: a muted line of its own
 ├────────────────────────────────────────┤  ← ds::divider
 │ Don't show again   [Settings…] [On ↵]   │  ← one row: link left, compact buttons right
 └────────────────────────────────────────┘
 ╭────────╮                                  ← the anchor ringed in the accent (spotlight)
```

- `HintCard`: its last paint's places (what's under the mouse), the hover, the text wrapped
  once per content, width and scale.
- `HintCardSpec { icon, eyebrow, title, body, place, counter, buttons, focus }`,
  `CardButton { label, main, aside }`, `HintPlace { anchor, area, scale }`, `CardHit`
  (`Close`, `Button(i)`, `Card`).
- `place(size, anchor, area, (gap, margin))`: pure — under the anchor, above it when
  there's no room below, kept inside the area, the notch off the corners; no anchor: the
  area's top-right corner.
- The surface is the popovers' (`ds::surface`: their background, border, shadow, radius);
  the buttons are `ds::button` with `ButtonSpec::compact` (as wide as their label).
- `anchors::begin()` / `report(id, rect)` / `find(id)`: see [anchors](anchors.md). The app
  reports `workbench_model::hints::ANCHOR_ACTIVE_TAB`.

## Options

| Option | Default | Effect |
|---|---|---|
| `WIDTH` | 312 | the card's width (narrower when the area is) |
| `HintCardSpec.icon` (`HintStep::icon`) | none | an icon in a soft accent circle before the texts |
| `HintCardSpec.eyebrow` (`HintStep::eyebrow`) | "Tip" | the small accent label over the title, in capitals |
| `HintCardSpec.place` (`HintStep::place`) | none | where it lives in the settings, a muted line with a gear |
| `CardButton.main` | — | in the accent, with its `↵` (one per step) |
| `CardButton.aside` | — | a quiet link on the footer's left ("Don't show again"); its own row only when the row doesn't fit |
| `HintCardSpec.counter` | none | "2 / 4" left of the × (a tour) |
| `HintCardSpec.focus` | none | the button the keys are on, with the shared focus ring |
| `HintPlace.anchor` | none | the notch and the spotlight; none: the top-right corner |

## States and interaction

- Floating (as the notifications): it takes no key of the editor's, blocks nothing; only
  the mouse on the card reaches it, and a click elsewhere leaves it be (never closes it).
- A click on the card gives it the keyboard: ← → / Tab move between its buttons, ↵ presses
  the focused one (else the main one), Esc closes it; a press elsewhere takes the keyboard
  back.
- × or Esc: closed for this run. "Don't show again", Done, or an action that acknowledges:
  remembered for good (`[hints] seen` in the app's settings). An action runs its command.
- `hover(p)` / `over_button` for the hand; `hit(p)`; `hide()` once it closes.

## Accessibility

The card is a `Dialog` named by its title, its text as its value; its buttons are
`ds::button`s (each a `Button` named by its label).

## Cost

- Nothing at all while no hint shows (the overlay isn't open; the anchors' list is cleared
  and reused each frame, never grown again once it has room).
- The text wrapped once per content, width and scale; a frame shapes a few short lines.

## Example

```rust
// A module, at start:
ctx.contribute(workbench_model::HintContribution {
    hint: workbench_model::Hint {
        id: "feature.tip",
        steps: vec![workbench_model::HintStep {
            title: "feature.tip_title",
            body: "feature.tip_body",
            anchor: Some(workbench_model::hints::ANCHOR_ACTIVE_TAB),
            actions: vec![workbench_model::HintAction::primary("feature.tip_on", TURN_ON)],
        }],
    },
});
// When it fits:
host.show_hint("feature.tip");
// A view pointing at one of its parts, as it draws it:
ui_components::anchors::report("feature.button", rect);
```

## Don't

- Draw a hint in a view: contribute it and ask for it.
- Close it on a click outside, or take the editor's keys while it isn't focused.
- Show a hint again once seen: the app's book says; an id never changes once shipped.
- Put the user's data in a hint's text: it's the module's own words (i18n keys).
