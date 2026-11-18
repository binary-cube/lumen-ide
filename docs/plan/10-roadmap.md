# 10 — Roadmap

Fiecare fază se încheie cu ceva **rulabil și demonstrabil**. Estimările sunt pentru o persoană cu normă întreagă asistată de AI și sunt orientative.

## Faza 0: Fundație (≈1 săptămână) ✅ local, în așteptarea CI
- [x] Workspace Cargo cu structura din doc 02 (crate-uri goale, dar cu dependențele corecte).
- [x] `rust-toolchain.toml` (Rust 1.99, ediția 2024).
- [x] CI (GitHub Actions): `fmt`, `clippy -D warnings`, `test`, `cargo-deny`, build pe macOS/Windows/Linux.
- [x] Script de verificare a regulilor de dependență DDD (`cargo xtask check-deps`).
- [x] `kernel`: ID-uri, poziții, range-uri, event bus.
- [x] ADR-urile 0001–0007 scrise și acceptate.

**Gata când:** `cargo build` trece pe toate cele 3 platforme în CI. (Local, pe macOS, trec build, clippy, teste și check-deps; CI rulează după primul push.)

## Faza 1: Fereastră și text pe GPU (≈2–3 săptămâni) ✅ (validare manuală 120 FPS în curs)
- [x] `platform-api` + `platform-winit`: fereastră, input, DPI scaling.
- [x] `render-api` (contracte) + `render-wgpu` (quads + glyphs) + `render-software` (snapshot tests).
- [x] `text-layout`: încărcare fonturi, shaping, atlas de glife.
- [x] Afișarea unui fișier read-only cu scroll fluid.
- [x] Overlay de debug (F12: timp de frame, statistici de randare).
- [x] `lumen --snapshot out.png <fișier>`: randare headless cu driverul software.

- [x] Deschidere fișier (Cmd/Ctrl+O) și proiect (Cmd/Ctrl+Shift+O): dialoguri native, meniu nativ pe macOS, `lumen <fișier|folder>`.
- [x] Arbore de proiect minimal (adus din faza 3): încărcare leneșă, directoare întâi, click pentru expandare / deschidere.

**Gata când:** un fișier de 100k linii se derulează la 120 FPS pe un MacBook, iar snapshot test-ul trece în CI pe backend software.

## Faza 2: Editare (≈2–3 săptămâni) ✅ (validare manuală a latenței în curs)
- [x] `document`: rope, tranzacții, undo/redo grupat pe cuvinte, multi-cursor, selecții.
- [x] Input: tastare, IME (diacritice, taste moarte, CJK), navigare (grafem, cuvânt, linie, document, pagină), clipboard.
- [x] Mouse: click, Shift+click, Alt+click (cursor nou), dublu-click (cuvânt), triplu-click (linie), drag.
- [x] Deschidere și salvare (port `DocumentStorage`, scriere atomică), detecție encoding (UTF-8/BOM/UTF-16/Latin-1) și LF/CRLF.
- [x] **Taburi multiple** (adus din faza 3): fiecare tab cu documentul, undo-ul, scroll-ul și căutarea lui; confirmare la închiderea fișierelor modificate.
- [x] Find / Replace per tab: text sau regex, case, cuvânt întreg, contor, replace one / all (un singur pas de undo).
- [x] Comenzi și scurtături dintr-un singur registru (`command-model`): meniul nativ macOS și keymap-ul se generează din aceeași listă.
- [x] Status bar: poziție, selecție, encoding, terminator de linie, indentare.
- [x] Property-based tests (`proptest`) pentru tranzacții și undo.
- [x] Overlay F12: latența input → frame (p95).

**Gata când:** poți edita confortabil un fișier, iar latența tastă → pixel e sub 8 ms la p95 (se citește din overlay-ul F12).

## Faza 3: Workbench și teme v1 (≈3 săptămâni)
- [ ] `ui-kit`: layout (taffy), focus, hit-test, liste virtualizate, popup-uri.
- [ ] `workbench-model` + `ui-workbench`: panouri, editor groups, taburi, status bar.
- [ ] Modulul `project-tree` (lazy load, watcher, `.gitignore`, operații de fișier).
- [x] `theming` + `themes-builtin` (Lumen Dark, Lumen Light, TOML), teme ale utilizatorului din `~/.config/lumen/themes`, urmarea temei sistemului, comanda Switch Theme.
- [ ] Hot reload pentru fișierele de temă.
- [x] Design: card principal rotunjit, arbore cu selecție tip „pastilă”, hover și ghidaje de indentare.
- [x] Splash screen la pornire (minimum 0,9 s, cu estompare; `EDITOR_NO_SPLASH=1` îl dezactivează).
- [ ] `command-model`: registry, command palette, keymap, Go to file (fuzzy).
- [ ] `settings-model`: straturi default → user → proiect.
- [ ] `extensions`: `Module` trait, registry, activare auto sau explicită.

