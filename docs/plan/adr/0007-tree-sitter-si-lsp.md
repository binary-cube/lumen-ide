# ADR-0007: tree-sitter pentru sintaxă; indexator nativ PHP + LSP

**Status:** Acceptat (2026-10-19)

## Context
Avem nevoie de parsing incremental, rapid și tolerant la erori, cu suport pentru limbaje imbricate (PHP → HTML → JS/CSS). Inteligența semantică e scumpă de construit.

## Decizie
- **tree-sitter** pentru toate limbajele inițiale (gramatici mature pentru php, html, javascript, css și scss), cu sistem de straturi și injecții.
- **PHP:** indexator nativ (diferențiatorul produsului), cu LSP (Phpactor / Intelephense) ca alternativă configurabilă prin același contract.
- **JS/CSS/SCSS:** LSP la început (typescript-language-server, vscode-css-languageserver), cu providere native adăugate unde aduc valoare.

## Consecințe
+ Highlighting bun din faza 4, iar inteligență JS/CSS rapid prin LSP.
− Pentru LSP, utilizatorul are nevoie de Node instalat. Se pot livra binarele împreună cu aplicația sau se pot descărca la prima utilizare.
