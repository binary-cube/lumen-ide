# tree-sitter-dotenv (vendored)

The dotenv grammar for tree-sitter (`.env` files: `KEY=value`, `export`, quoted values,
`$VAR` / `${VAR}` interpolation, comments), from <https://github.com/pnx/tree-sitter-dotenv>:
version 1.1.3, commit `b4f0b0d154a348d4ae57dfc4d37f128559fbde6d`, MIT license (see
`LICENSE`). It isn't published on crates.io.

`src/parser.c` (and `grammar.json`, `node-types.json`) were regenerated from the patched
`grammar.js` (below) with `tree-sitter generate --abi 15` (tree-sitter CLI 0.27.1), for the
workspace's `tree-sitter` 0.27. `queries/` and `bindings/rust/build.rs` are copied
unchanged, `src/scanner.c` with a local patch (below), `bindings/rust/lib.rs` without its
test module (it needs `tree-sitter` as a dev-dependency: the editor's module tests the
grammar instead). The editor's own queries live in
`crates/modules/languages/dotenv/queries/`.

Used by `crates/modules/languages/dotenv` (`lang-dotenv`). To update: take `grammar.js`,
`src/scanner.c`, `queries/` and `bindings/rust/` of a new release, apply the local patches
again, regenerate `src/` with the same command. Not a workspace member (see `exclude` in the
root `Cargo.toml`), so it isn't under the project's lints.

Local patches (keep them when updating), each marked `Local patch` in `grammar.js`:

- double-quoted values: escapes (`\"`, `\n`, `\\`) are `escape_sequence` nodes, and a `$`
  that starts no name (`"costs $5"`) is text — upstream, `\"` ended the value and a lone
  `$` was a syntax error;
- `${NAME:-default}` (and `-`, `:=`, `=`, `:?`, `?`, `:+`, `+`): the operator and the
  default are `operator` / `default` nodes inside the `variable` — upstream, only `${NAME}`;
- `0` is a number (upstream, a decimal had to start with `1`–`9`).

And in `src/scanner.c`: the parameters its functions don't use are cast to `void`, and
`advanceWS` is declared `static void` (upstream `void static`) — `-Wunused-parameter` warned
on every build.
