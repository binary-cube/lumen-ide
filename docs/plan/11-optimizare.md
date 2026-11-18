# 11. Review și optimizare (CPU / RAM / GPU / structură)

Data: 2026-10-19. Punct de plecare: commit `240a8b4`.

**Regula de bază:** nu schimbăm nimic din design (aspect, comportament vizibil). Fiecare pas
se termină cu `cargo fmt`, `clippy`, testele workspace-ului, `cargo xtask check-deps`, un
snapshot comparat cu cel dinainte (unde pasul atinge desenarea) și un commit separat.

## Măsurători de pornire (release, macOS, un proiect, un fișier de 6500 de rânduri)

| Ce | Valoare |
|---|---|
| CPU în repaus | ~0,5% (7 treziri/s), bucla e `ControlFlow::Wait`, fără redesenare continuă |
| Memorie (physical footprint) | ~200 MB (vârf 251 MB) |
| din care suprafețe GPU (IOSurface) | ~120 MB |
| din care heap (malloc) | ~30 MB |
| atlasul de glife | 20 MB alocați de la pornire (mască 2048² R8 + color 2048² RGBA) |
| fire | 12 |

Ce e deja bine: fără MSAA, bufferele GPU se refolosesc, un singur draw instanțiat per strat,
`Scene` își păstrează capacitatea, parsarea tree-sitter e incrementală pe un fir separat,
diagnosticele și căutarea sunt în cache per versiune, editorul desenează doar rândurile
vizibile, fără `unwrap`/`expect` în codul de producție.

---

## Etapa 1 — corectitudine (bug-uri găsite la review)

- [x] **`settings.toml` se poate șterge**: `semantic.rs:52-62` și `language.rs:32-46` citesc cu
  `.parse().ok().unwrap_or_default()` și scriu înapoi — un fișier cu o greșeală de sintaxă își
  pierde toate celelalte secțiuni. Un singur modul `config.rs` (`config_dir`, `read_section`,
  `update_section` care refuză să scrie dacă parsarea a eșuat, scriere atomică tmp + rename);
  folosit și de `themes.rs`, `modules.rs`, `recent.rs`, `levels.rs`.
- [x] **Comanda „Compară”** ține indici de taburi (`app.rs:1675`) — pot arăta spre alte taburi
  dacă lista se schimbă până ajunge comanda. Se codifică `DocumentId`.
- [x] **Intrările `untitled:{id}`** din indexul de simboluri nu se șterg la închiderea tabului.
- [x] **Firul grafului (pânza)** ține `symbols.read()` cât citește fișiere de pe disc → poate
  bloca UI-ul la `refresh_symbols`. Se copiază ce trebuie, se eliberează lock-ul, apoi I/O.
- [x] **`php` blocat** (`lang-php/src/interpreter.rs:59`): la timeout, procesul copil se omoară.

## Etapa 2 — CPU la fiecare tastă (cel mai mare câștig la scris)

- [x] **Parsare PHP completă pe firul UI la fiecare tastă**: `refresh_symbols` (`workbench.rs:919`)
  → `extract(&buffer.to_string())` → `parser.parse(text, None)`. Simbolurile se extrag pe firul
  de sintaxă (din arborele lui) și vin împreună cu rezultatul parsării; `update_completion` nu
  mai cere refresh când rezultatul e pe drum. Făcut: firul `lumen-symbols` (`symbols.rs`),
  cereri comasate per document; sincron doar la „go to definition”.
- [x] **Copii ale întregului fișier per tastă**: `auto_close` :873, `update_completion` :1031,
  `definition_at` :1141, `sync_occurrences` :373, `select_first` :1105. Interfețele de
  inteligență primesc `&TextSnapshot` / o fereastră în jurul cursorului.
- [x] **`sync_text` (split cu același fișier)**: două copii complete + comparare per tastă →
  se reaplică modificările (`changes_since`) și nu se mai adaugă pas de undo per sincronizare.
- [x] **Căutarea de utilizări pornește un fir nou la fiecare tastă** (cheia include
  `declared_line`) → cheie pe nume + tip + fișier, plus debounce.
- [x] **Existența unui fișier se testează citindu-l tot** (`engine.rs:65, 447`); `composer.json`
  se reparsează la fiecare „go to definition” → callback `exists` (doar metadata) și harta
  PSR-4 în cache per proiect.

## Etapa 3 — CPU per cadru (desenare)

- [x] **Colorarea sintaxei se recalculează la fiecare cadru** (`workbench.rs:4405-4420`) → cache
  cheiat pe (document, numărul parsării, interval vizibil cu margine).
- [x] **`shape_line` alocă la fiecare apel**, chiar și din cache (`render/text/src/lib.rs:161-167`)
  → căutare după `&str` (fără `Box<str>` și clonarea familiei de font).
- [x] **Fiecare rând se modelează de două ori** (a doua trecere lovește cache-ul de rânduri) (selecție + text, `editor_view.rs:889/1057`) →
  rezultatul primei treceri se refolosește.
- [x] **Cache-ul de rânduri se golește complet la 2048 intrări** → evacuare pe două generații
  (păstrăm ce s-a folosit în cadrul curent și în cel precedent).
- [x] **`Folds::fold_at` e liniar**, apelat de 2× per rând vizibil și la fiecare mișcare de mouse
  → căutare binară; totalul rândurilor ascunse calculat o dată în `rebuild()`.
- [x] **Marcajele din scrollbar** (erori, apariții — până la 5000) refăcute la fiecare cadru →
  cache pe versiune + mărimea pistei.
- [x] **Rândul activ din gutter**: `selections().any(line_of)` pe fiecare rând → liniile
  cursoarelor calculate o dată per cadru.
- [x] **Sesiunea se construiește și se compară la fiecare cadru** (`save_session_if_changed`) →
  steag „modificat” setat de schimbările de taburi / split / terminal / zoom / temă. Făcut
  mai simplu și sigur: comparare cel mult o dată la 500 ms; închiderea salvează pe loc.
- [x] **Breadcrumbs, titlurile taburilor, status bar-ul** (breadcrumbs în `Memo`) reconstruite la fiecare cadru (cu
  `scope_at` = filtrare + sortare a simbolurilor, `format!`, iar numărul de caractere
  selectate copiază selecția) → cache pe (document, versiune, cursor); numărul de caractere
  din indicii rope (O(log n)) — făcut.
- [x] **Pastilele de culoare** recalculate la fiecare pas de scroll → cache per rând și versiune.
- [x] **Panoul Structură**: clonează și modelează toate nodurile la fiecare cadru, fără decupare
  la zona vizibilă; arborele se reface la fiecare mișcare a cursorului → doar rândurile
  vizibile; reconstruire doar la schimbarea simbolurilor.
- [x] **Redimensionarea desenează de două ori per eveniment** (`app.rs:1540/1545`).
- [x] **macOS: `presentsWithTransaction` mereu activ** (`render-wgpu/src/macos.rs:26`) — fiecare
  cadru așteaptă pe firul principal. Activ doar în timpul redimensionării (aspectul rămâne
  identic: fără sărituri la resize).
- [x] **`prepare()` (rasterizarea glifelor) înainte de `get_current_texture`** → munca se pierde
  când fereastra e acoperită / la timeout. Întâi suprafața, apoi pregătirea.
- [x] **Evenimentele `Wake` nu sunt comasate** (pty zgomotos / indexer) → un `AtomicBool`
  „trezire în așteptare”; setările nu se mai reconstruiesc la fiecare `Wake` fără schimbare.
