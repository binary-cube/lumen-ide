# Dropdown field (`ui_components::ds::dropdown`)

**For:** the field showing a choice, its list opened by a press. The field is this
component; the list under it is always the shared `picker::Picker` (an editor bar asks the
app for it with `ToolbarAction::List`).

**Where it's used:** about a dozen views: the settings' `SettingsRow::Dropdown`, the
database console's bar (transaction and mode pills, compact with a prefix), the HTTP
client's method (`value_color`), connection forms (a driver's icon:
`dropdown_with_icon`).

## Anatomy

```
 ┌──────────────────────────────────────────────┐
 │ ● [icon] Prefix: Value ……………… note   ▾     │
 └──────────────────────────────────────────────┘
   dot  icon  muted   value (cut)  muted  chevron (enabled only)
```

## Options

`DropdownSpec` says what it shows; `DropdownLook` what a place adds. Both are plain
structs (`DropdownLook::default()` is `dropdown`'s look).

| Option | Default | Effect |
|---|---|---|
| `spec.value` | — | the choice shown, cut with `…` to its room |
| `spec.open` | — | its list is open: the border in the accent |
| `spec.hovered` | — | a faint tint (never while open or disabled) |
| `spec.enabled` | — | off: muted, no chevron, never lit |
| `spec.note` | none | a muted note on the right (why it's off), half the room at most |
| `spec.placeholder` | none | muted, while `value` is empty |
| `look.prefix` | none | a muted run before the value (`Tx:`) |
| `look.dot` | none | a status dot first, in this color |
| `look.compact` | off | a toolbar pill: smaller font and room (its height is the rect's) |
| `look.value_color` | the text's | the value and its chevron in this color (a method) |
| `dropdown_with_icon(…, icon)` | none | an icon after the dot, before the prefix |
| `dropdown_width(…)` / `dropdown_width_with_icon(…)` | — | the width that shows the whole value (rounded up) |

## States and interaction

- Stateless: the view keeps the rect as its hit; a press opens the view's `Picker` under
  it and sets `open`; the picker takes the keys (`captures_keys`) while open.
- Hover lights the field only while it can be used and its list is closed.

## Accessibility

A `ComboBox` with the value as its `value`, `expanded` while its list is open, `disabled`
when off; its name is the text on its row. Its texts are under its group.

## Cost

- Shaping hits the text system's cache; the value and the note are cut with `ellipsize`,
  which allocates a `String` each paint.
- The field's back, border and texts are drawn without `ctx.fade`: a dropdown in a fading
  overlay doesn't fade with it.

## Example

```rust
fn pill_spec(value: &str, hovered: bool, enabled: bool) -> ds::DropdownSpec<'_> {
    ds::DropdownSpec { value, open: false, hovered, enabled, note: None, placeholder: None }
}

let tx_look = ds::DropdownLook {
    prefix: Some(&tx_prefix),
    dot: tx_dot,
    compact: true,
    ..ds::DropdownLook::default()
};
let r = Rect::new(x, (mid - px(PILL_H) / 2.0).round(), w, px(PILL_H));
ds::dropdown_styled(layer, text, icons, &ds, r, (pill_spec(tx_value, hovered, enabled), tx_look));
self.hits.push((r, tool));
```

## Don't

- Open a context menu from a dropdown's field: the shared picker, with its search, its ✓
  and its keys' footer.
- Size a pill by hand: `dropdown_width` counts its dot, icon, prefix and chevron.