**Gata când:** deschizi un folder, navighezi în arbore, deschizi mai multe taburi, schimbi tema live, iar modulele se pot activa sau dezactiva din `settings.toml`.

## Faza 4: Module de limbaj și highlighting ✅
- [x] Sistemul de module (`extensions`): contractul `Module`, contribuții tipizate (orice crate poate defini un tip nou de contribuție), activare izolată (panic → modul marcat `Failed`), dependențe între module, selecție din `~/.config/lumen/settings.toml` (`[modules] mode / enabled / disabled`).
- [x] `languages`: descriptori de limbaj, detecție (glob → nume → extensie → shebang → detectoare de conținut), regiuni de folding.
- [x] `syntax-treesitter`: straturi de limbaj, injecții din 4 surse (declarative, reguli din module, marcaje `// language=json`, detectoare de conținut), parsare pe thread separat, **incrementală pe straturi** (o tastă într-un fișier PHP de 100k linii: ~45 ms în loc de ~1 s), highlight doar pe zona vizibilă.
- [x] Module: `lang-php`, `lang-html`, `lang-javascript`, `lang-css`, `lang-scss`, `lang-json`, `lang-yaml`.
- [x] JSON recunoscut în HTML (`<script type="application/json">`, `ld+json`, atribute), JS (`JSON.parse`), PHP (`json_decode`, heredoc `<<<JSON`) și oriunde prin marcaj sau detecție de conținut.
- [x] Folding pentru toate limbajele (PHP, HTML, JS, CSS, SCSS, JSON, YAML, inclusiv în straturile încorporate), cu etichete `12 chei` / `3 elemente`; săgeți în gutter, ⌘- / ⌘= / ⌘⇧- / ⌘⇧=, ↑/↓ sar peste regiunile colapsate.
- [x] Culori din temă, cu scope-uri specifice limbajului (`property.json`, `variable.php`) și fallback ierarhic.
- [x] Comentare cu ⌘/ după limbajul de la cursor (HTML din PHP → `<!-- -->`, JS → `//`, CSS → `/* */`).
- [x] `.editorconfig` (adus mai devreme).
- [x] Test de echivalență: parsarea incrementală dă exact rezultatul parsării complete (straturi, limbaje, folding) pe 5 tipuri de editări.
- [ ] Indentare automată pe baza arborelui (`indents.scm`) — momentan se păstrează indentarea liniei anterioare + bloc după `{`.
- [ ] Italic / bold din temă (comentarii italice) — necesită variante de font în renderer.

## Între faze (cerințe noi) ✅
- [x] Săgeți de folding vizibile (triunghiuri vectoriale, accent pentru regiunile colapsate, fundal la hover).
- [x] Consumul ferestrei în status bar: CPU, RAM (sysinfo, la 2 s, fără redesenări inutile; oprit cât toate ferestrele sunt ascunse) și GPU (memoria alocată de renderer).
- [x] Proiecte multiple în ferestre separate: New Window (⇧⌘N), „Open Project” întreabă „Fereastra aceasta / Fereastră nouă” (ADR-0008).

## Faza 4b: Module pentru framework-uri
- [ ] `lang-twig`: gramatică Twig, HTML încorporat, folding pe blocuri (`{% block %}`, `{% if %}`, `{% for %}`).
- [ ] `lang-vue`: componente `.vue` cu `<template>` (HTML + expresii), `<script>` (JS / TS), `<style>` (CSS / SCSS după `lang`).
- [ ] React: JSX / TSX (`lang-typescript` + extensiile `.jsx` / `.tsx`).
- [ ] `htmx`: modul care extinde HTML (atribute `hx-*` evidențiate, JS în `hx-on:*`); autocomplete pentru atribute în faza 6.

: Terminal (≈2 săptămâni)
- [x] `terminal-session` + `pty` (alacritty_terminal: PTY + emulator, ConPTY pe Windows).
- [x] Modulul `terminal-panel`: taburi, randare GPU, paleta din temă, pornit în rădăcina proiectului.
- [ ] Selecție cu mouse-ul și copiere din terminal; link-uri `fișier:linie` clicabile.
- [ ] Link-uri `fișier:linie` deschise în editor.