- [x] **Sfatul de eroare** redesenează la fiecare mișcare a mouse-ului peste eroare → doar când
  se schimbă eroarea / poziția afișată.

## Etapa 4 — pânza (diagrama)

- [x] **Curbele sunt mii de puncte (quad-uri)** per cadru, fără eliminarea muchiilor din afara
  vederii → eliminare după dreptunghiul punctelor de control; punctele eșantionate se țin în
  coordonatele pânzei (calculate o dată per layout), nu la fiecare cadru. Aspectul rămâne
  același.
- [x] **Tragerea unui card refacea tot layout-ul** la fiecare mișcare → se mută doar cardul și
  capetele muchiilor lui.
- [x] **Catalogul pentru ⌘K** (3 stringuri × până la 200k fișiere) refăcut la fiecare cerere de
  graf, fără anularea cererilor vechi → construit la prima folosire a căutării; generație /
  anulare ca la `usages.rs`.
- [x] **Zoom-ul continuu umple cache-urile de text** (o mărime de font nouă per cadru) → mărimile
  fonturilor la zoom rotunjite în trepte fine (diferența nu se vede).

## Etapa 5 — memorie (RAM / GPU)

- [x] **Indexer-ul ține toate simbolurile proiectului de două ori** (`indexer/src/lib.rs:349-409`;
  de trei ori la build) → `files` ține doar amprenta (mărime, dată, hash); simbolurile doar în
  `SymbolIndex`.
- [x] **Atlasul de glife** (pornește la 1024² = 1 MB, crește la 2048² / 4096² înainte de a se goli; „Clear and Rebuild Caches” îl readuce la mărimea de start): 20 MB de la pornire → atlasul color creat la primul emoji, pornind
  mic; atlasul mască pornește de 1024² și crește înainte de a se goli; evacuare după ultima
  folosire, nu golire completă.
- [x] **Tabelele de chei pentru glife** (golite la „Clear and Rebuild Caches” și peste 200.000 de chei, împreună cu atlasul) (`glyph_keys`, `cache_keys`, `metrics`) cresc fără limită
  → golite odată cu atlasul.
- [x] **Texturile imaginilor expiră după 600 de cadre** (acum și după 20 s nefolosite), iar în repaus nu se desenează cadre →
  `Renderer::release_image(id)` explicit (splash, picker) + expirare și după timp; selectorul de
  culori rescrie o singură textură în loc să creeze una nouă la fiecare pas de drag.
- [x] **Istoricul de undo** e limitat la 1000 de pași, nu la mărime → buget în octeți (ex: 32 MB
  per document), `VecDeque` în loc de `Vec::remove(0)`.
- [x] **Arborii de sintaxă ai taburilor nevăzute** rămân în memorie (de două ori: tab + worker)
  → eliberați după câteva minute fără să fie vizibile; reparsare la activare.
- [x] **Vocabularul căutării** nu se curăță niciodată; vectorii semantici rămân după
  dezactivarea căutării semantice → curățare la `df = 0`; vectorii eliberați odată cu modelul.
- [x] **Cache-ul de imagini din Markdown** nu se golește → se păstrează doar imaginile încă
  referite; pixelii CPU eliberați după încărcarea pe GPU.
- [x] **Indexul de fișiere** (până la 200k stringuri) rămâne pe viață și se învechește →
  eliberat după o perioadă fără folosire; actualizat din watcher.
- [x] **Istoricul terminalului**: implicit 10k rânduri (~50 MB per terminal ocupat) → 5k.
- [x] **Fiecare fereastră își are propriile iconițe** (SVG-uri copiate și parsate) și propriul
  monitor de resurse (un fir per fereastră, toate măsoară același proces) → partajate în
  `Shared`.
- [x] **Indexer-ul nu poate fi oprit** la închiderea ferestrei → steag de oprire verificat în
  buclele de build / update.
- [x] Istoricul înapoi / înainte limitat (100), ca cel al taburilor închise.

## Etapa 6 — structura codului

- [x] **`workbench.rs` (6557 de rânduri)** → director `workbench/` cu submodule coezive: `tree`,
  `intelligence`, `side_panel`, `links`, `search`, `levels`, `files`, `editing`, `input`,
  `paint`, `status`, `viewers`, `groups`, `session`, `overlays`, `zoom`. Fără schimbări de
  logică (doar mutare), un commit.
- [x] **`app.rs` (1708 rânduri)** → `actions`, `closing` (mașina de stări salvare / închidere),
  `menus`, `theme`, `input`.
- [x] **Taburile virtuale** (pânză, compare, studio) — același cod copiat de 3 ori → un singur
  `open_virtual(path, data)`; extensia virtuală declarată de `ViewerProvider`, nu hardcodată
  în app.
- [x] **Rutarea overlay-urilor** împrăștiată în ~8 funcții (cu seturi diferite în `home_visible`
  și `overlay_open`) → `active_overlay() -> Option<Overlay>` și dispatch dintr-un loc. Făcut:
  enum-ul `Overlay` (ordinea de prioritate), `overlay_open` / `modal_open` / `home_visible` și
  textul tastat; rămân tastele speciale și click-urile.
- [x] **Cod duplicat**: potrivire fuzzy (2 variante), navigare în liste (6 copii → `ListCursor`
  în ui-kit), matematica culorilor (`mix` ×4, HSL ×2, hex ×3 → `Rgba::mix`, `to_hex`, `hsl` în
  kernel), lista de directoare ignorate (5 variante diferite → o singură constantă),
  iterarea tuturor taburilor din toate grupurile (`all_tabs()`).
- [x] **Crate-uri goale** (`project-tree`, `workbench-model`, `storage`, `lsp-client`) și
  dependențe nefolosite (`pty` în app) → scoase (sau lăsate doar dacă au un rost planificat
  în `10-roadmap.md`). Făcut: `lsp-client`, `project-tree`, `pty` scoase din app;
  crate-urile `project-tree` și `lsp-client` rămân (planificate în roadmap).
- [x] `clone()` inutile ale `Vec<Arc<…>>` (`workbench.rs:2045, 2134`).
- [x] `#[allow(clippy::too_many_arguments)]` (~25) → structuri de context pentru desenare, ca
  `ViewerPaint`.
- [x] `level_picker`: `Result<_, ()>` → `enum LevelClick { Pick, Inside, Outside }`.
- [x] Comentarii `SAFETY` pentru `unsafe` (toate blocurile au, verificat automat).

## Etapa 7 — cache: mecanism comun, golire, izolare per proiect

- [x] **Primitive comune** în `kernel::cache`: `Memo` (o valoare per cheie),
  `GenerationalCache` (mărime limitată, două generații, căutare fără alocare), `Purge`
  (golire la cerere). Toate cache-urile noi și cele existente se construiesc din ele.
- [x] Pe ele: liniile shape-uite (per font), culorile de sintaxă per tab, culorile din cod.
- [x] **„Clear and Rebuild Caches”** (meniul File): golește cache-urile workbench-ului,
  textul shape-uit + cheile de glife împreună cu atlasul și texturile GPU, apoi
  reconstruiește indexul proiectului (cache-ul de pe disc din `.lumen/`).
- [x] **Izolare per proiect**: tot ce ține de proiect stă în `ProjectData`, înlocuit întreg la
  deschiderea altui proiect. Repară: indexul de simboluri al proiectului anterior rămânea
  activ dacă noul proiect nu pornea un indexer; istoricul ⌘⇧T, utilizările, graful în lucru
  și căutările recente treceau de la un proiect la altul.
