# Lumen — project conventions

- **English in the code.** Everything in the code and its metadata is English: comments
  (`//`, `///`, `//!`), log messages, error / panic messages, `Cargo.toml` descriptions,
  module manifests, xtask output. User-facing text lives in the locales
  (`crates/domain/core/i18n/locales/{ro,en}.toml`); the plan documents (`docs/plan`) stay Romanian.
- **Every feature is a module** (`crates/modules/*`), contributed through extension points;
  modules load lazily and release their resources when unused.
  Modules build their views with `ui-components` (the `Viewer` contract, `ds`, the palette,
  splitters…), never `ui-workbench` (the app's own screens, which change often: only the
  app depends on it, so changing a screen recompiles only the app).
- **Modules don't know each other.** Removing any one changes nothing in the others (a test
  takes each out in turn). No module depends on another's crate, reads its files or names its
  commands, views or windows. They meet only through four mechanisms, each for one thing
  (`docs/plan/69-comunicare-module.md`), never a fifth:
  - **contribution** (`ctx.contribute`): "I offer something", known at start, lists too;
  - **event** (the project's bus): "it happened", any listeners, no answer;
  - **request** (`contracts`, `Answers::of`, `host.ask`; a view: `ViewerAction::Ask`): "do it /
    tell me", at most one answers, data both ways. `None` means nobody answers: handle it;
  - **host**: what only the app can do (the window, tabs, the clipboard).

  What needs a partner is offered only while there's one (`workbench_model::presence`), so
  no button does nothing. State per open project: `PerProject`. Shared contracts belong in
  `contracts` or `workbench-model`, by domain. The rules are in `architecture.toml`, checked
  by `cargo xtask check-deps`: a new crate gets a layer there.
- **Tool windows are registered, never hardcoded.** Anything with a button on the window's
  sides (left top, left bottom / the bottom area, right) — a module's or the app's own —
  registers a `ToolWindowProvider` (`ui-components`) at `register`: its id, icon, title
  (tooltip and header), dock, order, toggle command, and what showing it does (a panel made
  on first show, views it opens as tabs, or a command). Its right-click menu is a
  `MenuContribution` with the same id. The app only discovers and lays them out: one dock
  per side, the same buttons, tooltips and headers for all; nothing is created before it's
  first shown.
- **Each project has its own events.** What happens in a project (saves, renames, deletions,
  the disk's changes, batches of changes, refactorings, indexing) is published on its own
  bus (`ProjectData.events`, given to modules as `ProjectContext.events` / `host.events()` /
  `ProjectView.events`), made with the project and gone with it — never a bus shared by
  windows: another project's work must never reach, delay or disturb this one. Modules
  follow those events (with queues that merge repeated work, `kernel::queue`) instead of
  being called.
- **Identities are UUID v4** (`uuid` crate) when something needs one that never repeats
  (a bottom window's views…), never a name or a position.
- **Layers (DDD, see `docs/plan/02-arhitectura-ddd.md`)**: domain is pure (models, ports,
  use cases), infra implements ports, `app` only wires things together. Declared in
  `architecture.toml` (as deptrac.yaml in PHP), checked by `cargo xtask check-deps`.
- **Settings** are declared once in `crates/app/src/settings.rs` (`Setting<T>`: section,
  key, type, default, validation) and read / written only through the settings service
  (`crate::config::settings()`: cached, announces `SettingChanged`) — never by section /
  key strings elsewhere. A module's own settings are declared the same way in its crate
  (`src/settings.rs`) and shown through a `SettingsContribution` (`workbench-model`): its
  block in the settings window, what a choice saves, how the values reach its domain —
  the app never knows them.
- **Caches** are built from `kernel::cache` (`Memo`, `GenerationalCache`, `Purge`) and
  can be purged ("Clear and Rebuild Caches"). Project-scoped data lives in `ProjectData`
  and is replaced wholesale when another project opens — nothing may leak between projects.
- **Performance**: nothing heavy on the UI thread (a click never reads a file or git: a view
  prepares its action with `ui_components::deferred::Deferred`, an answer or a command uses
  `host.spawn`); no per-frame work that could be cached;
  RAM/CPU/GPU budgets matter. Must work on Windows.
- **What the user sees stays; everything under it is as cheap as it can be.** A change never
  takes away a feature or changes how it looks or behaves to save resources. Under that
  surface, memory, CPU, GPU and paint cycles are the budget, and every new feature answers:
  - **Exists only while it's used**: previews, pictures and whatever a page or a window makes
    go when it closes; nothing is loaded before it's first needed.
  - **The UI thread only draws and answers**: disk, git, network, processes and parsing run
    in the background (`kernel::work`, `host.spawn`, `Deferred`); a result arrives by a poll.
    Menus and `enabled` read nothing but what's already in memory.
  - **Nothing is done twice or for nothing**: what's made once is kept under what it depends
    on (a generation, a version, a stamp); frames are drawn only when something changed, and
    an animation asks for the next one only while it moves; a text embedded in the binary is
    borrowed, not copied.
  - **Measure before choosing**: compare what the options really hold (a thumbnail on disk
    is decoded into a texture bigger than the points that draw it) and write the numbers in
    the commit or the plan.
- Before committing: `./scripts/check.sh` (fmt, then clippy for the host and Windows, the
  tests and `cargo xtask check-deps`, in parallel); verify visual changes with `--snapshot`
  hooks.
- **Language data is ours**: PHP stubs (`crates/modules/languages/php/data/php.stubs`) are
  generated by `cargo xtask php-stubs --update` (or `./scripts/build.sh stubs`) from every
  PHP version in `docker/php/compose.yaml` (Alpine images, extended by
  `docker/php/Dockerfile` with PHP's bundled extensions and, on the newest versions,
  popular PECL ones) via reflection — never copied from third-party stub files. Each
  record names its extension (missing ones in `composer.json` are pointed out). New PHP releases are added automatically (monthly workflow
  `.github/workflows/language-data.yml`). Users need neither PHP nor Docker.
  CSS / HTML / JS data (`crates/modules/languages/{css,html,javascript}/data/`) comes from
  MDN's public-domain (CC0) data via `cargo xtask web-data`, converted to our format.

- **Tests live in separate files**, per crate under `tests/unit/`, mirroring `src/`
  (`src/search.rs` → `tests/unit/search.rs`, `src/workbench/mod.rs` →
  `tests/unit/workbench.rs`, `src/main.rs` → `tests/unit/bin.rs`), included from the source
  file as `#[cfg(test)] #[path = "…"] mod tests;` (so they can still test private items).
  Tests across crates go in `tests/*.rs` (integration tests). Data files are read from
  `CARGO_MANIFEST_DIR` or with paths relative to the test file.

- **Modals never close on an outside click** (About, settings, dialogs, project search,
  projects list…): only via ×, Esc or their buttons — a stray click must not lose the
  user's work. While a modal is open nothing behind it gets keys, text, commands, scroll
  or hover (`Workbench::modal_open`). Popovers keep `ds::POPOVER_GAP` from the bottom bar.
  Every window over the editor is an `OverlaySlot` on one stack (`Overlay::ALL`, drawing
  order = input order; the common behavior is the default, each overrides what differs);
  a module brings its own as a view (`host.open_modal` framed, `host.open_overlay` bare,
  modal or popover), never as app code.
- **The app's name lives in `brand.toml`** (name, id, project dir, bundle id): code uses
  `kernel::brand` (`NAME`, `ID`, `PROJECT_DIR`, `named("… {app} …")`), scripts and
  packaging read the file — never hardcode the name; nothing the user sees may depend on it.
- **No app name in internal identifiers.** The app may be renamed: new crates, modules,
  thread names, ids saved in settings (themes, keymaps, icon styles), virtual tab
  extensions (`view-*`), GPU labels, temp dirs, log names… use descriptive technical terms
  (`indexer`, `default-dark`, `refactor`), never `lumen`. When an id that was saved in user
  settings changes, the old one keeps working (an alias). Crates have plain technical
  names (`kernel`, `document`, `lang-php`, `ui-workbench`; descriptive where a bare name
  would clash: `code-intelligence`, `version-control`, `settings-model`), environment
  variables the `EDITOR_` prefix (`EDITOR_SNAPSHOT_*`); only the binary bears the brand's id.