**Gata când:** rulezi `vim`, `htop` și `php artisan serve` în taburi separate, fără artefacte.

## Faza 6: Language intelligence (≈6–8 săptămâni, cea mai mare fază)
- [ ] `code-intelligence`: contracte, router, anulare de cereri, panoul „Find usages”.
- [x] **Autocomplete contribuit de fiecare modul** (contribuție `CompletionProvider`): PHP (clase, metode, variabile, funcții built-in), HTML (taguri, atribute), CSS / SCSS (proprietăți, valori, variabile), JS, JSON (chei din schema fișierului, ex: composer.json, package.json), YAML; framework-uri: Laravel / Symfony (rute, view-uri, servicii, config), Twig (filtre, funcții, variabile din controller), Vue (directive, componente), React (props, hooks), HTMX (atribute `hx-*` și valorile lor).
- [ ] `lsp-client`: client LSP generic (pornire proces, JSON-RPC, sincronizarea documentelor).
- [ ] JS/CSS/SCSS prin LSP: completion, definition, references, format.
- [ ] **PHP nativ:** indexator (simboluri + referințe), composer PSR-4, stubs, cache pe disc.
- [ ] PHP: autocomplete (N1 → N2), go to definition, find usages, outline.
- [ ] Formatare: adaptor extern (php-cs-fixer / pint / prettier) + format on save.
- [ ] Indexare pe fundal cu progres în status bar.

**Gata când:** pe un proiect Laravel real, Ctrl+Click pe o clasă sau metodă duce la definiție, Alt+F7 listează toate utilizările, iar autocomplete după `$this->` și `Model::` e corect în peste 90% din cazurile tipice.

## Faza 7: Polish și release v0.1 (≈3 săptămâni)
- [ ] Search in project (ripgrep ca librărie: `grep-searcher`).
- [ ] Persistarea sesiunii (taburi deschise, layout, poziții de cursor).
- [ ] Pachete: `.dmg` semnat (macOS), `.msi` (Windows), AppImage/`.deb` (Linux).
- [ ] Verificarea bugetelor de performanță pe toate platformele.

## Faza 8: Extensibilitate dinamică (după v0.1)
- [ ] Contracte exportate în WIT; host `wasmtime`.
- [ ] Primul modul terț de exemplu (ex: `lang-twig` ca WASM).
- [ ] Moduri de framework: Laravel (facades, rute, view-uri Blade), Symfony.
- [ ] Git (diff gutter, blame), Xdebug (DAP), integrare AI (opțional).

## Next (de făcut în continuare)
- [x] **Bug — sugestiile de completare:** detaliul unui rând (semnătura, ex. `clearstatcache`
  → `(bool $clear_realpath_cache = false, string $filename…`) iese din pill-ul rândului
  selectat și e tăiat de marginea popup-ului. Trebuie să rămână în interiorul pill-ului, cu
  padding-ul lui, tăiat cu `…` la capăt.
- [x] **Badge-uri pe butoanele Remote / Git și listele lor** (planul 26).
- [x] **PHP — parametru opțional înaintea unuia obligatoriu:** `function ceva($a = 1, int $b)`
  trebuie semnalat (PHP 8.0+: „Optional parameter $a declared before required parameter $b
  is implicitly treated as a required parameter”, deprecat; de la nivelul de limbaj ales).
- [x] **PHP — completarea nu propune `__construct`** (și celelalte metode magice) când scrii
  `function __con…` în corpul unei clase.

## Riscuri principale și cum le reducem

| Risc | Impact | Mitigare |
|------|--------|----------|
| UI kit propriu cere mult timp | Mare | Scope strict la ce folosim; inspirație din GPUI/Floem; widget-uri doar la nevoie |
| Inferența de tipuri PHP e grea | Mare | Abordare graduală N1–N3 + LSP PHP ca plan B, prin același contract |
| Text rendering diferit pe platforme (gamma, hinting) | Mediu | Snapshot tests per platformă, comparație vizuală cu un terminal nativ |
| IME pe Windows și Linux | Mediu | Testat devreme (faza 2), nu la final |
| Injecții tree-sitter pentru PHP+HTML combinat | Mediu | Spike tehnic la începutul fazei 4 (2–3 zile) înainte de designul final |