- [x] Restul cache-urilor pe primitive: marcajele din scrollbar, breadcrumbs, titlurile
  taburilor, status bar-ul, panoul Structură, imaginile din Markdown (`GenerationalCache` cu
  limită în octeți), metricile fonturilor.
- [x] Cache-urile de pe disc (`.lumen/index/symbols.bin`, `search.bin`) au în antet
  `cache_stamp` = versiunea aplicației + versiunea formatului: o versiune nouă a aplicației
  sau un format schimbat le reconstruiește. (Căile sunt relative, deci un proiect mutat
  își păstrează cache-ul valid.)

## Etapa 8 — DDD: servicii / use case-uri pe straturile din `02-arhitectura-ddd.md`

Azi, multă logică de aplicație (persistență, căutări, reguli de limbaj) stă în crate-ul
`app` (composition root). Ținta: domeniul (pur) definește modelul și porturile, stratul
`application` al fiecărui context are use case-urile, `infra` implementează porturile, iar
`app` doar le leagă.

- [x] **Persistență → `storage`** — făcut: sesiunea (`SessionStore` /
  `TomlSessionStore`), setările (`SettingsStore` / `TomlSettingsStore`), proiectele recente
  (`TomlRecentStore`), `.lumen/project.toml` (niveluri și framework-uri);: porturi în domeniu
  (`SettingsStore`, `SessionStore`, `RecentProjects`, `ProjectSettings` pentru
  `.lumen/project.toml`), adaptoare TOML în `storage` (conținutul actual din
  `config.rs`, `session.rs`, `recent.rs`, `levels.rs`). Modelul `Session` trece în
  `workbench-model` (domeniu, fără `toml`).
- [x] **Use case-uri în `application/`** (comandă / interogare, fără UI):
  - Workspace: `OpenProject`, `RestoreSession`, `SaveSession`, `RevealInProject`;
  - Document: `SaveDocument`, `CloseDocuments` (mașina de stări de la închidere, azi în
    `app/closing.rs`, cu un port pentru întrebarea „Salvezi?”);
  - Intelligence: `FindUsages` (azi `app/usages.rs`, cu regulile per limbaj mutate în
    `LanguageIntelligence`), `GotoDefinition`, `BuildDependencyGraph`;
  - Theming: `ComposeTheme`, `SaveUserTheme`, `SelectProjectTheme`.
- [~] **Taburile virtuale** (pânză, compare, studio): un singur `open_virtual` (făcut), extensia
  declarată de `ViewerProvider`; cererile viewer-elor (`theme.*`, graful) tratate de
  handler-e înregistrate de module, nu de `match` pe stringuri în `app`.
- [x] `terminal-panel` și `themes-builtin` devin module reale (contribuie prin
  puncte de extensie, nu folosite direct din `app`).
- [x] Testele use case-urilor fără fereastră (porturi false / în memorie).

## După optimizare (cereri noi)

- [x] **Dublu Shift** deschide căutarea în proiect (ca „Search Everywhere”).
- [x] Cursorul clipește; câmpurile fără focus nu arată cursor.
- [x] Bara de căutare în fișier refăcută.
- [x] Meniu contextual pe taburile terminalului (Rename / Pin / Close / Close Others).
- [x] Zoom cu rotița doar cu ⌘⌥ (⌘ e pentru link-uri).
- [x] Rezultatele din setări duc la element și îl evidențiază.
- [x] **Go to definition în JS și CSS** (variabile / funcții / clase locale și din alte fișiere,
  selectori și variabile CSS), ca într-un editor bun.
- [x] **Pipetă** (eyedropper) în selectorul de culori din studio: ia o culoare de pe ecran.
- [x] **Seturi de scurtături**: Lumen, PhpStorm, VS Code, Zed, Sublime — alese din setări.
- [x] ⌘A / copy / cut / paste în câmpul cu focus, peste tot (input_owner).
- [x] Popover-ele la aceeași distanță de ancoră (ds::POPOVER_GAP); fereastra „Despre” se
  închide doar din esc; lista de proiecte nu mai tremură la scroll.
- [x] Testele: unitare rămân lângă cod (`#[cfg(test)]`, excluse din build-ul normal), dar
  blocurile mari se mută în fișiere separate (`foo/tests.rs`); testele de integrare (doar
  API public) în `crates/<crate>/tests/`.

## Etapa 9 — comentarii în engleză

- [x] Toate comentariile din cod (`.rs`, `.wgsl`, scripturi) traduse în engleză, verificat
  automat că doar comentariile s-au schimbat. Regula e în `CLAUDE.md`.

## Etapa 10 — lizibilitatea codului (fără schimbări de comportament)

> Etapele 6–8 și 10: încheiate de planul 21 (`21-refactorizare-calitate.md`): rutarea
> overlay-urilor de stiva din planul 17; contexte în loc de argumente multe; nicio funcție de
> peste 150 de rânduri; use case-urile în domeniu, testate fără fereastră; temele și
> terminalul module; cache-urile pe primitive, golite peste tot; review-ul.

- [x] Hook-urile de snapshot din `main.rs` (45 de `std::env::var("EDITOR_SNAPSHOT_*")`) →
  `SnapshotOptions::from_env()` citit o dată, tipizat, plus pași mici, fiecare cu un nume.
- [x] Funcțiile foarte lungi (`mouse_down` ~380 rânduri, `edit_key` ~320, `paint_group`)
  împărțite în funcții cu un singur rol; `if`-uri imbricate → `match` / early return.
- [x] Rutarea overlay-urilor printr-un `active_overlay()` (vezi etapa 6).
- [x] Review pe crate-uri: nume clare, fără duplicare, fără `#[allow]` evitabile.

## Scroll

- [x] Tremur la scroll cu inerție: pozițiile de scroll se desenează la pixel întreg peste
  tot; setările își măsoară conținutul doar din cadrul curent (derulau și când totul
  încăpea).

## Dicționare de limbaj

- [x] **PHP**: stub-urile noastre, generate prin reflection din fiecare versiune PHP
  (5.6 … 8.5, imagini Alpine din `docker/php/compose.yaml`): 2.143 funcții, clase, metode,
  constante, cu versiunea de apariție / eliminare. Folosite la completare (toate funcțiile,
  cu semnătură), verificarea pe versiune (inclusiv funcții / clase eliminate) și lista de
  versiuni (din date, nu scrisă în cod). Automat: `--update` găsește versiunile noi pe
  Docker Hub; workflow lunar care deschide un PR.
- [x] **CSS, HTML, JS / DOM**: datele noastre, convertite din datele MDN publicate ca
  domeniu public (CC0) — `cargo xtask web-data`: 652 de proprietăți CSS cu sintaxa și
  valorile lor, elementele și atributele HTML (cu valori, ex: `input type`), obiectele JS
  și interfețele DOM cu membrii lor; marcaj depreciat / experimental / nestandard.
  Înlocuiesc listele scrise de mână la completare. Același workflow lunar.
- [x] Avertismente pentru ce e depreciat: elemente / atribute HTML (`<center>`, `bgcolor`),
  proprietăți CSS (`clip`, `page-break-*`), API-uri JS / DOM (`escape()`,
  `document.execCommand`) — tăiate în editor, marcaj galben pe bara de scroll, tooltip,
  numărul de avertismente în bara de stare. Calculate pe firul de analiză.
