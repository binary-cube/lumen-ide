# 05 — Language intelligence

## 1. Contracte (porturi în `code-intelligence`)

```rust
pub trait CompletionProvider: Send + Sync {
    fn trigger_characters(&self) -> &[char];                // PHP: '$', '>', ':', '\\'
    async fn complete(&self, req: CompletionRequest) -> Result<CompletionList>;
    async fn resolve(&self, item: CompletionItem) -> Result<CompletionItem> { Ok(item) } // documentație leneșă
}

pub trait DefinitionProvider: Send + Sync {
    async fn definition(&self, at: DocumentPosition) -> Result<Vec<Location>>;
}

pub trait ReferencesProvider: Send + Sync {
    async fn references(&self, at: DocumentPosition, include_declaration: bool)
        -> Result<ReferenceStream>;                         // stream: rezultatele apar progresiv în UI
}

pub trait FormattingProvider: Send + Sync {
    async fn format(&self, doc: TextSnapshot, range: Option<TextRange>, opts: FormatOptions)
        -> Result<Vec<TextEdit>>;
}

pub trait DiagnosticsProvider: Send + Sync { /* push prin evenimente */ }
pub trait SymbolsProvider: Send + Sync { /* outline + workspace symbols */ }
```

Notă: în codul real folosim `async-trait` sau trait-uri care returnează `BoxFuture`, pentru că avem nevoie de `dyn`.

Toate cererile primesc un **`CancellationToken`**. Când utilizatorul tastează din nou, cererea veche se anulează.

## 2. Două tipuri de implementări pentru același contract

| Adaptor | Avantaje | Dezavantaje |
|---------|----------|-------------|
| **Nativ** (tree-sitter + index propriu) | Rapid, fără dependențe externe, control total, funcționează offline | Mult de muncă pentru semantică completă (tipuri, inferență) |
| **LSP** (`lsp-client`, proces extern) | Semantică matură (Intelephense, Phpactor, tsserver, vscode-css-ls) | Dependență externă, latență IPC, configurare |

`IntelligenceRouter` combină rezultatele: pentru fiecare limbaj se configurează ordinea (`native`, `lsp`, sau ambele cu merge). **Strategia recomandată:** pornim cu **LSP pentru JS/CSS/SCSS** (rezultat bun rapid) și cu **indexator nativ pentru PHP** (diferențiatorul produsului), iar LSP-ul PHP rămâne disponibil ca opțiune.

## 3. Evidențierea sintaxei (syntax highlighting)

```
DocumentChanged(edits, version)
  → SyntaxService (thread de fundal): parse incremental pe straturi
  → highlights.scm → capturi ("function.method", "variable.builtin", "string")
  → HighlightSpan { range, scope: Scope("function.method") }
  → Theming: scope → stil (culoare, bold, italic), cu fallback ierarhic:
        "function.method.call" → "function.method" → "function" → default
  → UI: doar liniile vizibile (+ un buffer) se trimit la randare
```

- **Highlight doar pentru viewport.** Parsarea e completă și incrementală, dar interogarea `highlights` rulează doar pe intervalul vizibil.
- Până sosește parse-ul nou, se folosesc span-urile vechi, ajustate prin edit-uri, ca să nu apară „flicker” de culori.
- **Semantic highlighting** (strat peste cel sintactic): culori diferite pentru proprietăți, parametri sau constante, pe baza rezolvării din index.

## 4. Formatarea codului

- Port `FormattingProvider`. Adaptoare:
  - `ExternalFormatter`: rulează un proces (`php-cs-fixer`, `pint`, `prettier`) pe stdin/stdout și calculează diff-ul minim (`similar`) ca listă de `TextEdit`, ca să păstreze cursorul și undo-ul.
  - `NativeFormatter` (ulterior): formatter bazat pe arbore, pentru CSS/SCSS (simplu și sigur).
  - `LspFormatter`.
