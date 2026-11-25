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

## Releases: two builds, the app and the web engine (plan 99, asked 2026-11-25)

- **Two programs, two builds.** The app (the editor) and the web engine (Chromium through
  CEF, ~100 MB) are built and published apart. The engine is never in the app's package:
  the app downloads it itself, at the first page that needs it, from our own repository
  (`brand.toml` `repository`): the rolling release `web-engine`
  (`https://github.com/<repository>/releases/download/web-engine/web-engine.json`, a signed
  list, then the package for its system).
- **The app:** a tag `v<version>` (the version of `Cargo.toml`, bumped in a commit named
  after it) runs `release.yml`: a draft the user reads and publishes.
- **The engine:** a tag `web-engine-v<Chromium version>-<engine::BUILD>` runs
  `web-engine.yml`: it builds, signs and packs every system, and makes a draft; running the
  workflow with `publish` and the version moves it to the rolling `web-engine` release (only
  then can apps download it); `relist` changes only the minimum safe version.
- **When the engine needs a build of its own:** only when `web-host/`, CEF/Chromium
  (`cef` in `web-host/Cargo.toml`) or the protocol (`web-engine-protocol`, `PROTOCOL`)
  change. A new Chromium: its version in the tag, `engine::BUILD` back to 1; a fix of ours
  with the same Chromium: `engine::BUILD` + 1. A change of `PROTOCOL` needs a new engine
  published BEFORE the app that speaks it (an app finds no engine of its protocol until
  then: it keeps the system's pages).
- **Order:** the engine first (published), then the app. An app release that changes
  nothing of the above needs no engine build.