- [x] Cardul de la hover (după ~0,6 s pe un cuvânt): semnătura funcțiilor / metodelor din
  proiect, funcțiile / clasele / constantele native PHP cu versiunea („PHP 8.0+”,
  „Eliminat în PHP 8.0”), sintaxa proprietăților CSS, elementele / atributele HTML și
  API-urile DOM cu starea lor (depreciat / experimental / nestandard).
- [x] Semnăturile metodelor JS / DOM: din `lib.*.d.ts` ale TypeScript (Apache-2.0; licența
  și NOTICE copiate lângă date), citite de `cargo xtask web-data` (`xtask/src/dts.rs`:
  interfețe cu `extends`, `declare var` / `function`; dintre overload-uri, cel cu tipuri
  generice rezolvabile, apoi cel mai general). ~5100 din ~7700 de membri; în hover și la
  ajutorul la parametri; pentru obiecte de tip necunoscut, când toate interfețele sunt de acord.
- [x] Ajutor la parametri în timp ce scrii într-un apel (`str_contains(|`): semnătura
  deasupra cursorului, cu parametrul curent evidențiat (variadicul ia restul); apare la
  `(` / `,` sau la o sugestie cu paranteze, urmărește argumentul, dispare la ieșirea din
  apel / Esc / click. Când e ascuns nu costă nimic; semnătura se caută o dată pe apel.

Propunerea inițială (surse):

Semnăturile funcțiilor / claselor și versiunea de la care există fiecare, din surse
oficiale, convertite la build (xtask) în tabele compacte, încărcate leneș per modul:

- PHP: `phpstorm-stubs` (Apache-2.0) — toate funcțiile, clasele și constantele
  din PHP și extensii, cu tipuri, parametri și `@since` / `@deprecated`.
- JS / DOM: `lib.*.d.ts` din TypeScript (Apache-2.0) — obiectele standard și DOM-ul.
- CSS / HTML: `mdn/data` și `@mdn/browser-compat-data` (CC0) — proprietăți, valori,
  sintaxă, elemente, atribute și suportul pe browsere.

Folosite pentru: completare cu semnătură, ajutor la parametri, hover cu documentație,
verificarea pe versiune (înlocuiește tabelul scris de mână `SINCE`), funcții depreciate.

## Module de framework (cerere nouă)

- [x] Module separate (`crates/modules/fw-*`) pentru **Symfony, Laravel, Yii** (PHP) și
  **React, Vue, htmx** (web). Modulul aduce doar detectorul (`Framework`: `composer.json`,
  `package.json`, `bin/console`, `artisan`, `yii`, layout-uri cu htmx); ajutorul
  (`FrameworkSupport`) se creează doar pentru proiectul care îl folosește și dispare cu el.
  Ce adună din proiect (șabloane, rute) e în `Memo` pe generația fișierelor (watcher + salvare).
  - Symfony: șabloane în `render('…')` / `{% extends %}` / `include()`, rute din `#[Route]`
    și `config/routes*.yaml` în `path()` / `redirectToRoute()` (cu calea la hover).
  - Laravel: view-uri (`view('a.b')`, `@extends`), rute (`->name()` din `routes/`), chei de
    config (`config('app.name')`).
  - Yii: view-urile controllerului (`render('…')` → `views/<id>/`).
  - htmx: atributele `hx-*` alături de cele HTML, valorile lor, sintaxa și descrierea la
    hover — pe versiunea proiectului (2 sau 4, din `package.json` / `htmx.org@N` în layout),
    din datele de editor ale htmx (0BSD) generate de `xtask web-data` pentru fiecare major.
  - React: hook-urile / API-ul cu semnături (hover, parametri, completare `use…`).
  - Vue: directivele `v-*` în șablon, API-ul Composition; fișierele `.vue` se deschid ca HTML.
- [x] Framework-urile PHP se exclud reciproc (familia `php`): unul singur activ.
- [x] Setări → „Module”: framework-urile proiectului cu comutator și motiv (detectat din …,
  ales, înlocuit), salvate în `.lumen/project.toml` (`[frameworks]`); modulele de bază
  listate ca mereu active. Nume și descrieri traduse (`[module]` în locales).
- [x] Resurse: detectarea citește fiecare fișier o dată; rutele Symfony se recitesc doar din
  fișierele schimbate (după mtime, `GenerationalCache`); generația ignoră `.lumen/` și
  `.git/`; „Clear and Rebuild Caches” recreează ajutorul framework-urilor.
- [x] Blade ca limbaj (`lang-blade`, gramatica tree-sitter-blade MIT inclusă în `third_party`):
  HTML + directive + PHP în ecouri, folding pe directive, fără erori false în fragmente.
- [x] Componentele React (exportate cu nume cu majusculă) și Vue (fișierele `.vue`) din proiect:
  completare după `<` și salt la definiție.
- [ ] Mai departe: servicii Symfony / container Laravel.

## Cereri noi (în lucru, în ordine)

- [x] Cardul de hover: mouse-ul poate intra pe card (nu dispare), fundal distinct de editor
  (după temă, nu alb pe alb).
- [x] Indicator pentru fișierele modificate nesalvate + salvare automată după o pauză de
  scris, la schimbarea tabului / pierderea focusului.
- [x] Stub-uri pentru extensiile PECL (http, redis, imagick, mongodb, amqp, apcu, igbinary,
  msgpack, yaml, memcached, swoole…), generate prin reflection ca restul (fără fișiere
  copiate; licențele extensiilor verificate); clasele lor și cele din `vendor` după `new`.
- [x] Încă 14 teme (Grove, Neon City, Mocha, Retro, Onyx, Ocean, Ink, Rosé, Solar Dark; Latte,
  Solar Light, Sakura, Mint, Daylight), complete, generate din paleta fiecăreia.
- [x] Seturi de iconițe (Setări → Iconițe): Lumen, Fin, Accentuat, Duoton, Geometric — stiluri ale
  acelorași desene (atributele SVG rescrise), parsate doar când sunt folosite; comune ferestrelor.

## Refactorizare (cerere nouă, prioritară)

- [x] **Rename** (Shift+F6, meniul de click dreapta din editor, meniul Refactor): clasă,
  interfață, trait, enum, metodă, funcție, proprietate, constantă, variabilă, parametru,
  clasă / id / variabilă CSS — în PHP, JS, CSS, HTML; `use`-uri și docblock-uri incluse;
  fișierul redenumit odată cu clasa (PSR-4).
- [x] Previzualizare reactivă (numele nou urmărit pe măsură ce e scris), bife pe loc / fișier,
  locurile nesigure nebifate, aplicare pe fișierele deschise (nesalvate) și închise,
  anulare dintr-o dată („Undo Last Refactoring”).
- [x] Conflicte: numele nou deja folosit (alt tip / funcție în namespace, un membru al
  aceleiași clase, o variabilă din funcție, un nume CSS) — arătate, „Refactorizează oricum”.
- [x] Reindexare imediată după aplicare / anulare (Go to Declaration găsește numele nou).
- [x] Redenumirea fișierului unei clase PHP (tab → Rename File) redenumește clasa, prin
  previzualizare.
- [x] **Move Class**: mutarea fișierului / folderului în alt folder (drag & drop în arbore /
  F6) schimbă namespace-ul după PSR-4 și actualizează `use`-urile (și cele grupate), referințele
  complet calificate, docblock-urile și adaugă importurile devenite necesare.
- [ ] Șiruri care sunt nume de clasă (`'App\\Models\\User'` — la mutare deja, nebifate;
  de făcut la rename și în configurări YAML / XML).
- [ ] La mutarea fișierelor HTML / CSS / JS: căile relative care le referă (`src`, `href`,
  `import`).
