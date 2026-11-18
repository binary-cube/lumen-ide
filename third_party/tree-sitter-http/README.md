# tree-sitter-http (vendored)

The grammar of HTTP request files for tree-sitter (`.http`: requests separated by `###`,
their headers and bodies — JSON, XML, GraphQL injected —, `@name = value` variables,
`{{variable}}` references, comments with `@name`-style directives, `< {% %}` / `> {% %}`
scripts, `< ./file` bodies), from <https://github.com/rest-nvim/tree-sitter-http>: commit
`db8b4398de90b6d0b6c780aba96aaa2cd8e9202c` (2025-03-29), MIT license (see `LICENSE`). It
isn't published on crates.io in this version.

`src/` (`parser.c`, generated with ABI 14, which the workspace's `tree-sitter` 0.27 reads),
`queries/`, `grammar.js` and `LICENSE` are copied unchanged; `bindings/rust/` is written here
(as the upstream's, without its test module: the editor's module tests the grammar instead).
There is no external scanner. The editor's own queries live in
`crates/modules/languages/http/queries/`.

Used by `crates/modules/languages/http` (`lang-http`). To update: take `src/`, `queries/`,
`grammar.js` of a new commit. Not a workspace member (see `exclude` in the root
`Cargo.toml`), so it isn't under the project's lints.
