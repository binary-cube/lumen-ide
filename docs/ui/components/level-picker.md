# Language level picker (`ui_components::level_picker`)

**For:** choosing a language's version (PHP 8.3) from the status bar: for the project, or
only for the active file.

**Where it's used:** the `language-levels` module, as its popover (`view-level-picker`)
above the status bar's item; the choice goes back as `AppRequest::SetLevel`.

## Anatomy

```
 ┌ PHP version ─────────────────────────────────┐
 │ Detected 8.2 from composer.json              │
 │ [ For the project | Only for index.php ]     │  the scope (with a file only)
 │ [🔍 Filter                             × ]   │  the shared search field
 │ [Auto (8.2)] [PHP 8.4]  [PHP 8.3]            │  pills, 3 a row; chosen: accent
 │ [PHP 8.2]    [PHP 8.1]  …                    │
 ├──────────────────────────────────────────────┤
 │ ← → move · ↵ pick · tab scope · esc close    │
 └──────────────────────────────────────────────┘
          ▲ the status bar item (POPOVER_GAP)
```

## Options

| Option | Default | Effect |
|---|---|---|
| `LevelChoices { anchor, name, levels }` | — | the item it opens above, the language, its versions oldest first |
| `project`, `auto` | — | the project's level, chosen automatically or not |
| `detected`, `source` | `None` | the detected version and where from |
| `file_name`, `file` | `None` | the active file and its own level (the scope shows only with a file) |
| `open(&choices)` / `close()` | closed | opens on the file's scope when there's one |
| `set_viewport(rect, scale)` | — | where it may place itself |

## States and interaction

- `click(p)`: `Pick(LevelPick::Project(…) / File(…))`, `Inside` (the scope, the ×, empty
  space) or `Outside` (it closes).
- The filter is a `SearchField` (×, ⌘A, a caret): `press_field`, `drag`, `field_key`,
  `type_text`, `backspace`, `command("select_all")`.
- `step(±1, row)` moves among the pills (a row with ↑ ↓), `toggle_scope` is Tab,
  `confirm` picks the one under the keys. The keys are mapped by its owner today.
- It appears over 130 ms (opacity and a 6 px lift).

## Accessibility

Nothing yet: the pills and the scope don't describe themselves; the filter does through
`SearchField` (see the audit).

## Cost

- Exists while open. Each paint builds its pills' labels and the footer's words again.

## Example

```rust
fn set_data(&mut self, data: &dyn std::any::Any) {
    let Some(choices) = data.downcast_ref::<LevelChoices>() else { return };
    self.picker.open(choices);
    self.choices = Some(choices.clone());
}

fn paint(&mut self, ctx: ViewerPaint<'_>) {
    let Some(choices) = &self.choices else { return };
    self.picker.set_viewport(ctx.bounds, ctx.scale);
    let painter = Painter { text: ctx.text, scene: ctx.scene, palette: ctx.palette, icons: ctx.icons };
    self.picker.paint(choices, painter);
}
```

## Don't

- Open it as app code: it's the module's popover view.
- Close it on a press inside: only `Outside`, Esc or a pick.