- [x] **Change Signature** (⌘F6 / Ctrl+F6, click dreapta, meniul Refactor): parametrii unei
  funcții / metode / constructor (pe clasă sau `new X`) — adăugare (cu valoarea pentru
  apelurile existente), ștergere, mutare ↑ ↓, nume, tip, valoare implicită — aplicați la
  toate apelurile (argumente mutate, golurile completate cu valoarea implicită sau trecute
  după nume în PHP 8, argumentele variadice păstrate), PHP și JS, cu aceeași previzualizare.

## Istoric local (cerere nouă)

- [ ] **Local History** (istoricul local al fișierelor), independent de git (merge și fără git instalat sau
  fără depozit): la fiecare salvare (și înainte de refactorizări, revert-uri, modificări din
  afară) se păstrează o versiune a fișierului, pe disk, în folderul de date al proiectului
  (comprimată, deduplicată după conținut).
- [ ] Lista versiunilor unui fișier (data, ce a declanșat-o: salvare / refactorizare / extern),
  comparare cu versiunea curentă în modulul de comparație, readucerea unei versiuni sau a unei
  bucăți din ea; și pentru fișierele șterse.
- [ ] Configurabil din setări: câte versiuni pe fișier (implicit generos, de ex. 200) și cât timp
  (de ex. 30 de zile); curățarea se face în fundal.
- [ ] Pe DDD: domeniul istoricului (versiuni, politica de păstrare, pur), un port de stocare,
  modul cu UI-ul; ascultă evenimentele `FileSaved` / refactorizare. Merge și pe Windows.

## Windows (cereri noi)

- [ ] **Memoria pe Windows (~500 MB cu un proiect deschis)**: profilare pe Windows (DX12 /
  Vulkan: swapchain, atlasul, texturile; heap-ul procesului), ținta: cât pe macOS (~90–110 MB),
  ca să concurăm cu editoarele native ușoare. De verificat: backend-ul wgpu ales, numărul de
  buffere, `desired_maximum_frame_latency`, memoria driverului, alocatorul.
- [x] **Meniurile contextuale desenate de noi** (în locul celor native Win32 / NSMenu): aceeași
  formă, culori din temă, iconițe și scurtături pe toate sistemele.
- [x] **Fereastra fără chenarul Windows** (ca pe macOS): bara de titlu a noastră (fără
  titlul / chenarul sistemului), colțuri rotunjite (Windows 11: DWM `DWMWA_WINDOW_CORNER_
  PREFERENCE`; Windows 10: regiune / umbră proprie), butoanele minimizare / maximizare /
  închidere în dreapta (ordinea Windows) desenate de noi pe temă — subțiri, hover discret,
  închidere roșie la hover; trasul ferestrei din bara de sus (hit-test `HTCAPTION`), dublu
  click = maximizare, Snap Layouts păstrat (hover pe maximizare, `HTMAXBUTTON`), redimensionare
  pe margini. Ecran complet: bara se ascunde și apare la marginea de sus.
  ✓ (fără chenarul sistemului, umbra păstrată; colțuri rotunjite prin DWM pe Windows 11 —
  pe Windows 10 rămân drepte, fără umbră proprie; minimizare / maximizare (restaurare) /
  închidere în bara noastră, închiderea roșie la hover; trasul și dublu click din bară;
  marginile redimensionează și maximizarea e butonul sistemului pentru Snap Layouts, printr-un
  subclass `WM_NCHITTEST`. Ecranul complet (comanda „Ecran complet”, F11): bara se ascunde și
  conținutul îi ia locul; la marginea de sus a ecranului coboară peste conținut (cu fundal
  propriu), butonul din mijloc iese din ecranul complet, marginile nu mai redimensionează; pleacă
  când mouse-ul o părăsește; colțurile drepte pe ecran complet. Butoanele ferestrei mai mici
  (36 px), hover-ul pe maximizare desenat și când sistemul ține mouse-ul acolo (Snap Layouts,
  `WM_NCMOUSEMOVE`). Colțurile rotunde le taie aplicația, pe GPU, oricare ar fi sistemul
  (Windows 10, mașini virtuale): DX12 prezintă prin DirectComposition (`DxgiFromVisual`, alfa
  premultiplicat), fereastra fără bitmapul sistemului și fără umbra lui; la final o mască
  antialiasată (`fs_window_corner`, `dst * (1 - alfa)`) pe cele 4 colțuri, rază 8 × scala;
  nu și maximizată / pe ecran complet. Vulkan / GL: colțuri drepte. `EDITOR_DX12_HWND=1`:
  prezentarea de dinainte. De făcut: verificat pe o mașină Windows reală — compilat și
  verificat cu clippy pentru Windows, nerulat încă acolo.)

## Editor și căutare (cereri noi)

- [x] Ghidaje de indentare în editor (liniile verticale ale blocurilor), subtile, din
      culorile temei; blocul cursorului evidențiat.
- [x] „Go to Line” (⌘L / Ctrl+L; Ctrl+G în preset-ul VS Code): `linie[:coloană]`.
- [x] În căutările de fișiere (bara laterală, ecranul de start; în căutarea în proiect — încă nu):
      `class.php:11` (sau `class.php:11:5`) deschide fișierul direct la linia (și coloana)
      dată.

## Pachete de iconițe (cerere nouă)

- [ ] Pe lângă stilurile setului nostru (Regular, Light, Bold, Duotone, Geometric): pachete
      întregi, cu alte desene — ca module, încărcate doar cât sunt alese (licențe permisive:
      Lucide ISC, Tabler MIT, Phosphor MIT; Material Symbols Apache-2.0), și seturi de
      iconițe pentru fișiere (pe tipuri și foldere speciale: `src`, `tests`, `config`…).
- [ ] Previzualizare în Setări → Icons, ca acum; alegerea per proiect, opțional.

## Module încărcate la nevoie (cerere nouă)

- [x] Inventar (măsurat, release, macOS): activarea tuturor modulelor — 2,3 ms în total (doar
      descrieri și provideri); datele, la prima nevoie: stub-urile PHP 2,4 ms / ~2,9 MB,
      dicționarul JS 2,3 ms / ~1,7 MB, CSS 0,4 ms / ~0,5 MB, HTML ~0; gramaticile compilate:
      PHP 46 ms / ~6 MB, Blade 32 ms / ~6,7 MB, JS 10 ms / ~0,6 MB, restul sub 3 ms.
- [x] `ModuleManager` (app): datele modulelor citite la prima nevoie (`OnDemand` în
      `kernel::cache`) și eliberate după 10 minute nefolosite, sau citite în fundal la
      pornire; setarea „Încărcarea” (La nevoie / Toate la pornire), bifa „La pornire” pe
      fiecare modul, starea „În memorie”; orice modul se poate opri (de la următoarea
      pornire), cele de care depind altele rămân pornite cât timp acelea sunt.
- [ ] Măsurat pe Windows.
- [ ] Și gramaticile compilate eliberate când niciun fișier al limbajului nu mai e deschis.

## Terminal colorat (cerere nouă)

- [x] Ieșirea colorată din oficiu, ca în terminalele obișnuite: `ls` cu culori (`CLICOLOR`,
  `LSCOLORS` / `LS_COLORS`), `COLORTERM=truecolor`, `TERM=xterm-256color`, prompt-ul colorat
  al shell-ului; culorile ANSI vin din temă (deja), deci arată bine în orice temă.
- [x] Stiluri de afișare la alegere (Setări → Terminal): paletă ANSI după temă / clasică /
  contrast mare, cursor, font, înălțimea rândului.
