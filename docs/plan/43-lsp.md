# 43 — Serverele de limbaj (LSP) ca module (planul 43)

Cerut (2026-11-02): LSP pentru CSS, HTML, JavaScript/TypeScript, SCSS (implicit), limbaje noi
(C/C++, Lua, Zig, Bash, YAML, TOML, Docker) ca la openidea, PHP deschis pentru un LSP; serverele
și Node vin **în installer** (userul nu instalează nimic), totul vizibil în Task Manager;
„foarte important să fim optimi”; dezvoltarea pe un **branch nou**.

## Ce face openidea (verificat în cod)

- `languages/*.toml`: pe limbaj, `[[language-servers]]` (name, command, args, root-markers,
  install-hint, `fallback`); `ide-lsp`: un server pe (nume, rădăcină), pornit la primul fișier,
  rezultatele mai multor servere combinate, repornire cu backoff după crash.
- `ide-tools`: **nu le pune în installer** — le instalează la cerere în
  `<app-data>/tools/<nume>/` (pachetele npm în `<app-data>/tools/node_modules`, **cu npm-ul
  sistemului**), binarele (clangd, lua-language-server…) din release-urile GitHub, sau le găsește
  în `PATH`; o pagină „Language Servers” (instalează / dezinstalează / versiune / cale aleasă).
- Noi mergem mai departe, cum ai cerut: **totul în installer**, inclusiv Node — nimic de
  instalat, nimic descărcat la prima folosire, nimic cerut de la sistem.

## Principii

1. **Module, ca acum.** `crates/infra/code/lsp-client` (crate-ul gol rezervat de la început: „adaptor
   pentru porturile Intelligence”) devine clientul generic; fiecare limbaj e un modul de limbaj
   (`crates/modules/languages/<limbaj>`) care își declară, în date, gramatica tree-sitter și
   serverul; un modul nou `lsp` (tools) are managerul de procese, starea și pagina din setări.
   Modulele nu depind unele de altele; aplicația doar le leagă.
2. **Pe proiect.** Un server pe (server, rădăcina proiectului), pornit la primul fișier al
   limbajului, oprit când proiectul se închide sau după inactivitate; diagnosticele, progresul,
   erorile pe bus-ul proiectului; nimic dintr-un proiect nu ajunge în altul.
3. **Leneș și eliberat.** Niciun proces pornit fără un fișier deschis al limbajului; oprit după
   N minute fără fișiere ale lui deschise (setare, implicit 10); Node pornit doar ca proces al
   serverului care îl cere, niciodată „în plus”.
4. **Nimic pe firul UI.** Comunicarea pe un fir per server; cererile depășite anulate
   (`$/cancelRequest`); tastarea nu așteaptă niciodată un server.
5. **Tree-sitter rămâne** pentru colorare, pliere, structură, selecție: instant, fără server.
6. **Codul încorporat rămâne al nostru** (CSS/JS în HTML, Blade, Twig, PHP; SQL în heredoc):
   serverele LSP nu-l înțeleg singure; fișierele întregi merg la LSP.

## Ce intră în installer (`scripts/build.sh`, `packaging/`)

- **Node** (runtime-ul oficial, versiunea LTS, per platformă: macOS arm64/x64, Windows x64,
  Linux x64), folosit doar de serverele care îl cer.
- **Servere Node**: `@vtsls/language-server` + `typescript` (JS/TS), `vscode-langservers-extracted`
  (HTML, CSS, SCSS, LESS, JSON), `bash-language-server`, `yaml-language-server`,
  `dockerfile-language-server-nodejs` (+ `@microsoft/compose-language-service` pentru compose).
- **Servere native**, per platformă: `clangd` (C/C++), `lua-language-server`, `zls` (Zig),
  `taplo` (TOML).
- Toate cu versiuni fixate și sume de control într-un manifest (`packaging/lsp.toml`),
  descărcate de `cargo xtask lsp-bundle` la build (nu la rulare), cu licențele lor în
  `NOTICE` (toate permisive: MIT / Apache-2.0 / Apache-2.0 cu excepția LLVM).
