# 28. Review: DDD, decuplare, organizarea fișierelor, înregistrarea modulelor, monitorizare

Cererea: o analiză a codului — respectăm DDD, suntem decuplați, fișierele sunt organizate,
modulele își înregistrează totul, monitorizăm pe fiecare modul CPU / RAM / GPU / rețea / disc.

Analiza: patru treceri prin cod (doar citire), apoi verificate prin sondaj (setarea temei citită
după chei, textele românești din `parse.rs`, extensiile `view-diff` / `view-theme` dublate,
`DEPENDENCY_DIRS` în kernel, rândurile Task Manager-ului scrise de app — toate confirmate).
Fiecare constatare se reverifică în cod când îi vine rândul.

## Ce e bine

- **Dependențele între crate-uri**: `check-deps` trece; niciun modul nu depinde de alt modul
  (37 de `Cargo.toml` verificate), niciun modul nu folosește `ui-workbench`.
- **Domeniul e aproape pur**: nicio citire de fișiere, proces, rețea, fir sau variabilă de mediu
  în `crates/domain/*/src`.
- **Testele**: toate cele 276 de `mod tests;` sunt în `tests/unit/` cu `#[path]`, niciun bloc
  inline, niciun fișier lipsă sau orfan.
- **Fără „lumen” în identificatori** (doar aliasurile vechi permise); fără comentarii
  ne-englezești în `.rs`; zero TODO / FIXME; nicio setare a unui modul citită după chei.
- **Eliberarea**: bucla `module-release` (60 s, `IDLE` 10 min) eliberează stub-urile PHP, datele
  MDN și gramaticile compilate; pool-urile `kernel::Pool` își închid firele după 30 s.

## Ce nu e bine (după gravitate)

### A. Funcționalități ale modulelor ținute de app (decuplare — mare)

1. **Refactorizările**: `app/workbench/refactorings.rs` (609 linii) și `app/refactor_ports.rs`
   (299) au logica de domeniu a redenumirii (declarație vs. folosire, variabile locale, regula
   offset-ului după sigiliu, `$this`), regulile de conflict și de certitudine; `WorkbenchHost`
   expune `rename_at_cursor`, `change_signature_at_cursor`, `move_context*`,
   `apply_refactoring`… → cazurile de utilizare în `refactor` / `code-intelligence`, porturile
   în modulul `refactoring`; host-ul păstrează doar primitive generale.
2. **Git în editor**: blame-ul, „compare with HEAD”, gutter diff-ul (`app/workbench/vcs.rs`,
   `app/gutter_diff.rs`) și metodele git din host (`toggle_annotations`, `committed_text`,
   `annotated_commit`) → `vcs-git` le contribuie prin furnizori de marcaje / adnotări; host-ul
   are doar sloturi generice.
3. **Find Usages / Structure**: `app/usages.rs` (`UsageFinder`) și `self.data.usages` → în
   modulul care le arată.
4. **Compare**: comanda `TREE_COMPARE`, rândurile `compare:` din meniul tabului,
   `open_compare`, `COMPARE_EXT` dublat → contribuite de modulul `compare`.
5. **Theme Studio**: comanda, meniul, `open_theme_studio`, `THEME_STUDIO_EXT` dublat → în modul.
6. **Comenzi dublate în app**: `TOGGLE_TERMINAL`, `TOGGLE_PROJECT`, `PROJECT_TOOL`,
   `SEARCH_PROJECT`, `SEARCH_FILES`, `REVEAL_IN_PROJECT` (definite / scurtate în app, deși sunt
   ale modulelor) → modulele le declară, scurtăturile vin prin `PresetShortcuts`.
7. **Id-uri scrise ca text**: `"remote.open"`, `"tasks.open"` (bara de stare, tooltip),
   `host.view_command("search")` în canvas → puncte de extensie (element în bara de stare,
   acțiune în lista de proiecte) sau constante comune.
8. **Iconițele ferestrelor** (folder, terminal, git, server, graph, compare, cpu…) stau în
   `render/icons`; doar structure, diagrams și twig își aduc SVG-urile → fiecare modul își aduce
   iconițele (`IconSvg`).
9. **Capturile** (`snapshot.rs`) importă `vcs_git`, `local_history`, `diagrams`, `compare` →
   comenzi rulate după nume sau „hook”-uri de captură contribuite de module (prioritate mică).

### B. Cunoștințe de limbaj în domeniu (DDD — mare)

