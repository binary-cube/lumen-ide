# tree-sitter-twig (vendored)

Gramatica Twig pentru tree-sitter, copiată neschimbată din
<https://github.com/gbprod/tree-sitter-twig> (commit `2208d2a3c3ee7ef378e97df2e51c18feb7ee9dfc`,
versiunea 1.1.0), licență MIT (vezi `LICENSE`).

Inclusă în repo pentru că nu e publicată pe crates.io. Folosită de `crates/modules/languages/twig`.
Pentru actualizare: se copiază din nou `src/`, `queries/`, `bindings/rust/` și `grammar.js`.
Nu e membru al workspace-ului (vezi `exclude` în `Cargo.toml`-ul rădăcină), deci nu intră
sub lint-urile proiectului.
