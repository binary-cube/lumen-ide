# How code is written here, and how modules talk

The rules of `CLAUDE.md`, made concrete by what has been agreed while working on the
project. Read with it; when they differ, `CLAUDE.md` wins.

## Writing code

- **English everywhere in code**: comments, docs, logs, errors, `Cargo.toml` descriptions.
  User-facing text lives in `crates/domain/core/i18n/locales/{ro,en}.toml` (both, always);
  plans in `docs/plan/` stay Romanian. Replies to the user are Romanian.
- **Doc comments say what a thing is or does, in short prose**, often with a colon:
  `/// The file's environments: the public one, the private one, the local one.` A module
  starts with `//!` describing what it is for and how it behaves (what runs where, what is
  kept, what is let go). Comments explain *why*, not *what the next line does*.
- **Names are descriptive technical terms** (`indexer`, `http-environments`), never the
  app's name (`lumen`); thread names too (`http-run`, `http-view`). A saved id that changes
  keeps an alias.
- **Small, focused files**; a big feature is a folder (`editor/{mod,form,paint,…}.rs`).
  One place for each thing: no duplicated helpers (share them in the crate:
  `format::method_color`, `page::open_in_browser`).
- **Tests in separate files** under `tests/unit/`, mirroring `src/`, included with
  `#[cfg(test)] #[path = "…"] mod tests;`. Every new behavior gets a test; fixes get a
  test that failed before. Temp files under `std::env::temp_dir()` with a fresh UUID,
  removed at the end. A test against the network uses a server of its own on the loopback.
- **Domain is pure** (`crates/domain/<group>/*`): models, parsing, rules, no I/O. Infra
  (`crates/infra/<group>/*`) implements ports. A new crate gets its layer (and, for an infra crate a
  module owns, `module_infra.owned`) in `architecture.toml`; `cargo xtask check-deps` checks.
  External crates allowed in the domain are listed there with the reason.
- **Crates are grouped by domain**, with the modules' group names (`code`, `database`,
  `vcs`, `http`, `tools`, `appearance`, `viewers`, `project`; `core` for what the whole app
  uses; in `infra` also `files`, `system`, `media`, `remote`): a new crate goes in its
  group's folder (`crates/domain/http/http-requests`, `crates/infra/http/http-transport`),
  its folder keeping its own name.
- **Settings** only through `Setting<T>` and the settings service; **caches** from
  `kernel::cache`; **ids** that never repeat are UUID v4.
- **Untrusted input is handled as hostile**: no unchecked arithmetic on server/user values
  (`saturating_*`, `checked_*`), no byte-index slicing that could split a char, no newline
  injection when writing a file from imported data, limits on walks (depth, count, no
  symlinks), secrets hidden wherever shown, never logged.

## How modules talk (never a fifth way)

Modules don't know each other: no module depends on another's crate, reads its files or
names its commands, views or windows. Removing one changes nothing in the others.
They meet only through:

1. **Contribution** (`ctx.contribute`): "I offer something", known at start (a viewer
   provider, a tool window, a command, a menu, an assist, a service, a reporter).
