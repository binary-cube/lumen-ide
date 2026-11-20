# How code is written here, and how modules talk

The rules of `CLAUDE.md`, made concrete by what has been agreed while working on the
project. Read with it; when they differ, `CLAUDE.md` wins.

The rules are split by topic, each file linking the ones it touches. Read the one for what
you're about to do; when two differ, the more specific wins, and `CLAUDE.md` over all.

| Topic | File |
|---|---|
| How code is written (language, comments, names, files, tests, hostile input) | [code.md](code.md) |
| How modules talk (the four ways, registries, contracts) | [modules.md](modules.md) |
| The components to build views from, tables, menus, the status bar, settings | [ui-components.md](ui-components.md) |
| Modals and windows over the editor (footer buttons and keys, closing, progress) | [modals.md](modals.md) |
| The UI thread, budgets, caching, locks | [performance.md](performance.md) |
| The log (`tracing`, fields, levels, secrets) | [logging.md](logging.md) |
| Privacy: no telemetry, anonymous recordings, consent | [privacy.md](privacy.md) |
| Working on a step (plans, gate, commits, review, snapshots, replies) | [workflow.md](workflow.md) |
| Features with rules of their own (HTTP client, diagrams) | [features.md](features.md) |

When the user asks for the same thing a second time, it becomes a rule in the file of its
topic (a new file, linked from here, when no topic fits).