- Format on save, opțional, per limbaj.
- Pentru `.php` multi-limbaj, formatarea se aplică pe straturi (PHP prin php-cs-fixer, iar blocurile `<style>` și `<script>` opțional prin prettier).

## 5. Indexatorul nativ PHP (piesa centrală)

### Model de domeniu

```rust
pub struct Symbol {
    pub id: SymbolId,
    pub kind: SymbolKind,          // Namespace, Class, Interface, Trait, Enum, Function, Method,
                                   // Property, Constant, ClassConstant, EnumCase, Variable, Parameter
    pub fqn: FullyQualifiedName,   // \App\Http\Controllers\UserController::store
    pub location: Location,
    pub visibility: Option<Visibility>,
    pub modifiers: Modifiers,      // static, abstract, final, readonly
    pub signature: Option<Signature>,
    pub doc: Option<DocBlock>,     // PHPDoc: @param, @return, @var, @property, @method
    pub container: Option<SymbolId>,
}

pub struct Reference {
    pub target: SymbolRef,         // nerezolvat (nume) sau rezolvat (SymbolId)
    pub location: Location,
    pub kind: ReferenceKind,       // Call, New, Extends, Implements, Use, TypeHint, StaticAccess, Read, Write
}
```

### Pipeline

```
1. Discovery    : listarea fișierelor (ignore crate: .gitignore + excluderi din setări)
2. Prioritizare : fișierele proiectului înainte de vendor/; fișierele deschise primele
3. Parse        : rayon, tree-sitter, tags.scm → Symbol + Reference brute
4. Rezolvare    : namespace curent + use-uri → FQN; composer.json (PSR-4/PSR-0/classmap/files)
5. Stocare      : index în memorie (HashMap FQN → Symbol, invers SymbolId → Vec<Reference>)
                  + cache persistent pe disc (storage, ex: redb), invalidat pe hash de conținut
6. Incremental  : FileChanged → reindexarea fișierului → diff de simboluri → evenimente
```

- `vendor/` se indexează doar pentru **declarații** (nu și pentru referințe), pentru economie, și se reindexează doar când se schimbă `composer.lock`.
- Stubs pentru funcțiile și clasele built-in PHP (din `JetBrains/phpstorm-stubs`, licență Apache-2.0) se livrează cu modulul.

### Funcționalități

| Funcție | Implementare |
|---------|--------------|
| **Autocomplete** | Context din arborele sintactic: după `$this->` sunt membrii clasei curente și ai părinților; după `Foo::` sunt membrii statici și constantele; în poziție de tip sunt clasele/interfețele cu auto-`use`; variabilele locale vin din `locals.scm`. Ranking: potrivire fuzzy, apoi proximitate (fișier, namespace), apoi frecvență. |
| **Go to definition** | Nodul de sub cursor → nume → rezolvare FQN → `Symbol.location`. Pentru membri: tipul receiver-ului (inferență simplă: `new`, type hint, `@var`, return type, PHPDoc). |
| **Find usages** | Index invers `SymbolId → [Reference]`. Rezultatele vin ca stream și sunt grupate pe fișier, apoi afișate într-un panou dedicat. |
| **Diagnostice** | Faza 1: erori de sintaxă (nodurile `ERROR` din tree-sitter). Faza 2: clase sau funcții nedefinite, `use` nefolosite. |

### Inferența de tipuri (gradual)

Inferența este cea mai grea parte. O abordăm incremental:
1. **N1:** tipuri explicite (type hints, return types, `new X`, `@var`).
2. **N2:** propagare locală prin atribuiri, `instanceof`, `@return` din PHPDoc, `self`/`static`/`parent`.
3. **N3:** generics din PHPDoc (`@template`, `Collection<User>`), facades și magic Laravel (prin pluginuri de framework, mai târziu).

## 6. Go to file / simboluri în workspace

- Index fuzzy (nucleo) pe căile fișierelor și pe FQN-urile simbolurilor.
- „Search Everywhere” (Shift Shift) combină fișiere, clase, simboluri și comenzi.
