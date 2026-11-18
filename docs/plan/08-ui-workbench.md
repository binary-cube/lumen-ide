# 08 — UI și workbench

## 1. Layout (organizarea clasică de IDE, mai aerisită)

```
┌────────────────────────────────────────────────────────────────────────────┐
│ ● ● ●   php-editor ▾        main ▾                       🔍  ⚙            │  ← title bar (custom, integrat)
├──┬──────────────────────┬──────────────────────────────────────────────────┤
│📁│ PROJECT           ⋯  │ UserController.php ×  │ app.scss ×  │ index.js × │  ← taburi
│🔍│ ▾ app                │ ──────────────────────────────────────────────── │
│⎇ │   ▾ Http             │  12  class UserController extends Controller     │
│  │     ▸ Controllers    │  13  {                                           │
│  │   ▸ Models           │  14      public function store(Request $r)       │
│  │ ▸ config             │  15      {                                       │
│  │ ▸ resources          │  16          $user = User::cr│                   │
│  │   composer.json      │                       ┌──────────────────┐       │
│  │                      │                       │ ƒ create()  Model│       │  ← autocomplete
│  │                      │                       │ ƒ createMany()   │       │
│  │                      │                       └──────────────────┘       │
├──┴──────────────────────┴──────────────────────────────────────────────────┤
│ TERMINAL   zsh ×  │ php artisan serve ×  │ +                          ⌄ ×  │  ← terminal cu taburi
│ ~/project $ composer install                                               │
├────────────────────────────────────────────────────────────────────────────┤
│ PHP 8.3 │ UTF-8 │ LF │ 4 spaces │ Ln 16, Col 30 │ ⎇ main │ ⟳ Indexing 72%  │  ← status bar
└────────────────────────────────────────────────────────────────────────────┘
```

- **Tool window bar** (stânga, îngust): icoane pentru Project, Search, Git (ulterior), Structure.
- Panourile se pot redimensiona, ascunde (Cmd/Ctrl+1 pentru Project, Alt+F12 pentru Terminal) și muta (stânga/dreapta/jos).
- Layout-ul se salvează per proiect (`.lumen/workspace.json`).

### Direcția de design („mai light și mai frumos”)
- Mai puține borduri: separarea se face prin nuanțe de fundal, nu prin linii.
- Spațiere generoasă, colțuri rotunjite subtile (4–6 px), umbre doar pentru popup-uri.
- Icoane SVG monocrome, colorate prin temă.
- Animații scurte (100–150 ms) pentru deschiderea panourilor și popup-urilor, care pot fi dezactivate.
- Un singur font de UI (Inter sau fontul sistemului) și un font de cod (JetBrains Mono implicit).

## 2. Arhitectura UI

```
workbench-model (domeniu): stare pură
   Layout, PanelState, EditorGroups, Tabs, Focus, ActiveContext
        ▲ comenzi                 │ evenimente
        │                         ▼
ui-workbench: view-uri care citesc starea și produc elemente
   ProjectTreeView, EditorView, TabBarView, TerminalView, StatusBarView, PaletteView
        │
ui-kit: framework UI propriu, minimal
   Element tree (retained), layout cu `taffy` (flexbox), hit-testing, focus, input,
   scroll, virtualizare liste, text input, popup/overlay, animații
        │
render-api: Scene
```

**De ce un UI kit propriu și nu egui/iced/Slint:** editorul are cerințe specifice (text cu milioane de linii, virtualizare, randare custom de glife, teme complete, latență minimă). Framework-urile generale fie nu ne dau control pe pipeline-ul de text, fie impun propriul renderer. Alte editoare native scrise în Rust au ajuns la aceeași concluzie, cu kit-uri UI proprii. Kit-ul e mic și orientat strict pe nevoile noastre (vezi ADR-0005).

- Model: **retained element tree** + **reactive state** (signals simple). Se recalculează doar ce s-a schimbat.
- **Virtualizare** obligatorie pentru arbore, editor, terminal și liste de rezultate: se construiesc doar elementele vizibile.

## 3. Editor view

- Gutter: numere de linie, markeri de fold, markeri git, breakpoint-uri (viitor), icoane de diagnostic.
- Zona de text: linia curentă, selecții, multi-cursor, bracket matching, indent guides, squiggles pentru diagnostice, inlay hints (viitor).
- Minimap: opțional, mai târziu.
- Scroll fluid (scrolling pe pixeli, inerție pe trackpad), „scroll past end”.
- Sticky scroll (antetul clasei sau funcției curente fixat sus): ulterior.

## 4. Arborele proiectului (modulul `project-tree`)

- Sursa de date: `Workspace` (domeniu) + `FileSystem` port (`fs-local`, cu `notify` pentru watch și `ignore` pentru `.gitignore`).
- Încărcare leneșă a directoarelor (se citește la expandare), deci fără scanare completă la pornire.
- Acțiuni: new file/folder, rename, delete (în coș, prin crate-ul `trash`), copy path, reveal in Finder/Explorer, „Select opened file” (autoscroll).
- Icoane per tip de fișier, furnizate de modulele de limbaj (`Language::icon()`).
- Filtrare rapidă la tastare (speed search: tastezi direct în arbore și se filtrează).

## 5. Comenzi și scurtături

- Orice acțiune e o `Command` înregistrată (inclusiv cele din module). Meniurile, paleta și scurtăturile folosesc același registry.
- Keymap-uri: „Lumen Default”, „JetBrains” (pentru a fi familiar), „VS Code”. Configurabil în `keymap.toml`.
- Condiții `when` (ex: `editorFocus && language == php`).
