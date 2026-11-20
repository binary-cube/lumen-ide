# Working on a step

Part of the project's rules: [index](project-conventions.md). Related: [writing code](code.md), [the log](logging.md).

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
- **Replies to the user are in Romanian**; code, comments, commits and `ROADMAP.md` in
  English; plans (`docs/plan/`) in Romanian.
- **No other products' names** in code, docs, plans or commits (describe the behavior);
  no app name in internal identifiers (`kernel::brand` for what the user sees).
- **Show, don't open**: a visual change is checked with a `--snapshot` (hooks
  `EDITOR_SNAPSHOT_*`, an isolated `HOME`, the real `CARGO_HOME`/`RUSTUP_HOME`), the PNG's
  path given to the user — never opened for them.
- **Commit or push only when the user says so**; push as the project's account
  (`github-microthread`). Releases and dist builds are the user's.
- **Keep the user posted**: a long piece says in a line what it's doing; a new request in
  the middle is noted and done after, unless it's about what's being done.
- **These rules are kept current**: what the user asks more than once becomes a rule here,
  in the file of its topic.