2. **Event** (the project's own bus: `ProjectContext.events` / `ProjectView.events`):
   "it happened", any listeners, no answer. Each project has its own bus.
3. **Request** (`contracts`, `Answers::of`, `host.ask`; a view: `ViewerAction::Ask`):
   "do it / tell me", at most one answers; `None` means nobody does — handle it.
4. **Host** (`WorkbenchHost`, `ViewerAction::WithHost`, `AppRequest`): what only the app can
   do (windows, tabs, clipboard, pickers, opening a browser, the system's web engine).

What needs a partner is offered only while there is one (`workbench_model::presence`).
Per-project state is `PerProject<T>`. A view gets its project through `set_data`
(`ProjectView`); a panel's work after a picked file comes back through the host.
Modules build their UI from `ui-components` (never `ui-workbench`).

Made concrete while building the agents (plan 79):

- **Registries the app fills.** A neutral domain crate holds a contract and its registry
  (`agents::AgentProvider`, `mcp::McpTool`); modules contribute, the app registers what
  was contributed at startup. A consumer lists the registry and never names a provider
  (the MCP server names no tool's module; the sessions panel names no agent).
- **A tool window that needs something** says so with `ToolWindowProvider::available`
  (e.g. only while some agent is installed): read from memory, never the disk.
- **Requests between modules are small, named contracts** in `contracts` (`RunInTerminal`,
  `RenameTerminal`, `TerminalRunning`, `RenameSession`, `OpenAgentChat`): the asker handles
  `None` / `false` (nobody answers, the tab is gone: let it go).
- **Identity by key, never by title**: a tab another module opened is found by its key
  (`contracts::terminal::tab_key`) or a UUID, never by its name or a name read from a file.
- **A reply that may come later** is a value that answers once and says "refused" if
  dropped (`mcp::Reply`); whoever waits may give up, and then nothing runs late.
- **An agent is a folder of `modules/ai/agents/`** on `agents::AgentProvider`; what agents
  read alike on disk (finding the program, bounded listings and reads, values kept by
  stamp) is `agent-files`, never copied into each; its own formats stay in its module and
  are checked read-only on a real install before they're written. What the panel keeps of
  a session (favorites, marks, folders) is keyed by agent + id, so a new agent needs
  nothing there.
- **Static tables per project**: an entry kept for a project (its wake, its notifier, its
  favorites) is removed when it closes (`kernel::Subscription::on_end` returned from
  `ProjectService::attach`); never "the last window's" handle kept for all.

## Menus

- **Settings before Properties**: where a menu offers both (a connection's settings, an
  entry's properties), "… Settings…" comes first and "Properties…" right after it, last in
  its group — the same in every menu.
- **Menus are the app's one component** (`ViewMenuEntry` → the context menu); what a menu
  needs that it lacks is added to it (a greyed-out row, a ✓ at the end, a color dot:
  `RowLook`), never drawn by a module. A row shown greyed out is one the user should know
  exists but can't use here (an agent not hosted yet, one not installed); a row with no
  partner at all stays hidden (`presence`).

## The status bar

- **Every item has a priority** (`StatusItem::essential` / `spare`, normal by default):
  what doesn't fit goes under `…` by it, the farthest from the corner first; items that
  belong together are `joined`. Never an item the bar draws over another.
- **An item with more to say gets a window, not more items**: the bar's
  `status_popover` (label / value rows, a row running its command, graphs, a pin), floating,
  its rows made at its paint from what's already in memory.

## The UI thread only draws and answers

- **Nothing heavy on the UI thread**: no disk, git, network, process or parsing of anything
  big. Work goes to a worker (`kernel::work::run` with a named pool, `host.spawn`,
  `ui_components::deferred::Deferred`, a module's `Pending<T>`), and comes back by a poll;
  the view's paint waker wakes the app when it's ready. A click never reads a file.
- **Nothing per frame that could be cached**: parse once per version, read states once per
  change (a generation), build strings once, follow wakers once. Frames only when something
  changed; an animation asks for the next frame only while it moves (`frame_deadline`,
  `is_animating`).
- **Exists only while used**: views, engines, previews are made when first needed and let go
  when closed or hidden long enough; nothing loaded before it's needed. Memory has budgets
  (e.g. response bodies per project), the oldest let go to disk.
- **Measure before choosing**, and write the numbers in the commit or the plan.
- **Never I/O, waking or calling out under a lock**: a lock only moves data in or out;
  writing to a process goes through a thread of its own fed by a channel, a wake happens
  after the guard is dropped. Saves of one file go one at a time (the latest state taken
  inside the worker), through a temporary file renamed over it.
- **Cheap checks first in what runs every frame or poll**: compare a generation before
  cloning a list; return when a queue is empty before looking at anything else.
- **Reading another program's output** (an agent's pipes, its session files): bounded
  lines, lossy UTF-8, one bad line skipped, never the end of the stream; titles from it
  tidied to one line and capped; anything typed into a terminal stripped of control
  characters.

## Working on a step

- Plans in `docs/plan/NN-*.md` (Romanian): each step gets **Făcut** (what was done, how)
  and **Lăsat** (what wasn't, why) once done.
- **Gate before every commit**: `./scripts/check.sh` (fmt, clippy host + Windows, tests,
  check-deps) must give four `ok`; run it in the background. One commit per step, a
  descriptive message (title, then what changed and why), ending with the
  `Co-Authored-By` line. Never commit `.lumen/`, `.codegraph/` or `.claude/` — except
  `.claude/rules/` (these rules: committed, kept current).
- After a large piece, a **review by a separate agent** (not anchored on the author's view),
  then the findings verified and fixed (or weighed and written down).
- **Never touch the user's real files, keychain, projects or containers**; run the app with
  an isolated `HOME` and `EDITOR_NEW_INSTANCE=1`; snapshots go to the scratchpad; no git
  writes on user repos to test; no dist builds (the user runs them).
- **What the user sees stays**: a change never removes a feature or changes how it behaves
  to save resources; "foarte foarte important să nu stricăm nimic din ce avem deja
  funcțional".

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