10. `intelligence/usages.rs` (`CSS_SELECTORS` cu `php`, `twig`, `vue`…) și `css.rs`
    (`css_match`: `class="`, `className=`, `classList.add(`) → reguli date de `lang-css` /
    limbajele care pot conține clase CSS.
11. `$`, `\`, `->`, `::class`, `$this`, `function` / `trait` scrise direct în `intelligence`
    (`lib.rs` `word_at`, `fits_kind_plainly`; `engine.rs`; `signature.rs`; `outline.rs`;
    `fuzzy.rs`) și `refactor` (`NameStyle`, `signature.rs::is_call`, alias-ul `as` din
    `moving.rs`) → din `Naming` / `SourceRules` / `SignatureSyntax` / `NamespaceRules`.
12. `kernel/paths.rs`: `vendor`, `node_modules`, `storage`, `var/cache`, `public/build` (Laravel /
    Symfony) → contribuite de module (composer → `vendor`, npm → `node_modules`, framework-urile
    → folderele generate), date ca `ProjectLayout`.
13. `theming/studio.rs`: scope-uri `variable.php`, `property.json` → date ale temei (mic).

### C. I/O și politici dublate în app / infra (mediu)

14. `std::fs` direct în app (`refactor_ports.rs`, `usages.rs`, `workbench/intelligence.rs`,
    `refactorings.rs`, `files.rs`) în loc de portul `FileSystem`; constructorul
    `code_intelligence::Project` copiat de două ori; `MAX_FILE = 2 MiB` de trei ori → un
    `ProjectReader` și o singură politică de mărime.
15. Caractere de declanșare a completării (`$ > : \ #  @`) în `app/workbench/editing.rs`,
    `$` în `problems.rs` / `links.rs` → `LanguageIntelligence::completion_triggers()` și `Naming`.
16. `indexer/search.rs`: lista de extensii căutate și regulile „minified” scrise în infra; `indexer`
    decide „fără membrii privați ai bibliotecilor” → din limbajele înregistrate / domeniu.
17. `syntax-treesitter/parse.rs:756`: texte românești („cheie / chei”, „element / elemente”) în
    infra, ocolind i18n → numărul și felul, textul din locale. Alias-urile de limbaj
    (`js`→`javascript`) din `injection.rs` → de la module.
18. `app/frameworks.rs`: regula de excludere între framework-uri din aceeași familie → domeniu.
19. Setarea temei citită după chei (`app/themes.rs:187-210`, `"appearance"`, `"follow_system"`) →
    `Setting<T>` în `settings.rs`.

### D. Proiecte care se pot încurca (regula „fiecare proiect cu ale lui” — mediu)

20. Pool-uri pe proces, cu un fir: `git-read`, `git-blame`, `usages`, `module-job` (al tuturor
    modulelor), `vcs-git` `git-status`, `remote-pending` → munca unui proiect o poate întârzia pe
    a altuia. Pool / coadă pe proiect (`ProjectData` sau serviciul modulului pe proiect).
21. Stări globale: clipboard-ul arborelui (dublează `clipboard_files` din host), `USAGE` din
    local-history, `RECENT` din căutare (nu se șterge niciodată) → pe proiect, eliberate cu el.

### E. Monitorizarea pe module (mare — e cererea)

Ce există: `ResourceUse` (memorie, GPU, trimis / primit, citit / scris, CPU, fire, activități),
contoare atomice (`DiskIo`, `CpuMeter`, `Traffic`, `Activities`), colectorul Task Manager-ului
(1 s, CPU pe fir din `process_stats`, rândul „altele” pentru firele nerevendicate, RAM-ul
„neraportat” = RSS − suma).

| Modul | CPU | RAM | GPU | Rețea | Disc |
|---|---|---|---|---|---|
| indexing | măsurat | — | — | — | real |
| local-history | măsurat | — | — | — | citit real |
| project-tree | firele `paste` / `trash` | — | — | — | paste |
| remote-projects | firele `remote-` | estimare | — | doar conținutul fișierelor | = fișierele |
| terminal-panel | firele PTY | — | — | — | — |
| task-manager | firul lui | — | — | — | — |
| app „Fișiere deschise” | — | estimare | — | — | salvări |
| app „Randare” | — | — | estimare pe tot | — | — |
| **celelalte 28** (lang-*, fw-*, vcs-git, refactoring, compare, diagrams, canvas, viewer-*, structure, quality, colors, emmet, icon-packs, themes…) | — | — | — | — | — |

