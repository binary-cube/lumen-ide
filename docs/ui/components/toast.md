# Toasts (`ui_components::toast`)

**For:** drawing the notifications: each a glass card with its kind's icon, title, detail,
at most two actions and a × ringed by the time left; the stack at the edge the user chose.

**Where it's used:** the app's toast stack (`workbench::toasts`); views and modules only
return `Notification`s, never draw them.

## Anatomy

```
 ┌───────────────────────────────────────────────┐
 │ ▣  Title of the message  ×2              (×)  │  ← kind's icon in a tinted square;
 │    A line or two of detail, the last cut…     │    × with the time left as a ring
 │    [ Primary ]  [ Other ]                     │  ← at most two actions
 │    ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━            │  ← progress (work going on)
 └───────────────────────────────────────────────┘
```

- What's shown and its rules are `workbench_model::Notifications`; this lays it out,
  draws it, says what's under the mouse.
- `ToastStack`: the layout (a `Memo` keyed by the stack's generation and the width), where
  each card went, the hover, each counting card's ring.
- `CardLayout` (texts shaped once), `StackPlace { area, position, scale }`, `ToastHit`
  (`Close`, `Action`, `Card`).
- `kind_tone`, `kind_color`, `kind_icon`, `ring_steps`, `layout_card`.

## Options

| Option | Default | Effect |
|---|---|---|
| `StackPlace.position` | the user's (Settings) | the edge (corners or centered); the newest nearest it |
| `WIDTH` / `GAP` / `MARGIN` | 380 / 10 / 16 | a card's widest, the space between cards, from the side |
| a notification's `actions` | none | at most two buttons (the first tinted by the kind) |
| its progress | none | a bar: how far, or sweeping when not known |

## States and interaction

- `hover(p)`: what's under the mouse changed; while hovered the stack's time stands still
  (`hovered`).
- `hit(p)`: `Close(id)`, `Action(id, command)`, or `Card(id)` (nothing happens, nothing
  under it gets the click). `over_button` for the hand.
- Cards enter and leave from the chosen edge; frames at most ~30 a second for the rings,
  ~60 for a sweeping bar, only while one shows.

## Accessibility

Each card is an `Alert` named by its title, its detail as its value; its buttons are
`ds::button`s.

## Look

- The card is frosted glass: what's under it blurred strongly (`GLASS_BLUR` 28 logical
  pixels: colors, never text that reads), a surface over it (`GLASS_SURFACE` 0.88) through
  which the blur shows; its text reads clearly over a busy page too.
- Where nothing can be blurred under it (`StackPlace::backdrop` false: drawn on the layer
  over the system's web pages, which aren't the window's pixels), the card is opaque.

## Cost

- Nothing at all while the stack is empty (the rings let go too).
- Laid out once per content and width; a frame reuses the placed areas' vector.
- `memory()` reports the rings' images.

## Example

```rust
toasts: ui_workbench::toast::ToastStack,
self.toasts.paint(painter, &self.data.notifications, place);
match wb.toasts.hit(p) { Some(ToastHit::Close(id)) => …, _ => … }
```

## Don't

- Draw a notification in a view: return a `Notification`.
- Show more than two actions: the rest belongs in what the notification opens.
