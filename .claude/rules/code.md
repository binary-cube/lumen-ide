# Writing code

Part of the project's rules: [index](project-conventions.md). Related: [modules](modules.md), [UI components](ui-components.md), [the log](logging.md), [performance](performance.md).

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

## How it reads

- **Plain, short prose in comments**, as the code around it: what a thing is, then why it
  behaves so (`/// The rows asked again: the grid asks only when its range changes.`).
  No comment that repeats the next line; no history ("was", "now", "fixed") — that goes in
  the commit.
- **Names say what a thing is in the domain** (`wanted`, `shown`, `latest`), not its type
  (`vec`, `map`, `data`); a `bool` reads as a state (`paused`, `open`); a function as what
  it gives or does (`listed()`, `ask_wanted()`).
- **Constants, never magic numbers**: a size, a delay, a limit is a named `const` with a
  doc comment saying its unit and why that value (`/// The bytes read at a time.`).
- **Data goes in DTOs, never long lists of arguments** (asked 2026-11-24): what a
  function, a constructor or a contract takes is a named struct of its own (its fields
  documented, `Default` where a field has an obvious one, new fields added without
  touching the callers), or a few small tuples of what belongs together
  (`(doc, folds)`). Never `#[allow(clippy::too_many_arguments)]` (none is left): clippy refuses
  the long list, `cargo xtask check-deps` refuses allowing it, for the whole project. The same for what modules
  pass each other (a request's, an event's payload): a struct that can grow, not a
  function with one argument more each time.
- **Small functions, early returns** (`let … else { return }`), `match` over chains of
  `if`; no `unwrap`/`expect` outside tests (clippy forbids them: handle the `None`).
- **One way of doing a thing**: before writing a helper, look for it (`codegraph explore`,
  the crate's `format`, `ds`, `kernel`); found in two places, it moves to one.
- **Reuse before writing**: a view is built from what [ui-components](ui-components.md)
  has; what a component lacks is added to the component, for everyone.
- **Agnostic examples (asked 2026-11-25):** a comment, a doc or a test never names the
  user's own files, folders, projects or identifiers (what they reported a bug with): a
  neutral example of the same shape (`docs/guide.md`, `FEATURE_ENABLED`) says the same.
- **What a user reads is never in the code**: every label, tooltip, error shown goes
  through `i18n` (`ro` and `en` together).