Lipsuri:
- **CPU pus pe app** deși e al modulelor: `git-` → „Git”, `image-decode` / `diagram` →
  „Previzualizări”, `refactor-` / `find-usages` → „Refactor”, `graph` (canvas) → „Index”,
  `module-job` → „operații pe fișiere” (rândurile scrise în `app/workbench/task_data.rs`).
- **CPU al nimănui** („altele”): `quality`, `compare-diff`, `gutter-diff`, `history-usage`,
  `process-*`, `remote-retry/sync/transfer/plan/apply`, `stats-probe`.
- **Procese copil necontorizate**: shell-urile terminalului, formatoarele / linter-ele, `git`
  (fetch / pull / push: nici rețea, nici CPU, nici disc).
- **Rețea**: remote numără doar conținutul fișierelor (fără listări, protocol, SSH).
- **Disc**: căutarea în proiect, Find Usages, citirile git / blame, imaginile, markdown-ul,
  încărcarea stub-urilor / datelor, cache-ul listărilor remote.
- **RAM**: indexul, stub-urile PHP, datele MDN, gramaticile compilate, imaginile decodate,
  scrollback-ul terminalului, cache-urile git — nimic raportat.
- **GPU**: o singură estimare pe tot renderer-ul, fără texturile pe vedere / modul.

### F. Organizarea fișierelor (mic — întreținere)

22. Fișiere mari cu mai multe responsabilități: `settings_view.rs` 2646, `editor_view.rs`
    2435, `connect_view.rs` 2334, `app/workbench/mod.rs` 1815, `canvas/lib.rs` 1729,
    `session.rs` 1431, `log_view.rs` 1418, `task-manager/view.rs` 1392, `compare/lib.rs` 1292,
    `theme-studio/lib.rs` 1196 → împărțite pe stare / logică / desenare (modelul din `vcs-git`,
    `refactoring`: `lib.rs` doar înregistrarea).
23. `lang-twig`: fără teste, SVG-ul la rădăcina crate-ului (nu în `svg/`); `lsp-client`,
    `platform-winit` fără teste.
24. Cod nefolosit: `ds::{code_chip, icon_badge, XS}`, `search::embed_all`,
    `refactoring::MOVE_EXTENSION`, `describe_layers`, `layer_count`, `commands::from_specs`,
    `icons::{is_colored, key_styled}`, `kernel::brand::BUNDLE_ID` ș.a. → verificate și scoase.
25. Texte vechi: `cargo test -p lumen` în comentariile din `tests/unit/modules.rs` (pachetul e
    `app`), `brand.toml` pomenește `lumen-*`, `.lumen/` scris direct în ~38 de comentarii,
    `scripts/build.sh` cu mesaje românești.

## Plan (etape, fiecare: commit separat, `check.sh` verde, capturile comparate)

- [x] **28a. Monitorizarea pe module** (E). Făcut:
  - **Firele**: fiecare modul își declară firele (`ModuleThreads`); CPU-ul lor apare pe rândul
    modulului (după rândurile de proiect ale reporterului lui). Scoase din app rândurile
    „Previzualizări” și „Refactor”; „Git” al app-ului ține doar firele lui (`git-read`,
    `git-blame`, `gutter-diff`, până la 28d). Revendicate: `image-decode`, `diagram`, `graph`,
    `compare-diff`, `quality`, `process-*`, `refactor-*`, `find-usages`, `history-usage`,
    `remote-*`, `git-status/log/commit`, `file-index`.
  - **Memoria datelor**: `Module::memory()` — stub-urile PHP, datele MDN (CSS, HTML, JS),
    cântărite (`OnDemand::peek`, `kernel::memory::{vec_bytes, map_bytes}`); indexul de simboluri
    (`SymbolIndex::memory`) pe rândul indexului.
  - **Vederile**: `Viewer::memory()` (RAM + GPU) adunat pe modulul vederii (taburi și ferestrele
    de jos): imaginile (`ImageData::gpu_memory`), Markdown-ul cu imaginile lui.
  - **Programele pornite**: `ResourceUse.processes`; Task Manager-ul măsoară fiecare program cu
    descendenții lui (`process_stats::ProcessTrees`, prin sysinfo): CPU, memorie, disc. Raportate:
    shell-urile terminalului (cu ce rulează în ele), `git` (fetch / pull / push / commit),
    formatoarele și verificările (`process::Running`).
  - **Discul**: `ModuleCounters` (disc / rețea pe modul): imaginile, Markdown-ul, graful
    canvas-ului; în app: Find Usages (pe index), versiunile din git (pe Git).
  - Rămas: traficul protocolului remote (listări, SSH / FTP) — se numără doar conținutul
    fișierelor (ar cere învelirea conexiunii TCP); GPU-ul pe module e doar al texturilor
    vederilor, restul (glife, suprafețe) rămâne la „Randare”.

