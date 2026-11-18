# 02 — Arhitectură (Domain-Driven Design)

## 1. Straturi

Fiecare bounded context respectă arhitectura hexagonală (ports and adapters):

```
┌──────────────────────────────────────────────────────────┐
│  app (composition root): leagă totul, singurul binar      │
├──────────────────────────────────────────────────────────┤
│  infrastructure / adapters                                │
│  wgpu, winit, tree-sitter, notify, portable-pty, LSP, fs  │
├──────────────────────────────────────────────────────────┤
│  application                                              │
│  use cases, command handlers, ports (traits)              │
├──────────────────────────────────────────────────────────┤
│  domain                                                   │
│  entități, value objects, agregate, evenimente de domeniu │
│  ZERO dependențe externe (doar std + kernel)              │
└──────────────────────────────────────────────────────────┘
```

**Regula de dependență:** săgețile merg doar în jos. `domain` nu depinde de nimic din afara lui. `application` depinde de `domain` și își definește porturile (traits). `infrastructure` implementează porturile. `app` le instanțiază și face legăturile (dependency injection manuală, fără framework).

Regula este **impusă mecanic** prin structura crate-urilor (un crate de domeniu nu are în `Cargo.toml` dependențe de infrastructură) și verificată în CI cu `cargo-deny` plus un script care validează graful de dependențe (vezi doc 11).

**Regulile verificate** (pasul 36h, ca arhitectura să nu mai alunece):
- `crates/domain/clippy.toml`: domeniul nu are fire, așteptări, mediu, disc (`std::fs`,
  `Path::{exists, is_dir, is_file, metadata, canonicalize}`, `OpenOptions`), ceas
  (`Instant::now`, `SystemTime::now`: primește un ceas, `fn() -> Instant`), programe
  (`process::Command`) sau conexiuni (`std::net`) ale lui: le primește prin porturi.
- `crates/modules/clippy.toml`: modulele nu-și fac fire (nici `thread::scope`); un fir care
  trebuie să fie al lui spune de ce, cu `#[allow]` pe funcție.
- `cargo xtask check-deps` (`xtask/src/main.rs`): un modul folosește doar adaptoarele
  `infra` permise — cele comune (`process`, `files`, `fs-local`, `storage`, `syntax-ts`,
  `image-decode`, în `MODULE_INFRA`) și cele ale funcției lui (`git` pentru `vcs-git`,
  `pty` pentru `terminal-panel`…, în `MODULE_OWNED_INFRA`); un adaptor nou e al aplicației
  până intră în listă.
- `cargo xtask check-deps` (`xtask/src/app_sources.rs`), în sursele aplicației: niciun
  modul numit în afara încărcării (`modules.rs`), nicio comandă (`"git.log"`) sau fir
  (`"git-status"`) al unui modul scris ca text, iar în `app/src/workbench/` niciun adaptor
  (`storage`, `fs_local`): vin prin `Ports`. Codul doar pentru capturi (un `mod` în spatele
  lui `#[cfg(feature = "snapshot")]`) e scutit: binarul livrat nu-l are.
- `cargo xtask check-deps` (`xtask/src/conventions.rs`), în sursele tuturor crate-urilor
  (fără `tests/`): niciun modul de teste scris în fișier (`#[cfg(test)] mod tests { … }`:
  testele stau în `tests/unit`, cu `#[path]`), niciun text cu id-ul aplicației (`"lumen…"`)
  decât un alias pentru ce s-a salvat înainte de o redenumire, spus într-un comentariu chiar
  deasupra, nicio setare declarată în afara `settings.rs`-ului crate-ului (`Setting::new`),
  nicio cheie de traducere numită în cod (`t("tasks.title")`, `tr!("…")`) care să lipsească
  din locale.

## 2. Bounded contexts

