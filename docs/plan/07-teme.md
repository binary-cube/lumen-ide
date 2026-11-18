# 07 — Teme

## 1. Principiu

**Nicio culoare hardcodată în cod.** Fiecare widget cere un **token semantic** (`editor.background`, `tree.item.selected.background`), iar tema îl rezolvă. Temele sunt date, nu cod.

## 2. Modelul de domeniu (`theming`)

```rust
pub struct Theme {
    pub id: ThemeId,
    pub name: String,
    pub appearance: Appearance,              // Light | Dark | HighContrast
    pub extends: Option<ThemeId>,            // moștenire: o temă poate suprascrie doar câteva valori
    pub palette: HashMap<String, Color>,     // culori numite, refolosite prin "$nume"
    pub ui: HashMap<UiToken, Style>,         // tokeni de interfață
    pub syntax: HashMap<Scope, TextStyle>,   // scope-uri de sintaxă (ierarhice)
    pub terminal: AnsiPalette,               // 16 culori ANSI + fg/bg/cursor/selection
    pub fonts: Option<FontOverrides>,
}

pub struct TextStyle { pub fg: Option<Color>, pub bg: Option<Color>, pub bold: bool, pub italic: bool, pub underline: Option<UnderlineStyle> }
```

### Rezolvarea scope-urilor (ierarhic, cu fallback)

`variable.other.member.php` → `variable.other.member` → `variable.other` → `variable` → `editor.foreground`

Rezolvarea se face o singură dată și se cachează într-un `ResolvedTheme` (vector indexat direct). Rezultatul nu se caută în hashmap la fiecare frame.

## 3. Format fișier (TOML)

```toml
# themes/default-dark.toml
name = "Default Dark"
appearance = "dark"

[palette]
bg0 = "#1e1f22"
bg1 = "#2b2d30"
fg  = "#bcbec4"
accent = "#3574f0"
orange = "#cf8e6d"
purple = "#c77dbb"
green  = "#6aab73"

[ui]
"window.background"              = "$bg0"
"editor.background"              = "$bg0"
"editor.foreground"              = "$fg"
"editor.line.current.background" = "#26282e"
"editor.gutter.foreground"       = "#4b5059"
"editor.selection.background"    = "#214283"
"editor.cursor"                  = "$fg"
"panel.background"               = "$bg1"
"tree.item.selected.background"  = "#2e436e"
"tab.active.border"              = "$accent"
"statusbar.background"           = "$bg1"

[syntax]
"keyword"          = { fg = "$orange" }
"string"           = { fg = "$green" }
"variable.php"     = { fg = "$purple" }
"function.method"  = { fg = "#56a8f5" }
"comment"          = { fg = "#7a7e85", italic = true }
"tag.html"         = { fg = "#d5b778" }

[ui]
# ... culorile interfeței, apoi ale terminalului (fundal, text, cursor, cele 16 ANSI):
"terminal.background"    = "$card"
"terminal.foreground"    = "$code"
"terminal.cursor"        = "$accent"
"terminal.ansi.black"    = "#1e1f22"
"terminal.ansi.red"      = "#f75464"
# ... green, yellow, blue, magenta, cyan, white, brightBlack … brightWhite
# (Setări → Terminal → Culorile: ale temei / clasice / contrast mare — doar terminalul.)
```

### Decorațiunile (planul 41)

Pe lângă culori și `[caret]`, o temă poate declara decorațiuni — o listă de straturi
(`[[decorations.layers]]`, fiecare cu `kind` = `garland` / `particles` / `corners` / `glow` și
opțiunile felului), validate la citire; lista înlocuiește pe cea a temei extinse (formatul
întâi, `[decorations.garland]`, `.snow`, `.corners`, se citește mai departe, cu moștenirea pe
fel). Le desenează modulul `theme-decorations`, doar când e ceva de desenat; utilizatorul își
poate pune și decorațiunile lui (o presetare sau straturile din studio) peste orice temă.
Formatul complet: [41-decoratiuni-teme.md](41-decoratiuni-teme.md).

### Opțiunile temei (planul 41e)

O temă își poate declara propriile setări (`[[options]]`: comutator, alegere, glisor, culoare,
cu etichete pe limbi), arătate doar cât e activă (Setări → Aspect, studioul); o alegere
schimbă nume din `[palette]` (tokenii scriși `$nume` le urmează), un glisor sau o culoare dau
valoarea unui câmp de decorațiune (`"@id"`), oricare poate arăta / ascunde un strat (`when`).
Valorile sunt ale utilizatorului, per temă, în setări. Detalii: planul 41.

## 4. Teme livrate

- **Default Dark** (tema întunecată implicită)
- **Default Light** și **Midnight** (numele temelor livrate nu poartă numele aplicației)
- Ulterior: High Contrast, și un importer pentru teme în formatul JSON al VS Code (`.json`), pentru că sunt multe disponibile.

## 5. Funcționalități

- Hot reload: modificarea fișierului de temă reaplică tema imediat (prin watcher).
- Tema poate urma setarea de sistem (light/dark automat).
- Temele pot veni din: modulul `themes-builtin`, directorul utilizatorului (`~/.config/lumen/themes/`) și, ulterior, din pluginuri.
- Validare: tokenii necunoscuți generează avertismente, iar tokenii lipsă primesc fallback din tema de bază. Tema nu poate „sparge” UI-ul.
- **Contract de tokeni versionat:** lista oficială de `UiToken` e generată și documentată (`docs/theme-tokens.md`, generat din cod), ca autorii de teme să știe ce pot stiliza.

## 6. Și dimensiunile (design tokens)

Pe lângă culori, tema (sau un „design profile” separat) definește spacing, radius, înălțimea rândurilor din arbore și dimensiunile fonturilor UI. Astfel se pot face variante „compact” și „comfortable” fără cod nou.