- Locul: **o singură dată, lângă executabilul aplicației**, organizat pe foldere (decis
  2026-11-02): `lsp/node/<versiune>/`, `lsp/servers/<server>/<versiune>/`, `lsp/manifest.toml`
  (`<bundle>/Contents/Resources/lsp/` pe macOS, `lsp\` lângă `.exe` pe Windows), doar citire,
  folosit de toate proiectele; nimic copiat sau instalat per proiect. Doar starea fiecărui
  server pe proiect (cache-uri / indexuri, unde serverul are) stă în cache-ul aplicației, pe
  proiect, ștearsă cu „Clear and Rebuild Caches”.
- În dezvoltare (`cargo run`), același layout sub `target/lsp/` (pus de `cargo xtask
  lsp-bundle`), găsit lângă binar.
- **Mărime** (estimare de verificat): Node ~30–45 MB, serverele Node ~40 MB, clangd ~30–40 MB,
  lua-ls ~10 MB, zls ~5 MB, taplo ~10 MB ⇒ **~+130–150 MB** pe platformă (comprimat mai puțin).
  Se măsoară la 43a și se raportează.
- Actualizări: un workflow lunar (ca la stub-urile PHP) care propune versiuni noi ale
  serverelor, cu testele rulate pe ele.

## Clientul (`lsp-client`)

- JSON-RPC pe stdio; inițializare cu capabilitățile noastre; `workspace/configuration`,
  `workspace/didChangeWatchedFiles` din watcher-ul proiectului (fără watcher propriu al
  serverului), `window/workDoneProgress` → starea în bara de jos.
- Sincronizare incrementală, cu conversia pozițiilor (UTF-16 ↔ bytes) testată exhaustiv;
  `didChange` grupat (debounce mic), versiuni pe document.
- Funcții mapate pe porturile Intelligence: completion (+ resolve, snippets, textEdit,
  additionalTextEdits / importuri), hover (markdown în cardul nostru), signature help,
  definition / declaration / type definition / implementation, references, document symbols
  (structura), rename (prin refactorizări, cu undo și previzualizare), code actions (bec),
  formatting / range formatting, diagnostice (push și pull) în lista și banda de erori de
  acum, inlay hints (opțional), document highlight (ocurențele).
- Mai multe servere pe un limbaj (ex. TS + ESLint mai târziu): rezultatele combinate.
- Crash: repornire cu pauze crescătoare; după 3 căderi în 3 minute, oprit și arătat în Task
  Manager cu „Repornește”.
- Limite: memoria Node (`--max-old-space-size`, setare per server), numărul de elemente de
  completare, timeouts per cerere.

## Limbajele

- **Implicit prin LSP**: JavaScript, TypeScript, TSX, CSS, SCSS, HTML, JSON (fișiere întregi).
  Motorul nostru rămâne pentru codul încorporat și ca rezervă (server oprit / căzut / încă
  pornește): fără „nimic” niciodată.
- **Noi**: C/C++ (clangd; `compile_commands.json` detectat, altfel flags implicite), Lua
  (lua-language-server), Zig (zls; folosește `zig` din PATH dacă există pentru build-on-save,
  altfel doar analiza), Bash (bash-language-server; shellcheck nu e inclus — de discutat),
  YAML (yaml-language-server, cu SchemaStore offline), TOML (taplo, cu scheme pentru
  `Cargo.toml` / `pyproject.toml`), Docker (Dockerfile + compose).
  Fiecare: un modul de limbaj cu gramatica tree-sitter (vendored în `third_party`, cum e
  tree-sitter-sql), query-uri de colorare / pliere / structură, iconițe, descrierea serverului.
- **PHP**: motorul nostru rămâne principal. Clientul e pregătit pentru un server opțional
  (phpactor sau intelephense, de ales mai târziu — planul 43 nu-l include în installer).

## Task Manager

- Fiecare server: un rând sub proiectul lui — numele, limbajul, PID, starea (pornește /
  indexează X% / gata / inactiv / căzut), memoria (RSS, inclusiv procesele copil, ex. tsserver
  sub vtsls), CPU % și CPU total, de când rulează, numărul de documente deschise, cererile pe
  minut și latența p50 / p95, ultima eroare.
- Acțiuni: Repornește, Oprește, Arată logul (stderr + traficul LSP, opțional, în fereastra de
  jos), Deschide setările serverului.
- Măsurat din procesul aplicației ca până acum (sampler-ul existent, 0 când e ascuns).

## Setări

- Pagina „Limbaje → Servere de limbaj”: pe server — pornit / oprit, implicit / motorul nostru
  (unde avem unul), memoria maximă, argumente în plus, „folosește alt executabil” (calea);
  oprirea după inactivitate. Prin `Setting<T>` / `SettingsContribution`.

## Optim (bugete, măsurate și testate)

- 0 procese pornite la deschiderea unui proiect fără fișierele limbajului.
- Tastarea: 0 blocări pe firul UI; o cerere de completare veche anulată la tasta următoare.
- RAM: raportat per server în Task Manager; tsserver plafonat; servere oprite la inactivitate.
- CPU în repaus: 0 de la client (fără polling); serverele măsurate în repaus.
- La 43a: comparația pe un proiect TypeScript real (ca la PHP): motorul nostru vs vtsls —
  corectitudine, latență, RAM, CPU.

## Pașii (pe branch-ul `lsp`)

- **43a — clientul și managerul**: `lsp-client` (protocol, sincronizare, mapare), modulul
  `lsp` (procese, stare, Task Manager, setări), manifestul și `cargo xtask lsp-bundle`,
  `build.sh` care le pune în bundle, primul limbaj: **JavaScript / TypeScript** (vtsls) +
  comparația. Teste cu un server fals (in-memory) și cu cele reale.
- **43b — web**: HTML, CSS, SCSS, JSON prin `vscode-langservers-extracted`, implicit; codul
  încorporat rămâne al nostru.
- **43c — limbaje noi 1**: C/C++, Lua, Zig.
- **43d — limbaje noi 2**: Bash, YAML, TOML, Docker.
- **43e — măsurători și imagini**: fiecare server în release (pornire, RAM, CPU, latență),
  Task Manager cu servere, setările (alegerea motorului, „+ Adaugă un server”), imaginile de test.
- **43f — propriul nostru server** (opțional, la final): motorul PHP expus ca server LSP
  (`language-server`), testat cu clientul nostru.
  **Făcut 2026-11-09:** `crates/language-server` (PHP; un limbaj nou = un modul în
  `engines::modules`), peste `lsp-protocol`: completion, hover, signature help, definition,
  references, document symbols, diagnostice push; sincronizare incrementală. Testat cu clientul
  nostru (`tests/with_our_client.rs`). Nivelul PHP din `composer.json`; indexul în folderul de date
  al aplicației, nimic în proiect; inclus în build-urile `--with-servers`. **Nefăcute, intenționat:**
  rename (locurile lui Find Usages merg după nume: ar schimba și metoda omonimă din altă clasă;
  cel bun e în `refactor` și cere porturi), formatare (motorul nu are formatter propriu).

Alegerea motorului per limbaj și serverele descrise în date / adăugate de user intră încă de
la 43a (clientul e construit în jurul lor), nu la final.

Fiecare pas: un commit, `check.sh` verde, Windows inclus; nimic din ce merge acum nu se strică
(motorul nostru rămâne rezerva).

## Alegerea motorului și adăugarea ușoară a unui server (cerut 2026-11-02)

„Să nu pierdem nimic din ce avem deja făcut, să am și opțiune ce să folosim, default sunt
serverele LSP; o interfață ca să fie ușor să adaug repede ceva nou, un alt server sau chiar
propriul nostru server.”

- **Un singur contract pentru „furnizorii de inteligență”** (porturile Intelligence existente):
  motorul nostru de acum (PHP, JS, CSS, HTML, SCSS…) e un furnizor, fiecare server LSP e un
  furnizor (prin adaptorul `lsp-client`). Nimic din motoarele noastre nu se șterge: rămân
  furnizori de sine stătători, testați ca până acum.
- **Alegerea per limbaj** (Setări → Limbaje → <limbaj> → „Motorul”): „Server LSP (<nume>)”
  (implicit unde avem server), „Motorul inclus” (al nostru), „Amândouă” (rezultatele
  combinate: completarea unită și deduplicată, diagnosticele ambelor, definiția de la primul
  care răspunde). Schimbarea se aplică pe loc, fără repornirea aplicației; motorul neales nu
  se încarcă / serverul neales nu pornește.
- **Un server nou = un fișier de date**, fără cod: `languages/<limbaj>.toml` (sau un fișier al
  userului în folderul de configurare) cu `[[language-servers]]`: `name`, `command` (relativ la
  `lsp/` din aplicație sau absolut), `args`, `env`, `root-markers`, `initialization-options`,
  `settings`, `file-patterns`, `fallback`. Userul poate adăuga un server al lui din Setări
  („+ Adaugă un server”: comanda, limbajele, argumentele) — apare imediat în alegerea de mai
  sus și în Task Manager.
- **Propriul nostru server**: motoarele noastre expuse și ca server LSP (un binar mic,
  `language-server`, din aceleași crate-uri — de ex. PHP), ca să poată fi folosite și din alte
  editoare și ca să testăm clientul contra lor; în aplicație rămân în proces (fără costul unui
  proces separat). Pas separat, la final (43f), dacă timpul permite.

## Decis (2026-11-02, „go”)

- Totul în installer, o singură dată lângă aplicație, organizat pe foldere.
- LSP implicit la JS / TS / CSS / SCSS / HTML / JSON; limbaje noi C/C++, Lua, Zig, Bash, YAML,
  TOML, Docker.
- shellcheck pentru Bash: inclus doar după verificarea licenței (GPL-3, program separat).
- ESLint / Prettier: doar din `node_modules`-ul proiectului, dacă le are; nimic inclus.
- Dezvoltarea pe branch-ul `lsp`: `main` rămâne neatins; userul decide la final dacă se unește.

## Stare

### 43a — clientul, managerul, bundle-ul, JS/TS prin vtsls (2026-11-02, branch `lsp`)

**Făcut.**

- **Portul** (`code-intelligence::servers`, domeniu): `LanguageServers` — `served`, `complete` /
  `resolve`, `hover`, `signature_help`, `definition`, `references`, `rename_places`,
  `highlights`, `symbols`, `format`, `code_actions` / `run_action`; `Engine` (Server / BuiltIn /
  Both); `Reply<T>` — răspunsul vine pe alt fir, UI-ul doar îl întreabă (`poll`), un fir de
  fundal îl poate aștepta (`wait`), aruncat nerăspuns trimite `$/cancelRequest`;
  `ServedCompletion` + `rank` (fuzzy pe `filterText`, apoi ordinea serverului) +
  `merge_completions` (Both: ale serverului întâi, fără dubluri); `ServedLocation`,
  `WorkspaceChange`, `CodeAction`; `ServerData` (TOML-ul unui modul de limbaj, înregistrat de
  app, citit de modulul serverelor — niciun modul nu-l numește pe altul). `CompletionItem` are
  `replace` (intervalul serverului), `edits` (importuri), `resolve`.
- **Documentele pe bus-ul proiectului** (`document::events`): `DocumentOpened / Changed /
  Closed` cu instantaneul textului (O(1)); aplicația le anunță (`announce_documents`: la fiecare
  editare și înaintea fiecărui cadru, o dată pe cale chiar dacă fișierul e în două grupuri),
  modulul le urmează — nimeni nu-l cheamă.
- **`lsp-client`** (infra): JSON-RPC (`jsonrpc`), textul așa cum îl are serverul (`text`:
  `Lines`, `Mirror` — o singură schimbare incrementală per versiune, poziții UTF-8 / UTF-16 /
  UTF-32 negociate la `initialize`, testate exhaustiv cu proptest), `uri` (Windows: `/C:/`,
  `/c%3A/`), `snippet`, `markdown` (hover → semnătura + documentația cardului), `spec`
  (formatul de date), `bundle` (`lsp/` lângă aplicație), `convert` (completare cu
  `textEdit` / `itemDefaults` / snippet / `additionalTextEdits`, hover, signature help cu
  offset-uri UTF-16, locații și link-uri, highlights, simboluri ierarhice, workspace edit cu
  redenumiri de fișiere, diagnostice în caractere, code actions), `status` (stare, PID, uptime,
  documente, cereri/minut, p50/p95, ultima eroare, progres, log de 2000 de linii), `launch`
  (proces prin `process::start`: grup de procese, fără consolă pe Windows, arborele oprit la
  drop; sau o funcție — testele), `client`: un fir „driver” per server (`lsp-<nume>`), plus
  scriitor / cititor / stderr; schimbările grupate (25 ms) și trimise mereu înaintea unei
  întrebări; o întrebare nouă de același fel anulează pe cea veche; timeout pe cerere;
  `workspace/configuration`, `workspace/applyEdit`, `$/progress`, `window/logMessage`;
  crash → repornire după 1 s, 2 s, 4 s…, documentele redeschise; 3 căderi în 3 minute →
  oprit până la „Repornește”; fără documente N minute → se oprește singur; fără termene de
  așteptat firul doarme (0 CPU). `fake`: un server LSP de test (în memorie sau proces).
- **Modulul `lsp`** (`crates/modules/tools/lsp`, crate `language-servers`, id `lsp`): catalogul
  (datele modulelor + `~/.config/<app>/language-servers.toml` al userului, unde un nume îl
  înlocuiește pe cel al modulului), `projects` (un server per (server, rădăcină) per proiect,
  pornit la primul document al limbajului al cărui motor e serverul, oprit la închiderea
  proiectului, la inactivitate, sau pe loc când motorul limbajului nu mai e al lui), portul,
  diagnosticele pe bus (`lsp-<server>`, golite la crash / oprire), `applyEdit` ca refactorizare
  (un undo), Task Manager (`report`), setările (`page`, `settings`), comenzile (`Show Code
  Actions` ⌘., `Format with Language Server` ⇧⌥F, `Restart Language Servers`, `Show Language
  Server Log`), vederea de log (grupul de jos).
- **Setările** (`[language-servers]`): `engines` (`typescript=built-in`,
  `tsx=server:<nume>`, `javascript=both`; lipsă = serverul), `idle_minutes` (2/5/10/30/60/niciodată, implicit 2 din 2026-11-09,
  implicit 10), `max_memory` (1–8 GB, implicit 3 GB: `--max-old-space-size` al tsserver prin
  `typescript.tsserver.maxTsServerMemory`), `trace`, `disabled`. Blocurile „Servere de limbaj”
  (limbajul, motorul: „Server (vtsls)” / „Motorul inclus” / „Amândouă”, starea serverului cu
  Repornește, memoria, inactivitatea, traficul) și „Adaugă un server” (nume, comandă, limbaje,
  argumente → scris în fișierul userului, apare imediat ca motor). Se aplică pe loc.
- **Aplicația**: completarea (doar serverul: lista lui, filtrată din răspuns pe măsură ce se
  tastează, cerută din nou doar la alt început de cuvânt sau listă incompletă; Both: unite),
  importul unei sugestii rezolvat după acceptare, hover, semnătura (la `(` / `,`), Go to
  Declaration (sare la răspuns; niciunul → motorul inclus), ocurențele simbolului de sub cursor,
  Rename / Find Usages prin calea de refactorizare existentă (previzualizare + undo: locurile
  serverului, așteptate pe firul de fundal al previzualizării, găsite din nou după linie și
  coloană — CRLF-ul nu strică nimic), diagnosticele (în lista, banda și tooltip-urile de acum;
  unde serverul singur răspunde, erorile de sintaxă și analizele motorului inclus nu mai apar de
  două ori și nici nu se calculează — datele lui nu se încarcă). Codul încorporat (JS în HTML /
  PHP / Blade / Twig) rămâne pe motorul inclus; la fel cât timp serverul pornește / e oprit /
  a căzut.
- **Task Manager**: grupul „Servere de limbaj”, câte o parte per server sub proiectul ei —
  procesul (memoria și CPU-ul copiilor: tsserver sub vtsls), detaliul cu limbajele, starea
  (pornește / indexează X% / gata / inactiv / repornește / căzut / oprit), PID, de când rulează,
  documente, cereri pe minut, latența p50 / p95, ultima eroare, versiunea; acțiunile Repornește,
  Oprește, Arată logul, Setări (`ActOutcome::Command`). Butoanele părții trec pe rândul
  următor când nu încap.
- **Bundle-ul**: `packaging/lsp.toml` (Node 26.13.0 LTS de pe nodejs.org cu SHA-256 fixat pe
  platformă; vtsls 0.3.0 + TypeScript 5.9.3 din `packaging/lsp/vtsls/package-lock.json`, toate
  pachetele cu SHA-512, doar registry.npmjs.org, `npm ci --ignore-scripts`), `cargo xtask
  lsp-bundle [--platform <p>|all] [--out <dir>]` → `lsp/manifest.toml`, `lsp/node/<v>/`,
  `lsp/servers/vtsls/<v>/`, `lsp/NOTICE`; în dezvoltare `target/lsp/` (găsit lângă binarul din
  `target/debug`; binarele de test din `deps/` nu-l găsesc, ca testele să nu pornească servere
  reale din întâmplare). `build.sh` / `build.ps1` îl pun în `Contents/Resources/lsp` (macOS;
  universal: cele două Node unite cu `lipo`), `lsp\` lângă `.exe`, `lsp/` lângă binarul Linux.
  Pe Windows serverele Node pornesc direct cu `node.exe` și fișierul JS, arborele oprit cu
  `taskkill /T`. Licențele: `packaging/lsp/NOTICE` (MIT, Apache-2.0, ISC). Build-urile dist nu
  au fost rulate (le rulează userul).
- **JavaScript / TypeScript / TSX prin vtsls, implicit**: `crates/modules/languages/javascript/
  data/language-servers.toml` (fără root-markers → rădăcina proiectului: un singur vtsls per
  proiect; TypeScript-ul inclus, nu al proiectului; 200 de sugestii pe listă; fără descărcări
  automate de tipuri — `disableAutomaticTypeAcquisition`).

**Mărimea lui `lsp/`** (măsurată, `cargo xtask lsp-bundle --platform all`):

| Platforma | Node | vtsls + TypeScript | Total | Arhivat (zip -9) |
|---|---|---|---|---|
| macOS arm64 | 141 MB | 29 MB | 168 MB | 49 MB |
| macOS x64 | 143 MB | 29 MB | 170 MB | 50 MB |
| Linux x64 | 144 MB | 29 MB | 171 MB | 52 MB |
| Windows x64 | 96 MB | 29 MB | 123 MB | 42 MB |

Node e mult peste estimarea planului (30–45 MB): programul oficial `node` de 26.x are singur
~141 MB pe macOS. Ținem doar programul și licența lui (fără npm, headere, man).

**Comparația TypeScript** (motorul nostru vs vtsls prin clientul nostru; proiectul public
`sindresorhus/ky` 2.2.0 clonat în scratchpad, 99 de fișiere TS, cu `source/probes.ts` scris
pentru sonde și un `tsconfig.json` modern în locul celui care extinde un pachet neinstalat;
58 de sonde cu răspunsul așteptat, fiecare întrebată de 3 ori; harness-ul Rust
`scratchpad/ts-compare/harness`):

| Categoria (sonde) | al nostru ok / parțial / nimic / greșit | vtsls ok / parțial / nimic / greșit |
|---|---|---|
| completion (15) | 2 / 0 / 13 / 0 | 14 / 1 / 0 / 0 |
| definition (12) | 4 / 0 / 4 / 4 | 12 / 0 / 0 / 0 |
| hover (10) | 0 / 0 / 8 / 2 | 10 / 0 / 0 / 0 |
| signature (6) | 2 / 0 / 3 / 1 | 6 / 0 / 0 / 0 |
| references (5) | 2 / 3 / 0 / 0 | 5 / 0 / 0 / 0 |
| rename (5) | 4 / 1 / 0 / 0 | 5 / 0 / 0 / 0 |
| diagnostics (5) | 2 / 0 / 3 / 0 | 5 / 0 / 0 / 0 |
| **toate (58)** | **16 / 4 / 31 / 7 (27% corect)** | **57 / 1 / 0 / 0 (98% corect)** |

| Cererea | al nostru p50 / p95 ms (prima) | vtsls p50 / p95 ms (prima) | al nostru p50 / p95 ms (caldă) | vtsls p50 / p95 ms (caldă) |
|---|---|---|---|---|
| completion | 0,00 / 0,04 | 1,42 / 5,52 | 0,00 / 0,03 | 0,86 / 3,37 |
| definition | 0,28 / 0,33 | 0,58 / 2,26 | 0,27 / 0,31 | 0,34 / 0,79 |
| hover | 0,23 / 0,56 | 0,64 / 3,81 | 0,22 / 0,27 | 0,36 / 1,08 |
| signature | 0,26 / 0,27 | 0,91 / 2,13 | 0,26 / 0,26 | 0,54 / 0,94 |
| references | 140 / 1275 | 4,79 / 16 | 142 / 1291 | 1,68 / 3,36 |
| rename | 2,73 / 141 | 1,26 / 2,45 | 2,24 / 142 | 1,19 / 1,60 |
| diagnostics (până la publicare) | 1,04 / 2,02 | 815 / 894 | 1,02 / 1,10 | ≤ 2000 (nimic nou de publicat) |

- Singurul parțial al lui vtsls: `document.` fără prefix — lista e tăiată la 200 de sugestii
  (setarea noastră `entriesLimit`), `querySelector` vine la prima literă tastată (lista e
  incompletă, cerută din nou).
- Al nostru: fără inferență de tipuri, completarea după `.` e goală aproape peste tot; definiția
  unui import duce la linia importului; referințele între fișiere găsesc doar declarația.
  „Greșit” la semnătura `padStart` e numele parametrului din datele MDN (`targetLength`), nu din
  `lib.d.ts`.
- **Resurse** (macOS arm64): vtsls gata în 306 ms de la pornire, primele diagnostice ale
  proiectului la 754 ms, total rece 1,08 s; 3 procese (vtsls + tsserver semantic + tsserver
  „partialSemantic”), RSS 522 MB după încărcare, 579 MB după sonde (plafonat de
  `max_memory`); CPU în repaus (30 s): serverul 0,5%, clientul nostru 0,007%. Motorul nostru:
  indexul în 175 ms (0,14 s CPU), +15 MB RSS, 0% CPU în repaus.

**Optim, verificat de teste**: niciun proces la deschiderea unui proiect fără fișiere ale
limbajului (`nothing_starts_without_a_document_of_a_served_language`,
`a_project_without_files_of_a_served_language_starts_no_server`); tastarea nu așteaptă un
server blocat (`typing_never_waits_for_a_hung_server`, în client și în aplicație: 200 de
schimbări de 256 KB și 200 de întrebări sub 500 ms, 200 de taste + cadre sub 3 s cu serverul
care nu citește nimic); clientul nu face polling (driverul doarme până la un termen; 0,007% CPU
măsurat); tsserver plafonat (`max_memory`); RSS-ul fiecărui server în Task Manager.

**Teste**: `lsp-client` 96 (jsonrpc, text + proptest UTF-8/16/32 și sincronizare, uri, snippet,
markdown, spec, bundle, convert, status, client contra serverului fals — ciclul de viață,
sincronizarea, fiecare întrebare, anularea, timeout-ul, crash / repornire / 3 căderi, server
blocat, inactivitate, fișiere urmărite, progres, configurare —, plus vtsls real dacă
`target/lsp` există); modulul `lsp` 24 (catalog, setări, pagina de setări, adăugarea unui
server, proiecte, port, Task Manager); aplicația 8 (anunțarea documentelor, completare cu
import, hover / semnătură / ocurențe, definiție, redenumire, diagnostice, server blocat);
domeniul 12 (`Reply`, `rank`, `merge`); Task Manager 1 (listarea unui server). Niciun test nu
atinge `~/.config`, keychain-ul sau `recent.toml`, niciunul nu folosește rețeaua.

**Imagini** (noi): `lsp-completion` (un fișier TS cu serverul fals ca proces — diagnosticele
`TODO` / `FIXME` și completarea după `console.`), `task-manager-servers` (scenariul demo:
vtsls cu node + tsserver și un server căzut), `settings-language-servers` (motorul pe limbaj),
`settings-language-servers-add` („Adaugă un server”). Comparate cu referința (`ui-snapshots.sh
compare`), se schimbă doar, așteptat: `settings-4` (pagina „Calitatea codului” începe acum cu
blocul serverelor), `settings-8` și `about` (un modul în plus: numărul lor), iar `task-manager*`
doar cifra de memorie vie din bara de jos; restul sunt identice.

**Abateri**:

- Blocurile de setări stau pe pagina „Calitatea codului” (uneltele limbajelor), nu pe o pagină
  nouă „Limbaje → Servere de limbaj”: o pagină nouă ar fi mutat navigarea din toate imaginile
  `settings-*`.
- Alegerea motorului e un singur `Setting<Vec<String>>` (`engines`), nu câte un `Setting<T>` pe
  limbaj: limbajele vin din date, iar un `Setting` are cheia fixă.
- Simbolurile documentului sunt mapate și expuse prin port, dar Structura rămâne pe tree-sitter
  (principiul 5); code actions și formatarea sunt comenzile modulului, nu becul / „Reformat
  File”; pull diagnostics și inlay hints nu sunt încă.
- Un singur server per proiect pentru vtsls (fără root-markers); `root-markers` rămân în format
  pentru servere ca clangd.

**Pentru 43b–43f**: un server nou = un `[[language-servers]]` în `data/language-servers.toml`
al modulului de limbaj (contribuit ca `ServerData`) + o intrare în `packaging/lsp.toml`
(server Node: un `package.json` + lockfile în `packaging/lsp/<server>/`; server nativ: de
adăugat descărcarea per platformă în `xtask/src/lsp_bundle.rs`, ca la Node).

### 43b — web: HTML, CSS, SCSS, JSON prin `vscode-langservers-extracted` (2026-11-02, branch `lsp`)

**Făcut.**

- **Serverele**: `vscode-langservers-extracted` 4.10.0 (serverele HTML, CSS și JSON,
  MIT), un singur pachet pentru trei servere: `vscode-css` (CSS + SCSS, un proces pe proiect
  pentru amândouă), `vscode-html`, `vscode-json`, descrise ca date în
  `crates/modules/languages/{css,html,json}/data/language-servers.toml` (contribuite ca
  `ServerData` de modulele de limbaj; modulul JSON depinde acum de `code-intelligence` doar
  pentru asta), pornite cu Node-ul aplicației, cu heap-ul plafonat la limita de memorie din
  setări (`--max-old-space-size=${max-memory}`), formatarea răspunsă (`provideFormatter`).
  **Implicit prin LSP** pentru `css`, `scss`, `html`, `json`; „Motorul inclus” și „Amândouă”
  rămân în setări, pe limbaj, ca la JS (blocul „Servere de limbaj” le listează singur, din
  catalog). LESS nu există ca limbaj în editor (nicio gramatică / modul): nu e adăugat; serverul
  CSS l-ar servi cu un `"less"` în `languages` când va exista modulul.
- **Bundle-ul**: `packaging/lsp/vscode-langservers-extracted/` (package.json + lockfile: 33 de
  pachete, toate de pe registry.npmjs.org cu SHA-512), intrarea în `packaging/lsp.toml`, cu două
  lucruri noi în `cargo xtask lsp-bundle`: `prune` (căi relative la folderul serverului, `*` un
  nume: din TypeScript 4.9 — încărcat de serverul HTML pentru `<script>`, dezactivat la noi —
  rămâne doar biblioteca, 64 → 13 MB; testele pachetului) și `[[servers.files]]` (fișiere de
  date descărcate o dată în `target/lsp-cache/files/<sha256>`, doar de pe
  raw.githubusercontent.com, la un commit / tag fixat, SHA-256 verificat, puse în folderul
  serverului: schemele JSON). Licențele în `packaging/lsp/NOTICE` (MIT, Apache-2.0,
  BSD-2-Clause, ISC).
- **JSON fără rețea**: serverul tratează singur doar `file:` (`handledSchemaProtocols =
  ["file"]`); orice alt URL îl cere clientului (`vscode/content`), care răspunde din câmpul nou
  `contents` al descrierii (prefix de URL → folder; numele fișierului = restul URL-ului, fără
  `..`, `\`, `:`; fragmentul și query-ul ignorate) și refuză restul („not bundled (downloads are
  off)”, scris și în logul serverului). Scheme incluse (`schemas/` lângă server, 1,4 MB):
  `package.json`, `tsconfig*.json`, `jsconfig*.json`, `composer.json` (al Composer-ului, tag
  2.9.8, MIT), `.eslintrc*`, `.prettierrc*`, `.babelrc*` / `babel.config.json`, `.stylelintrc*`
  — SchemaStore la commitul `1f9ebc83` (Apache-2.0) — plus toate schemele la care trimit
  (ava, nodemon, semantic-release, jscpd, madge, quikrun, partial-eslint-plugins): 15 fișiere,
  niciun „Problems loading reference”. `tsconfig.json`, `jsconfig.json`, `.eslintrc`, `.babelrc`,
  `.jshintrc`, `.swcrc`, `devcontainer.json` și `*.jsonc` merg ca `jsonc` (comentariile permise)
  — câmp nou `file-language-ids` (glob pe numele fișierului, înaintea extensiei). Serverele CSS
  și HTML nu citesc nimic din rețea (doar `file:`; nicio setare de descărcare).
- **Codul încorporat rămâne al nostru** (principiul 6): serverul HTML pornește cu
  `embeddedLanguages = {css: false, javascript: false}` și `html.validate.scripts/styles =
  false`; în `<style>` / `<script>` aplicația întreabă tot motorul inclus (ca la 43a).
- **Nimic pierdut: ce păstrează motorul inclus** — câmp nou `built-in` în descriere
  (`code_intelligence::servers::Kept`, portul `LanguageServers::kept`): `diagnostics` (HTML:
  serverul n-are diagnostice proprii, deci erorile de sintaxă și elementele învechite ale
  motorului nostru rămân, analizele lui se calculează mai departe), `deprecated` (CSS / SCSS:
  serverul nu marchează proprietățile învechite — `clip`, `zoom`… —, ale noastre rămân lângă
  diagnosticele lui), `rename` (CSS / SCSS: Rename și Find Usages rămân ale motorului nostru,
  care găsește clasa / id-ul / variabila și în HTML, PHP, Blade, Twig; serverul doar în foaia de
  stil). În plus, general: unde serverul nu are nimic de spus, răspunde motorul inclus — la
  completare (nou: o listă goală a serverului → a noastră, de ex. clasele proiectului în
  `class="…"`, pe care serverul HTML nu le știe), la hover și la declarație (ca la 43a), la
  redenumire (nou: niciun loc de la server → ale noastre, de ex. o clasă din HTML).
- **Clientul**: redenumirea unui nume mai lung decât cuvântul de sub cursor (`--gap`,
  `card-title`, `probe-center`) nu mai dădea niciun loc (numele trimis era `gap`): acum numele
  redenumit e textul pe care îl înlocuiește editarea de sub cursor.
- **Task Manager**: serverele apar singure, ca vtsls (câte o parte pe server, sub proiect: un
  proces Node fiecare, memoria, CPU, cererile, latența).

**Mărimea lui `lsp/`** (măsurată, `cargo xtask lsp-bundle --platform all`): serverele web
adaugă 44 MB pe platformă (aceleași fișiere JavaScript peste tot; TypeScript 4.9 tăiat la 13 MB,
core-js 15 MB, schemele 1,4 MB), 6,7 MB arhivat.

| Platforma | Total (43a → 43b) | Arhivat (zip -9) |
|---|---|---|
| macOS arm64 | 168 → 195 MB | 49 → 54 MB |
| macOS x64 | 170 → 198 MB | 50 → 56 MB |
| Linux x64 | 171 → 198 MB | 52 → 59 MB |
| Windows x64 | 123 → 150 MB | 42 → 48 MB |

(Totalurile 43a erau rotunjite din `du`; cele de acum sunt ale xtask-ului, în MiB.)

**Comparația web** (motorul nostru vs serverele prin clientul nostru; proiecte publice clonate
în scratchpad: HTML5 Boilerplate `dist/` și Pico CSS (SCSS, `composer.json`), cu fișiere de
sonde scrise pentru ele — `probes.css`, `probes.html`, `probes.scss`, un `tsconfig.json`; 70 de
sonde, fiecare întrebată de 3 ori; harness-ul Rust `scratchpad/web-compare/harness`). Pentru JSON
motorul nostru are doar erorile gramaticii (nicio inteligență JSON). „Aplicația” = ce arată
editorul implicit: răspunsul serverului, al nostru unde serverul tace sau unde e păstrat
(`built-in`).

| Limbajul (sonde) | al nostru ok / parțial / nimic / greșit | serverele | aplicația (implicit) |
|---|---|---|---|
| CSS (24) | 19 / 0 / 5 / 0 (79%) | 21 / 2 / 1 / 0 (87%) | 24 / 0 / 0 / 0 (100%) |
| SCSS (16) | 11 / 0 / 4 / 1 (68%) | 14 / 0 / 1 / 1 (87%) | 15 / 0 / 0 / 1 (93%) |
| HTML (16) | 12 / 0 / 4 / 0 (75%) | 11 / 0 / 5 / 0 (68%) | 15 / 0 / 1 / 0 (93%) |
| JSON (14) | 3 / 0 / 11 / 0 (21%) | 14 / 0 / 0 / 0 (100%) | 14 / 0 / 0 / 0 (100%) |
| **toate (70)** | **45 / 0 / 24 / 1 (64%)** | **60 / 2 / 7 / 1 (85%)** | **68 / 0 / 1 / 1 (97%)** |

- Serverele câștigă: pseudo-clasele și at-rule-urile (`a:ho`, `@med`), proprietățile
  necunoscute, valorile lipsă, regulile goale, variabilele / funcțiile SCSS ale fișierului
  (al nostru oferă ale Pico-ului, nu pe ale fișierului), referințele de caractere și ARIA în
  HTML, perechea de tag-uri la redenumire, tot JSON-ul (completare, hover, erori de tip din
  scheme, JSONC).
- Al nostru câștigă: clasele din CSS-ul proiectului în `class="…"`, declarația unei clase
  (din HTML în CSS), Rename / Find Usages ale unei clase în HTML, proprietățile și elementele
  învechite, hover-ul unui mixin — exact ce păstrează aplicația (`built-in` și răspunsul nostru
  unde serverul tace).
- Greșit la amândouă: hover pe `$probe-gap` (serverul descrie `padding`); nimic la amândouă:
  ghilimeaua neînchisă în HTML.

| Cererea (caldă) p50 / p95 ms | al nostru | serverele |
|---|---|---|
| completare CSS / SCSS / HTML / JSON | 0,05 / 12,5 · 0,12 / 4,0 · 0,02 / 0,66 · — | 0,84 / 9,4 · 1,45 / 11,5 · 1,50 / 8,1 · 0,50 / 2,5 |
| hover | ≤ 0,007 | 0,16–0,29 / 0,19–0,37 |
| declarație | 0,003–0,004 | 0,13–0,15 / 0,18–0,21 |
| referințe / redenumire | 6,7–7,3 (caută în cele 298 de fișiere) | 0,17–0,23 |
| diagnostice (până la publicare) | 0,10–0,14 | ~740 (500 ms ale serverului înainte de validare + 200 ms așteptați de harness) |

**Resurse** (macOS arm64, release): gata în 152 ms (CSS), 251 ms (HTML), 104 ms (JSON); primul
răspuns 8 / 4 / 28 ms; câte un proces Node; RSS după încărcare 99 / 156 / 98 MB, după sonde
97 / 99 / 82 MB (HTML-ul încarcă TypeScript-ul la pornire, apoi GC-ul îl strânge); CPU în repaus
(30 s): 0,1% / 0,2% / 0,17%; clientul ~0,03% (procesul harness-ului, cu firele indexului).
Motorul nostru: indexul celor 298 de fișiere în 551 ms (0,48 s CPU), +33 MB RSS, 0% CPU în
repaus. Pornite leneș pe limbaj (niciun proces fără un fișier al limbajului deschis), oprite
după inactivitate (10 min implicit), un proces pe server pe proiect.

**Teste**: `lsp-client` (+8: `contents` — fișierele din foldere, nimic în afara lor, nimic
descărcat; `file-language-ids`; `built-in` citit și verificat; descrierile celor patru module
citite; serverele reale CSS / SCSS, HTML, JSON din `target/lsp` — completare, hover, diagnostice,
formatare, redenumirea `--gap`, codul încorporat neatins, schemele incluse fără descărcări, un
`$schema` străin refuzat), modulul `lsp` (+1: ce păstrează motorul inclus, din serverele
pornite), aplicația (+3: diagnosticele motorului inclus rămân lângă un server fără ale lui; o
listă goală a serverului lasă sugestiile noastre; un server care lasă redenumirea motorului
nostru nu e întrebat, deprecările rămân), domeniul (+1: `Kept`), xtask (+4: `prune`, căile
datelor, schemele fixate pe GitHub, copierea fără ce e tăiat). Niciun test nu atinge
`~/.config`, keychain-ul, `recent.toml` sau rețeaua.

**Imagini**: nouă `lsp-css-completion` (un fișier CSS cu serverul fals ca proces, descris cum e
cel CSS: completarea lui `col`, avertismentul `TODO` al lui și `clip` tăiat de motorul inclus,
păstrat lângă el). Comparate cu referința, se schimbă doar, așteptat: `settings-language-servers`
(limbajele HTML, CSS, SCSS, JSON în alegere, primul afișat fiind HTML-ul, cu „Server
(vscode-html)”), `settings-4` (același bloc pe pagina „Calitatea codului”) și `task-manager*`
(cifra de memorie vie, ca la 43a).

**Abateri**:

- Implicit e serverul, dar nu „singur” peste tot: ce face motorul nostru mai bine rămâne al lui
  prin datele serverului (`built-in`) și prin răspunsul nostru unde serverul tace (vezi mai sus) —
  altfel s-ar fi pierdut clasele proiectului în HTML, redenumirea claselor în HTML / PHP /
  Blade, deprecările CSS și diagnosticele HTML.
- Scheme JSON: doar setul inclus (fișierele pe care le are aproape orice proiect web / PHP);
  un `$schema` spre altă adresă primește avertismentul serverului că nu se poate încărca
  (descărcările sunt oprite), unde un client cu acces la rețea l-ar descărca.
- Serverul HTML încarcă TypeScript 4.9 la pornire chiar cu modul JavaScript oprit (e cerut la
  încărcarea modulului): ~50 MB RSS în plus până la primul GC; păstrat doar ce încarcă.
- LESS: neadăugat (editorul nu are limbajul).

### 43c — limbaje noi 1: C / C++ (clangd), Lua (lua-language-server), Zig (ZLS) (2026-11-02, branch `lsp`)

**Făcut.**

- **Trei module de limbaj noi** (`crates/modules/languages/{cpp,lua,zig}`, crate-uri `lang-cpp`,
  `lang-lua`, `lang-zig`, id-uri `lang.cpp`, `lang.lua`, `lang.zig`), făcute ca YAML / JSON:
  descriptorul (extensii, comentarii, iconița din setul aplicației), gramatica tree-sitter de pe
  crates.io (compatibilă cu tree-sitter 0.27, ABI 14–15: `tree-sitter-c` 0.24.2,
  `tree-sitter-cpp` 0.23.4, `tree-sitter-lua` 0.5.0 și `tree-sitter-zig` 1.1.2 — ultimele două
  ale organizației tree-sitter-grammars, întreținute; toate MIT), query-urile noastre de colorare
  pe scope-urile temelor (scrise de noi, „primul pattern câștigă”: specificele întâi), pliere,
  indentare, descrierea serverului (`data/language-servers.toml`). Motorul inclus e doar
  sintaxa (fără completare / declarații proprii): implicit răspunde serverul.
  - **C / C++** — un singur modul pentru amândouă (gramatica C++ o extinde pe a C, un singur
    server): C `.c`; C++ `.cpp .cc .cxx .c++` și headerele `.h .hpp .hh .hxx .h++ .ipp .tpp
    .inl`. **`.h` e citit ca C++**: un header poate fi inclus din ambele limbaje, iar gramatica
    C++ citește bine headerele C, pe când cea C nu poate citi headere C++ (clase, namespace-uri,
    template-uri); clangd decide singur cum compilează un header (din baza de compilare: flag-urile
    unui fișier care îl include), oricum îi spune editorul. Query-urile: o parte comună (doar
    noduri pe care le au ambele gramatici) după cea proprie fiecăruia; preprocesorul
    (`keyword.directive`), macro-urile, apelurile și declarațiile (și prin `::` / template-uri),
    tipurile (`type.builtin` pentru cele primitive), parametrii, câmpurile, constantele
    (enumeratori, nume ALL_CAPS), atributele, cuvintele C++. Erorile gramaticii nu sunt raportate
    (`own_errors: false`, ca la SQL): macro-urile pe care gramatica nu le expandează ar da alarme
    false; diagnosticele reale sunt ale lui clangd. Folderele generate (`build`,
    `cmake-build-*`, `.cache/clangd`) nu sunt căutate ca text.
  - **Lua** — `.lua`, `.luacheckrc`, shebang `lua` / `luajit`; comentariu `--`, bloc `--[[ ]]`;
    iconița `moon`. Biblioteca standard (`print`, `pairs`, `require`…) colorată ca
    `function.builtin`, `self` ca `variable.builtin`, metodele `a:b()`, câmpurile, atributele
    `<const>`, etichetele `::x::`. Indentarea: `end` e acum un token de închidere în
    `syntax-ts::indent` (o linie care începe cu `end` rămâne la nivelul blocului).
  - **Zig** — `.zig` și `.zon`; iconița `zap`. Built-in-urile (`@import`), tipurile, câmpurile,
    etichetele, `_`, numele PascalCase ca tipuri. Cache-ul build-ului (`.zig-cache`,
    `zig-cache`) nu e listat, `zig-out` nu e căutat.
  - Iconițele fișierelor: setul aplicației (`file-code`, `moon`, `zap`) și culorile `icon.c`,
    `icon.cpp`, `icon.lua`, `icon.zig` în toate cele 29 de teme (luate din culorile lor
    existente); seturile de iconițe (Material, Catppuccin, Symbols, vscode-icons) regenerate cu
    `cargo xtask icon-packs` cu extensiile `.cc .cxx .hh .hxx .lua .zig .zon`, limbajele `c`,
    `cpp`, `lua`, `zig` și numele `CMakeLists.txt`, `compile_commands.json`, `.clangd`,
    `.clang-format`, `.luarc.json`, `build.zig`, `build.zig.zon` (doar adăugate: nimic din ce
    era nu s-a schimbat). Numele modulelor în locale (ro / en).
- **Serverele, ca date**:
  - **clangd 23.1.0** (`lang-cpp`): `--background-index` (proiectul indexat o dată în fundal —
    referințe și declarații în fișiere nedeschise —, apoi incremental; indexul e cel obișnuit
    al lui clangd, `.cache/clangd/index` lângă baza de compilare, recitit la pornirea următoare),
    `-j=2` (doi workeri: primul index al unui proiect mare durează mai mult, dar nu ia toată
    mașina), `--background-index-priority=low`, `--pch-storage=disk` (preambulurile fișierelor
    deschise în fișiere temporare mapate, nu în RAM: `memory` e puțin mai rapid și cu zeci de MB
    în plus per fișier deschis), `--limit-results=200`, `--log=error`. Root markers:
    `compile_commands.json`, `compile_flags.txt`, `.clangd` — **nu** `CMakeLists.txt` (un proiect
    CMake are câte unul în aproape fiecare folder: un clangd per folder ar indexa aceleași
    headere iar și iar; un singur clangd citește toate bazele de compilare pe care le găsește).
    Urmărește `compile_commands.json` / `compile_flags.txt` / `.clangd` (un configure nou e
    recitit). `XDG_CACHE_HOME = ${cache}`: indexul fără bază de compilare merge (pe Linux) în
    folderul serverului din proiect, nu în cache-ul userului.
  - **lua-language-server 3.19.2** (`lang-lua`): `--logpath=${cache}/log`,
    `--metapath=${cache}/meta` (logul și definițiile bibliotecii standard pe care le generează
    merg în folderul lui din proiect, **niciodată în bundle-ul aplicației**, care e doar citire —
    altfel le-ar scrie lângă program, stricând semnătura aplicației pe macOS), `--loglevel=error`;
    root markers `.luarc.json` / `.luarc.jsonc`; setări `Lua`: telemetria oprită,
    `checkThirdParty = false` (nicio întrebare despre bibliotecile terților), preîncărcare
    limitată (2000 de fișiere, 500 KB), apelurile completate ca snippet.
  - **ZLS 0.17.0** (`lang-zig`): `--log-file ${cache}/zls.log --log-level warn`,
    `global_cache_path = ${cache}`, root marker `build.zig`. **Zig nu e inclus**: ZLS găsește
    singur `zig` în PATH (biblioteca standard — `std.` —, pachetele build-ului, erorile build-ului
    la salvare vin de la el); fără el, ZLS citește fișierele proiectului (declarații, completare,
    referințe, erori de sintaxă și de nume — AstGen-ul e în ZLS). Măsurat fără Zig instalat.
- **Clientul** (`lsp-client`):
  - placeholder nou **`${cache}`**: folderul serverului în folderul de date al proiectului
    (`<rădăcină>/.lumen/lsp/<nume>`, numele curățat — un server al userului numit oricum rămâne
    un singur folder acolo), făcut de modul când serverul pornește, doar dacă descrierea îl
    folosește (`Launch::cache`); proiectul are deja `.gitignore` acolo;
  - un program numit fără extensie e găsit și cu `.exe` pe Windows (`${server}/bin/clangd` e
    `clangd.exe` acolo);
  - câmp nou **`completion-kinds`** în descriere (tipul LSP, după nume → al nostru,
    `convert::CompletionKinds`, verificat la citire): **reparația din imaginea
    `lsp-css-completion`** — „$”-ul verde era tipul „variabilă”; serverul CSS dă proprietățile ca
    `Property` (arătat ca „p”), serverul HTML tag-urile ca `Property`, atributele ca `Value`,
    valorile ca `Unit`. Acum CSS / SCSS: `property → attribute` („a”, ca motorul inclus pentru
    proprietățile CSS), HTML: `property → tag`, `value / function → attribute`, `unit → value`;
    restul păstrează sensul general al LSP. Serverul fals dă cuvintele unei foi de stil ca
    proprietăți (ca serverul CSS), descris în imagini ca serverul CSS.
- **Bundle-ul** (`cargo xtask lsp-bundle`, `packaging/lsp.toml`): servere native — release-ul
  oficial pe platformă, doar de pe GitHub Releases (`https://github.com/<owner>/<repo>/releases/
  download/…`, verificat), SHA-256 fixat (digest-ul GitHub al fișierului), despachetat o dată în
  `target/lsp-cache/native/<sha256>/`; `strip` (folderul de sus al arhivei), `prune` ca la Node,
  `universal` (programele macOS pentru ambele arhitecturi: doar cea a platformei păstrată —
  tăiată din Mach-O-ul universal în Rust, fără `lipo`, deci merge de pe orice gazdă). clangd:
  păstrate programul și headerele builtin ale clang (`stddef.h`, intrinsicile…, 16 MB, fără ele
  nu citește nimic); lăsate afară bibliotecile runtime ale clang (compiler-rt, 190 MB pe macOS:
  sanitizere, profilare — clangd nu le citește). LuaLS: lăsate afară copiile `meta/` generate de
  testele release-ului (se generează în `${cache}`). `build.sh` universal: fiecare program
  Mach-O din bundle-ul arm64 cu geamăn Intel e unit cu `lipo` (Node, clangd, LuaLS, ZLS; testat
  pe cele două bundle-uri). Licențele în `packaging/lsp/NOTICE` (clangd: Apache-2.0 cu excepția
  LLVM; LuaLS: MIT; ZLS: MIT).
- **Setări**: limbajele noi apar singure în blocul „Servere de limbaj” (din catalog), cu „Server
  (clangd)” / „Motorul inclus” / „Amândouă”. Rândul „Limbajul” (acum 11 limbaje) trece deja pe
  mai multe rânduri când nu încape (`paint_choices_row`): verificat în imagine, rămâne așa (un
  dropdown ar ascunde limbajele; la 43d se adaugă 4).
- **Task Manager**: serverele apar singure (câte o parte per server sub proiect); clangd, LuaLS
  și ZLS sunt câte un singur proces (fără copii), cu PID-ul și memoria lor.

**Mărimea lui `lsp/`** (măsurată, `cargo xtask lsp-bundle --platform all`, MiB; arhivat cu
`zip -9`): serverele native adaugă ~111 MB pe macOS, ~100 MB pe Linux, ~78 MB pe Windows; aproape
tot e clangd (programul singur: 84 MB pe macOS după tăierea la o arhitectură — release-ul e
universal, 169 MB —, 50 MB pe Windows, 72 MB pe Linux după `strip-debug`; plus headerele clang,
16 MB). LuaLS 8–10 MB, ZLS 3,4–3,8 MB.

| Platforma | clangd | LuaLS | ZLS | Total (43b → 43c) | Arhivat (43b → 43c) |
|---|---|---|---|---|---|
| macOS arm64 | 99,7 | 8,0 | 3,4 | 195 → 305,6 | 54 → 84,6 |
| macOS x64 | 100,1 | 8,0 | 3,8 | 198 → 309,0 | 56 → 86,5 |
| Linux x64 | 87,6 | 8,9 | 3,7 | 198 → 297,5 | 59 → 89,3 |
| Windows x64 | 65,1 | 10,1 | 3,7 | 150 → 228,3 | 48 → 75,2 |

- `strip-debug` (nou în xtask, pe platformă): release-urile Linux ale lui clangd și ZLS își țin
  informația de debug (clangd 142 → 72 MB, ZLS 19 → 3,7 MB fără ea); scoasă cu `strip` al
  gazdei (Linux) sau unul încrucișat (`x86_64-linux-gnu-strip`, `llvm-strip`, `strip`-ul MinGW
  din binutils — citește și ELF; folosit aici pe macOS); fără niciunul, programul rămâne
  întreg (și se spune). Pe Windows, `zls.pdb` (13 MB, simbolurile) e lăsat afară (`prune`).
- Gramaticile noi în binarul aplicației: ~4,7 MB de cod de parser (bibliotecile statice: C++
  3,3 MB, Zig 0,7, C 0,6, Lua 0,06) — estimat din bibliotecile compilate, release-ul nu a fost
  reconstruit pentru măsurare.

**Măsurători** (macOS arm64, release; proiecte publice clonate în scratchpad, cu câte un fișier
de sonde scris pentru ele: **cJSON** (C, `compile_commands.json` scris pentru 4 fișiere, `-std=c89`;
99 de fișiere C/H în repo), **lume** (Lua), **zig-clap** (Zig, **fără Zig instalat**); 49 de sonde,
fiecare întrebată de 3 ori, prin clientul nostru; harness-ul Rust `scratchpad/native-compare/harness`,
rezultatele în `scratchpad/native-compare/results.json`). Motorul inclus al acestor limbaje e doar
sintaxa: nu răspunde la nicio sondă de completare / hover / declarație / referințe (doar erorile
de sintaxă Lua / Zig ale gramaticii).

| Limbajul (sonde) | completare | hover | declarație | referințe | diagnostice | total |
|---|---|---|---|---|---|---|
| C — clangd (17) | 5/5 | 3/3 | 3/3 | 2/2 | 4/4 | 17/17 |
| Lua — LuaLS (16) | 6/6 | 3/3 | 3/3 | 1/1 | 3/3 | 16/16 |
| Zig — ZLS (16) | 6/6 | 3/3 | 3/3 | 1/1 | 3/3 | 16/16 |

(Sonda de referințe C prin tot proiectul — din indexul de fundal, 8 locuri în `cJSON_Utils.c` și
sonde — a ieșit „parțial” doar pentru că așteptarea număra și un comentariu; răspunsul e corect.
`@impo` în Zig: ZLS filtrează built-in-urile fără `@`, cum le cere editorul.)

| Serverul | gata (initialize) | primul răspuns după deschidere | RSS după încărcare / după sonde | CPU în repaus (30 s) | completare p50 / p95 (caldă) | hover | declarație | referințe | diagnostice după o editare |
|---|---|---|---|---|---|---|---|---|---|
| clangd | 77 ms | 192 ms (preambulul) | 74 / 72 MB | 0,07% | 3,5 / 5,1 ms | 0,32 / 0,44 | 0,14 / 0,16 | 0,51 / 0,54 | ~185 ms |
| LuaLS | 62 ms | 2 ms | 42 / 29 MB | 0,73% | 1,0 / 1,5 ms | 1,0 / 1,0 | 1,0 / 1,0 | 0,95 | ~280 ms |
| ZLS | 12 ms | 1 ms | 5 / 12 MB | 0% | 1,1 / 2,7 ms | 0,06 / 0,07 | 0,07 / 0,14 | 0,06 | ~45 ms |

Câte un proces fiecare (fără copii), clientul nostru 0,01–0,02% CPU în repaus. Indexul de fundal
al lui clangd pentru cJSON: 92 KB în `.cache/clangd/index`; LuaLS a scris 140 KB în
`.lumen/lsp/lua-language-server/` (log + meta), nimic în bundle.

**Teste**: module noi `lang-cpp` 8 (query-urile compilează, extensiile, colorarea unui fișier C și
a unuia C++ — fiecare fel de scope —, erorile gramaticii neraportate, plierea, indentarea,
descrierea lui clangd), `lang-lua` 7 (+ `end` care închide un bloc), `lang-zig` 6; `lsp-client`
(+11: `${cache}` — folderul în proiect, numele curățat, făcut doar când e folosit —, `.exe` doar
pe Windows, `completion-kinds` citite și verificate, tipurile CSS / HTML ca ale motorului inclus,
clangd / LuaLS / ZLS reale din `target/lsp` — completare cu tipul ei, hover, declarație,
diagnostice, PID-ul; LuaLS nu scrie nimic în bundle); aplicația (+1: un limbaj doar cu sintaxa —
C — primește completarea, hover-ul și declarația de la server); xtask (+6: sursele native doar
din GitHub Releases, fiecare platformă fixată, tăierea Mach-O universal — cu permisiunile
păstrate —, `strip-debug`). Niciun test nu atinge `~/.config`, keychain-ul, `recent.toml` sau
rețeaua (serverele reale doar unde există `target/lsp`).

**Imagini**: nouă `editor-cpp` (un fișier C++ colorat, din afara proiectului, ca `sql-editor`).
Comparate cu referința, se schimbă doar, așteptat: `lsp-css-completion` (sugestiile serverului
CSS cu „a”, ca proprietățile motorului inclus, în locul „$”-ului verde), `settings-language-servers`
și `settings-4` (C, C++, Lua, Zig în alegere — rândul trece pe două rânduri), `about` și
`settings-8` (68 de module), `task-manager*` (cifra de memorie vie).

**Abateri / ce nu s-a făcut**:

- Gramaticile vin de pe crates.io (MIT, întreținute), nu vendored în `third_party` cum spunea
  planul: SQL a fost vendored doar pentru că crate-ul n-avea licența și trebuia regenerat; aici
  nu e cazul.
- clangd scrie indexul de fundal în `.cache/clangd/index` lângă baza de compilare (locul lui
  obișnuit): clangd n-are opțiune pentru alt loc. Fără bază de compilare,
  pe Linux merge în `${cache}`; pe macOS / Windows în cache-ul userului (`~/Library/Caches/clangd`).
- „Clear and Rebuild Caches” nu șterge folderele `.lumen/lsp/<server>` (le refac serverele).
- Zig nu e inclus (decis în plan): fără `zig` în PATH, ZLS nu știe `std.` și nu face build-on-save.
- Build-urile dist nu au fost rulate (le rulează userul); unirea universală macOS testată pe cele
  două bundle-uri, `strip-debug` pe Linux testat doar cu `strip`-ul MinGW pe macOS.

### 43d — limbaje noi 2: Bash, YAML, TOML, Docker (2026-11-02, branch `lsp`)

**Făcut.**

- **Trei module de limbaj noi** (`crates/modules/languages/{bash,toml,dockerfile}`, crate-uri
  `lang-bash`, `lang-toml`, `lang-dockerfile`, id-uri `lang.bash`, `lang.toml`, `lang.dockerfile`),
  făcute ca la 43c (descriptor, gramatică de pe crates.io compatibilă cu tree-sitter 0.27,
  query-urile noastre pe scope-urile temelor, pliere, indentare, `data/language-servers.toml`):
  - **Bash** — `tree-sitter-bash` 0.25.1 (tree-sitter, MIT). `.sh .bash .zsh .ksh .command`,
    fișierele de pornire ale shell-urilor (`.bashrc`, `.bash_profile`, `.bash_login`,
    `.bash_logout`, `.bash_aliases`, `.profile`, `.zshrc`, `.zshenv`, `.zprofile`, `.zlogin`,
    `.zlogout`), `PKGBUILD` / `APKBUILD`, shebang `sh` / `bash` / `zsh` / `dash` / `ksh`.
    **zsh inclus**: gramatica Bash citește fără nicio eroare un `.zshrc` obișnuit (export,
    `setopt`, `autoload`, `bindkey`, `alias`, `zstyle`, funcții, `[[ ]]`, bucle — testat);
    doar sintaxa proprie zsh (`${(j:,:)x}`, glob qualifiers) ar ieși greșit. Colorarea: comenzile
    (builtin-urile shell-ului separat), funcțiile declarate, opțiunile (`-v`, `--force`) ca
    atribute, variabilele, `$` / `${ }` / `$( )` / `$(( ))`, here-doc-urile, regex-ul din
    `=~`, cuvintele cheie, operatorii. Indentarea: `fi`, `done`, `esac` închid acum un nod în
    `syntax-ts::indent` (ca `end` la Lua).
  - **TOML** — `tree-sitter-toml-ng` 0.7.0 (tree-sitter-grammars, MIT). `.toml`, plus
    `Cargo.lock`, `uv.lock`, `poetry.lock`, `Pipfile` (nu un fișier numit `config`: e la fel de
    des al Git-ului / SSH-ului). Antetele `[tabel]` / `[[tabel]]` ca `tag`, cheile ca
    `property`, datele ca `constant`; plierea pe tabele (până la antetul următor, cu numărul de
    chei), array-uri și tabele inline.
  - **Dockerfile** — `tree-sitter-containerfile` 0.9.2 (WharfLab, MIT: gramatica întreținută
    care continuă `tree-sitter-dockerfile`, al cărui crate e vechi și cere tree-sitter 0.20).
    `Dockerfile`, `Containerfile`, `Dockerfile.*`, `Containerfile.*`, `*.dockerfile` (și scrise
    cu altă capitalizare). Instrucțiunile, imaginea / tag-ul / digest-ul, numele etapei,
    flag-urile (`--mount`, `--from`), variabilele (`${PHP_VERSION}`), here-doc-urile;
    **comenzile shell** (`RUN` / `CMD` / `ENTRYPOINT` în forma shell, corpul unui `RUN <<EOF`)
    sunt colorate **ca Bash** prin injecție (gramatica e a modulului Bash, după id; fără el,
    text simplu — modulele nu depind unul de altul); erorile Bash din comenzi nu sunt raportate
    (`injected_errors: false`: `\`-urile și variabilele Dockerfile-ului ar da alarme false).
  - Iconițele: cele din setul aplicației (`terminal`, `settings`, `docker`, deja folosite de
    regulile pe nume / extensie); culori noi `icon.bash` / `icon.toml` / `icon.dockerfile` în
    toate cele 29 de teme (din `icon.shell` / `icon.config` / `icon.docker` ale fiecăreia);
    seturile de iconițe regenerate (`cargo xtask icon-packs`, doar adăugiri) cu `.ksh`,
    `.command`, `.dockerfile`, `.containerfile`, `Containerfile`, `compose.yml`, `.bashrc`,
    `.bash_profile`, `.zshrc`, `.profile`, `pyproject.toml`, `.taplo.toml`, `rustfmt.toml` și
    limbajele `bash` (formatul temelor de iconițe îl știe ca `shellscript`: alias nou în xtask), `toml`,
    `dockerfile`. Numele modulelor în locale (ro / en).
- **Serverele, ca date**:
  - **bash-language-server 5.8.1** (MIT, prin Node-ul aplicației): completare (funcțiile
    fișierului și ale celor incluse cu `source`, variabilele, builtin-urile), hover (`help` /
    `man` pentru comenzi), declarație, referințe, redenumire; analiza de fundal a scripturilor
    proiectului (max. 500), `explainshellEndpoint` gol (nimic din rețea).
    **ShellCheck (GPL-3.0) nu e inclus** — decizia de licență: serverul îl rulează ca **program
    separat** (`shellcheck` din PATH, nimic legat, nimic distribuit); dacă userul îl are,
    avertismentele lui apar ca diagnostice, altfel nu. La fel `shfmt` pentru formatare. Serverul
    nu raportează erori de sintaxă proprii (opțiunea `enableSourceErrorDiagnostics` e despre
    `source`-uri, nu sintaxă): **erorile gramaticii motorului inclus rămân** (`built-in =
    ["diagnostics"]`, ca la HTML).
  - **yaml-language-server 2.0.0** (Red Hat, MIT, prin Node), pentru modulul YAML existent
    (care primește acum descrierea serverului și dependența de `code-intelligence`). **Nimic
    descărcat**: catalogul SchemaStore oprit (`schemaStore.enable = false`), catalogul CRD
    Kubernetes oprit, serverul e rugat să ceară schemele http(s) clientului (notificarea
    `yaml/registerContentRequest` — câmp nou în descriere, `on-initialized`, trimis după
    `initialized`, la fiecare pornire), care răspunde din schemele incluse (`contents`, ca la
    43b) și refuză restul (un `# yaml-language-server: $schema=https://…` spre altă adresă →
    „not bundled (downloads are off)” în logul serverului); `http.proxy` spre un port local
    închis, ca siguranță. Scheme incluse (pattern-urile yaml-language-server: sfârșitul căii,
    `*` orice, fără `{a,b}`): GitHub workflows (și Gitea / Forgejo), GitHub actions
    (`action.yml`), Dependabot, pre-commit — SchemaStore la același commit ca la 43b —, **Docker
    Compose** (`compose*.yaml`, `docker-compose*.yml`: schema specificației, din compose-go la
    tag-ul v2.16.2, Apache-2.0). Motorul inclus YAML era doar sintaxa (plus funcțiile PHP care
    citesc YAML-ul, ex. rutele Symfony, neatinse): implicit răspunde serverul.
  - **dockerfile-language-server-nodejs 0.15.0** (MIT, prin Node): completarea instrucțiunilor,
    a flag-urilor, a etapelor; hover cu documentația instrucțiunilor; declarația / referințele
    unei etape (`--from=build`); diagnosticele (instrucțiuni învechite, porturi greșite,
    argumente lipsă…) cu setările implicite ale serverului; formatare. Nimic din rețea.
  - **Compose**: fișierele compose sunt YAML — schema lor prin yaml-language-server.
    `@microsoft/compose-language-service` **nu e inclus**: ar fi un al doilea server pe
    aceleași fișiere YAML (motorul se alege pe limbaj, unul singur), iar completarea lui de
    imagini / tag-uri întreabă Docker Hub (rețea). Schema dă deja completarea, hover-ul și
    verificarea cheilor.
  - **taplo 0.10.0** (MIT, program nativ: release-ul oficial de pe GitHub — un singur program
    gzip-uit pe macOS / Linux, zip pe Windows; SHA-256 fixat pe platformă, calculat la
    descărcare: release-ul nu are digest GitHub), `taplo lsp stdio`, root markers `.taplo.toml`
    / `taplo.toml` (configurarea lui, citită: formatare, reguli). **Nimic din rețea**:
    `catalogs = []` (catalogul SchemaStore oprit), schemele date prin `file://` din bundle
    (placeholder nou **`${server-uri}`**: folderul serverului ca URI), iar orice altă schemă
    (`#:schema https://…` într-un fișier, configurarea taplo a proiectului) nu poate fi luată:
    `HTTP(S)_PROXY` / `ALL_PROXY` ale procesului lui spre `127.0.0.1:9` (port închis) —
    singura cerere pe care o face înainte de a-și primi setările (catalogul, la `initialize`,
    cu setările lui implicite) e refuzată pe loc, local; verificat în testul cu serverul real.
    Scheme: `Cargo.toml`, `.cargo/config.toml`, `pyproject.toml`, `rustfmt.toml` (SchemaStore,
    același commit). taplo ia singur schemele la care trimite o schemă (`$ref` spre
    `https://www.schemastore.org/…`), deci **xtask le face autonome la bundle**
    (`[[servers.inline]]`, nou): fiecare `$ref` spre o schemă inclusă (URL-urile relative
    rezolvate față de `$id`) e pus în schemă, sub `definitions/x-bundled-<nume>`, cu
    referințele `#…` ale lui mutate după el; o referință neinclusă e o eroare a bundle-ului.
    Cargo: lint-urile `rust` / `clippy` / `rustdoc` / `cargo` și quikrun (6 fișiere → 419 KB);
    pyproject: toate secțiunile `tool.*` (ruff, black, mypy, poetry, pdm, uv, pytest, hatch…
    29 de fișiere → 1,1 MB); părțile nu mai rămân în bundle.
- **Clientul** (`lsp-client`): `on-initialized` (notificări fără parametri trimise după
  `initialized`; o notificare fără parametri nu mai trimite `"params": null`), `${server-uri}`.
- **`cargo xtask lsp-bundle`**: servere native livrate ca un singur program gzip-uit
  (`program = "taplo"`, decomprimat cu `flate2`, executabil), `[[servers.inline]]`; pachetele
  npm cu lockfile din registry.npmjs.org (SHA-512): `packaging/lsp/{bash-language-server,
  yaml-language-server,dockerfile-language-server-nodejs}/`; din bash-language-server e lăsat
  afară build-ul de debug al runtime-ului web-tree-sitter (2,7 MB). Licențele în
  `packaging/lsp/NOTICE` (MIT, ISC, BSD-2/3-Clause, Blue Oak 1.0.0, Apache-2.0 pentru scheme;
  ShellCheck menționat ca neinclus).
- **Reparații din recenzia 43c**:
  1. imaginea `editor-cpp` arăta în bara de jos calea lungă a fișierului din afara proiectului:
     mostrele limbajelor noi (C++, Bash, TOML, Dockerfile) sunt puse acum **în proiectul
     imaginii** doar pentru cazul lor (`src/shapes.cpp`, `deploy.sh`, `Cargo.toml`,
     `Dockerfile`) și șterse după — bara arată calea scurtă, celelalte imagini nu se schimbă;
  2. **„Clear and Rebuild Caches” golește folderele serverelor**: eveniment nou pe bus-ul
     proiectului, `workspace::events::CachesCleared` (anunțat de aplicație înaintea lui
     `ReindexRequested`); modulul serverelor oprește serverele care scriu în folderul lor
     (`${cache}`), așteaptă să se oprească (pe pool, nu pe firul UI), șterge
     `<proiect>/.lumen/lsp/` întreg (și folderele unor servere care nu mai rulează) și le
     pornește din nou pentru documentele deschise; între timp nu pornește nimic. Serverele
     fără folder (vtsls, cele web) nu sunt atinse.
- **Setări**: limbajele noi apar singure în blocul „Servere de limbaj” — 16 limbaje, rândul
  „Limbajul” pe două rânduri (verificat în imagine: arată bine, fără dropdown).
- **Task Manager**: serverele apar singure (o parte per server, sub proiect; câte un proces).

**Mărimea lui `lsp/`** (măsurată, `cargo xtask lsp-bundle --platform all`, MiB; arhivat cu
`zip -9`): bash-language-server 22,9, yaml-language-server 19,1 (prettier-ul lui, pentru
formatare, 9,6), dockerfile-language-server 3,2 (aceleași pe toate platformele), taplo
11,9–14,9 (programul + schemele, 1,9 MB).

| Platforma | taplo | Total (43c → 43d) | Arhivat (43c → 43d) |
|---|---|---|---|
| macOS arm64 | 11,9 | 305,6 → 353,1 | 84,6 → 99,7 |
| macOS x64 | 12,7 | 309,0 → 357,3 | 86,5 → 101,8 |
| Linux x64 | 13,9 | 297,5 → 347,0 | 89,3 → 104,8 |
| Windows x64 | 14,9 | 228,3 → 278,9 | 75,2 → 90,7 |

Gramaticile noi în binarul aplicației: Bash e cea mai mare (parser.c 9,5 MB de sursă), TOML și
Containerfile mici; release-ul nu a fost reconstruit pentru măsurare.

**Măsurători** (macOS arm64; proiecte publice clonate în scratchpad, cu fișiere de sonde scrise
pentru ele: **nvm** (`probes.sh`, care include `nvm.sh` — 6742 de linii — cu `.`),
**awesome-compose** (un `compose.yaml` și un workflow GitHub de sonde; `flask-redis/Dockerfile`),
**ripgrep** (un `Cargo.toml` de sonde), **flask** (`pyproject.toml`); 57 de sonde, fiecare
întrebată de 3 ori, prin clientul nostru; harness-ul Rust `scratchpad/misc-compare/harness`,
rezultatele în `scratchpad/misc-compare/results.json` și `run.log`). Motorul inclus al acestor
limbaje e doar sintaxa: la sonde răspunde doar la erorile de sintaxă.

| Serverul — fișierul (sonde) | ok / parțial / nimic / greșit | ce n-a mers |
|---|---|---|
| bash-language-server — nvm (13) | 10 / 0 / 2 / 1 | funcțiile din a doua jumătate a lui `nvm.sh` (de ex. `nvm_version`, l. 1161): nici completare, nici hover, nici declarație — serverul nu citește tot fișierul inclus (`nvm_echo`, l. 21, merge peste tot, iar referințele lui: 376 de locuri în proiect) |
| yaml-language-server — compose (11) | 7 / 0 / 2 / 2 | valorile `pull_policy` (schema nu le enumeră), hover pe `depends_on` (alt text), declarația unui alias duce la valoarea ancorei (linia de sub `&common`), `privileged: maybe` neraportat — corect, de fapt: schema permite text (interpolare) |
| yaml-language-server — workflow (7) | 7 / 0 / 0 / 0 | — |
| taplo — Cargo.toml (9) | 9 / 0 / 0 / 0 | — (inclusiv lint-urile, din schema făcută autonomă) |
| taplo — pyproject.toml (6) | 6 / 0 / 0 / 0 | — (secțiunea `tool.ruff`, inclusă la fel) |
| dockerfile-language-server — Dockerfile (11) | 9 / 0 / 1 / 1 | referințele unei etape (nimic); `--from` vine ca `--from=stage` (snippet: corect, doar sonda îl cerea fără) |
| **toate (57)** | **48 / 0 / 5 / 4 (84%)** | |

| Serverul | gata (initialize) | RSS după încărcare / după sonde | CPU în repaus (30 s) | completare p50 (caldă) | hover | declarație | diagnostice după o editare |
|---|---|---|---|---|---|---|---|
| bash-language-server | 377 ms | 170 / 134 MB | 0% | 0,66 ms | 0,40 | 0,35 | — (fără ShellCheck) |
| yaml-language-server | 204–235 ms | 97–103 / 64–90 MB | 0,03–0,07% | 1,0–1,1 ms | 0,28–0,86 | 0,13 | ~240 ms (+300 ms așteptați de harness) |
| taplo | 18–45 ms | 30–70 / 18–37 MB | 0% | 1,4 ms (pyproject 9,5) | 0,26–1,8 | — | ~35 ms (+300) |
| dockerfile-language-server | 232 ms | 72 / 51 MB | 0,03% | 0,52 ms | 0,40 | 0,71 | ~35 ms (+300) |

Câte un proces fiecare (fără copii); clientul nostru 0,007–0,02% CPU în repaus. Referințele
Bash p50 64 ms (caută în scripturile proiectului).

**Teste**: module noi `lang-bash` 8 (query-urile, detectarea — extensii, nume, shebang-uri —,
colorarea fiecărui fel de scope, plierea, indentarea și `fi` / `done` care închid, un `.zshrc`
tipic citit fără erori, descrierea serverului fără ShellCheck), `lang-toml` 7, `lang-dockerfile`
7 (și comenzile shell colorate ca Bash cu gramatica modulului Bash, fără ea text simplu; un
`RUN` cu here-doc pliat o singură dată), `lang-yaml` +1 (serverul descris fără descărcări, toate
schemele lui răspunse de aplicație); `lsp-client` +5 (`on-initialized` trimis după
`initialized` și după o repornire, notificare fără `params`, `${server-uri}` cu spații,
descrierile celor patru module, cele patru servere reale din `target/lsp` — Bash: completare,
declarație, hover; YAML: schema Compose cerută aplicației, completarea unui workflow, o schemă
străină refuzată și spusă în log; taplo: Cargo.toml (și lint-urile), pyproject (ruff), o eroare
de sintaxă, nicio cerere care să iasă din mașină; Dockerfile: diagnostice, completare, hover,
etapa unui `--from`); modulul `lsp` +1 („Clear and Rebuild Caches”: serverul cu folder pornit
din nou cu folderul gol, folderul unui server vechi șters, serverul fără folder neatins); xtask
+4 (`.gz` cu un singur program — recunoscut, decomprimat executabil —, URL-uri relative față de
`$id`, o schemă făcută autonomă — referințe în ambele sensuri, fragmente, date neatinse, o
schemă inclusă o singură dată, ce lipsește spus), taplo în lista serverelor native. Niciun test
nu atinge `~/.config`, keychain-ul, `recent.toml` sau rețeaua (serverele reale doar unde există
`target/lsp`; taplo doar spre portul local închis).

**Imagini**: noi `editor-bash`, `editor-toml`, `editor-dockerfile` (mostrele puse în proiectul
imaginii pentru cazul lor); se schimbă, așteptat: `editor-cpp` (calea scurtă `src/shapes.cpp`
în bara de jos și fișierul în arbore), `settings-language-servers` și `settings-4` (Bash, TOML,
Dockerfile și YAML în alegere: 16 limbaje pe două rânduri), `about` și `settings-8` (71 de
module; descrierea YAML), `task-manager*` (cifra de memorie vie), `sql-file-attached` (doar
timpul interogării, care variază).

**Abateri / ce nu s-a făcut**:

- ShellCheck nu e inclus (GPL-3.0): folosit din PATH dacă există; fără el, Bash nu are
  diagnostice de la server, doar erorile de sintaxă ale motorului inclus (păstrate).
- `@microsoft/compose-language-service` nu e inclus (vezi mai sus): compose prin schema din
  yaml-language-server.
- GitLab CI și OpenAPI nu au schemă inclusă: schemele lor nu sunt pe GitHub la un commit fixat
  (gitlab.com, spec.openapis.org) — xtask ia fișiere de date doar de pe
  raw.githubusercontent.com; se pot adăuga cu o sursă nouă verificată.
- taplo face, la `initialize`, o cerere a catalogului SchemaStore cu setările lui implicite
  (înainte de a le primi pe ale noastre): refuzată local (proxy-ul spre portul închis), nimic nu
  iese din mașină; se vede ca avertisment în logul serverului. Altfel ar fi trebuit să nu-i
  dăm folderul proiectului (și s-ar fi pierdut `.taplo.toml`-ul proiectului).
- Cargo-ul vechi `.cargo/config` (fără extensie) nu e detectat ca TOML.
- Build-urile dist nu au fost rulate (le rulează userul); unirea universală macOS a lui taplo
  verificată cu `lipo` pe cele două programe.

### 43e — paritate: nimic din ce făcea motorul inclus nu se pierde (2026-11-02, branch `lsp`)

Cerut: „să nu pierdem go-to-definition, refactor-ul, find-usage și ce nu ofereau lsp-urile”;
„și la json să avem tot via lsp dar să nu pierdem din ce avem”. Serverele rămân implicite; ce
făcea motorul inclus la `e7f8a76b` (înainte de branch) rămâne. (Pasul „măsurători și imagini”
al planului a fost făcut pe parcurs, la 43a–43d; 43e e paritatea.)

**Regula aplicată** (generic, nu pe limbaj):

- ce n-are echivalent LSP → mereu al motorului inclus / al modulelor, oricare ar fi motorul;
- ce au amândouă → întâi serverul; motorul inclus când răspunsul e gol, o eroare, a expirat,
  sau serverul nu e gata / a căzut (nu doar „gol”);
- Rename / Find Usages → locurile serverului în fișierele limbajelor lui **plus** ale motorului
  inclus în fișierele celorlalte limbaje (fiecare fișier o singură dată: fără dubluri);
- diagnosticele: analizele motorului inclus fără pereche la server rămân, nu de două ori.

**Auditul** (Server = implicit; Amândouă = „Both”; „—” = motorul inclus n-avea nimic):

| Limbaj | Funcția (motorul inclus la `e7f8a76b`) | Înainte de 43e cu Server | Acum (Server / Amândouă) |
|---|---|---|---|
| JS / TS / TSX | completare: numele proiectului, vocabularul DOM / JS | doar a serverului (ok, o știe) | la fel; plus **hook-urile React** (framework, `merge`) — **pierdute, refăcute** |
| JS / TS / TSX | completare: eroare / timeout al serverului | **nimic**, întrebat din nou la fiecare cadru | a motorului inclus, serverul neîntrebat din nou pentru cuvânt |
| JS / TS / TSX | hover, semnătură, declarație | serverul, al nostru doar la „gol” | al nostru și la eroare / timeout / cădere; hover-ul și declarația modulelor / framework-urilor (hook React, componentă) **întâi** (erau pierdute la un răspuns al serverului) |
| JS / TS / TSX | Rename / Find Usages (inclusiv în HTML, PHP, Twig, Vue, Svelte) | doar locurile serverului: **folosirile din HTML / PHP / Twig pierdute** | uniunea: ale serverului în js/ts/tsx, ale motorului inclus în celelalte; un local rămâne în funcția lui |
| JS / TS / TSX | Change Signature, Move, Type Diagram, Move Statement, stilul codului, documentarea | neatinse (motorul inclus) | la fel |
| JS / TS / TSX | analize: neutilizate, învechite (DOM) | **nici calculate, nici arătate** | calculate și arătate, în afară de locurile marcate de server (fără dubluri) |
| JS / TS / TSX | erori de sintaxă | ale serverului | la fel (Amândouă: ale noastre fără cele suprapuse) |
| CSS / SCSS | completare: clasele / id-urile / variabilele / mixin-urile proiectului (`.ca`, `var(--`, `$x`, `@include`) | **pierdute** când serverul avea ceva | unite cu lista serverului (contextul `Symbols` al motorului inclus) |
| CSS / SCSS | Rename / Find Usages, învechite | păstrate (`built-in`, 43b) | la fel |
| CSS / SCSS | hover, declarație | serverul, al nostru la „gol” | și la eroare / timeout |
| HTML | completare: clasele proiectului în `class="…"` | păstrate („gol” → al nostru) | la fel; plus atributele **htmx / Vue** (framework, `merge`) — **pierdute, refăcute** |
| HTML | hover htmx / Vue, componente Vue (declarație) | pierdute dacă serverul răspundea | ale framework-urilor întâi |
| HTML | diagnostice, învechite | păstrate (`built-in`) | la fel |
| HTML | Rename / Find Usages (clase, id-uri spre CSS) | serverul (doar tag-uri) sau al nostru | uniune; clasele — al nostru (serverul n-are locuri) |
| JSON | doar erorile gramaticii (nicio inteligență; modulele citesc `composer.json` / `package.json` din proiect, nu din editor) | ale serverului | la fel — nimic de pierdut; serverul aduce schemele |
| YAML | erorile gramaticii; Symfony citește rutele din fișiere | ale serverului | la fel |
| C / C++ / Lua / Zig / Bash / TOML / Dockerfile | doar sintaxa (nou la 43c / 43d) | — | Bash își păstrează erorile de sintaxă (`built-in`); Amândouă: lista serverului apare și unde motorul inclus n-are nimic (era goală) |
| toate | ocurențele de la server | eroarea serverului → **cerere la fiecare cadru** | cerute o dată pe cuvânt și versiune |
| toate | Structura, breadcrumb, plierea, „Reformat File”, ⌘ (sublinierea), auto-închiderea tag-urilor | motorul inclus / tree-sitter | la fel |
| PHP / Blade / Twig / SQL | totul | fără server: neatins | la fel |

**Făcut.**

- **Completarea** (`update_completion`): unde serverul singur răspunde, lista lui primește
  sugestiile framework-urilor care se unesc (`merge`: hook-urile React, atributele htmx /
  Vue — fiecare cu intervalul ei) și, unde contextul motorului inclus e `Symbols` (numele
  proiectului din alt fișier / limbaj: clase, id-uri, variabile, mixin-uri), sugestiile lui
  (`merge_completions`: ale serverului întâi, fiecare etichetă o dată). Un răspuns pierdut
  (eroare, timeout, cădere) e ca o listă goală: arată motorul inclus și nu mai întreabă
  serverul pentru același cuvânt (înainte: nimic arătat, cerere nouă la fiecare cadru).
  Cu „Amândouă”, lista serverului apare și unde motorul inclus n-are context sau
  inteligență (C, Lua… cu Amândouă: înainte, nimic).
- **Hover / declarație**: asistentele modulelor (consola SQL) și framework-urile întâi (n-au
  echivalent LSP), apoi serverul, apoi motorul inclus — la gol, eroare, timeout, cădere
  (`Reply` „Gone” = niciun răspuns, verificat pentru fiecare întrebare).
- **Semnătura**: la fel (era deja; acum testat pentru eroare / gol / timeout).
- **Ocurențele**: un răspuns pierdut se ține minte ca „niciunul” pentru cuvânt și versiune.
- **Rename / Find Usages** (`ServedRename`): portul are `languages_with(proiect, limbaj)`
  (limbajele serverului care răspunde: vtsls → javascript / typescript / tsx); în fișierele
  lor locurile sunt ale serverului, în celelalte (HTML, PHP, Twig, Vue…) rezolvă motorul
  inclus, cu fișierele lui candidate (`usage_extensions`) — uniunea, fiecare fișier citit o
  dată. Un simbol local al motorului inclus nu se caută în alte fișiere, iar când serverul nu
  răspunde rămâne în domeniul lui (înainte: căutat în tot proiectul, cu locuri „nesigure”).
- **Diagnosticele**: analizele motorului inclus (neutilizate, învechite, mai noi decât nivelul,
  lipsă din manifest) se calculează mereu (`declarations_only` scos) și se arată, în afară de
  cele peste care cade un diagnostic al unui server (`ProjectDiagnostics::of_sources`, sursele
  serverelor recunoscute prin `servers::diagnostics_source` / `is_server_source`); erorile de
  sintaxă rămân ascunse unde serverul singur răspunde (are parserul lui), în afară de `built-in
  = ["diagnostics"]` (HTML, Bash); `deprecated` (CSS) le arată și peste ale serverului.
- **Iconița Dockerfile** (`editor-dockerfile`): regula pe nume dădea cheia de culoare `docker`,
  pe care aplicația o trimitea setului de iconițe ales (implicit „symbols”) ca limbaj — setul
  știe `dockerfile`, iar un `Dockerfile` n-are extensie: ieșea iconița generică. Regulile
  `Dockerfile` / `Dockerfile.*` au acum cheia limbajului (`dockerfile`, culoarea
  `icon.dockerfile` din teme): iconița docker în arbore, tab și căutare, în orice set.
- **Modulul `lsp`**: `languages_with`; `request_timeout` (doar testele: un timeout scurt).
- **Serverul fals**: `failing` / `empty` / `unanswered` (eroare, `null`, niciun răspuns).

**Teste** (aplicația, cu serverul fals): declarația, hover-ul, semnătura și completarea cu
serverul care dă eroare / nimic / nu răspunde (timeout) → ale motorului inclus, completarea
neîntrebată din nou; ocurențele neîntrebate la fiecare cadru; redenumirea = uniunea (funcția
din script și folosirea ei din pagina HTML); redenumirea fără server (eroare / gol / timeout)
→ a motorului inclus; un local fără server rămâne în funcția lui; analizele rămân lângă
diagnosticele serverului, nu peste ele; hook-urile React lângă lista serverului și cardul lor
fără întrebarea serverului; clasele altei foi de stil lângă lista serverului CSS; iconița
Dockerfile în fiecare set. Modulul `lsp`: `languages_with`. Domeniul: sursa diagnosticelor unui
server, `of_sources`.

**Imagini**: comparate cu referința (`ui-snapshots.sh compare`), se schimbă doar
`editor-dockerfile` (iconița docker în arbore, tab, breadcrumb și bara de jos, în locul celei
generice) și, ca până acum, `task-manager*` (doar cifra de memorie vie din bara de jos); restul
sunt identice.

**Ce nu se poate păstra identic**:

- Neutilizatele / învechitele raportate de server (ex. vtsls: „declared but never read”, tag-ul
  „deprecated”) apar în stilul serverului (slab), nu tăiate ca ale noastre; ale noastre se
  arată doar unde serverul nu marchează nimic.
- Când serverul răspunde ceva (nu gol) la hover / declarație, răspunsul lui câștigă chiar dacă
  al nostru ar fi dus în alt limbaj — în practică serverele tac exact acolo (clase în șiruri,
  căi), unde răspunde motorul inclus.
- Sublinierea ⌘ (eticheta legăturii) rămâne a motorului inclus, ca înainte (clicul întreabă
  serverul).
- ~~Un server care nu răspunde la completare ține lista așteptând până la timeout-ul cererii
  (10 s), apoi motorul inclus.~~ Rezolvat la 43f: după 200 ms se arată lista motorului inclus.

### 43f — măsurători în aplicație și finisaj (2026-11-02, branch `lsp`)

(Pasul „propriul nostru server” al planului, opțional, rămâne pentru mai târziu — vezi mai jos;
43f a devenit măsurătorile cap-coadă în aplicația reală și ce au cerut ele.)

**Nimic nu mai așteaptă un server tăcut.** Până acum, un server care nu răspundea la completare
ținea lista închisă până la timeout-ul cererii (10 s); la fel cardul hover, semnătura și Go to
Declaration.

- **Completarea**: lista serverului e așteptată **200 ms** (`SERVER_GRACE`), apoi se arată a
  motorului inclus (unde limbajul are una). De ce 200 ms: serverele măsurate la 43a–43d răspund
  la completare în 0,5–10 ms (p95 sub 6 ms la vtsls), deci unul sănătos nu e tăiat niciodată, iar
  o listă care apare în ~200 ms se citește încă drept imediată; unul blocat (indexează, s-a
  agățat) costă o cincime de secundă în loc de 10 s. Un server care a ratat termenul și n-a mai
  răspuns de atunci nu mai e așteptat la cuvântul următor (lista motorului inclus pe loc), până
  răspunde iar. Când răspunsul vine după ce lista motorului inclus a fost arătată pentru cuvânt,
  sugestiile lui **se adaugă după** ale ei (`CompletionCache::late`): nimic nu se mută sub cursor,
  selecția rămâne pe aceeași sugestie (popup-ul o ține după etichetă), ordinea rămâne și cât se
  tastează mai departe în același cuvânt (și la o listă incompletă cerută din nou). Același lucru
  la „Amândouă” (acolo lista motorului inclus apare imediat). Firul UI nu așteaptă nimic: un
  singur redraw e programat la sfârșitul termenului (`served_deadline`, în lista termenelor
  cadrului, ca autosave-ul).
- **Hover și semnătura**: tot 200 ms, apoi cardul / parametrii motorului inclus; răspunsul
  târziu al serverului e păstrat pentru cuvânt (folosit la următorul hover) sau arată semnătura
  dacă motorul inclus n-a avut una — fără să înlocuiască un card deja arătat (fără pâlpâire).
- **Go to Declaration**: **1 s** (`DEFINITION_GRACE`), mai lung: e o acțiune cerută, primul
  răspuns semantic al unui server poate aștepta încărcarea proiectului (vtsls ~1 s), iar două
  sărituri ar fi mai rele decât o mică așteptare. La termen, dacă motorul inclus știe declarația
  se sare acolo și întrebarea serverului e aruncată (`$/cancelRequest`: nicio a doua săritură);
  dacă nu știe, se așteaptă în continuare răspunsul serverului.
- **Corectat pe drum**: cât timp un card hover aștepta serverul, termenul lui era în trecut, iar
  cadrul cerea redraw imediat — cadre continue (la rata ecranului) până la răspuns, adică până la
  10 s cu un server tăcut. Acum `hover_deadline` dă, cât se așteaptă serverul, sfârșitul
  termenului: niciun cadru între timp.

**Teste** (aplicația, serverul fals; timeout-ul cererilor în teste 2 s, ca termenele să se
distingă de el): `a_silent_servers_suggestions_dont_wait_for_its_timeout` (lista motorului
inclus după termen, înainte de timeout, redraw-ul programat; cuvântul următor fără așteptare),
`a_late_servers_suggestions_join_the_list_without_moving_it` (serverul răspunde după 600 ms:
lista arătată rămâne prima, în aceeași ordine, selecția pe aceeași sugestie, și după încă o
literă), `a_silent_servers_card_signature_and_declaration_dont_wait_for_its_timeout` (cardul,
parametrii, declarația; fără cadre cât se așteaptă cardul; întrebarea declarației anulată).
Serverul fals: `slow` (o metodă răspunsă după o pauză). Modulul `lsp`: `idle_after` (oprirea la
inactivitate în secunde, pentru măsurători, ca `request_timeout`).

**Cum s-a măsurat** (macOS arm64, M2 Max; build release; HOME izolat, `EDITOR_NEW_INSTANCE=1`;
proiectele publice din scratchpad: `ky` de la 43a, plus `pico.css` de la 43b; serverele din
`target/lsp`; scripturile în `scratchpad/mine/43f/`): aplicația reală (`idle.sh`: RSS după 20 s,
CPU în următoarele 60 s, procesele copil) și, pentru tastare și oprirea la inactivitate,
binarul de snapshot în release cu două cârlige noi (doar cu feature-ul `snapshot`):
`EDITOR_SNAPSHOT_REAL_LSP=1` (serverele reale, nu cel fals), `EDITOR_SNAPSHOT_TYPING=<n>` (n
taste la sfârșitul fișierului activ, ~12 pe secundă, fiecare urmată de cadrul ei; între taste,
răspunsurile serverelor luate și pictate ca în aplicație; tipărește costul pe firul UI al fiecărei
taste și al fiecărui răspuns: editarea, răspunsurile folosite, scena cadrului — desenarea pe GPU
e aceeași cu sau fără server) și `EDITOR_SNAPSHOT_IDLE=<s>` (taburile închise, apoi procesul
rămâne s + 25 s; serverele se opresc după s secunde fără documente).

**Repaus** (aplicația reală; „web” = `ky/source/index.ts` + `package.json` + `pico.css`
deschise: vtsls, serverul JSON și cel CSS):

| Scenariul | Procese copil | RSS aplicație | RSS servere | CPU repaus 60 s: aplicația / serverele |
|---|---|---|---|---|
| un fișier PHP (niciun limbaj al unui server) | 0 | 105 MB | 0 | 0,033% / 0 |
| web, motoarele incluse (toate `built-in`) | 0 | 118 MB | 0 | 0,033% / 0 |
| web, servere (implicit), rularea 1 | 5 node | 121 MB | 679 MB (538 după 80 s) | 0,050% / 0,133% |
| web, servere (implicit), rularea 2 | 5 node | 120 MB | 682 MB | 0,033% / 0,133% |

Cele 5 procese: vtsls + cele două tsserver (semantic, sintactic) + serverul JSON + cel CSS;
tsserver are aproape toată memoria (plafonat de `max_memory`, 3 GB implicit; la 43a: 522–579 MB
singur). Aplicația nu cheltuie nimic în plus în repaus cu serverele pornite (clientul nu face
polling).

**Tastarea** (300 de taste la sfârșitul lui `ky/source/index.ts`, ~12/s; costul pe firul UI):

| Motorul | tastă p50 / p95 / p99 / max | răspunsuri pictate (n; p50 / p95 / max) | lista arătată după | RSS servere (vârf) |
|---|---|---|---|---|
| vtsls (implicit) | 1,96 / 5,45 / 6,05 / 6,43 ms | 886; 0,23 / 0,39 / 5,14 ms | 185 / 300 taste | 669 MB |
| motorul inclus | 3,12 / 5,55 / 7,97 / 9,32 ms | — | 182 / 300 taste | 0 |

Cu serverul, tasta costă chiar mai puțin (lista vine filtrată din răspuns; motorul inclus își
calculează contextul și sugestiile la fiecare tastă); niciun cadru nu trece de 10 ms, departe de
16,7 ms (60 Hz). Bugetul „0 blocări pe firul UI” e respectat: răspunsurile serverului doar se
iau (`poll`), sub 1 ms la p99.

**Oprirea la inactivitate** (`EDITOR_SNAPSHOT_IDLE=20`, aceleași trei fișiere, 12 taste, apoi
toate taburile închise; eșantion la 5 s):

| Momentul | Procese servere | RSS servere | RSS aplicație |
|---|---|---|---|
| cu fișierele deschise | 5 | 682 → 778 MB | 50–54 MB |
| taburile închise (t ≈ 10 s) | 5 | 778 MB | 54 MB |
| t ≈ 31 s (20 s după închidere) | **0** | **0** | 54 MB |

Toate procesele (inclusiv copiii tsserver) se termină, memoria lor se întoarce în întregime;
nimic nu rămâne pornit. (Aplicația din snapshot e mai mică decât cea reală: fără GPU.)

**Pornirea aplicației** nu e afectată: serverele pornesc la primul cadru (când documentele sunt
anunțate pe bus), pe firul lor (`lsp-<nume>` lansează procesul), la ~6 ms după ce driverul de
randare e activ; fereastra e creată în 158 ms (web, servere) / 176 ms (web, motoarele incluse) /
292 ms (PHP, prima pornire după build, rece). Un proiect fără fișierele unui limbaj nu pornește
niciun proces (0 procese copil, tabelul de mai sus; testele 43a îl verifică).

**Bugetele planului**, verificate:

| Bugetul | Rezultatul |
|---|---|
| 0 procese la un proiect fără fișierele limbajului | ✔ 0 procese (aplicația reală) |
| tastarea: 0 blocări pe firul UI, cererea veche anulată | ✔ p99 6 ms cu vtsls; un server tăcut nu mai ține nici lista (200 ms) |
| RAM raportat, tsserver plafonat, servere oprite la inactivitate | ✔ ~680 MB pentru trei servere web; oprite și eliberate la termen |
| CPU în repaus: 0 de la client | ✔ aplicația 0,033% cu și fără servere; serverele 0,13% |

Nimic n-a depășit un buget; singura problemă găsită (cadrele continue cât un card hover aștepta
serverul) e corectată.

**Imagini**: comparate cu referința (`ui-snapshots.sh compare`), nicio imagine nouă; se schimbă
doar `task-manager*` (în ambele teme), ca până acum doar prin cifra de memorie vie din bara de jos
(45 → 46 MB); restul sunt identice.

### 43g — phpactor pentru PHP (2026-11-02, branch `lsp`)

> Actualizat la 43j: phpactor rămâne implicit doar cât timp intelephense nu e instalat
> (acesta e primul în date acum); pe Windows tot motorul inclus până la o instalare.

Cerut: „hai cu phpactor dar la fel să nu pierdem deja ce avem și să putem face switch sau ce mai
trebuie”; să meargă și pe Windows; userul nu are nevoie nici de PHP, nici de Docker.

**Decizii (înainte de cod).**

- **Serverul**: phpactor 2026.10.07.0 (MIT), release-ul oficial `phpactor.phar` de pe GitHub
  (SHA-256 = digest-ul GitHub al fișierului), în `lsp/servers/phpactor/<v>/phpactor.phar`.
  Comparația de dinainte (`scratchpad/lsp-compare`): al nostru 60% corect, phpactor 83%,
  intelephense 88% (dar redenumirea doar în varianta plătită, licență proprietară: exclus).
- **PHP-ul care îl rulează, inclus** (userul nu are nevoie de PHP): un PHP CLI static per
  platformă din build-urile **static-php-cli** (MIT; PHP sub licența PHP 3.01), setul
  `minimal` pe macOS / Linux (`iconv mbstring pcntl posix filter tokenizer zlib phar`) și
  `spc-min` pe Windows (`ctype fileinfo filter iconv mbstring tokenizer phar`): exact ce cere
  phpactor (`mbstring`, `tokenizer`, `posix`, `phar`), fără `openssl` / `curl` (nimic din
  rețea nu poate fi folosit), 7–8 MB. Sursa: `dl.static-php.dev`, serverul de distribuție al
  proiectului (build-urile lui din GitHub Actions, legate din documentația static-php-cli);
  GitHub Releases-ul proiectului are doar unealta `spc`, nu PHP-ul — deci xtask primește o
  sursă nouă, doar pentru PHP, cu URL fixat pe versiune și SHA-256 fixat (calculat la
  descărcare, ca la taplo). PHP 8.4.22 pe toate platformele (aceeași versiune peste tot;
  phpactor cere ≥ 8.2 și citește singur sintaxa 8.5 prin parserul lui, nu prin PHP-ul care
  rulează). Placeholder nou `${php}`, ca `${node}`; `lsp/php/<v>/php` (`php.exe`).
  Interpretorul PHP al proiectului (detectat de `interpreter.rs`) **nu** e folosit pentru
  phpactor: rezultatul nu trebuie să depindă de ce PHP are userul (extensii lipsă, xdebug,
  versiuni vechi); el rămâne pentru nivelul limbajului și uneltele de calitate.
- **Pornit cu** `${php} -n -d memory_limit=${max-memory}M -d display_errors=stderr
  ${server}/phpactor.phar language-server`: `-n` = niciun `php.ini` al sistemului (build-ul
  static ar citi `/usr/local/etc/php/conf.d`), erorile PHP pe stderr (niciodată în
  protocol), `BOX_REQUIREMENT_CHECKER=0` (verificarea phar-ului cere `openssl` pentru
  `amphp/socket`, nefolosit la stdio).
- **Windows — verificat în surse**: README-ul phpactor la tag-ul 2026.10.07.0 spune „Linux or
  MacOS (Windows users will need to use WSL)”; issue-ul „Windows support?” (#1858) e deschis,
  PR-ul cu CI pe Windows (#2792) nemergiat; `composer.json` cere `ext-posix` (nu există pe
  Windows); PHP pe Windows nu poate face `stream_select` pe pipe-uri, deci stdio nu merge —
  extensia de editor a phpactor pornește pe Windows serverul pe TCP (`--address`, PR #181), iar
  alte editoare l-au oprit implicit pe Windows. **Decizia**: pe Windows PHP rămâne implicit pe motorul
  inclus (date, nu cod: `[language-servers.os.windows] default-engine = "built-in"` în
  descriere); phpactor rămâne de ales în Setări și pornește acolo prin TCP (câmp nou
  `transport = "tcp"`, placeholder `${port}`: un port liber pe 127.0.0.1 ales la fiecare
  pornire), cum face extensia de editor a phpactor — neverificat pe o mașină Windows, spus în setări /
  plan.
- **Optimizat** (opțiunile de inițializare ale phpactor = configurarea lui, peste a userului):
  - **CPU în repaus**: cauza celor 2,1–2,7% era watcher-ul de fișiere — fără evenimentele
    clientului, phpactor alege `find` / `php` (scanează tot proiectul la 5 s). Acum
    `indexer.enabled_watchers = ["lsp"]`: schimbările de pe disc vin de la watcher-ul
    proiectului nostru (`workspace/didChangeWatchedFiles`, `file-patterns = **/*.php`).
  - **niciun proces în plus**: diagnosticele și code actions „outsourced” (un `phpactor`
    pornit ca program la fiecare analiză — cu PHP-ul din PATH, deci nici n-ar merge fără
    PHP) oprite; `php -l` (providerul `php`: un proces la fiecare editare, erori false pe
    sintaxa mai nouă decât PHP-ul inclus, dublura erorilor noastre de sintaxă) oprit — doar
    providerul `code-action` (analizele WorseReflection); indexarea inițială în 2 procese
    (`indexer.parallel_workers = 2`: 4 s în loc de 24 s cu „auto” pe macOS, 7 s serial).
  - **zgomot**: coduri ignorate — `worse.missing_member` (fals pe orice clasă cu `__call` /
    `__callStatic`: `User::where()` din Eloquent), `worse.unused_import` (ale noastre, tăiate,
    și fără fals-pozitivul din docblock-uri), `worse.docblock_missing_*` /
    `worse.missing_return_type` (sugestii pedante).
  - **cache-ul** în `${cache}` (`<proiect>/.lumen/lsp/phpactor`: indexul, cache-ul
    WorseReflection, `XDG_CACHE_HOME`), șters de „Clear and Rebuild Caches” (43d).
  - **fără unelte externe** (phpstan, psalm, php-cs-fixer, phpcs, mago: oprite implicit,
    cum sunt la phpactor; uneltele de calitate ale aplicației rămân ale noastre),
    `language_server_configuration.auto_config = false` (fără întrebări), verificarea de
    încredere a `.phpactor.json` din proiect păstrată (întrebarea primește „nu”: config-ul
    unui proiect străin nu rulează nimic).
  - un singur phpactor pe proiect (fără root-markers: `composer.json` e și în fiecare pachet
    din `vendor/`).
- **Implicit Server, dar nimic pierdut** (`built-in = ["completion", "diagnostics", "rename"]`):
  - **completarea = uniunea**: câmp nou `completion` în `Kept` — lista phpactor (tipurile:
    lanțuri, generice, `?T`, `static`, shape-uri, membrii claselor built-in, `@method` /
    `@property`, fațade) întâi, apoi ale motorului inclus pe care nu le are (fiecare etichetă
    o dată), plus sugestiile framework-urilor (Laravel / Symfony / Yii: rute, view-uri,
    traduceri — ale lor, înaintea serverului, ca la 43e); fereastra de 200 ms a lui 43f
    rămâne: un phpactor ocupat (indexează) nu ține lista;
  - **Rename / Find Usages: ale noastre** (phpactor: N02 / N03 nimic, R04 eroare, R05 greșit);
  - **diagnosticele: ale noastre toate** (sintaxa, neutilizate, învechite, nivelul PHP,
    extensiile lipsă din `composer.json`, verificările framework-urilor) **plus** ale phpactor
    (metodă / clasă inexistentă, apel învechit, variabilă nedefinită…), fără dubluri;
  - hover, semnătura, declarația: phpactor întâi, al nostru la gol / eroare / timeout (43e/f);
    framework-urile și modulele întâi; Structura, refactorizările, intențiile, sublinierea ⌘,
    PHP-ul din Blade / Twig: ale noastre, ca înainte.
  - Cât timp phpactor pornește / indexează, răspunde motorul inclus (serverul nu e „gata”
    până la `initialize`; după, cererile lui au fereastra de 200 ms); progresul indexării
    (`$/progress`) apare în Task Manager („indexează X%”).
- „Motorul inclus” / „Amândouă” rămân de ales pe limbaj (PHP apare singur în blocul „Servere
  de limbaj”), aplicate pe loc.

**Făcut.**

- **Bundle-ul** (`packaging/lsp.toml`, `cargo xtask lsp-bundle`): secțiune nouă `[php]` (versiune,
  licență, `[php.platforms.<p>]` cu URL + SHA-256, `[[php.files]]` — licențele PHP și
  static-php-cli, fixate pe GitHub); sursa PHP-ului acceptată doar de la
  `https://dl.static-php.dev/static-php-cli/…/php-*` (`static_php`), descărcat o dată în
  `target/lsp-cache/php/<sha256>/`, despachetat (`.tar.gz` / `.zip`), pus în
  `lsp/php/<v>/php` (`php.exe`); manifestul primește `[php]` (`Bundle::php`). Un server care e
  doar fișierele lui (fără `packages`, fără `platforms`): phpactor, al cărui PHAR vine prin
  `[[servers.files]]`, care acceptă acum și fișierele unui release GitHub (nu doar
  raw.githubusercontent.com). `build.sh` universal unește singur și PHP-ul (orice program
  Mach-O cu geamăn). Licențele în `packaging/lsp/NOTICE` (PHP 3.01, static-php-cli MIT,
  libiconv LGPL-2.1 legat static în PHP-ul macOS / Linux — programul se poate reface din
  sursele publice —, zlib; phpactor MIT, dependențele lui MIT / BSD-3 / Apache-2.0 pentru
  `phpstorm-stubs`).
- **Clientul** (`lsp-client`): `${php}` (`Places::php`, `Missing::Php`); `transport = "tcp"`
  (`Transport`): portul liber ales la fiecare pornire înlocuiește `${port}` în argumente,
  clientul se conectează (20 s cât serverul pornește; ieșit înainte → eroare), stdout-ul
  serverului citit și aruncat; `default-engine`; `[language-servers.os.<os>]` (`OsSpec`:
  `default-engine`, `transport`, `args`) aplicat la citire (`parse_for(text, origin, os)`;
  `parse` = OS-ul curent), OS-uri și motoare necunoscute refuzate.
- **Modulul `lsp`**: motorul implicit al unui limbaj vine din descrierea primului lui server
  pe OS-ul acesta (`settings::default_choice`): o alegere egală cu implicitul nu se salvează;
  pe Windows PHP e „Motorul inclus” până îl alege userul (atunci se salvează `php=server`).
  Pagina de setări spune de ce, când motorul inclus e implicit din descriere: „phpactor nu e
  suportat de autorii lui pe acest sistem: implicit răspunde motorul inclus (îl poți alege
  totuși mai sus)”.
- **Domeniul**: `Kept::completion` (`built-in = ["completion"]`): sugestiile motorului inclus
  se unesc cu ale serverului oriunde amândouă au (`update_completion`: ale serverului întâi,
  apoi ale noastre fără dubluri; restul regulilor 43e/43f — framework-urile, fereastra de
  200 ms, lista târzie adăugată la sfârșit — neschimbate).
- **Modulul PHP**: `data/language-servers.toml` (phpactor, ca mai sus), contribuit ca
  `ServerData`. Blade / Twig rămân ale motorului inclus (limbajul fișierului nu e `php`).
- **Task Manager**: phpactor apare singur (un proces `php`, PID, memorie, CPU), cu „Indexing
  workspace X%” cât indexează (progresul `$/progress` al lui), apoi gata.
- **Găsit pe drum (phpactor)**: o întrebare pusă înainte ca indexul să fie gata, într-un
  proiect fără `vendor/` (clasele găsite doar prin index), îi rămâne în cache-ul
  WorseReflection ca „clasă negăsită” până la următoarea editare a documentului; cu cache-ul
  oprit nu se întâmplă, dar latențele cresc de 3–50× (hover-ul stub-urilor 0,9 → 49 ms p50) —
  păstrat pornit: în editor orice tastă trimite o editare. Directorul indexului e făcut de
  phpactor doar sub `%cache%` (un `index_path` explicit nu era creat): de aceea
  `XDG_CACHE_HOME = ${cache}`.
- **Și (cerut între timp, același arbore)**: serverele nu mai sunt abandonate niciodată —
  după 3 căderi în 3 minute se odihnesc 60 s (dublat la fiecare repetare, maxim 10 min) și
  sunt încercate din nou (Repornește le pornește pe loc); logul fiecărui server e păstrat și pe
  disc, `<proiect>/.lumen/lsp/logs/<server>.log` (2 MB, o rotație), căderile ajung în logul
  aplicației.

**Windows — verdictul**: phpactor nu e suportat oficial pe Windows (README-ul tag-ului:
„Linux or MacOS (Windows users will need to use WSL)”; #1858 deschis, CI-ul Windows #2792
nemergiat, `ext-posix` cerut; stdio nu merge — `stream_select` pe pipe-uri; extensia de editor
a phpactor folosește TCP pe Windows; alte editoare l-au oprit implicit). Deci pe Windows **PHP rămâne implicit pe
motorul inclus**, din date; phpactor rămâne de ales și pornește atunci prin TCP (testat pe
macOS: aceleași răspunsuri ca pe stdio, `phpactor_answers_over_a_local_port`); pe o mașină
Windows nu a fost încercat. PHP-ul Windows inclus (`spc-min`, 3,4 MB, împachetat cu UPX de
static-php-cli) n-are `posix`.

**Paritatea PHP** (motorul inclus la `df57d34b` vs implicitul de acum, Server):

| Funcția (motorul inclus) | Acum (Server, implicit) | Amândouă / Motorul inclus |
|---|---|---|
| completarea: stub-uri, proiect, variabile, framework-uri (Laravel / Symfony / Yii: rute, view-uri, traduceri, config…), `merge` | **uniunea**: ale phpactor întâi, toate ale noastre după, fără dubluri; framework-urile cu lista lor proprie — ale lor, ca înainte | ca înainte (43a) |
| hover, semnătura, declarația | phpactor întâi (tipurile), al nostru la gol / eroare / timeout / 200 ms (1 s la declarație); framework-urile / modulele întâi | — |
| Rename / Find Usages | **ale noastre** (serverul neîntrebat) | ale noastre |
| erorile de sintaxă, neutilizate, învechite, nivelul PHP, extensiile lipsă, verificările framework-urilor | **toate ale noastre** + ale phpactor (clasă nerezolvată, apel învechit, variabilă nedefinită…), fără dubluri | — |
| Structura, breadcrumb, plierea, ocurențele ⌘, refactorizările (Change Signature, Move, Extract…), intențiile, documentarea, uneltele de calitate (PHP CS Fixer, Pint, PHPCS, PHPStan) | neatinse (motorul inclus) | la fel |
| PHP în Blade / Twig, SQL în heredoc | motorul inclus | la fel |
| cât phpactor pornește / indexează / a căzut | motorul inclus (fereastra de 200 ms la fiecare întrebare) | — |

**Măsurători** (macOS arm64, M2 Max; phpactor pe PHP-ul inclus, cu descrierea de mai sus,
prin harness-ul Python al comparației — `scratchpad/mine/43g/client`, rezultatele în
`scratchpad/mine/43g/results/`; „al nostru” = rezultatele comparației de dinainte;
„implicit” = ce arată aplicația: răspunsurile celor două combinate după regulile de mai sus,
calculate din aceleași răspunsuri, `score_default.py`; 146 de sonde pe features / laravel /
symfony-demo):

| Categoria (sonde) | al nostru ok / parțial / nimic+greșit | phpactor (configurat) | implicit (aplicația) |
|---|---|---|---|
| completion (37) | 15 / 1 / 21 | 33 / 2 / 2 | 33 / 2 / 2 |
| definition (25) | 21 / 1 / 3 | 22 / 2 / 1 | 22 / 2 / 1 |
| references (10) | 7 / 2 / 1 | 7 / 1 / 2 | 7 / 2 / 1 |
| rename (7) | 5 / 1 / 1 | 5 / 0 / 2 | 5 / 1 / 1 |
| hover (14) | 8 / 0 / 6 | 14 / 0 / 0 | 14 / 0 / 0 |
| signature (8) | 7 / 0 / 1 | 7 / 0 / 1 | 8 / 0 / 0 |
| diagnostics (10) | 2 / 0 / 8 | 2 / 0 / 8 | 4 / 0 / 6 |
| symbols (5) | 5 / 0 / 0 | 5 / 0 / 0 | 5 / 0 / 0 |
| stubs (25) | 25 / 0 / 0 | 25 / 0 / 0 | 25 / 0 / 0 |
| zgomot (5) | 4 / 0 / 1 | 5 / 0 / 0 | 4 / 0 / 1 |
| **toate fără stubs / zgomot** | **70 / 5 / 41 (60%)** | **95 / 5 / 16 (81%)** | **98 / 7 / 11 (84%)** |

- Implicitul (84%) trece de phpactor de la comparație (83%) și de cel configurat (81%: G01 —
  metodă inexistentă — pierdut prin `worse.missing_member` ignorat; fals pe orice clasă
  magică) și nu e sub al nostru în nicio categorie: semnătura S01 (phpactor nimic) și
  referințele / redenumirile (R04 eroare, R05 greșit, N02 / N03 nimic la phpactor) sunt ale
  noastre, diagnosticele unite (G02, G05 ale lui + G08, G09 ale noastre). Zgomotul rămas (Z01)
  e al nostru și de dinainte: „requires ext-…” pentru extensii nedeclarate în `composer.json`
  (corect, de fapt, pentru proiectul de sonde).
- Câștigurile phpactor sunt cele de la comparație: tipurile (lanțuri, promoted, generice,
  `?T`, `static`, shape-uri, `list<T>`), membrii claselor built-in, `@method` / `@property` /
  fațade, hover pe variabile; ratate de amândouă: Eloquent prin `__callStatic` (C26, D16),
  C32, enum-uri prin `static` (C07 / C08 parțial).

| Proiectul | fișiere indexate | index (prima dată) | RSS vârf (indexare, 3 procese) / după sonde | CPU în repaus (120 s, după sonde) | cache pe disc |
|---|---|---|---|---|---|
| features | 570 | 4,1 s | 165 / 105 MB | 0,28% | 14 MB |
| laravel | 8 764 | 31,7 s | 275 / 177 MB | 0,25% | 81 MB |
| symfony-demo | 14 285 | 50,4 s | 362 / 174 MB | 0,27% | 148 MB |
| (comparația: phpactor în docker, configurat implicit) | | 5 / 37 / 58 s | 309–615 / 130–287 MB | 0,38 / 2,1 / 2,7% | |

- **CPU în repaus**: 2,1–2,7% → 0,13–0,16% în repaus curat (indexul gata, 0 sau 20 de
  documente deschise, 90 s) și 0,25–0,28% imediat după sonde. Cauza celor 2–3% era watcher-ul
  `find` / `php` (scanarea proiectului la 5 s); oprit. Ce rămâne e un timer intern al phpactor
  (`IndexerHandler::watch` verifică anularea la 100 ms, `BufferedWatcherProcess` la 500 ms) —
  neconfigurabil fără a modifica PHAR-ul; ținta ≤ 0,1% nu e atinsă. Fără documente PHP
  deschise serverul se oprește după inactivitate (10 min implicit), ca toate.
- Pornirea: gata în 0,11–0,12 s; a doua pornire refolosește indexul din `${cache}` (0,03–0,08 s
  pentru features). Indexarea cu `parallel_workers = 2`: 4,1 s pe features (auto: 24 s pe
  macOS, serial: 7 s); vârful de memorie e al celor 2 procese de indexare, care se termină.
- Latența (caldă, p50 / p95 ms): completare 16 / 34, declarație 1,3 / 16, referințe 1,6 / 4,
  redenumire 1,6 / 5, hover 0,8 / 8, semnătură 1,7 / 10, stub-uri 0,9 / 9 (prima întrebare:
  completare 21 / 121 — p95 sub fereastra de 200 ms, dar nu cu mult; ce trece de ea vine după
  lista noastră, la sfârșit, 43f). Diagnosticele după o editare: 1–6 ms (plus `diagnostic_sleep_time`
  de 1 s al lui).
- **Mărimea lui `lsp/`** (MiB, `cargo xtask lsp-bundle --platform all`): PHP 7,4 (macOS) /
  2,6 (Linux, UPX) / 3,4 (Windows, UPX) + phpactor 4,6: macOS arm64 353,1 → 365,1, macOS x64
  357,3 → 369,3, Linux x64 347,0 → 354,2, Windows x64 278,9 → 286,9; arhivat (zip -9) +7,1–8,0 MB.

**Teste**: `lsp-client` +5 (`what_differs_on_an_os_is_applied_when_read`,
`the_php_placeholder_is_the_bundled_php`, `the_php_modules_description_is_read` pe fiecare OS,
phpactor real din `target/lsp` pe stdio și pe un port local — completarea unei proprietăți
promovate, hover pe variabilă, declarația, diagnosticul de apel învechit, fără zgomot, un singur
proces, cache-ul în `${cache}`, HOME-ul lui temporar); modulul `lsp` +2 (un server implicit
oprit pe OS pornește doar ales; pagina spune de ce și salvează doar ce diferă de implicit);
domeniul (`Kept::completion`); aplicația +3 (serverul fals descris ca phpactor: uniunea
sugestiilor — ale lui întâi, `array_map` din stub-uri după —, redenumirea în alt fișier PHP
fără serverul întrebat, eroarea de sintaxă a noastră lângă avertismentul serverului); xtask +2
(PHP fixat pe fiecare platformă din sursa lui, aceeași versiune; phpactor din release-ul GitHub;
sursa PHP refuzată în rest).

**Imagini**: comparate cu referința (`ui-snapshots.sh compare`), se schimbă doar, așteptat,
`settings-language-servers` și `settings-4` (ambele teme): PHP în alegerea limbajelor (17, tot pe
două rânduri) și eticheta memoriei („serverele Node și PHP”: limita e și `memory_limit`-ul
PHP-ului). Imaginile cu fișiere PHP (Shop.php…) sunt identice: imaginile nu au bundle-ul, deci
motorul inclus răspunde ca înainte.

### 43h — intelephense instalabil (2026-11-02, branch `lsp`)

> Actualizat la 43j: intelephense e implicitul PHP pe orice OS odată instalat (primul în
> date), phpactor altfel; bara de descărcare îl oferă și pe el, iar bara de jos comută
> motorul.

Cerut (decis cu userul): ca într-un magazin de extensii — intelephense e proprietar (licența lui
interzice redistribuirea), deci **nu intră în installer**: userul îl instalează cu un clic din
Setări, o singură dată pentru toate proiectele, în folderul de date al aplicației, de pe
registry.npmjs.org, la o versiune fixată; dacă e deja instalat (npm global / PATH) e găsit și
oferit. Devine un motor PHP de ales lângă phpactor și motorul inclus. Cheia de licență
(opțională, funcțiile premium) stă în keychain-ul sistemului, niciodată într-un fișier text;
dată serverului ca `initializationOptions.licenceKey`; dacă există `~/intelephense/licence.txt`,
e oferită (doar citită). Implicit: phpactor pe macOS / Linux; pe Windows (phpactor nesuportat)
intelephense, odată instalat (motorul inclus până atunci). Nimic din motorul inclus nu se pierde
(regulile 43e / 43g).

**Decizii (înainte de cod).**

- **Instalarea, fără npm**: Node-ul inclus n-are npm (`lsp-bundle` pune doar `bin/node`), iar
  `lib/intelephense.js` din pachetul publicat e un bundle webpack care cere doar modulele Node
  (verificat: niciun `require` din `node_modules`) — deci tarball-ul singur e instalarea întreagă.
  Aplicația descarcă `https://registry.npmjs.org/intelephense/-/intelephense-<v>.tgz` (versiunea
  fixată în descrierea serverului, cu `integrity`-ul SHA-512 al registry-ului fixat tot acolo:
  verificat înainte de orice despachetare), îl despachetează (`package/`) într-un folder
  temporar și îl mută atomic în `<config>/lsp/installed/intelephense/<v>/`. Mai simplu și mai
  sigur decât npm: niciun script rulat, nicio dependență în plus, un singur fișier verificat;
  merge la fel pe Windows (căi, `node.exe`, nicio consolă: nu pornește niciun proces).
- **Date, nu cod**: descrierea din `data/language-servers.toml` al modulului PHP primește
  `[language-servers.install]` (`npm`, `version`, `integrity`, `program`, `licence-url`);
  `${server}` e folderul pachetului instalat (al nostru sau cel găsit). Un server cu `install`
  care nu e instalat nu apare ca motor și nu pornește.
- **Găsirea unei instalări existente** (în fundal, ținută minte): folderul nostru întâi, apoi
  programul `intelephense` din PATH (legătura lui urmată până la pachet), apoi `npm root -g`
  (npm-ul sistemului, doar când pagina PHP e deschisă și nimic nu s-a găsit). Rulat tot cu
  Node-ul nostru (rezultatul nu depinde de Node-ul userului).
- **Alegerea motorului**: alegerile PHP devin „phpactor”, „intelephense” (doar instalat),
  „Motorul inclus”, „Amândouă”. Implicitul vine din date: primul server al limbajului al cărui
  motor implicit pe OS-ul acesta nu e „built-in” și care e disponibil (unul instalabil doar
  instalat) — pe Windows phpactor are `default-engine = "built-in"`, deci intelephense instalat
  devine implicit acolo; pe macOS / Linux rămâne phpactor.
- **Cheia de licență**: `secrets` (keychain-ul: serviciul `<id>.language-servers`), câmp nou
  `secret-options = { licenceKey = "intelephense-licence" }` în descriere: citită de pe firul
  serverului, la pornire, niciodată pe firul UI; scoasă din logul traficului (și din logul de pe
  disc). Câmp mascat în Setări (rând nou `Secret`); `~/intelephense/licence.txt` oferit dacă
  există și nu e pusă o cheie.
- **Notificările lui**: licența intelephense interzice ascunderea notificărilor lui, deci
  `window/showMessage` ajunge ca notificare (câmp nou `show-messages = true`, doar la el).
- **Rutarea** ca a lui phpactor (`built-in = ["completion", "diagnostics"]`): completarea
  unită, diagnosticele unite fără dubluri, hover / semnătură / declarație întâi serverul;
  Structura, refactorizările, framework-urile, Blade / Twig ale noastre. Rename / Find Usages:
  funcție premium — fără cheie intelephense nu anunță `renameProvider`, deci rămân ale noastre;
  cu cheie, uniunea 43e (locurile lui în PHP, ale noastre în Blade / Twig), cu rezerva noastră
  la gol / eroare. Decis din capabilitățile lui, nu printr-o regulă nouă.
- **Configurat**: telemetria oprită, `storagePath` = `${cache}` (pe proiect, șters de „Clear and
  Rebuild Caches”), `globalStoragePath` = `${data}` (placeholder nou: folderul lui de date al
  aplicației, pentru toate proiectele — activarea licenței), `--max-old-space-size` = memoria din
  setări, fișierele excluse (cele implicite + view-urile compilate Laravel, cache-ul Symfony).
- **Task Manager**: descărcarea apare ca operație (Download, octeți) a modulului.

**Făcut.**

- **Clientul** (`lsp-client`): `install` (`Install`: `npm`, `version`, `integrity`, `entry`,
  `program`, `licence-url`, `size`), `secret-options`, `show-messages`, placeholder `${data}`,
  `Missing::NotInstalled`; un server instalabil nu cere bundle-ul pentru `${server}`. Modul nou
  `install`: URL-ul tarball-ului (doar `registry.npmjs.org`, și pachete `@scope/`), verificarea
  SHA-512 (base64 ca npm), despachetarea proprie a tarball-ului (ustar + nume lungi pax; doar
  fișiere și foldere — legăturile ignorate, o cale care iese din folder refuză tot arhiva), un
  folder temporar lângă cel final mutat atomic, `installed_in` (al nostru, și o versiune mai
  veche lăsată de o versiune anterioară a aplicației), `find_existing` (programul din PATH cu
  legătura urmată, shim-ul `.cmd` de pe Windows, `npm root -g`), `uninstall`. HTTP prin `ureq`
  (cel folosit deja de modelul căutării semantice), cu limită de mărime. La pornire (pe firul
  serverului) opțiunile secrete se citesc din `SecretStore` și se pun în `initializationOptions`;
  `Status::redact`: secretele înlocuite cu `‹redacted›` în log (în memorie și pe disc, în
  ultima eroare, și înainte de tăierea liniilor de trafic — nicio jumătate de cheie).
  `window/showMessage` → `Event::Message` la serverele cu `show-messages`.
- **Modulul `lsp`**: `install` (starea per server: căutat / lipsă / instalat — al nostru sau
  găsit —, instalarea cu progres, oprirea, dezinstalarea care întâi oprește serverele, ca
  fișierele să poată fi șterse pe Windows; operația Download în Task Manager, partea
  „Instalări de servere de limbaj”, cu Stop), `licence` (keychain-ul `<id>.language-servers`,
  ce știm — setată sau nu — citit în fundal, salvarea care repornește serverele care o
  folosesc, importul fișierului de licență), `settings::default_choice` / `offered` (implicitul
  și alegerile din date, ca mai sus), pagina: alegerile motorului doar cu serverele oferite,
  „Amândouă” păstrează serverul ales, rândurile serverului instalabil — „nu e inclus
  (proprietar): 4,8 MB, instalat la cerere [Instalează intelephense]” (întrebare înainte: licența,
  ce se descarcă, unde), progresul cu Oprește, „instalat, pentru toate proiectele
  [Dezinstalează]” (întrebare, roșu), câmpul mascat al cheii, „Am găsit o cheie în …
  [Folosește-o]”, notele (licența, folderul). Notificări: „Se instalează…” → instalat / eșuat.
- **UI** (`workbench-model`, `ui-workbench`): rânduri noi `SettingsRow::Secret` (câmp mascat,
  scris de la gol, ce e păstrat nu se arată niciodată) și `SettingsRow::Actions` (card cu
  butoane care nu distrug nimic, în culoarea accentului).
- **Modulul PHP**: descrierea intelephense (`data/language-servers.toml`), după phpactor.
- **Zgomotul diagnosticelor** verificat pe toate fișierele `app/` (laravel) și `src/`
  (symfony-demo): 0 diagnostice (nicio „metodă nedefinită” pe Eloquent: `@mixin` e înțeles) —
  nicio setare `diagnostics.*` oprită.

**Rutarea** (motorul inclus la `be759fff` vs intelephense ales):

| Funcția | intelephense fără cheie | intelephense cu cheie |
|---|---|---|
| completarea | uniunea (a lui întâi, a noastră după, framework-urile ca înainte) | la fel |
| hover, semnătura, declarația | el întâi, al nostru la gol / eroare / timeout | la fel |
| Rename / Find Usages | ale noastre (el nu anunță `renameProvider`) | locurile lui în PHP + ale noastre în Blade / Twig (43e) |
| diagnosticele | toate ale noastre + ale lui, fără dubluri | la fel |
| code actions (⌘.) | — (nu le anunță) | ale lui |
| Structura, refactorizările, intențiile, Blade / Twig, uneltele de calitate | ale noastre | ale noastre |

**Măsurători** (macOS arm64, M2 Max; intelephense 1.18.5 pe Node-ul inclus, configurat ca în
descriere, harness-ul comparației — `scratchpad/mine/43h/client`, rezultatele în
`scratchpad/mine/43h/results/`; 146 de sonde):

| Categoria (sonde) | al nostru | phpactor (43g) | implicit cu phpactor | intelephense (configurat) | implicit cu intelephense |
|---|---|---|---|---|---|
| completion (37) | 15 / 1 / 21 | 33 / 2 / 2 | 33 / 2 / 2 | 33 / 3 / 1 | 33 / 3 / 1 |
| definition (25) | 21 / 1 / 3 | 22 / 2 / 1 | 22 / 2 / 1 | 24 / 0 / 1 | 24 / 0 / 1 |
| references (10) | 7 / 2 / 1 | 7 / 1 / 2 | 7 / 2 / 1 | 10 / 0 / 0 | 7 / 2 / 1 |
| rename (7) | 5 / 1 / 1 | 5 / 0 / 2 | 5 / 1 / 1 | 0 / 0 / 7 (fără cheie) | 5 / 1 / 1 |
| hover (14) | 8 / 0 / 6 | 14 / 0 / 0 | 14 / 0 / 0 | 14 / 0 / 0 | 14 / 0 / 0 |
| signature (8) | 7 / 0 / 1 | 7 / 0 / 1 | 8 / 0 / 0 | 8 / 0 / 0 | 8 / 0 / 0 |
| diagnostics (10) | 2 / 0 / 8 | 2 / 0 / 8 | 4 / 0 / 6 | 9 / 0 / 1 | 9 / 0 / 1 |
| zgomot (5) | 4 / 0 / 1 | 5 / 0 / 0 | 4 / 0 / 1 | 4 / 0 / 1 | 4 / 0 / 1 |
| **toate fără stubs / zgomot** | **60%** | **81%** | **84%** | **88%** | **90%** |

| Proiectul | index (prima dată) | RSS vârf / după sonde | CPU în repaus (120 s, după sonde) | cache pe disc | latența caldă p50 / p95 (completare) |
|---|---|---|---|---|---|
| features | 0,3 s | 270 / 133 MB | 0,18% | 1 MB | 0,3 / 0,6 ms |
| laravel | 3,4 s | 529 / 209 MB | 0,27% | 14 MB | 2,0 / 2,8 ms |
| symfony-demo | 3,2 s | 404 / 203 MB | 0,19% | 15 MB | 0,5 / 0,7 ms |
| (phpactor, 43g) | 4,1 / 31,7 / 50,4 s | 165–362 / 105–177 MB | 0,25–0,28% | 14–148 MB | 16 / 34 ms |

- Implicitul cu intelephense (90%) trece de cel cu phpactor (84%) și nu e sub al nostru în nicio
  categorie; câștigurile lui: diagnosticele (9/10: clase nedefinite, argumente lipsă, apeluri
  învechite), definițiile (24/25), referințele (10/10 — rămân însă ale noastre fără cheie, vezi
  mai jos). Pornire 0,27–0,41 s; diagnosticele după o editare ~1 s (temporizarea lui).
- **CPU în repaus curat** (indexul gata, refolosit din `${cache}`, 90 s fără documente):
  0,00–0,01% (phpactor: 0,13–0,16%) — fără timere proprii; a doua pornire refolosește indexul
  (0,02–0,23 s), 58–305 MB RSS după.
- Fără cheie, intelephense nu anunță rename, code actions, declaration, implementation, type
  definition, folding (licența: funcții premium) — rutarea le lasă motorului inclus singură.

**Teste**: `lsp-client` — instalarea cu un registry fals (URL-ul, progresul, nume lungi pax,
legături ignorate, nimic lăsat lângă), o descărcare care nu se potrivește nu instalează nimic,
o arhivă care iese din folder refuzată, pachetul recunoscut după nume și intrare, o instalare
găsită altundeva (`npm root -g`) și una mai veche a noastră, base64 / integritatea; opțiunea
secretă dată la pornire și niciodată în log (cu trace pornit, și un mesaj care o repetă),
fără secret nimic adăugat și mesajele doar în log; descrierea intelephense pe fiecare OS,
pornirea din folderul instalat / `NotInstalled` / fără Node; intelephense real (doar cu
`EDITOR_TEST_INTELEPHENSE` = folderul pachetului, niciodată instalat de test: completarea unei
proprietăți promovate, hover pe variabilă, declarația, apelul învechit, nimic nedefinit, fără
rename fără cheie, indexul în `${cache}`). Modulul `lsp` — oferit și pornit doar instalat (și
implicit când primul server e al motorului inclus pe OS, ca phpactor pe Windows), descărcarea
doar la cerere, operația în Task Manager, dezinstalarea oprește serverul; descărcarea greșită;
cheia din secrete la server, scoasă din log, ștearsă → repornit fără ea; instalarea mai veche;
cheia citită în fundal, fișierul de licență doar citit; pagina (Instalează, întrebarea, câmpul
cheii, fișierul oferit și folosit, alegerea motorului salvată doar când diferă, Dezinstalează
roșu). Aplicația — intelephense ales fără cheie: redenumirea a noastră, serverul neîntrebat; cu
cheie: întrebat. Niciun test nu atinge rețeaua, keychain-ul, `~/.config` sau `~/intelephense`.

**Imagini**: noi `settings-language-servers-php` (neinstalat: Instalează),
`-php-installed` (instalat: Dezinstalează, câmpul cheii, fișierul de licență oferit),
`-php-licensed` (cheie păstrată: câmpul cu puncte); restul identice cu referința.

**Windows**: instalarea nu pornește niciun proces (descărcare + despachetare în Rust), serverul
pornește cu `node.exe` inclus (fără consolă, ca celelalte servere Node), căile cu `\` merg; pe
Windows intelephense instalat devine implicit pentru PHP. Neîncercat pe o mașină Windows.

**Rămas**: Find Usages din referințele serverului fără cheie (intelephense 10/10 vs 7/2/1 al
nostru) — cere separarea Find Usages de locurile redenumirii; stub-urile după versiunea /
extensiile PHP ale proiectului (`environment.phpVersion`, `stubs`: implicitele lui acum);
actualizarea versiunii fixate (workflow-ul lunar); intelephense citește singur și
`~/intelephense/licence.txt` dacă nu i se dă o cheie (comportamentul lui).

### 43i — servere descărcate la cerere (2026-11-02, branch `lsp`)

Cerut (decis cu userul): aplicația are 457 MB, din care `lsp/` ~365 MB. Aplicația se livrează
**fără servere**; la deschiderea unui fișier al cărui server lipsește apare o **bară discretă
deasupra editorului** (nu un modal): „TypeScript: pentru completare și verificări complete
descarc vtsls + Node.js (~45 MB, o singură dată). [Descarcă] [Nu acum] [Folosește motorul
inclus]”. Fișierul merge pe motorul inclus între timp; serverul pornește singur când
descărcarea se termină. Descărcările sunt **o singură dată și comune** tuturor proiectelor,
ferestrelor și versiunilor aplicației, în folderul de date al userului (macOS
`~/Library/Application Support/<id>/lsp/`, Windows `%LOCALAPPDATA%\<id>\lsp\` — fără admin,
Linux `~/.local/share/<id>/lsp/`); Node e descărcat o dată pentru toate serverele Node.

**Decizii (înainte de cod).**

- **Ce intră în aplicație**: un manifest de descărcare mic, generat de `cargo xtask
  lsp-manifest` din `packaging/lsp.toml` și lockfile-urile din `packaging/lsp/<server>/`
  (`crates/modules/tools/lsp/data/downloads.toml`, inclus în binar cu `include_str!`, nimic
  lângă aplicație): pe componentă (Node, PHP, fiecare server) versiunea, licența, pe
  platformă URL-ul, SHA-256-ul și mărimea arhivei / programului, folderul de sus de scos,
  ce se păstrează (Node: doar `bin/node` și `LICENSE`), pachetele npm din lockfile (calea în
  `node_modules`, URL-ul `resolved`, `integrity` SHA-512, mărimea), fișierele de date (scheme,
  PHAR-ul phpactor, licențe) cu SHA-256-urile lor, `prune`, `universal`, `inline` — pașii
  făcuți la instalare. Mărimile vin dintr-o cerere HEAD la generare (ținute din manifestul
  vechi, ca un test fără rețea să verifice că manifestul e la zi).
- **Instalatorul** (generalizarea celui de la 43h, în `lsp-client`): pe componentă, doar
  HTTPS și doar de pe gazdele permise (nodejs.org, registry.npmjs.org, github.com și
  `*.githubusercontent.com` ale lui — release-uri și raw —, dl.static-php.dev), redirecționările
  urmate de noi (fiecare verificată, maximum 5), suma verificată **înainte** de despachetare,
  despachetat de codul nostru (tar.gz, tar.xz — `lzma-rust2`, Rust pur —, zip, gz simplu,
  fișiere simple), legăturile simbolice sărite, orice cale care iese din folder refuză arhiva,
  biții de execuție puși, programul universal macOS subțiat la arhitectura care rulează (clangd).
  Pachetele npm puse direct din lockfile în `node_modules` (căile din lockfile, inclusiv cele
  imbricate), fără npm și fără scripturi; `strip-debug` (doar Linux) nu se poate face fără
  `strip` — sărit (clangd ~142 MB pe Linux în loc de 72). Totul într-un folder temporar lângă
  cel final, mutat atomic (`<data>/lsp/node/<v>/`, `php/<v>/`, `servers/<s>/<v>/`, același
  layout ca bundle-ul), cu un `.installed` (versiunea, amprenta rețetei, mărimea pe disc).
  Arhivele mari se descarcă în `<data>/lsp/.downloads/<sha256>.part`, reluate cu `Range` după o
  întrerupere. Un **lacăt pe fișier** (`.locks/<componentă>.lock`, `File::lock`) per
  componentă: două ferestre / instanțe nu instalează același lucru deodată — a doua așteaptă și
  vede că e gata.
- **Ordinea locatorului**: per componentă, `lsp/` de lângă aplicație (build-urile complete,
  offline; `target/lsp` în dezvoltare) întâi, apoi folderul de date comun (versiunea fixată,
  altfel una mai veche instalată — merge până vine cea nouă, actualizată automat în fundal:
  userul a consimțit deja la componenta aceea). Build-ul dist implicit **nu mai include `lsp/`**
  (`scripts/build.sh` / `build.ps1`: `--with-servers` / `EDITOR_WITH_SERVERS=1` îl pune ca
  înainte).
- **Bara**: un `EditorToolbarProvider` al modulului `lsp` (punctul de extensie existent al
  barelor de deasupra editorului; contextul primește limbajul fișierului), arătată doar când
  motorul limbajului e un server, serverul lui e descris în manifest și îi lipsește ceva;
  textul spune limbajul, ce se descarcă (Node doar dacă lipsește) și mărimea însumată.
  [Descarcă] → progresul (octeți) în bară și ca operație în Task Manager (partea
  „Instalări de servere de limbaj” de la 43h), cu Oprește; eroarea cu Reîncearcă; fără rețea —
  mesaj clar, motorul inclus merge mai departe. [Nu acum] o ascunde pentru limbaj până la
  următoarea pornire. [Folosește motorul inclus] salvează motorul inclus pentru limbaj
  (setarea `engines`, ca din Setări). Nu ia focusul, nu primește taste.
- **Setări → Servere de limbaj**: un bloc nou „Componente descărcate”: fiecare componentă —
  instalată / nu, versiunea, mărimea pe disc sau de descărcat, [Descarcă] / [Șterge] (întrebat,
  serverele care o folosesc se opresc întâi), [Descarcă tot], spațiul folosit în total, folderul.
- **Curățarea (GC)**: la pornire, pe un fir de fundal: fiecare versiune instalată care nu e
  fixată de aplicația aceasta, nu e cea mai nouă rămasă a unei componente a cărei versiune
  fixată lipsește (rezerva în uz) și nu e numită de **fișierul de referințe** al altei versiuni
  a aplicației atins în ultimele 30 de zile (`refs/<versiune>-<amprenta manifestului>.toml`,
  rescris la fiecare pornire și după fiecare instalare) e ștearsă — sub lacătul componentei
  (nicio instalare în curs). Fișierele de referințe mai vechi de 30 de zile, folderele
  temporare și descărcările parțiale de peste 7 zile se șterg. Aplicația aceasta nu pornește
  servere înainte de curățare; o altă versiune care rulează acum și-a scris referințele la
  pornire.
- **intelephense** (43h) se mută în același folder (`<data>/lsp/servers/intelephense/<v>/`,
  `${data}` = `<data>/lsp/data/<server>`), prin același descărcător; o instalare din
  `<config>/lsp/installed/` (43h) e mutată acolo la pornire (și găsită acolo dacă mutarea nu
  merge). Licența, keychain-ul și consimțământul ei rămân ca la 43h; nu intră în „Descarcă tot”.
- **Testele** nu ating rețeaua, folderul de date real, `~/.config`, keychain-ul: folderul de
  descărcări și sursa (un registry / server în memorie) se dau din test; un binar de test (din
  `deps/`) n-are folder de descărcări decât dacă i se dă. Un test real (doar cu
  `EDITOR_TEST_LSP_DOWNLOAD=<folder>`) instalează tot din manifest și pornește fiecare server.

**Făcut.**

- **Manifestul** (`crates/modules/tools/lsp/data/downloads.toml`, 58 KB, 12 componente, 183 de
  URL-uri, inclus în binar): `cargo xtask lsp-manifest` (nou; `--check` fără rețea) din
  `packaging/lsp.toml` + lockfile-uri — Node / PHP (arhiva pe platformă, folderul de sus,
  `keep` = programul și licența, `bin`), serverele Node (fiecare pachet din lockfile, fără cele
  `dev` / `link` / `inBundle`, cu `os` / `cpu` dacă le are), serverele native (fișierul
  release-ului pe platformă, `strip`, `prune`, `universal`), fișierele de date (schemele,
  PHAR-ul, licențele), `inline`; mărimile din HEAD (sau, la cache-ul npm care nu le spune la
  HEAD, din `Content-Range` al primului octet), ținute apoi din manifest. Un test xtask îl
  refăce fără rețea și cere să fie identic.
- **`lsp-client`**: `fetch` (HTTPS, gazdele permise — cele de mai sus plus
  `static-php-cli.fra1.digitaloceanspaces.com`, unde redirecționează dl.static-php.dev, găsit la
  instalarea reală —, fără credențiale / port, redirecționările urmate și verificate de noi,
  `Range` pentru reluare, limită de mărime, eroarea „fără rețea” deosebită; `Memory` pentru
  teste / imagini: offline, întrerupt după N octeți, blocat până e eliberat), `archive`
  (tar în flux — ustar, pax, nume lungi GNU, numere base-256, biți de execuție, legături sărite,
  corp tăiat = eroare —, gzip, xz prin `lzma-rust2`, zip citit de noi — stored / deflate,
  zip64, legăturile Unix sărite —, gz simplu, fișier simplu; `Strip`, `keep`, `prune`, prefixul
  din `node_modules`; ieșirea din folder refuză arhiva), `assemble` (paşii comuni cu bundler-ul:
  `pruned`, `safe_relative`, `thin`, `inline_schema` — fișierul e și al xtask-ului prin
  `#[path]`, o singură sursă), `store` (layout-ul, `.installed`, `find` — fixată, altfel cea mai
  nouă rămasă —, `Lock` cu `File::try_lock`, `collect`, `write_refs`), `downloads` (modelul
  manifestului, verificarea lui, amprenta rețetei, `install`). 43h (`install`) trece pe ele
  (aceeași descărcare verificată, același despachetator, un lacăt, un `.installed`).
- **Modulul `lsp`**: `downloads` (folderul comun — `<data>/lsp`, niciodată pentru un binar de
  test; starea per componentă, progresul, oprirea, eșecul cu „offline”, instalarea în fundal
  una după alta, așteptarea unei instalări a aceleiași componente în același proces sau în
  altul, ștergerea care oprește întâi serverele care o folosesc — `ProjectServers::let_go` —,
  întreținerea la primul proiect: mutarea 43h, curățarea, referințele, actualizarea
  versiunilor vechi), `catalog::places` per componentă: bundle-ul, apoi descărcatele;
  `bar` (bara), `parts` (blocul din setări), comenzile „Download the File's Language Server” /
  „Use the Built-in Engine for the File's Language” (cea din urmă și a butonului barei:
  salvează în setările userului prin `host.user_settings()`); intelephense instalat în
  `<data>/lsp/servers/intelephense/<v>/`, datele în `<data>/lsp/data/`.
- **Bara**: `ToolbarContext` primește limbajul fișierului (`ui-components`; aplicația îl dă la
  deschidere și la mutare); bara modulului are înălțime 0 când n-are ce spune (barele se
  așează la fiecare cadru), deci dispare singură: descărcat, „Nu acum”, motorul schimbat.
  Textul: limbajul, părțile („vtsls + Node.js”: serverul întâi, Node doar dacă lipsește),
  mărimea însumată; în timpul descărcării „… 12 MB din 34 MB”, bara de progres pe marginea de
  jos, [Oprește] (ce s-a descărcat se păstrează și se reia); eșecul — „fără conexiune la
  internet” sau motivul — cu [Reîncearcă]; la final o notificare „TypeScript: vtsls e gata”
  (doar de la bara care a pornit descărcarea). Recalculată doar când se schimbă descărcările,
  setările sau ascunderile (două numere), nu la fiecare cadru.
- **Setări → Servere de limbaj**: blocul „Servere descărcate” (fiecare componentă: descărcată
  — versiunea, mărimea pe disc, [Șterge] întrebat —, nedescărcată — mărimea, [Descarcă] —, se
  descarcă — [Oprește] —, eșuată, „inclus în aplicație” la un build complet, „se
  actualizează la …”; [Descarcă tot (…)], spațiul folosit și folderul, nota despre surse).
- **Build-uri**: `scripts/build.sh` și `build.ps1` nu mai pun `lsp/` implicit; `--with-servers`
  / `-WithServers` / `EDITOR_WITH_SERVERS=1` îl pun ca înainte (build complet, offline).
  `cargo xtask lsp-bundle` rămâne (dezvoltarea, build-urile complete).

**Instalarea reală** (macOS arm64, M2 Max, rețeaua de acasă; `EDITOR_TEST_LSP_DOWNLOAD` într-un
folder din scratchpad, build release): toate cele 12 componente în **36 s**; arborii rezultați
sunt identici fișier cu fișier cu cei din `target/lsp` făcuți cu `npm ci` (în afară de
`package.json` / `package-lock.json` din rădăcină, necitite de servere), clangd subțiat identic;
cele 14 teste cu serverele reale (`real_servers`, `EDITOR_LSP_DIR` = folderul) trec pe ce s-a
descărcat.

| Componenta | descărcat | pe disc | timp |
|---|---|---|---|
| Node 26.13.0 | 29,2 MB | 140,7 MB | 3,3 s |
| PHP 8.4.22 | 3,3 MB | 7,4 MB | 1,7 s |
| vtsls | 5,0 MB | 27,0 MB | 2,1 s |
| vscode-langservers-extracted | 14,5 MB | 27,2 MB | 6,7 s |
| clangd | 95,4 MB | 99,6 MB | 4,9 s |
| lua-language-server | 3,1 MB | 7,6 MB | 0,6 s |
| zls | 1,0 MB | 3,4 MB | 0,6 s |
| bash-language-server | 3,6 MB | 18,1 MB | 2,0 s |
| yaml-language-server | 4,0 MB | 15,2 MB | 2,7 s |
| dockerfile-language-server-nodejs | 0,4 MB | 2,3 MB | 0,6 s |
| taplo | 6,5 MB | 11,9 MB | 10,0 s |
| phpactor | 4,6 MB | 4,6 MB | 0,9 s |

**Ce se descarcă pe grup de limbaje** (MB, comprimat, Node inclus o dată; din manifest):

| Grupul | macOS arm64 | macOS x64 | Linux x64 | Windows x64 |
|---|---|---|---|---|
| JS / TS (vtsls + Node) | 34,2 | 36,2 | 37,5 | 44,3 |
| HTML / CSS / SCSS / JSON (+ Node) | 43,7 | 45,7 | 47,0 | 53,7 |
| YAML (+ Node) | 33,2 | 35,2 | 36,5 | 43,2 |
| Bash (+ Node) | 32,9 | 34,8 | 36,1 | 42,9 |
| Docker (+ Node) | 29,6 | 31,6 | 32,9 | 39,7 |
| PHP (phpactor + PHP) | 7,9 | 8,0 | 7,2 | 7,9 |
| C / C++ (clangd) | 95,4 | 95,4 | 112,5 | 28,3 |
| Lua / Zig / TOML | 3,1 / 1,0 / 6,5 | 2,2 / 1,2 / 6,8 | 3,5 / 3,9 / 7,0 | 4,2 / 4,6 / 7,1 |
| tot | 170,8 | 172,4 | 194,1 | 118,9 |

(Node singur: 29,2 / 31,2 / 32,5 / 39,3 MB; o dată pentru toate serverele Node. clangd pe macOS
e zip-ul universal al lor — descărcat întreg, subțiat la instalare.)

**Mărimea aplicației fără servere** (estimare, fără build dist): binarul release macOS arm64
70,6 MB (universal ~2×); `lsp/` (~365 MB din cele 457) nu mai intră → aplicația ~90 MB
(universal, cu resursele), arhivată mai puțin; Windows: doar `.exe`-ul.

**Abateri / nefăcut**: `strip-debug` nu se poate face la instalare (fără `strip` la user):
clangd și ZLS pe Linux păstrează informația de depanare (clangd ~142 MB în loc de 72);
clangd pe macOS se descarcă universal (95 MB; release-ul lor n-are fișiere pe arhitectură).
Nicio verificare pe o mașină Windows (căile, `node.exe`, `File::lock` sunt ale bibliotecii
standard; testele „Windows” instalează platforma `windows-x64` pe macOS). Bara e o bară de
editor, deci una pe fișier: o consolă SQL își păstrează propria bară (SQL n-are server).
„Nu acum” e per limbaj, nu per server.

**Teste**: `lsp-client` — gazdele permise, redirecționările, verificarea SHA-256 / SHA-512,
prea mare, refuzat fără să fie cerut, fără rețea, descărcarea reluată după o întrerupere de
unde a rămas, partea greșită ștearsă, oprirea; arhivele (tar.gz și tar.xz cu nume lungi,
execuție, legături sărite, `keep`; prune și prefixul npm; ieșiri din folder refuzate în tar și
zip — `..`, absolut, `C:`, `\`; zip stored / deflate cu legături; gz simplu; arhive stricate);
store (fixată / mai veche / altă rețetă / fără marker, lacătul ținut de unul — al doilea
așteaptă și o primește după —, curățarea: ce e folosit aici și de altă versiune recent, nu ce
instalează alt proces, referințele vechi, temporarele, părțile vechi); downloads (manifestul
aplicației citit, sursele permise, căile; pachete pe alt sistem lăsate; runtime-ul doar cu
programul lui; zip-ul și `node.exe` pentru Windows; un server Node din lockfile cu pachete
imbricate, prune, schema inlinată; o sumă greșită nu instalează nimic; reluarea și oprirea;
doi instalatori — o descărcare, reluată; programul universal subțiat pe fiecare arhitectură;
altă rețetă refăcută) și testul real (doar cu `EDITOR_TEST_LSP_DOWNLOAD`). Modulul `lsp` —
oferta (ce trebuie, Node doar dacă lipsește, nimic unde nu se poate descărca), serverul pornit
singur după descărcare din folderul comun, operațiile din Task Manager, bundle-ul întâi,
fără rețea apoi din nou, ștergerea care oprește serverele, versiunea mai veche folosită și
actualizată, curățarea și referințele, mutarea 43h, a doua instalare care o așteaptă pe
prima; bara (doar unde e ceva de descărcat, textul cu mărimea, „Nu acum” pe toate barele
limbajului, motorul inclus salvat, progresul și oprirea, eșecul cu Reîncearcă și notificarea
de la final); blocul din setări (rândurile și butoanele, Șterge întrebat, Descarcă tot, fără
descărcări). Niciun test nu atinge rețeaua, folderul de date real, `~/.config` sau keychain-ul.

**Imagini**: noi `lsp-offer` (bara peste un fișier TypeScript), `lsp-offer-downloading`
(descărcarea la 40%: progresul, Oprește), `settings-language-servers-parts` (lista); în
`settings-language-servers*` apare blocul nou „Servere descărcate” (fără descărcări în imagini:
„Această versiune nu descarcă servere.”) și textele schimbate (`lsp_detail`, nota fără bundle).

### 43j — funcțiile premium, ale noastre (2026-11-02, branch `lsp`)

Cerut: intelephense își păstrează pentru licența plătită Rename, Code folding, Find all
implementations, Go to type definition, Go to declaration, Smart select, Code actions, Code
lens, Type hierarchy, Inlay hints, Document links. „Ce nu ajută intelephense să acoperim noi”:
le face motorul inclus, pentru toți, fără licență — întâi pentru PHP, generic unde e ieftin
(arborele tree-sitter, indexul nostru) — și le cere întâi serverului activ acolo unde îl are
(capabilitatea anunțată), cu regula 43e: serverul întâi, al nostru la gol / eroare / timeout /
serverul nu e gata; al nostru mereu unde nu răspunde niciun server. Nimic pierdut; nimic pe cadru,
nimic greu pe firul UI.

**Matricea funcție → sursă** (✓ = o face; „cap.” = doar dacă serverul o anunță; ce scrie în
coloana „al nostru” e și rezerva):

| Funcția | Motorul inclus (al nostru) | phpactor (implicit PHP) | intelephense cu cheie | alte servere |
|---|---|---|---|---|
| Rename | ✓ (43e, inclusiv Blade / Twig) | ✓ (Rename rămâne al nostru, `built-in = rename`) | ✓ (uniunea 43e) | vtsls, clangd… (uniunea 43e) |
| Code folding | ✓ tree-sitter, toate limbajele cu gramatică | — (nu-l cerem) | — (nu-l cerem) | — (nu-l cerem) |
| Go to declaration | ✓ (indexul) | ✓ întâi el | ✓ întâi el | ✓ întâi el |
| **Find all implementations** (⌘⌥B) | **nou**: indexul — clasele care implementează / extind (tranzitiv), metodele care suprascriu / implementează; listă dacă sunt mai multe | ✓ (`implementationProvider`) întâi el | ✓ | cap. (vtsls, clangd) |
| **Go to type definition** (⇧⌘B) | **nou**: tipul variabilei (`new`, parametru, `@var`), proprietății, al ce întoarce metoda / funcția (semnătura din index / stub-uri) | ✓ (`typeDefinitionProvider`) întâi el | ✓ | cap. (vtsls, clangd) |
| **Smart select** (⌥↑ / ⌥↓) | **nou**: arborele tree-sitter, orice limbaj cu gramatică (straturile încorporate incluse), interiorul parantezelor / ghilimelelor înaintea lor, mai multe cursoare | — (nu cerem `selectionRange`: arborele ajunge și e instant) | — | — |
| **Code actions** (⌥⏎, ⌘.) | **nou**: becul la cursor + meniul: ale noastre (refactorizările aplicabile — Rename, Change Signature, Move, diagrama tipului —, „Importă clasa”, „Șterge importul nefolosit”) | ✓ ale lui, în același meniu, deduplicate | ✓ | cap. (vtsls, clangd, CSS…) |
| **Code lens** (setare, implicit pornit) | **nou** PHP: „N folosiri · M implementări / suprascrieri” la clase, interfețe, metode — numărate în fundal din index (implementările) și din fișierele proiectului fără `vendor/` (folosirile), cache pe simbol + generația indexului | — | cap. | cap. (vtsls: referințe / implementări) |
| **Type hierarchy** (⌃H) | **nou**: popup cu supertipurile și subtipurile (indexul), „Arată diagrama” spre Type Diagram | — (nu o anunță) | ✓ | cap. (clangd) |
| **Inlay hints** (setare, implicit pornit) | **nou** PHP (și orice limbaj cu semnăturile în index): numele parametrilor la apeluri, înaintea argumentelor literale / ambigue, din index și stub-uri | cap. (oprite implicit la el) | ✓ | vtsls / clangd cu setările lor de nume de parametri pornite |
| **Document links** (⌘-clic, subliniate la ⌘) | **nou**: căile din `require` / `include`, șirurile care sunt căi din proiect, `import` / `require` JS / TS, `href` / `src` HTML, `url()` CSS, URL-urile (deschise în browser); `use` spre fișierul clasei rămâne „Go to Declaration” | — | cap. | cap. (serverele HTML / CSS din `vscode-langservers-extracted`) |

**Decizii (înainte de cod).**

- **Portul** (`code_intelligence::servers::LanguageServers`) primește `implementations`,
  `type_definition`, `type_hierarchy`, `inlay_hints`, `code_lens`, `document_links`; clientul
  (`lsp-client`) le anunță în `initialize` și le citește din capabilități (`Feature` nou pe
  fiecare); un server care nu le anunță → `None` → al nostru. Lanțurile LSP
  (`prepareTypeHierarchy` → `supertypes` / `subtypes`, `codeLens` → `codeLens/resolve`) se fac
  pe un fir al modulului, nu pe al aplicației.
- **Logica noastră e în domeniu** (pură, testată fără aplicație): `hierarchy`
  (implementări, suprascrieri, supertipuri / subtipuri, numărătorile lentilelor), `links`
  (legăturile unui text, rezolvate prin proiect), `inlay` (numele parametrilor la apeluri);
  limbajele spun doar ce e al lor (`LanguageIntelligence::type_at`, tipul scris al unei
  variabile). Selecția inteligentă e în `syntax-treesitter` (`SyntaxSnapshot::selection_ranges`).
- **Aplicația doar leagă** (ca Go to Declaration): comenzi noi în `commands.rs` cu tastele
  setului JetBrains (⌥↑ / ⌥↓, ⌘⌥B, ⇧⌘B, ⌃H, ⌥⏎) și preset-urile VS Code / Zed / Sublime fără
  conflicte; listele (implementări, ierarhie, acțiuni) sunt lista de alegere existentă
  (`ChoiceMenu`), lângă cursor.
- **Optim**: becul, lentilele și indiciile se calculează leneș, în fundal, după o pauză
  (debounce 300 ms / 600 ms), doar pentru rândurile vizibile (cu o margine), ținute pe
  (document, versiune, fereastră) — un cadru doar le citește; editorul le desenează din ce
  i s-a dat (nimic recalculat pe cadru). Indiciile sunt spații virtuale în rândul afișat:
  caret-ul, selecția, clicul, copierea rămân pe textul real.
- **Setări** (`Setting<T>` în `settings.rs`, blocul Editor): „Indicii în cod” (numele
  parametrilor) și „Lentile de cod” (folosiri / implementări), ambele pornite implicit.
- **43i**: textul barei de descărcare se scurtează și se rupe pe două rânduri când nu
  încape lângă butoane.

**Făcut — 43j1 (navigarea).**

- **Smart select** (`edit.extend_selection` ⌥↑, `edit.shrink_selection` ⌥↓; VS Code / Zed /
  Sublime: ⌃⇧⌥→ / ⌃⇧⌥←, ⌥↑ fiind „Move Line” acolo): `syntax_ts::selection_ranges` urcă
  nodurile arborelui (întâi interiorul unei perechi de delimitatori — paranteze, acolade,
  ghilimele —, apoi nodul), `SyntaxSnapshot::selection_ranges` trece din stratul încorporat
  în părinții lui până la document; primul pas e cuvântul (cu `$`-ul PHP); fiecare cursor
  crește singur; „Shrink” întoarce exact pașii făcuți (o altă selecție îi uită); fără
  gramatică: cuvântul, textul rândului, rândul, fișierul; în afara editorului tastele mută
  cursorul ca înainte.
- **Go to Implementation** (`navigate.implementation`, ⌘⌥B; VS Code: ⌘F12), **Go to Type
  Declaration** (`navigate.type_declaration`, ⇧⌘B), **Type Hierarchy**
  (`navigate.type_hierarchy`, ⌃H; fără tastă în VS Code / Zed / Sublime, unde e Replace), și în
  meniul editorului (Navigate). Întâi serverul, dacă anunță capabilitatea
  (`textDocument/implementation`, `typeDefinition`, `prepareTypeHierarchy` →
  `typeHierarchy/supertypes` / `subtypes` pe un fir `lsp-chain`, 4 niveluri, 120 de tipuri),
  răspunsul așteptat 1 s (`DEFINITION_GRACE`); gol / eroare / timeout / serverul nepregătit / fără
  capabilitate → al nostru. Al nostru (`code_intelligence::hierarchy`): `Subtypes` (indexul
  întors: cine are pe cine ca părinte, o dată pe întrebare), implementările tranzitive (fără
  interfețe, `vendor/` la urmă), suprascrierile (metodele cu același nume declarate de
  subtipuri; PHP fără majuscule), ierarhia (supertipuri cu tipurile necunoscute după nume,
  subtipuri, 6 niveluri, 500 de tipuri), `declared_type` / `class_in_type` (tipul proprietății,
  ce întoarce metoda, `?A|null` → `A`, `self` / `static` → clasa); PHP spune tipul scris al
  unei variabile (`LanguageIntelligence::type_at`: `new`, parametrul, `@var`, rândul ei inclus).
  Un rezultat: salt; mai multe: lista de alegere de lângă cursor (iconița tipului, calea
  relativă și rândul; ierarhia: supertipurile deasupra, tipul ales, subtipurile, „Arată
  diagrama” cu tasta ei). `refactor::plan_of` (modificările unui server ca plan de
  refactorizare) mutat din modulul `lsp` în domeniu, pentru acțiunile aplicației.
- **Clientul**: capabilitățile noi anunțate și citite (`Feature::{Implementation,
  TypeDefinition, TypeHierarchy, InlayHints, CodeLens, DocumentLinks}`), cererile lor, inclusiv
  `codeLens/resolve` și `documentLink`; serverul fals le răspunde (pentru teste și imagini).

**Făcut — 43j2 (becul, acțiunile, legăturile).**

- **Show Context Actions** (`code.show_actions`, ⌥⏎ și ⌘. — comanda `lsp.code_actions` a
  modulului `lsp` dispare, înlocuită): lista de alegere de lângă cursor cu, în ordine,
  remedierile noastre (**Importă clasa** — numele de la cursor nu se rezolvă, indexul știe o
  clasă cu numele lui: câte un rând pe clasă, prin `import_edit` al limbajului; **Șterge
  importul nefolosit** — analiza „unused” de la cursor, rândul `use …;` întreg), apoi
  refactorizările care se pot rula aici (rândurile modulelor din secțiunea Refactor a meniului
  editorului: Rename, Change Signature, Move… — nicio comandă scrisă ca text), apoi acțiunile
  serverului (`textDocument/codeAction`, fiecare titlu o dată, ale noastre întâi; `quickfix` /
  „preferred” printre remedieri, `source.*` la urmă). O acțiune a serverului: lucrată
  (`run_action`), aplicată ca refactorizare (`refactor::plan_of`, un singur undo), comanda ei
  rulată de server (`workspace/executeCommand`, editările lui venind prin modul). Lista
  așteaptă serverul cel mult 200 ms (`SERVER_GRACE`), altfel apare fără el.
- **Becul**: după ce cursorul stă 300 ms (`BULB_DELAY`, un singur cadru programat) se calculează
  remedierile noastre (ieftine, indexul) și se întreabă serverul; becul apare în gutter, la
  rândul cursorului, în locul mostrei de culoare: galben (culoarea avertismentelor) pentru o
  remediere, stins pentru alte acțiuni; acțiunile `source.*` ale serverului (organize imports,
  peste tot) nu aprind becul. Clic pe el = ⌥⏎. Nimic pe cadru: o cheie (document, versiune,
  cursor) comparată.
- **Document links**: cu ⌘ apăsat, sub mouse, legăturile documentului subliniate ca până acum,
  eticheta spunând unde duc (fișierul, adresa); ⌘-clic le urmează (fișierul deschis — la rândul
  spus de server —, URL-ul în browser). Întâi ale serverului (`textDocument/documentLink`, cerute
  o dată pe versiune, cât timp ⌘ e ținut), altfel ale noastre (`links::link_at` pe rândul de
  sub mouse: URL-urile; căile în ghilimele existente în proiect — `require` / `include` (și
  după `__DIR__ .`), `import` JS / TS cu extensia subînțeleasă și `index.*`, `href` / `src`
  din rădăcină sau `public/`, `url()` CSS; nimic din afara proiectului). Declarația rămâne
  întâi (⌘-clic pe un nume), un URL câștigă peste un nume din el.

**Făcut — 43j3 (indiciile, lentilele, bara 43i).**

- **Inlay hints** (setarea „Numele parametrilor la apeluri”, `[editor] inlay_hints`, pornită):
  numele parametrului înaintea argumentelor literale (număr, șir, `true` / `false` / `null`,
  tablou), nu la argumentele numite, nici când numele funcției se termină cu al parametrului
  (`setTotal(5)`), un variadic o dată (`...parts:`); semnătura din index (metodă, funcție,
  constructorul la `new`) sau din limbaj (stub-urile PHP) — `code_intelligence::inlay`, generic
  pentru orice limbaj cu motorul nostru. Întâi serverul (`textDocument/inlayHint`, vtsls /
  clangd / intelephense; gol → al nostru). Doar rândurile vizibile cu o margine de 60 (în
  blocuri: un scroll mic nu cere nimic), după o pauză de 300 ms de la tastare (scroll-ul: imediat),
  în fundal (pool-ul proiectului; declarațiile fișierului din textul lui de acum), ținute pe
  (document, versiune, fereastră); între timp mutate prin modificările documentului.
  Editorul le face loc ca spații în rândul afișat (`expand_tabs_with` / `source_to_display_with`
  / `display_to_source_with`): caret-ul, selecția, clicul, squiggle-urile, copierea rămân pe
  textul real; desenate ca jetoane mici, estompate.
- **Code lens** (setarea „Folosiri și implementări după declarații”, `[editor] code_lens`,
  pornită): după rândul fiecărei clase / interfețe / trăsături / enumerări, funcții și metode
  (constructorii nu), la capătul rândului (nu deasupra: rândurile rămân ale textului) —
  „N folosiri” (rândurile care le folosesc în fișierele proiectului, fără `vendor/`, regulile
  Find Usages ale limbajului, `count_usages`: fiecare fișier citit o dată pentru toate
  declarațiile; peste 5000 de fișiere nu se numără; numărătorile celorlalte fișiere ținute pe
  generația fișierelor, deci la tastare se renumără doar fișierul lui) · „M implementări” /
  „M suprascrieri” (indexul). Clic pe o parte: Find Usages (`workbench_model::FIND_USAGES`) sau
  lista implementărilor. Întâi lentilele serverului (`textDocument/codeLens` +
  `codeLens/resolve`, pe firul modulului), gol → ale noastre. Prima dată imediat, după o editare
  după 600 ms.
- **Setări**: blocul „Informații în cod” (Editor), `crate::insight::InsightSettings`, cele două
  comutatoare, aplicate pe loc.
- **Bara 43i**: textul scurtat („TypeScript: completare și verificări complete cu vtsls +
  Node.js (~34 MB, o singură dată).”) și, unde nu încape lângă butoane, pe două rânduri
  (`wrap_in_two`), fiecare tăiat la loc.
- **PHP: serverul ales din bară, intelephense implicit** (cerut în timpul 43j): ordinea din
  date (`languages/php/data/language-servers.toml`) e acum intelephense, apoi phpactor, deci
  implicitul — primul server disponibil al cărui motor implicit aici nu e cel inclus — e
  **intelephense pe orice OS odată instalat**; altfel phpactor (descărcat; pe Windows motorul
  inclus, ca la 43g); nimic instalat: motorul inclus răspunde. Bara de descărcare, generic: un
  limbaj care descrie mai multe servere pe care le poate obține le oferă pe toate, în ordinea
  din date — „PHP: completare și verificări complete cu [intelephense (~N MB)] [phpactor
  (~7,9 MB)] [Nu acum] [Folosește motorul inclus]”, tooltip-ul fiecăruia spunând diferența
  (`lsp.offer_note_<server>`, dacă limbajul o are) și, la cel instalat de user, licența (clicul
  e acordul, apoi întrebarea 43h cu licența, versiunea și locul); pe Windows doar intelephense
  (phpactor e al motorului inclus acolo). Alegerea instalează / descarcă și salvează serverul
  ca motor al limbajului (`lsp.pick_offered`). Un server instalat de user e oferit doar unde
  build-ul descarcă (Node vine așa).
- **Comutatorul de motor**: în bara de jos, lângă limbaj, ce răspunde fișierului („PHP ·
  intelephense”, „motorul inclus”, „intelephense + motorul inclus”) — doar cât timp unul din
  serverele limbajului poate răspunde (nimic de comutat altfel, nimic pe verticală); clic:
  „Alege motorul limbajului fișierului…” (`workbench_model::CHOOSE_ENGINE`, al modulului
  `lsp`): fiecare server cu starea lui (folosit acum / gata / de descărcat / de instalat),
  motorul inclus, amândouă; alegerea se aplică pe loc și se salvează (de descărcat / de
  instalat: pornite, cu acordul pentru licență).
- **Imagini**: snapshot-urile așteaptă indiciile și lentilele (`insight_settled`); cazuri noi
  `insight` (un apel cu literali în Shop.php: `a:` / `b:`, lentilele), `context-actions`,
  `navigation-implementations`, `navigation-hierarchy`, `lsp-offer-php` (intelephense și
  phpactor de ales), `lsp-engine-switch` (lista motoarelor PHP). Se schimbă, comparate cu
  referința (101 imagini × 2 teme): toate cele cu Shop.php vizibil — doar lentilele după
  `class Shop`, `total()`, `items()` („nicio folosire” / „1 folosire”; și unde se văd pe sub
  un dialog sau la marginea unui panou); cele TypeScript / CSS / C++ / Bash / TOML /
  Dockerfile — lentilele lor și, cu serverul fals, indiciile lui sau ale noastre
  (`initialValue:`, `prices:`) și numele lui în bara de jos; `lsp-offer*` — textul scurtat și pe
  două rânduri; `settings-caret` / Editor — blocul nou „Informații în cod”; `settings-language-
  servers-php-installed` / `-licensed` — intelephense primul și implicit; `-parts` — phpactor
  descărcat, numit în bara de jos; `task-manager*` — doar cifra de memorie.
- **Măsurat** (teste): un fișier PHP de 5000 de apeluri — indiciile doar pentru fereastra
  vizibilă (sub 400, nimic după primele rânduri), în sub 5 s în debug; cadrele fără schimbări nu
  pornesc nicio lucrare (contorul de joburi neschimbat după 20 de cadre), aceeași listă dată
  editorului (același `Arc`); o tastare: indiciile mutate imediat, recalculate o singură dată
  după pauză.
- **Nefăcut / abateri**: lentilele stau la capătul rândului, nu deasupra (variantă
  la fel de uzuală); folosirile sunt numărate după nume (regulile Find Usages), fără `vendor/`;
  indiciile de tip ale variabilelor vin doar de la servere; „Smart select” nu cere
  `selectionRange` serverului (arborele ajunge); comutatorul din bara de jos apare doar unde un
  server al limbajului poate porni acum.

## Rezumatul planului 43

| Pasul | Ce s-a făcut |
|---|---|
| 43a | Clientul LSP generic (`lsp-client`: JSON-RPC, sincronizare incrementală UTF-8/16/32, anulare, repornire, inactivitate), modulul `lsp` (un server per (server, rădăcină) per proiect, Task Manager, setări: motorul per limbaj, memoria, inactivitatea, „Adaugă un server”), portul domeniului, bundle-ul (`cargo xtask lsp-bundle`, Node 26 + vtsls), JS / TS / TSX prin vtsls implicit (98% corect vs 27%) |
| 43b | HTML, CSS / SCSS, JSON prin `vscode-langservers-extracted`, implicit; scheme JSON incluse, nimic descărcat; ce face motorul inclus mai bine rămâne al lui (`built-in`) |
| 43c | C / C++ (clangd), Lua (lua-language-server), Zig (ZLS): module de limbaj noi cu gramatici tree-sitter, servere native fixate SHA-256 |
| 43d | Bash, YAML (cu scheme incluse), TOML (taplo), Dockerfile: module noi, servere; „Clear and Rebuild Caches” oprește / curăță serverele |
| 43e | Paritatea: nimic din ce făcea motorul inclus nu se pierde (completarea unită, rename = uniunea, analizele lângă diagnosticele serverului, rezerva la eroare / timeout) |
| 43f | Termenele (nimic nu așteaptă un server tăcut), măsurătorile cap-coadă, cadrele continue ale hover-ului corectate |
| 43g | PHP prin phpactor, implicit (pe Windows motorul inclus, phpactor de ales, prin TCP): PHP static inclus, phpactor configurat (fără watcher propriu, fără procese în plus, fără zgomot), uniunea completării, Rename / Find Usages și diagnosticele noastre păstrate; 84% implicit vs 60% al nostru / 81–83% phpactor |
| 43i | Serverele descărcate la cerere în loc de incluse: manifest mic în binar (`cargo xtask lsp-manifest`), instalator al nostru (HTTPS, gazde permise, sume verificate, tar.gz / tar.xz / zip, pachete npm din lockfile), folder comun în `<data>/lsp/`, lacăt între procese, reluare, curățare cu referințe; bara de deasupra editorului, lista din setări; aplicația ~90 MB în loc de 457 |
| 43j | Funcțiile premium ale intelephense făcute de motorul nostru, cu serverul întâi unde le are: Smart select, Go to Implementation / Type Declaration, Type Hierarchy, acțiunile de context cu bec, document links, inlay hints, code lens |
| 43h | intelephense instalabil la cerere (proprietar, neinclus): un clic în Setări, de pe registry.npmjs.org, SHA-512 fixat, o dată pentru toate proiectele; găsit dacă e deja instalat; cheia de licență în keychain, scoasă din loguri; implicit pe Windows odată instalat; 90% implicit cu el vs 84% cu phpactor |

**Mărimea lui `lsp/`** (MiB, la 43g): macOS arm64 365,1 (~107,6 arhivat), macOS x64 369,3
(~109,8), Linux x64 354,2 (~111,9), Windows x64 286,9 (~98,5). Node (~141 MB pe macOS) și
clangd sunt cea mai mare parte; PHP + phpactor ~8–12 MB.

**Puncte slabe cunoscute**:

- vtsls / tsserver: ~520–680 MB RSS pe proiect (plafonat la 3 GB); un proiect mare îl duce mai sus.
- bash-language-server ratează funcțiile târzii într-un fișier mare inclus cu `.` (43d); ShellCheck
  nu e inclus (GPL-3), doar folosit din PATH.
- Neutilizatele / învechitele serverului apar în stilul lui (slab), nu tăiate ca ale noastre (43e).
- Sublinierea ⌘ (eticheta legăturii) e a motorului inclus; clicul întreabă serverul.
- Un răspuns de hover venit după termen se folosește abia la următorul hover al cuvântului.
- Build-urile dist (macOS universal, Windows) n-au fost rulate de noi; mărimile sunt ale xtask-ului.

**Rămas pentru mai târziu**:

- **PHP (43g)**: CPU-ul în repaus al phpactor (0,13–0,28%, timerele lui interne); phpactor pe
  Windows neîncercat pe o mașină Windows; referințele phpactor unite cu ale noastre (R06 / N04
  le știe el) — acum doar ale noastre.
- **Propriul nostru server** (motorul PHP expus ca LSP, `language-server`): pasul opțional al
  planului, nefăcut.
- ~~**Inlay hints**~~ — 43j (numele parametrilor; tipurile variabilelor doar de la server).
- **Pull diagnostics** (`textDocument/diagnostic`), pentru serverele care nu publică singure.
- **Structura prin `documentSymbols`**: maparea există în port, Structura rămâne pe tree-sitter.
- ~~**Becul code actions**~~ — 43j.
- ESLint / Prettier din `node_modules`-ul proiectului; un workflow lunar pentru versiunile noi
  ale serverelor.
