# 04 — Module de limbaj

## 1. Contractul `LanguageModule`

```rust
// languages (domeniu: fără tree-sitter în semnături)
pub trait Language: Send + Sync {
    fn id(&self) -> LanguageId;                      // "php", "html", "javascript", "css", "scss"
    fn display_name(&self) -> &str;
    fn matchers(&self) -> &[FileMatcher];            // extensii, nume de fișier, glob, shebang
    fn config(&self) -> &LanguageConfig;             // comentarii, perechi de paranteze, indentare, word chars
    fn syntax(&self) -> Option<Arc<dyn SyntaxProvider>>;
    fn injections(&self) -> &[InjectionRule];        // ce alte limbaje pot apărea înăuntru
}

pub enum FileMatcher {
    Extension(&'static str),      // "php", "phtml", "inc"
    FileName(&'static str),       // ".php-cs-fixer.php"
    Glob(&'static str),           // "*.blade.php" (prioritate mai mare)
    Shebang(&'static str),        // "php", "node"
}

pub trait SyntaxProvider: Send + Sync {
    fn parse(&self, text: &TextSnapshot, previous: Option<&SyntaxTree>, edits: &[TextEdit]) -> SyntaxTree;
    fn highlights(&self, tree: &SyntaxTree, range: ByteRange) -> Vec<HighlightSpan>;
    fn folding_ranges(&self, tree: &SyntaxTree) -> Vec<FoldRange>;
    fn indent_for_line(&self, tree: &SyntaxTree, line: u32) -> IndentHint;
}
```

`SyntaxTree` este un tip opac în domeniu. Implementarea concretă se află în `syntax-ts`, adaptorul comun pentru tree-sitter, pe care toate modulele de limbaj îl reutilizează.

## 2. Detecția limbajului

`LanguageResolver` (serviciu de aplicație), în ordinea priorității:
1. **Override manual** (din status bar sau prin `files.associations` în setări).
2. `Glob` (cel mai specific câștigă: `*.blade.php` înaintea lui `*.php`).
3. `FileName` exact.
4. `Extension`.
5. `Shebang` (prima linie).
6. Fallback: `plaintext`.

Dacă două module revendică aceeași extensie, câștigă prioritatea declarată, iar conflictul apare în log și în UI.

## 3. Documente multi-limbaj (punctul critic: PHP)

Un fișier `.php` arată așa:

```php
<?php $title = "Hi"; ?>
<html>
  <style> body { color: red; } </style>
  <script> const x = <?= json_encode($data) ?>; </script>
  <h1><?= $title ?></h1>
</html>
```

### Modelul: **arbore de straturi de limbaj** (Language Layers)

```
Layer 0: PHP (tree-sitter-php, gramatica „php” cu noduri `text` pentru HTML)
 └── Layer 1: HTML (din nodurile `text`, combinate ca un singur document HTML)
      ├── Layer 2: CSS        (conținutul <style> și atributele style="")
      └── Layer 2: JavaScript (conținutul <script> și atributele on*="")
```

- Fiecare limbaj declară `InjectionRule`-uri (inspirate din `injections.scm` din tree-sitter). Exemplu: în HTML, `script_element > raw_text` devine `javascript`, iar `style_element > raw_text` devine `css`.
- PHP declară că nodurile `text` sunt `html`, cu **`combined: true`**: toate fragmentele HTML se parsează ca un singur document, ca tagurile deschise într-un fragment și închise în altul să fie corecte.
- Stratul se găsește cu `LanguageLayers::layer_at(offset)`. Fiecare funcție (highlight, completion, comentare cu Ctrl+/, indentare) **se rutează după limbajul de la poziția cursorului**.
- Parsarea e incrementală pe fiecare strat. La o editare se reparsează doar straturile afectate.

### Rutarea inteligenței

```
cursor la offset X
   → LanguageLayers.layer_at(X) = javascript (în <script> într-un .php)
   → IntelligenceRouter alege provider-ul pentru "javascript"
   → poziția se traduce în coordonatele stratului (range-uri incluse)
```

Pentru PHP, regiunile `<?= ... ?>` din interiorul JS sau HTML rămân PHP. Arborele de straturi gestionează corect și nivelul invers (PHP → HTML → JS, cu PHP înăuntrul JS-ului).

## 3b. Limbaje încorporate: API deschis, contribuit de module

Pe lângă cazurile clasice (CSS în `<style>`, JS în `<script>`), vrem să recunoaștem orice limbaj ajuns în altul, de exemplu **JSON în HTML, JS sau PHP**, și să-l afișăm cu culorile și **folding-ul** (expandare / colapsare) lui. Nimic din toate astea nu e scris fix în nucleu: fiecare regulă vine dintr-un modul, prin contractele de mai jos.

### Contracte (`languages`)