- [x] **Merge și pe Windows**: PowerShell / cmd / Git Bash cu ConPTY (secvențele VT activate),
  culorile din aceeași paletă.
- [x] **Colorarea elementelor din consolă după temă, opțională** (Setări → Terminal, un
  comutator): prompt-ul (utilizator@gazdă, directorul, `%`), mesajele shell-ului („Last
  login…”) și cursorul colorate din paleta temei, nu doar textul alb; oprit → ca acum.
  (Planul 20, făcut: mediul colorat, shell-urile găsite, Setări → Terminal, desenarea după
  setări, prompt-ul colorat opțional; pe Windows compilat și verificat cu clippy, nerulat
  încă acolo.)

## Proiecte la distanță (cerere nouă): FTP / FTPS / SFTP

- [ ] Modul `remote` (încărcat doar când e folosit): managerul de conexiuni (gazdă, port,
  protocol, utilizator, director rădăcină; parolele / cheile în keychain-ul sistemului, nu în
  fișiere) și „Deschide proiect la distanță…” lângă proiectele recente.
- [ ] Protocoale: SFTP (SSH, cu chei și agent), FTP și FTPS (TLS explicit / implicit); WebDAV
  mai târziu. Implementate ca port `FileSystem` în `infra` (ca `fs-local`), deci arborele,
  căutarea, indexul și editorul lucrează la fel ca pe disc.
- [ ] Datele proiectului (cache-uri, indexul de simboluri, căutarea, sesiunea, setările
  proiectului) în `~/.config/lumen/remote-projects/<conexiune>/<rădăcină>/`, nu pe server.
- [ ] Rapid: listări de directoare la cerere și în cache, transferuri în paralel pe un fir de
  fundal (UI-ul nu așteaptă rețeaua), o copie locală a fișierelor deschise / indexate,
  reîmprospătată după dată + mărime.
- [ ] Salvare sigură: scriere într-un fișier temporar pe server + redenumire (unde protocolul
  permite), verificare că fișierul de pe server nu s-a schimbat între timp (dată / mărime /
  hash) — altfel dialog de conflict cu comparația; reîncercare la erori de rețea; nimic
  pierdut dacă legătura cade (modificările rămân locale, marcate, și se trimit la revenire).
- [ ] Indicator de stare în bara de jos (conectat / se sincronizează / offline / conflict).
- [ ] **Merge și pe Windows**: SFTP prin `russh` (Rust pur), FTP / FTPS prin `suppaftp` cu
  `rustls`; parolele în Windows Credential Manager; căile locale prin `config_dir()`.
- [x] Domeniul `remote` (6a): conexiunile (protocol, mod, excluderi, unde se salvează), porturile
  `RemoteFs` / `Connector` / `LocalFiles` / `SecretStore`, evidența transferurilor
  (`Manifest`), planul pull / push / sync (conflict când ambele părți s-au schimbat; ștergerile
  doar la cerere), transferuri în paralel (o sesiune pe lucrător), jurnalul.
- [ ] Două moduri pe conexiune: **copie locală sincronizată** sau **lucru la distanță**
  (listări și fișiere aduse la cerere, salvarea urcă direct).
- [ ] Excluderi vizuale: arbore cu bife + lista celor excluse cu calea completă.
- [ ] Click dreapta → **Remote ›** (doar pe proiectele remote), cu iconițe: Pull (cu ce s-a
  schimbat pe server evidențiat, suprascriu / skip, comparație), Push, Sync.
- [ ] Panoul **Remote** jos (ca terminalul, cu iconiță lângă el): jurnalul transferurilor —
  urcat / descărcat, mărime, dată, ce s-a schimbat.
- [ ] Conexiunea salvată **în proiect sau global** (la alegere); parolele / cheile
  **niciodată în clar**: Keychain-ul sistemului, sau în proiect criptate (cheia în Keychain);
  avertisment dacă fișierele nu sunt ignorate de git.
- [ ] Modulul și dependențele de rețea pornesc doar la un proiect remote (lucrul local nu
  plătește nimic).
- [x] Infra (6b), Rust pur, la fel pe macOS / Linux / Windows: SFTP (`remote-sftp`, russh +
  ring: parolă, cheie, agent — OpenSSH / Pageant pe Windows; cheia serverului verificată în
  `known_hosts`, una nouă acceptată de utilizator după amprentă, una schimbată refuzată), FTP /
  FTPS explicit și implicit (`remote-ftp`, rustls: rădăcinile Mozilla incluse, certificatele
  proprii acceptate după amprentă; TLS 1.2 automat pentru serverele care cer reutilizarea
  sesiunii), urcare sigură (fișier temporar + redenumire); secretele (`secrets`): Keychain /
  Credential Manager sau seif criptat în proiect (ChaCha20-Poly1305, cheia în Keychain).
  Testate pe servere reale (Docker).
- [x] Reîncercări: transferurile și conectarea reîncercate pe o sesiune nouă, cu pauză
  crescătoare, doar la erorile trecătoare (rețea, timeout); setabile global / pe conexiune.
- [ ] La salvare / Push: verificat întâi dacă fișierul s-a schimbat pe server de la ultimul
  transfer — dacă da, utilizatorul e anunțat și se deschide comparația „al meu ↔ de pe server”.

## Baze de date (cerere nouă)

- [ ] Modulul `database` (o fereastră „Database” de IDE), cu drivere ca module
  separate — planul complet în `13-baze-de-date.md`. Merge și pe Windows (drivere Rust pur).
  Se face după ce termin tot ce e deja pe listă.

## Navigare și analiză, la nivel de IDE complet (cerere nouă)

Fiecare ca modul, în stil DDD: domeniul (modelul, use case-urile, porturile), infra / app
răspund porturilor, UI-ul ca vizualizator / panou contribuit; în meniul de click dreapta.

- [x] **Find Usages** (⌥F7 / Alt+F7): toate utilizările simbolului de la cursor (clase,
      metode, funcții, proprietăți, constante, variabile; clase / id-uri / variabile CSS),
      PHP, JS, HTML, CSS — grupate pe fișier, sigure / nesigure, cu linia; click → acolo.
      Refolosește găsirea locurilor de la Rename (același port `Resolver`) și serviciul unic
      de căutare.
- [x] **Compare with Clipboard**: selecția (sau fișierul) comparată cu textul din clipboard,
      în vizualizatorul de comparare existent.
- [x] **Diagrams**: diagrama claselor de la cursor (părinți, interfețe, trait-uri, copii,
      dependențe din constructor), ca vizualizator — modulul `diagrams`, cazul de utilizare
      `class_diagram` în domeniu, prin `ProjectQueries`; „Show Diagram” (tab, ⌘⌥⇧U, meniul
      Navigate, click dreapta), click pe un tip → centrat pe el (← înapoi), click pe un
      membru → codul lui.
  - [-] „Show Diagram Popup” — renunțat (cererea utilizatorului): diagrama se deschide în tab.

## Iterație de optimizare și review (cerere nouă)

Consumul pare mare: o trecere sistematică prin cod, măsurată, înainte de funcționalități noi
mari.

- [ ] Măsurători de referință (macOS + Windows): RAM la pornire / cu un proiect mare deschis /
      după o oră; CPU în repaus (trebuie ~0%) și la tastare; GPU (memorie, cadre pe secundă
      în repaus = 0); timp de pornire până la primul cadru.
