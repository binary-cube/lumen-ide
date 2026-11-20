# The log

Part of the project's rules: [index](project-conventions.md). Related: [writing code](code.md), [performance](performance.md), [privacy](privacy.md).

- **Everything goes through `tracing`**, written by the app's `logging` crate: one JSON
  object a line in `<log folder>/app-<day>.jsonl`, turned over by day and size, secrets
  hidden. `println!` / `eprintln!` only in xtask, tests, snapshots and the command-line
  tools (the toy driver, the language server).
- **The message says what happened, in a few plain words; the values go in fields**, never
  pasted into the text: `tracing::warn!(path = %path.display(), error = %e, "can't open")`,
  not `"can't open {}: {e}"`. An error is the field `error`; a duration `?elapsed` (written
  as `elapsed_ms`); a path `%path.display()`; an id or a name as it is. A field is filtered
  and searched; a text isn't.
- **Levels:**
  - `error`: something broke that the user will notice, or that loses data;
  - `warn`: something failed and was worked around (a file not read, a retry);
  - `info`: what happens once in a while and tells the story (the app starts and ends, a
    project opens, a file is saved, a server starts, an update, a slow frame);
  - `debug`: what helps look into one part (every file indexed, a dialog's answer);
  - `trace`: what's too much even for that.
  Never anything per frame, per key or per mouse move above `debug`.
- **Nothing secret, ever, by intent**: no password, token, key or cookie as a field or in a
  message (the `redact` net is a last guard, not a licence); no file's contents; a secret
  value is a `kernel::Secret`. SQL may be logged as a field (`%sql`) only where it failed.
- The user chooses the level and the folder (Settings → Log); `EDITOR_LOG` / `RUST_LOG`
  win over them.
- **Per-module levels** are set in Settings → Log (each active module a dropdown); a
  module's lines carry its crate as `target`, so a new module is registered in
  `crates/app/src/modules.rs` with `add(…, "<crate>")`.
