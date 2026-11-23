# Text options (`ui_components::text_options`)

**For:** how text is shown wherever it's shown: one value (`TextOptions`) the app hands
every view, so a choice made once (word wrap) holds in all of them.

**Where it's used:** every view and panel is told (`Viewer::set_text_options`,
`ToolPanel::set_text_options`); the views drawing text follow it (plan 96).

## Anatomy

- `TextOptions { word_wrap }`. A new option is a new field, its default what views did
  before, so one that doesn't read it is unchanged.
- `wrap_rows(line, columns, tab, out)` gives a line's rows when it's wrapped, as byte
  ranges:
  - a row breaks after a space or a mark in its second half, else where it's full;
  - a character is never split, and a tab goes to its stop;
  - a row holds at least `LEAST_WRAP_COLUMNS`.
- `char_columns(c)`: the columns a character takes (two for wide ones), the same for the
  editor and every other view.

## Behavior

- Told at a view's first frame and at each change, one generation compared per view
  (`Workbench::sync_text_options`); nothing else per frame.
- The setting is `[editor] word_wrap`, off by default. It's shown in Settings → Editor →
  "Text" and in View → "Word Wrap" (Alt+Z).
- A view never reads the setting itself and never wraps on its own: it follows what it's
  told, with these helpers.