| Context | Responsabilitate | Agregate / concepte cheie |
|---------|------------------|---------------------------|
| **Kernel** (shared kernel) | Tipuri comune, minime | `DocumentId`, `ModuleId`, `TextPosition`, `TextRange`, `ByteOffset`, `Uri`, `DomainEvent` |
| **Workspace** | Proiectul deschis, fișierele lui, arborele | `Workspace`, `ProjectRoot`, `FileNode`, `IgnoreRules`, `FileWatchEvent` |
| **Document** | Conținutul text și editarea | `Document` (agregat), `TextBuffer`, `Selection`, `CursorSet`, `Transaction`, `EditHistory` |
| **Language** | Ce limbaj are un fișier sau o regiune, structura sintactică | `LanguageId`, `LanguageModule`, `SyntaxTree`, `LanguageLayer`, `Injection`, `HighlightSpan` |
| **Intelligence** | Simboluri, index, navigare, completare | `Symbol`, `SymbolIndex`, `Reference`, `CompletionItem`, `Diagnostic` |
| **Theming** | Teme și rezolvarea culorilor | `Theme`, `ThemeToken`, `Color`, `FontStyle`, `ThemeRegistry` |
| **Terminal** | Sesiuni de terminal | `TerminalSession`, `TerminalGrid`, `ShellProfile` |
| **Workbench** | Layout-ul și starea UI-ului (fără randare) | `Layout`, `Panel`, `EditorGroup`, `Tab`, `Focus` |
| **Commands** | Comenzi și scurtături | `Command`, `CommandId`, `Keymap`, `KeyChord`, `When` (context de activare) |
| **Settings** | Configurare stratificată | `Settings`, `SettingsLayer` (default → user → proiect), `ModuleSelection` |
| **Extensions** | Module, ciclu de viață, registry | `ModuleManifest`, `ModuleRegistry`, `Capability` |

**Randarea nu e un bounded context de domeniu**, ci infrastructură. Contractele ei (`render-api`) sunt totuși separate și stabile, pentru că driverele se schimbă (vezi doc 06).

### Comunicarea între contexte

- **Sincron, prin porturi:** un context cere ceva altuia printr-un trait definit în `application` (ex: Intelligence cere de la Document un `TextSnapshot`).
- **Asincron, prin evenimente de domeniu:** `DocumentChanged`, `FileCreated`, `ThemeChanged`, `ModuleActivated` circulă pe un **event bus** tipizat din kernel.
- Contextele **nu își partajează entitățile**. Se schimbă ID-uri și DTO-uri imutabile (snapshot-uri), iar fiecare context își păstrează propriul model.

### Context map

```
        Settings ──────────────┐
           │                   ▼
Workspace ─┼──► Document ──► Language ──► Intelligence
           │        │            │               │
           │        ▼            ▼               ▼
           └──► Workbench ◄── Theming       (Completion,
                    │                         References)
                    ▼
                Terminal
         Extensions (registry) ── furnizează module tuturor contextelor
         Commands ── orchestrează acțiuni peste toate contextele
```

## 3. Model de concurență

- **Thread UI/render (main):** event loop-ul `winit` (`ControlFlow::Wait`), layout și construirea frame-ului. Nu face niciodată I/O lung sau parsing greu (măsurat: deschiderea unui proiect ~3 ms, a unui fișier de 21 MB ~23 ms).
- **Workeri lungi care unesc munca** (fire numite, câte unul): sintaxa, simbolurile, diff-ul gutter-ului, indexul, urmărirea discului, transferurile — cu cozile din `kernel::queue` (chei unite, liniște, loturi).
- **Pool-uri mărginite** (`kernel::work`, câte unul per fel de muncă, numit ca să apară în Task Manager): job-urile modulelor, citirile git, blame-ul, imaginile, căutarea utilizărilor. Firele pornesc la nevoie și se termină după 30 s fără lucru; interactivul trece înaintea fundalului; job-urile proiectului se anulează cu el.
- **Fără runtime async în core:** tokio doar în adaptoarele care îl cer (SFTP, Postgres), niciodată chemat de pe firul UI.
- **Comunicare:** canale `std::sync::mpsc` + trezirea buclei UI, evenimentele per proiect și **snapshot-uri imutabile**. Rope-ul (`ropey`) se clonează în O(1), deci indexatorul primește un snapshot și nu blochează editarea.
- **Versionare:** fiecare `Document` are o `DocumentVersion` monotonă. Rezultatele întârziate (highlight, diagnostice) se aplică doar dacă versiunea mai e validă sau poate fi remapată prin edit-urile intermediare.

## 4. Structura workspace-ului Cargo

