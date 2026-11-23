# Text that fits (`ui_components::ds::text`)

**For:** any text drawn in a room it may not fit: cut with `…` at its end, its start or
its middle, a file's name keeping its extension, a path keeping its file, wrapped between
words, a section's title and a section's header.

**Where it's used:** everywhere a name or a message is drawn: `ellipsize` in about eighty
files, `draw_ellipsized` where a long name is drawn every frame (running work, check rows),
`ellipsize_file` / `ellipsize_path` in tabs, breadcrumbs and the status bar, `wrap` in
cards and tooltips, `section_title*` in settings and panels, `section_header` in long
forms.

## Anatomy

```
 ellipsize          A long sentence that does no…
 ellipsize_start    …end that stays visible
 ellipsize_middle   /home/user/pro…/Controller.php
 ellipsize_file     asdfasdf asdf asdfa…fsad.php      (stem's end and extension kept)
 ellipsize_path     src/…/UserController.php          (the file's name first)
 wrap               two lines at most, broken between
                    words, the last ending in …
 section_header     [icon] TITLE ─────────────────────
```

## Options

| Function | Returns | Effect |
|---|---|---|
| `ellipsize(text, value, font, max)` | `String` | cut at the end; whole if it fits |
| `ellipsize_start(…)` | `String` | cut at the start (the end stays) |
| `ellipsize_middle(…)` | `String` | both ends stay |
| `ellipsize_file(…)` | `String` | the extension (≤ 12 bytes) and the stem's last few characters stay |
| `ellipsize_path(…)` | `String` | the file's name whole (or cut as a file), then as much of the folders as fits |
| `wrap(text, value, font, max, lines)` | `Vec<String>` | between words, at most `lines`, the last cut with `…` |
| `draw_ellipsized(layer, text, value, font, (x, baseline), max, color)` | `(width, cut)` | drawn cut at the end with nothing allocated: the glyphs that fit, then `…` |
| `section_title(layer, text, ctx, x, y, title, detail)` | y below | a semibold title and an optional muted line |
| `section_title_in(…, (x, y, width), …)` | y below | the same, both cut to `width` |
| `section_title_font(ctx)` | `Font` | the title's font, to measure it |
| `section_header(pen, (x, y, width), icon, title)` | y below | icon, title in capitals, a line to the end; `SECTION_HEADER` (22) high, `SECTION_SPACE` (14) between sections |
| `initials(name)` | `String` | the first letters of two words (an avatar's) |
| `NAME_MAX` / `PATH_MAX` | 220 / 420 | the widest a name or a path is shown among others (logical) |

## States and interaction

Pure functions of the text, the font and the room. A cut text's whole is the caller's to
show (its tooltip).

## Accessibility

What's drawn is read as a label — the cut text as shown. `section_title*` and
`section_header` are read as labels, not as headings; the header reads its title in
capitals.

## Cost

- Every cut shapes the whole text once (kept by the text system) and finds the cut from its
  glyphs; it is shaped again only when kerning makes the cut text a little wider.
- The `ellipsize*` functions and `wrap` return owned text: one allocation per call. In a
  paint that runs every frame, `draw_ellipsized` draws the same cut with none.
- `section_header` upper-cases its title (an allocation) each paint.

## Example

```rust
ds::draw_ellipsized(
    layer,
    text,
    &group_title(prompt, group),
    &header_font,
    (left, base),
    width,
    p.text_muted,
);

let mut y = ds::section_title_in(layer, text, ctx, (x0, top, width), &title, None) + px(14.0);
```

## Don't

- Slice a string by bytes to shorten it: a cut never splits a character here.
- Cut a file's name at its end (its extension is what tells it apart): `ellipsize_file`.
- Call `ellipsize` then `shape_line` then `draw_text` in a view drawn every frame:
  `draw_ellipsized`.
