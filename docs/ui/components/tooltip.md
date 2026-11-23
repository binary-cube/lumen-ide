# Tooltips (`ui_components::tooltip`)

**For:** the one place tooltips are shown: a part only says which tooltip it has under the
mouse (a `Tip`); the app shows it after a short rest, with a fade, over everything.

**Where it's used:** every part's `Control::tooltip` / a view's `tooltip_at`; the app owns
the one `Tooltips` and paints it last.

## Anatomy

```
        ┌──────────────────────┐
        │ Refresh   ⌘R         │   ← ds::tooltip pill: label, shortcut
        └──────────┬───────────┘
                   ▼              ← arrow toward the item's middle
               [ ⟳ ]              ← the anchor
```

- `Tip { anchor, label, shortcut, delay }`: what a part has under the mouse.
- `Tooltips`: the pending one and since when, the shown one and its fade, where it points,
  where it was drawn (its pill and arrow: what must show over a web page).

## Options

| Option | Default | Effect |
|---|---|---|
| `Tip::new(anchor, label)` | 450 ms rest | — |
| `.with_shortcut(Some(keys))` | none | the keys after the label |
| `.with_delay(d)` | 450 ms | a longer rest (a tab's path: not on every pass) |
| a label with `\n` | one line | first line names, the next tell values |

## States and interaction

- `set(Some(tip))` on every move: a new item restarts the wait; moving from one shown tip
  to the next shows it at once. `set(None)` / `hide()` (a click, a scroll, a key) hides it.
- `pointer(p)`: an item wider than 280 px shows its tip by the mouse, where it was when it
  appeared (it doesn't chase the mouse).
- `deadline()` schedules the frame it appears at; `is_animating` while it fades in.

## Accessibility

None of its own: an icon button's tooltip text is also its name, told by `ds::button`.

## Cost

One tip at a time; drawn only while shown; its areas vector reused.

## Example

```rust
fn tooltip(&self, p: Point) -> Option<Tip> {
    self.close.filter(|r| r.contains(p)).map(|r| Tip::new(r, i18n::t("debug.close")))
}
// the app: self.tooltips.set(tip); …; self.tooltips.paint(text, top, &palette, scale, window);
```

## Don't

- Draw a tooltip in a view: return a `Tip`.
- Build a tip's label on every frame when it's the same: it's asked on hover only.
