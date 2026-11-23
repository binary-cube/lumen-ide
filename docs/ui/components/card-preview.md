# Settings card pictures (`ui_components::card_preview`)

**For:** the pictures on a settings page's cards (`workbench_model::SettingsRow::Cards`):
a module paints what each choice looks like; the settings window knows none of them.

**Where it's used:** the settings window's card rows; the relax module paints its scenes'
cards (`ScenePreviews`).

## Anatomy

```
 ┌ card ──────┐ ┌ card ──────┐ ┌ card ──────┐
 │ ▓ picture ▓│ │ ▓ live  ▓▓ │ │ ▓ still ▓▓ │   one card live (the selected or hovered one),
 │ Name       │ │ Name    ✓  │ │ Name       │   the others a still frame, made once and kept
 └────────────┘ └────────────┘ └────────────┘
```

- `CardPreview` (contributed at `activate`): named as its rows name it.
- `CardPaint`: where and how a picture is painted.

## Options

| Option | Default | Effect |
|---|---|---|
| `id()` | — | the name its rows give (`SettingsRow::Cards::preview`) |
| `paint(value, live, CardPaint)` | — | card `value`'s picture, in layers of its own; `live`: as it is now |
| `frame_interval()` | 33 ms | the live card's pace |
| `release()` | — | none of its cards shows any more: what it made goes |
| `CardPaint { bounds, clip, scale, scene, text, palette }` | — | its area, what it may draw on (its layers are cut to `clip`) |

## States and interaction

- Live or still: the settings window picks the live one and asks it again at
  `frame_interval`; the others are asked for their still frame.
- No input: a press on a card is the settings row's (it chooses the value).

## Accessibility

None of its own: the card around it is the settings row's choice, named by its label.

## Cost

- One live picture at a time; the stills made once and kept by the module.
- `release` when the page is left or the window closes: nothing of them stays.

## Example

```rust
impl CardPreview for ScenePreviews {
    fn id(&self) -> &'static str {
        PREVIEW
    }

    fn paint(&self, value: i64, live: bool, p: CardPaint<'_>) {
        self.paint_at(value, live, p, Instant::now());
    }

    fn release(&self) {
        if let Ok(mut state) = self.state.lock() {
            *state = State::default();
        }
    }
}
```

## Don't

- Animate every card: only the live one is asked again.
- Keep the stills after `release`.
- Draw outside `clip`: the settings' content scrolls under it.