- [x] **28b. Decuplare ușoară** (A4–A8, C19). Făcut:
  - **Compare**: „Compare with Active File” din meniul arborelui e comanda modulului (citește
    textele prin `host.file_text`); extensia vederii e contractul `diff::COMPARE_VIEW` (app-ul
    și modulul o folosesc, nu o copiază). Submeniul „Compare with ›” al tabului rămâne al
    app-ului (listează taburile lui), deschizând vederea prin contract.
  - **Theme Studio**: comanda e a modulului; temele vin prin `host.themes()` (sursa dată de
    app); un configurator deschis își păstrează ciorna (în vedere); extensia e
    `theming::studio::STUDIO_VIEW`.
  - **Contracte în `workbench-model`** în loc de id-uri copiate / scrise ca text:
    `PROJECT_PANEL`, `TOGGLE_PROJECT_PANEL`, `SHOW_RESOURCES` (bara de stare → Task Manager),
    `OPEN_REMOTE_PROJECT` (lista de proiecte → remote); scurtăturile terminalului pe preseturi
    vin din modul (`PresetShortcuts`).
  - **Setarea temei**: `THEME` / `THEME_FOLLOWS_SYSTEM` (`Setting<T>`), nu chei scrise ca text.
  - Lăsat așa, cu motiv: iconițele ferestrelor rămân în setul comun de interfață (Lucide):
    pachetele de iconițe le înlocuiesc după nume, ca pe toate; `SEARCH_FILES` / `SEARCH_PROJECT`
    / `REVEAL_IN_PROJECT` sunt ale workbench-ului (căutarea e fereastra lui, implementată de
    module prin contract); `view_command("search")` din canvas e acțiunea vederii lui.

- [x] **28c. Limbajul din domeniu** (B10–B13, C15–C18). Făcut:
  - **Hook-uri noi în `LanguageIntelligence`**: `word_chars` (cuvântul de sub mouse, numele
    unui apel), `completion_triggers` (ce caractere pornesc completarea: în app nu mai e
    lista `$ > : \ # @`), `css_references` (cum se referă fișierele unui limbaj la clasele CSS:
    atribute, `classList.add(`, foi de stil); listele de unde se pot folosi selectorii CSS au
    trecut în `lang-css` / `lang-html`.
  - **`Naming`**: `type_name_member` (`::class`); variabilele completate cu sigiliul și
    receptorul limbajului (nu `$` / `$this` scrise în motor); membrii căutați fără sigiliu;
    eticheta proprietăților (breadcrumb, structură) cu sigiliul limbajului — în JS nu mai apare
    `$camp`.
  - **`refactor`**: `NameStyle` e date (caractere în plus, sigiliu), nu `$` scris în cod;
    `CallRules` (accesul la membri, `function`, `new`, sigiliul) date de `SignatureSyntax`;
    alias-ul unui import (`as`) scris de `NamespaceRules::aliased`.
  - **Altele**: `fits_kind_plainly` după `SourceRules`; plierea codului dă un număr
    (`FoldCount`), textul („2 elemente”) din locale — nu mai e română în infra; regula „fără
    membrii privați ai bibliotecilor” e `FileSymbols::keep_public`; alegerea framework-urilor
    (o familie, alegeri) e `code_intelligence::choose_framework`; problemele marcate după
    caracterele limbajului.
  - Folderele de dependențe / generate / nelistate vin din module (`kernel::paths::ProjectDirs`:
    PHP `vendor`, JavaScript `node_modules` și build-urile, Laravel `storage` / `public/build`,
    Symfony `var/cache`), înregistrate la pornire; kernel-ul doar răspunde despre ele.
  - Rămas: aliasurile `js` / `yml` /
    `htm` din injecții (3 intrări în infra); lista de extensii a căutării textuale rămâne
    (e de fișiere text, inclusiv limbaje fără modul); `signature_plainly` (forma „cum o scriu
    cele mai multe limbaje”) și separatorii din potrivirea fuzzy rămân generici.

