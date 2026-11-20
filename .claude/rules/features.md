# Features with rules of their own

Part of the project's rules: [index](project-conventions.md). Related: [modules](modules.md), [performance](performance.md).

## The HTTP client (plan 77)

- The `.http` file is the source: the format's established syntax and behavior (its
  published documentation is the reference: variables as JSONPath, Faker, scripts' API,
  directives, `Security.Auth`), plus what the user asked beyond it (https by default, the
  visual editor of 77l). What the visual editor does is written into `.http` so other
  clients of the format read it the same.
- Requests, scripts, reads and writes run on their own workers (`http-run`, `http-view`,
  `http-editor`, `http-list`, `http-graph`); the module's data folder is
  `.lumen/http/` with its own `.gitignore`.

## Diagrams (plan 80)

- The `.drawio` file is the source: draw.io's format, written back as read (unknown
  attributes, raw attribute text, order) so draw.io reads it the same; what we can't keep
  safely (a `"` in a single-quoted value) is written anew.
- A file is hostile: one budget for the whole file (elements of every page, bytes inflated),
  never per page; no `inf` / `NaN` written; exports clamp their size.
- A view's file follows its tab (`Viewer::moved`); its reads are numbered (an older one, or one
  before a save, is let go); a small field with the keys (a number typed, a search) takes the
  edit commands, never the cells under it.
- The view, the SVG and the PNG lay out texts with one function (`texts::lay`), so a label breaks
  the same everywhere; the PNG composes the view's own tiles and glyphs on a worker.
