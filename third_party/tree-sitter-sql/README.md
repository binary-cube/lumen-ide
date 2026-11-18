# tree-sitter-sql (vendored)

The general / permissive SQL grammar for tree-sitter (PostgreSQL, SQLite, MySQL and more),
from <https://github.com/DerekStride/tree-sitter-sql>: version 0.3.11 as published on
crates.io (`tree-sitter-sequel`, commit `7b51ecda191d36b92f5a90a8d1bc3faef1c7b8b8`), MIT
license (see `LICENSE`, taken from that commit: the crate doesn't ship it).

`src/parser.c` (and `grammar.json`, `node-types.json`, `src/tree_sitter/*.h`) were
regenerated from the unchanged `grammar.js` with `tree-sitter generate --abi 15`
(tree-sitter CLI 0.27.1), for the workspace's `tree-sitter` 0.27. `queries/` and
`bindings/rust/` are copied unchanged (only the crate's name in the binding's doc example:
`tree_sitter_sql`), `src/scanner.c` with one local patch (below). The editor's own queries
live in `crates/modules/languages/sql/queries/`.

Used by `crates/modules/languages/sql` (`lang-sql`). To update: take `grammar.js`,
`src/scanner.c`, `queries/` and `bindings/rust/` of a new release, regenerate `src/` with
the same command, then apply the local patch again. Not a workspace member (see `exclude`
in the root `Cargo.toml`), so it isn't under the project's lints.

Local patch (keep it when updating): in `src/scanner.c`, the dollar-quoted tag's index
(`add_char`'s `index` and `scan_dollar_string_tag`'s counter) is a `size_t` instead of an
`int`, as the buffer size it's compared with (`index + 1 >= *text_size`: `-Wsign-compare`
warned on every build).