- [ ] RAM: fonturile de sistem încărcate toate (fontdb) — doar cele folosite, restul la nevoie;
      indexuri și cache-uri (simboluri, căutare, stub-uri PHP) — dimensiuni, eliberare când nu
      sunt folosite (`Purge`); atlasul de glife; documentele închise.
- [ ] CPU: nimic recalculat pe cadru ce poate fi ținut minte (ghidaje, culori, layout-uri);
      firele de fundal care se trezesc degeaba; watcher-ele; parsarea incrementală.
- [ ] GPU: texturi (atlas, imagini) eliberate; cadre cerute doar la schimbări / animații.
- [ ] Module eager / lazy (vezi secțiunea „Module încărcate la nevoie”): inventar, ce pornește
      la boot, ce la primul fișier de un tip, în fundal, cu indicator mic de încărcare.
- [ ] Review de cod: funcții lungi, duplicări, use case-uri lipsă în domeniu, teste lente.
- [x] **Un `SettingsService` tipizat** (singleton global): fiecare setare declarată o dată
      (cheie, tip, valoare implicită, validare, setare globală / per proiect), citită și scrisă
      prin el, cu notificare la schimbare — în locul citirilor directe din `settings.toml`
      împrăștiate prin module (autosave, iconițe, semantic, scurtături…).
- [x] **Un singur serviciu pentru fișierele proiectului** (`ProjectFiles`, o instanță pe
      proiect, asincron, prin portul `FileSystem`) + un singur filtru (`FileFilter`, în
      domeniu): căutarea de căi (bara laterală, ecranul de start, căutarea în proiect),
      utilizările, refactorizările și canvas-ul îl folosesc; căutarea de text rămâne serviciul
      indexerului (lexical + semantic).
- [x] **Fațada de interogări** (`ProjectQueries`, port în domeniu): căi, declarații (fuzzy,
      pe tip / limbaj / dependențe / limită — `SymbolQuery`), text (lexical + semantic),
      fișiere filtrate — un instantaneu folosibil din orice fir, dat modulelor prin
      `WorkbenchHost::queries`; refactorizările o folosesc.
- [ ] (detaliul inițial) **Un singur serviciu de căutare** (domeniu + port, o instanță pe proiect): fișiere,
      foldere, text (lexical + semantic), simboluri, utilizări, locurile unei refactorizări —
      un API cu filtre (tip, limbaj, foldere, `vendor` da / nu, limite), asincron, cu anulare;
      bara laterală, ecranul de start, căutarea în proiect, Go to Symbol, utilizările și
      refactorizările îl folosesc pe el, fără indexuri / încărcări duplicate. Modelul semantic
      rămâne unul pe proces, încărcat la nevoie și eliberat după inactivitate.
- [ ] Măsurători după, comparate cu referința; rezultatele trecute la „Rezultate”.

## Ordinea de lucru

1. Etapa 1 (bug-uri) — risc mic, valoare mare. ✓
2. Etapa 6, primele două puncte (împărțirea fișierelor mari) — doar mutare de cod, ca restul
   modificărilor să fie mici și ușor de urmărit.
3. Etapele 2 → 3 → 5 → 4, în ordinea impactului.
4. Restul etapei 6.
5. Măsurătorile de la început, refăcute la sfârșit, în acest fișier.

## Următorii pași (în ordine)

1. ~~Change Signature~~ ✓
2. **Iterația de optimizare și review** (CPU / GPU / RAM, măsurat) + module eager / lazy —
   prima trecere făcută (vezi „Rezultate”); urmează interning-ul indexului, serviciile unice
   (căutare, setări), apoi: bara de stare cu lățimi rezervate (nu mai „joacă”), culorile
   selecției pe temele unde e prea saturată.
3. ~~**Find Usages**, **Compare with Clipboard**, **Diagrams** (module DDD)~~ ✓
4. ~~**Git**: push / pull / fetch, revert al unui bloc din gutter, istoricul cu arborele de
   commit-uri (Git Log)~~ ✓ (fetch / pull / push prin `git`-ul instalat; fără el: mesaj clar —
   de văzut ulterior fetch / pull fără `git`, cu gitoxide) — fără `git` instalat: citirea merge (gitoxide), operațiile cu
   remote-ul spun clar ce lipsește, nimic nu crapă.
4b. ~~**Delete în arbore** (fișiere / foldere, la coșul de gunoi al sistemului, cu confirmare,
   ⌫ / Delete) — lipsește din meniul de click dreapta.~~ ✓
4c. ~~**Comparatorul: săgețile de pe separator** mai elegante — vizibile doar pe diferența
   curentă / la hover pe rânduri, butoane mici lipite de marginea coloanei (nu peste
   separator), animate; tastele `[` / `]` și ⌥← / ⌥→ pentru diferența curentă.~~ ✓ (pastila de alegere în coloana rezultatului)
5. ~~**Move Class** (namespace + `use`-uri) și drag & drop în arbore.~~ ✓ (Refactor › Mută…, F6,
   click dreapta în arbore și în editor; trasul unui fișier / folder pe un folder din arbore
   deschide aceeași previzualizare; namespace-ul după PSR-4 din `composer.json`, `use`-urile —
   și cele grupate —, numele complet calificate, importurile devenite necesare, docblock-urile;
   șirurile cu numele clasei arătate nebifate; anulare cu „Undo Last Refactoring”)
5b. ~~**Meniurile de click dreapta grupate**: pe tab — Compare with ›, Split ›, Close ›; Git › și
   pe tab când fișierul e din proiect.~~ ✓ (și în arbore: New ›, Compare with ›)
5c. ~~**Git**: în istoricul unui fișier, la click pe un commit doar fișierul acela (cu opțiunea
   „toate fișierele din commit”); ramurile (Local / Remote / Tags, filtrul
   Branch); separatorul (Splitter) între listă și detaliile commit-ului.~~ ✓ (și: diff-ul se
   deschide deasupra istoricului — istoricul coboară jos, pe toată lățimea —, înlocuit la
   fiecare click; tastele ↑ ↓ ↵ Esc ajung acum și la vederile fără text: Git Log, Local History)
5d. ~~**Module**: încărcarea la nevoie / la pornire, orchestrată (`ModuleManager`), cu
   setări; orice modul se poate opri; numele lungi din arbore cu „…” (întregi la hover),
   fără scroll orizontal.~~ ✓
5e. ~~**Mută linia / blocul** (⌘⇧↑ / ⌘⇧↓, „Move Statement”: mută instrucțiunea întreagă): fiecare modul de
   limbaj știe, printr-un port, ce e un bloc (funcție, metodă, element HTML, regulă CSS).~~ ✓
   (PHP, JS / TS, CSS, HTML; ⌥⇧↑ / ⌥⇧↓ mută linia simplu; de adăugat: JSON / YAML, elementele
   de array cu virgulele lor, mutarea unei instrucțiuni în blocul vecin)
5f. ~~**Redenumirea crate-urilor `lumen-*`** și a variabilelor `LUMEN_*` cu nume tehnice
   (`lumen-kernel` → `kernel`…), ca numele aplicației să poată fi schimbat oricând.~~ ✓
   (descriptive unde un nume simplu s-ar fi ciocnit: `code-intelligence`, `version-control`,
   `settings-model`, `image-decode`…; variabilele `EDITOR_*`; doar binarul poartă `id`-ul
   din `brand.toml`)
