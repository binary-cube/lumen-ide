# tree-sitter-blade (vendored)

The Blade (Laravel) grammar for tree-sitter, copied from
<https://github.com/EmranMR/tree-sitter-blade> (commit
`b5291d1ba207a8ebb8383b2ecb8a8a6535210a50`, version 0.12.2), MIT license (see `LICENSE`).

Included in the repo because it isn't published on crates.io. Used by
`crates/modules/languages/blade`. To update: copy `src/`, `queries/`, `bindings/rust/` and
`grammar.js` again, then apply the local patch below. Not a workspace member (see
`exclude` in the root `Cargo.toml`), so it isn't under the project's lints.

Local patch (keep it when updating): in `src/scanner.c`'s `scan_comment`, the `'>'` case
that isn't a comment's end resets `dashes` and `break`s itself instead of falling through
into `default` (the same behavior; GCC's `-Wimplicit-fallthrough`, on in the Windows
build, warned about it).
