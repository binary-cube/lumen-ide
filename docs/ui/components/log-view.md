# Log view (`ui_components::log_view`)

**For:** a log of lines as they come ("Output"): each with its time, its kind and its
text; a statement's colored as code, an error's a link to its place.

**Where it's used:** the Services window's Output (a database console's session, the
sample service).

## Anatomy

```
 ┌──┬──────────────────────────────────────────────────────────┐
 │⏎ │ 10:00:23.698  shop.main> SELECT * FROM orders   (as code) │
 │⤓ │ 10:00:23.712  12 rows retrieved in 14 ms                  │
 │──│ 10:00:24.001  ERROR: column "x" does not exist  (a link)  │
 │⧉ │                                                           │
 │🗑 │                                                           │
 └──┴──────────────────────────────────────────────────────────┘
  side buttons: wrap, follow the end | copy, clear
```

- `Log` (shared, `Arc<Mutex<…>>`): whoever writes pushes lines from its thread, shown or
  not; at most `LIMIT` lines (10 000) and `BYTE_LIMIT` (32 MB), the oldest leaving.
- `LogLine { time, kind, prefix, text, code, link }`, `LogKind` (info, statement, result,
  warning, error, notice).
- `LogView`: a `Viewer` drawing a `Log`: its rows' layout, the selection of lines, the
  scroll, wrap and follow.

## Options

| Option | Default | Effect |
|---|---|---|
| `Log::new()` / `Log::with_limit(n)` | 10 000 lines | — |
| `LogLine::new(time, kind, text)` | — | `.with_prefix("shop.main>")`, `.as_code("sql")`, `.with_link(action)` |
| `LogView::new(log)` | follows the end, no wrap | — |
| `set_wrap(on)` / `set_follow(on)` | off / on | soft wrap at the width; keep the end in view |

## States and interaction

- Press: a line chosen (⇧ extends); on an error's text its link runs (`ViewerAction`).
- Keys: ↑ ↓, PgUp / PgDn, Home move the chosen line; End follows the end; Esc lets the
  selection go; ↵ follows the chosen line's link.
- Wheel: scrolls (scrolling up stops following). Side buttons: wrap, follow, copy, clear
  (`ds::button`s, with tooltips).
- Right-click: copy, select all, wrap, follow, clear. Commands: `copy`, `select_all`.
- Embedded in another view as a `Control` through `control::Embedded`.

## Accessibility

The side buttons are told as buttons; the lines aren't told yet.

## Cost

- Rows laid out only when the lines, the width or the wrapping change (new lines only
  appended), in a monospace font: a wrap is a count of characters, never a measure.
- A wrapped statement is tokenized once, not per row and frame; a frame draws only the
  rows in view.
- `memory()` reports the rows' layout.

## Example

```rust
let log = Log::new();
log.push(LogLine::new(time, LogKind::Statement, sql).with_prefix(prefix).as_code("sql"));
// the view
return Some(Box::new(LogView::new(runner.log().clone())));
```

## Don't

- Push lines on the UI thread in a loop: the writer pushes from its own.
- Keep a log unbounded: give it a limit (`with_limit`).