5g. ~~**Zona de jos, comună**: Terminal / Git (și Remote) în același loc, unul odată,
   comutate din bara din stânga; Git Log și istoricul unui fișier în fereastra Git (tab-uri),
   diff-ul deasupra în editor; fereastra ascunsă nu se desenează, după 5 minute își eliberează
   datele; click pe Git deschide direct log-ul (click dreapta: meniul); Commit în bara Git Log
   după un separator; componenta `ds::loader` (animație și / sau text, culorile temei).~~ ✓
6b. ~~**Task Manager al aplicației**: unde se duc memoria / CPU-ul / GPU-ul — pe module și
   componente (indexul, gramaticile, terminalul, conexiunile remote, vederile), sortabil,
   filtrabil; cele mai consumatoare în față.~~ ✓ (modulul `task-manager`: colector pe firul
   lui, CPU pe fir — macOS / Linux / Windows — atribuit componentelor după numele firelor;
   memoria și GPU-ul raportate prin `ResourceReporter` de aplicație și module; graficele
   ultimului minut, ale aplicației sau ale componentei selectate; din bara de stare.
   De adăugat: memoria indexului și a gramaticilor raportată de ele, defalcarea GPU-ului
   — atlas / imagini / suprafețe —, Linux verificat pe o mașină reală.)
6c. **Task Manager: discul și proiectele** (de discutat înainte de implementare):
   - **Operațiile pe disc** (citiri / scrieri: octeți, rată, fișiere) și **transferurile**
     (remote, lipiri, indexare), pe proiect, raportate de module, nu ghicite: fiecare modul
     știe ce face, deci un serviciu al lui le emite (ca `ResourceReporter` / `Traffic` acum).
     Modulul `task-manager` doar le colectează și le desenează.
   - **Vedere globală**: toate proiectele deschise (toate ferestrele) într-un singur loc,
     grupate pe proiect și pliabile, cu același filtru și aceeași sortare; alegerea între
     proiectul curent și toate.
   - Totul optim: contori atomici în module (fără blocări pe căile fierbinți), citiți de
     colectorul de pe firul lui doar cât fereastra e deschisă; nimic pe UI thread.
6. ~~**FTP / FTPS / SFTP** (și pe Windows).~~ ✓ (modulul `remote-projects`: wizard în modal —
   conexiunea, folderul de pe server, ce se transferă —, copie locală sincronizată sau direct
   pe server (structura citită la nevoie, fișierele aduse la deschidere); la salvare se
   verifică serverul (conflict: compară / urcă varianta mea / ia varianta de pe server);
   Remote › Pull / Push / Sync revizuite, „Adu local”, „Recitește de pe server”; jurnalul
   Transferuri; parole în keychain sau criptate în proiect; reîncercări; testat pe servere
   reale. De adăugat: setările de reîncercare în UI, listarea ținută între sesiuni, căutarea
   în fișierele neaduse direct pe server.)
6d. **Remote: ce așteaptă să fie urcat** (cerere nouă): în fereastra Remote, lângă
   „Transferuri”, un tab „De urcat” cu fișierele schimbate local și neurcate (marcajele
   `Changed` / `Local` deja calculate), cu un buton „Urcă tot” (și pe fiecare); setări — pe
   proiect, cu valoare globală implicită — pentru câte descărcări și câte urcări rulează în
   paralel (azi transferurile unei revizuiri rulează pe un fir; sesiuni SFTP / FTP separate
   pe fiecare fir, limitate). Construit ca indexarea (plan 15): o coadă pe proiect, pe
   magistrala proiectului (`kernel::queue`: un fișier salvat de 5 ori înainte să plece se urcă
   o dată; un lot pleacă grupat), cu urcările și descărcările pe cozi separate.
7. ~~**Istoric local** (versiuni pe fișier, comparare)~~ ✓ (modulul `local-history`: la fiecare salvare + versiunea de dinainte, 30 de zile, comparare / revenire; de adăugat: etichete, urmărirea la redenumire, ora locală în listă)
7b. **Istoricul local în Setări** (și în config): cât timp se păstrează versiunile unui fișier,
   câte versiuni pe fișier până la rotire, cât ocupă acum pe disc (afișat), buton „Șterge
   istoricul local” (purge, cu confirmare).
7c. ~~**Ghidul de memorie** (`docs/rust-memory-optimization-guide.md`)~~ ✓ — trecut prin el,
   măsurat (release, macOS, un proiect PHP de ~15.000 de fișiere indexat, câte 5 rulări,
   `EDITOR_SNAPSHOT_MEMORY=1`): alocatorul sistemului ~184 MB în regim stabil (amprenta
   maximă ~110 MB), mimalloc ~215 MB (~160 MB) — indexare cu ~10% mai rapidă, dar ține
   pagini libere pe fir —, deci pe macOS / Linux rămâne al sistemului; pe Windows mimalloc
   (heap-ul Windows returnează prost memoria; de măsurat pe o mașină Windows),
   `kernel::memory::release_free` cere și alocatorului aplicației să-și returneze paginile.
   Profilul de release avea deja `lto = "thin"` și `codegen-units = 1`; `panic = "abort"`
   nu (modulele care cad sunt prinse cu `catch_unwind`); `strip` nu câștigă nimic (fără
   informații de depanare în release); `opt-level = "s"` nu (un editor vrea viteză); tokio
   are deja doar feature-urile folosite. Rămân: capacități prealocate / mai puține clonări
   pe căile fierbinți (măsurat întâi, pe Task Manager), memoria indexului și a gramaticilor
   raportată.
8. **Baze de date**.
9. Pachete de iconițe, terminal colorat pe temă, `fișier:linie` în căutarea în proiect.

## Rezultate

Măsurat la fel ca la început (release, macOS, același proiect, un fișier deschis, după
indexare):

| Ce | Înainte | După |
|---|---|---|
| CPU în repaus | ~0,5% (7 treziri/s) | 0–0,2% (5 treziri/s) |
| Memorie (physical footprint) | ~200 MB | ~114 MB |
| din care suprafețe GPU (IOSurface) | ~120 MB | ~48 MB |
| vârf la pornire (indexare) | 251 MB | 260 MB (neschimbat: indexarea inițială) |

Rămâne de măsurat CPU-ul la scris (profilare cu un fișier mare).

### Iterația 2 (release, macOS, admin.acme.test — 15.879 fișiere indexate, `vendor` inclus)

| Ce | Înainte | După |
|---|---|---|
| Memorie (phys. footprint, după indexare) | 171 MB | ~157 MB |
| vârf la pornire | 254 MB | 180–217 MB |
| alocări vii (heap) | 108 MB / 851k | 94–98 MB / 810k |
| modelul semantic (64 MB) | încărcat la pornire | la prima căutare, eliberat după 10 min fără căutări |
| pipeline-ul de validare wgpu (~23 MB) | compilat la pornire | deloc (nu desenăm indirect) |
| CPU în repaus | 0% | 0% |

Ce s-a schimbat: validarea apelurilor indirecte wgpu oprită; modelul semantic încărcat la
nevoie (o singură instanță pe proces, asincron) și eliberat la inactivitate; cache-urile
indexului mapate din fișier (nu citite în heap); listele de simboluri și tabelele indexului
fără capacitate de rezervă; membrii privați din `vendor/` scoși din index (nefolosibili din
proiect); memoria eliberată dată înapoi sistemului după indexare (macOS / Linux / Windows).

Ce rămâne (măsurat): indexul de simboluri are ~90k simboluri, ~12 MB de text în ~450k
șiruri mici + tabelele de căutare; următorul pas — șirurile repetate partajate (`container`,
`use`-uri, namespace, limbaj: `Arc<str>` / interning) și `Symbol` mai compact (152 → ~100 B).