```rust
/// Contribuit de orice modul: găsește regiuni de alt limbaj într-un strat gazdă.
pub trait InjectionProvider: Send + Sync {
    fn id(&self) -> &str;                              // "php.json_decode", "html.script-json"
    fn hosts(&self) -> &[LanguageId];                  // pe ce limbaje rulează (gol = toate)
    fn detect(&self, ctx: &InjectionContext<'_>, out: &mut Vec<Injection>);
}

pub struct InjectionContext<'a> {
    pub host: LanguageId,
    pub tree: &'a SyntaxTree,          // arborele stratului gazdă
    pub text: &'a TextSnapshot,
    pub changed: ByteRange,            // se analizează doar zona modificată
}

pub struct Injection {
    pub range: ByteRange,              // conținutul (fără ghilimele / delimitatori)
    pub language: LanguageId,
    pub source: InjectionSource,       // decide prioritatea la conflict
    pub combined: bool,                // fragmentele se parsează ca un singur document
}

pub enum InjectionSource { Manual, CommentMarker, Declarative, Rule, Sniffed }

/// „Arată textul ăsta a <limbaj>?” Contribuit de modulul limbajului (ex: lang-json).
pub trait ContentSniffer: Send + Sync {
    fn language(&self) -> LanguageId;
    fn sniff(&self, text: &str) -> Option<Confidence>;   // ieftin, cu limite de mărime
}
```

### Cele cinci surse de reguli (în ordinea priorității)

| Sursă | Exemplu | Cine o contribuie |
|-------|---------|-------------------|
| **Manual** | comanda „Inject language…” pe un string (din meniul de acțiuni rapide, Alt+Enter), salvată per proiect | utilizatorul |
| **Comentariu marcaj** | `// language=json` sau `/* language=sql */` înaintea string-ului | nucleul (generic) |
| **Declarativ** | `injections.scm`: `<script type="application/json">`, `application/ld+json`, heredoc `<<<JSON` | modulul limbajului gazdă |
| **Reguli programatice** | argumentul lui `json_decode('…')` (PHP), `JSON.parse('…')` (JS), `response()->json(...)`; reguli din setări: `[[injections]] host = "php", call = "myJsonHelper", arg = 0, language = "json"` | module (inclusiv de framework: Laravel, Symfony) și utilizatorul |
| **Detecție de conținut** | un string care începe cu `{` / `[` și e JSON valid devine strat JSON | `ContentSniffer` din `lang-json` (la fel pentru YAML, SQL etc.) |

### Garanții de performanță
- Analiza rulează **doar pe zona modificată**, pe thread de fundal, iar rezultatul e cache-uit pe versiunea documentului.
- Sniffer-ele au limite stricte: lungime maximă (ex: 256 KB per string), validare ieftină (doar structura, fără alocări mari) și un buget de timp per document.
- Un provider care cade sau depășește bugetul e dezactivat pentru documentul respectiv, iar evenimentul apare în log.

### Afișare și folding
- Stratul injectat primește highlighting-ul, folding-ul (`folds.scm`) și ghidajele lui de indentare.
- Obiectele și array-urile JSON / YAML pot fi colapsate din gutter. Colapsat, un bloc arată `{ 12 chei }` sau `[ 40 elemente ]`.
- Acțiunea „Format fragment” formatează doar stratul injectat (ex: un JSON minificat într-un string PHP), ca un singur pas de undo.
- Ulterior: „Edit fragment” deschide fragmentul într-un tab separat, sincronizat cu sursa.

## 4. Modulele inițiale

| Modul | Gramatică tree-sitter | Extensii | Injecții | Inteligență inițială |
|-------|----------------------|----------|----------|----------------------|
| `lang.php` | `tree-sitter-php` (php) | `.php .phtml .inc .php8` | html (combined), heredoc/nowdoc cu etichetă (`SQL`, `HTML`) | Indexator nativ + LSP opțional (Phpactor / Intelephense) |
| `lang.html` | `tree-sitter-html` | `.html .htm` | css, javascript | Taguri și atribute, Emmet (ulterior) |
| `lang.javascript` | `tree-sitter-javascript` (+ `tree-sitter-typescript` ulterior) | `.js .mjs .cjs .jsx` | — | LSP (`typescript-language-server`) + fallback nativ pe simboluri locale |
| `lang.css` | `tree-sitter-css` | `.css` | — | Proprietăți și valori din date CSS (MDN), variabile `--x` |
| `lang.scss` | `tree-sitter-scss` | `.scss` | — | Variabile, mixin-uri și `@use`/`@import` cross-file |
| `lang.json` | `tree-sitter-json` | `.json .jsonc .lock` (`composer.lock`) | — | Chei / valori colorate, validare, formatare (pretty print) |
| `lang.yaml` | `tree-sitter-yaml` | `.yaml .yml` | — | Chei / valori, ancore, validare, formatare |

Mai târziu: `lang.blade`, `lang.twig`, `lang.sql`, `lang.typescript`, `lang.vue`, `lang.markdown`.

## 5. Structura unui modul de limbaj

```
crates/modules/languages/php/
├── Cargo.toml
├── queries/                 # tree-sitter queries (versionate împreună cu modulul)
│   ├── highlights.scm
│   ├── injections.scm
│   ├── folds.scm
│   ├── indents.scm
│   ├── locals.scm           # scope-uri și definiții locale
│   └── tags.scm             # simboluri pentru indexare
└── src/
    ├── lib.rs               # PhpModule: Module
    ├── language.rs          # PhpLanguage: Language
    ├── intelligence/
    │   ├── indexer.rs       # extragere simboluri (clase, funcții, metode, constante, namespace-uri)
    │   ├── resolver.rs      # rezolvare nume: use-uri, namespace curent, PSR-4 din composer.json
    │   ├── completion.rs
    │   ├── definition.rs
    │   └── references.rs
    └── formatting.rs        # adaptor spre php-cs-fixer / formatter nativ
```