- [x] **28d. Git în modul** — analizat, lăsat în app, cu motiv: barele din gutter, blame-ul
  și „compare with HEAD” din `app/workbench/vcs.rs` nu știu de git: lucrează prin portul
  `version_control::Repository` (dat de modulul git prin `VcsProvider`) și prin cazurile de
  utilizare ale domeniului (`changed_lines`, `blame_annotations`, `compare_with`). Comenzile
  și regulile git sunt în modul / infra. Mutarea ar cere un punct de extensie cu ciclul de viață
  al fiecărui document (deschis, schimbat, închis) doar ca să mute ce e deja generic.

- [x] **28e. Refactorizările în domeniu** (A1, A3, C14). Făcut:
  - `code_intelligence::rename`: ce redenumește o redenumire la cursor (`subject_at`:
    declarație, folosire prin definiția găsită, variabilă locală, receptorul niciodată),
    numele de la cursor cu sigiliul limbajului (`name_at`), scopul unei variabile locale,
    certitudinea unui loc (`Declared::declares` / `is` / `by_definitions`: sigiliul după limbaj,
    nu `+1` fix) și ce intră în conflict cu un nume nou (`clashes`). Testele mutate în domeniu.
  - App-ul păstrează adaptoarele porturilor (`ProjectSources`, `IndexResolver`,
    `IndexConflicts`): citesc starea lui (bufferele deschise, indexul), deci sunt cablare.
  - I/O: proiectul citit de pe disc pentru inteligență într-un singur loc
    (`app/project_disk.rs`, înainte copiat de două ori); limita fișierelor de cod o singură
    constantă (`workspace::MAX_SOURCE_FILE`, înainte în app ×2, indexer, canvas).
  - Căutarea de utilizări a panoului Structure e a modulului (`structure/src/usages.rs`, pe
    firul lui): `ProjectQueries` dă unde și după ce reguli se caută (`usage_search`) și textul
    unui fișier sursă (`read_source`); din app au plecat `UsageFinder` și `PanelContext.usages`.

- [x] **28f. Proiectele separate** (D20–D21). Făcut: pool-urile fiecărui proiect sunt ale lui
  (`ProjectJobs`: joburile modulelor, citirile git, blame-urile, căutarea de utilizări;
  `Changes` din vcs-git și `Outgoing` din remote își fac pool-ul la prima citire) — munca unui
  proiect nu mai așteaptă după a altuia; firele pornesc cu munca și se închid când stau.
  Lăsate, cu motiv: clipboard-ul arborelui e comun ferestrelor intenționat (copiezi într-o
  fereastră, lipești în alta); căutările recente se uită deja la închiderea proiectului
  (`search_view::forget`); `USAGE` din local-history e după cale (nu se amestecă).

- [x] **28g. Curățenie** (F22–F25). Făcut:
  - **Fișiere mari împărțite** (codul doar mutat, capturile identice): `settings_view` (2646 →
    `mod.rs` 954 + search / appearance / keymap / rows / modules), `editor_view` (2435 → 909 +
    errors / gutter / text / display / scroll), `connect_view` (2334 → 947 + view / paint /
    transfer), `canvas/lib.rs` (1729 → 426 + view / paint / search_paint), `log_view` (1418 →
    398 + commands / view / paint / branches), Task Manager `view.rs` (1392 → 593 + panel /
    paint), `compare/lib.rs` (1292 → 493 + view / diff_job / paint), `theme-studio/lib.rs`
    (1196 → 789 + view / preview).
  - **Cod nefolosit scos**: `ds::code_chip`, `ds::icon_badge`, `SearchIndex::embed_all`,
    `refactoring::MOVE_EXTENSION` (cele folosite doar de teste rămân: le verifică).
  - **Texte**: numele aplicației scos din comentarii (`<project dir>`, `<app id>`, 33 de
    fișiere); `cargo test -p app` în loc de `-p lumen`; `brand.toml` fără `lumen-*`;
    `scripts/build.sh`, comentariul din `ember.toml` și mesajele testelor în engleză.
  - **lang-twig**: iconița în `svg/`, primul test (query-urile compilează, un template se
    parsează).
  - Și restul: `app/workbench/mod.rs` (tab / file_icons / disk / text_util), `ds.rs` (badges /
    controls / feedback / text / scroll, reexportate: același API), `session.rs` (service /
    resources), `lang-php/intelligence.rs` (extract / context), `diagrams/lib.rs` (view / paint).