```
php-editor/
├── Cargo.toml                     # [workspace], versiuni comune în [workspace.dependencies]
├── rust-toolchain.toml
├── deny.toml
├── docs/plan/                     # acest plan
├── crates/
│   ├── kernel/                    # kernel: shared kernel
│   │
│   ├── domain/                    # ─── DOMENIU + APLICAȚIE (pur) ───
│   │   ├── workspace/             # workspace
│   │   ├── document/              # document
│   │   ├── language/              # languages        (contractul LanguageModule)
│   │   ├── intelligence/          # code-intelligence    (contracte completion/nav/refs)
│   │   ├── theming/               # theming
│   │   ├── terminal/              # terminal-session        (model, fără PTY)
│   │   ├── workbench/             # workbench-model       (stare UI, fără randare)
│   │   ├── commands/              # command-model
│   │   ├── settings/              # settings-model
│   │   └── extensions/            # extensions      (Module trait, registry)
│   │
│   ├── render/                    # ─── RANDARE ───
│   │   ├── render-api/            # render-api      (contracte: Renderer, Surface, Scene)
│   │   ├── render-wgpu/           # render-wgpu     (driver principal)
│   │   ├── render-software/       # render-software (tiny-skia: teste headless, fallback)
│   │   └── text/                  # text-layout            (shaping, rasterizare, atlas de glife)
│   │
│   ├── platform/                  # ─── PLATFORMĂ ───
│   │   ├── platform-api/          # platform-api    (Window, Clipboard, Dialogs)
│   │   └── platform-winit/        # platform-winit
│   │
│   ├── ui/                        # ─── UI ───
│   │   ├── ui-kit/                # ui-kit          (widget-uri, layout cu taffy, input)
│   │   └── ui-workbench/          # ui-workbench    (view-uri: tree, editor, tabs, terminal)
│   │
│   ├── infra/                     # ─── ADAPTOARE INFRASTRUCTURĂ ───
│   │   ├── fs-local/              # fs-local        (FileSystem port + notify + ignore)
│   │   ├── syntax-treesitter/     # syntax-ts       (motor de parsing comun, injecții)
│   │   ├── lsp-client/            # lsp-client             (adaptor generic LSP)
│   │   ├── pty/                   # pty             (alacritty_terminal: openpty / ConPTY + VT; shell-urile)
│   │   └── storage/               # storage         (cache index, stare sesiune)
│   │
│   ├── modules/                   # ─── MODULE (plug-and-play) ───
│   │   ├── lang-php/
│   │   ├── lang-html/
│   │   ├── lang-javascript/
│   │   ├── lang-css/
│   │   ├── lang-scss/
│   │   ├── project-tree/          # panoul de arbore e și el modul
│   │   ├── terminal-panel/
│   │   └── themes-builtin/
│   │
│   └── app/                       # lumen (binarul): composition root
│
├── themes/                        # teme livrate (TOML)
├── tests/fixtures/                # proiecte de test (Laravel mini, fișiere mixte PHP/HTML)
└── benches/
```

### Structura internă a unui crate de domeniu (exemplu: `document`)

```
document/src/
├── lib.rs
├── domain/
│   ├── document.rs        # agregatul Document
│   ├── buffer.rs          # trait TextBuffer + RopeBuffer (ropey e acceptat aici, vezi ADR-0004)
│   ├── selection.rs
│   ├── transaction.rs     # Edit, Transaction (compunere, inversare pentru undo)
│   ├── history.rs
│   └── events.rs          # DocumentOpened, DocumentChanged, DocumentSaved
└── application/
    ├── ports.rs           # trait DocumentStorage, trait ClipboardPort
    ├── commands.rs        # InsertText, DeleteSelection, Undo, Save...
    └── service.rs         # DocumentService: orchestrează use case-urile
```

> **Excepție pragmatică:** `ropey` intră în domeniu deoarece structura de date text *este* esența domeniului Document și nu depinde de I/O. Excepția e documentată în ADR-0004 și ascunsă în spatele trait-ului `TextBuffer`.

## 5. Composition root (`app`)

`app` este singurul loc care:
1. citește setările (`Settings`);
2. construiește `ModuleRegistry` și activează modulele selectate;
3. alege driverul de randare (`RendererFactory` în funcție de platformă sau setare);
4. instanțiază serviciile și le injectează porturile;
5. pornește event loop-ul.

```rust
// schiță
fn main() -> anyhow::Result<()> {
    let settings = SettingsLoader::load()?;
    let mut registry = ModuleRegistry::new();
    builtin_modules::register_all(&mut registry);          // toate modulele compilate
    registry.activate(&settings.modules)?;                 // auto sau listă explicită

    let platform = WinitPlatform::new()?;
    let renderer = render_factory::create(&settings.render, &platform)?; // Box<dyn Renderer>

    let app = Application::builder()
        .registry(registry)
        .renderer(renderer)
        .fs(LocalFileSystem::new())
        .build()?;
    platform.run(app)
}
```
