# Plan 53 — Compilare, teste și snapshot-uri mai rapide, `target/` mai mic

## Context (măsurat 2026-11-03)
- `target/` 36 GB (incremental 13 GB în `debug/`, 4,9 GB în `check-host/`; `debug/deps` 9 GB,
  383 000 de fișiere: 21 000 doar ale binarului aplicației — copii vechi ale fiecărei variante).
  `scripts/sweep.sh` golește tot peste 40 GB → următoarea compilare pornește de la zero.
- `check.sh`: 123 de binare de test (fiecare legat cu aproape toată aplicația); rularea lor
  ~70 s în total (cel mai lent 30 s, apoi 6 s, 5 s…), restul timpului e compilare și link.
- Snapshot-urile: ~520 de poze, făcute **una după alta**, fiecare pornind aplicația → o
  comparație completă durează ~30 de minute: cea mai mare pierdere de timp la fiecare pas.
- 76 de fișiere de test folosesc `sleep` (așteptări reale).

## Ce facem
1. **53a — snapshot-urile în paralel** (renunțat: comparațiile de snapshot-uri nu se mai rulează). `ui-snapshots.sh` pornește N procese odată (N = nuclee / 2,
   `EDITOR_SNAPSHOT_JOBS`), fiecare cu HOME / TMPDIR / port-uri proprii (izolare deja există);
   ordinea și rezultatele identice. Țintă: ~30 min → ~5 min. Plus `compare --changed`: doar
   grupurile atinse de fișierele schimbate față de ultimul commit (harta grup ↔ crate-uri în script),
   pentru iterație; comparația completă rămâne înainte de commit.
2. **53b — testele mai rapide fără să pierdem acoperire.**
   - Cele mai lente teste măsurate (`cargo test -- -Z unstable-options --report-time` sau timpi
     per binar): sleep-uri înlocuite cu așteptări pe condiție (`wait_until` cu notificare / pas mic),
     timeout-uri reale scurtate în teste (configurabile), fixture-uri mari construite o dată
     (`OnceLock`) și refolosite.
   - Testele de integrare ale fiecărui crate (`tests/*.rs`) adunate într-un singur binar per crate
     (`tests/it/main.rs` cu `mod`-uri): mai puține link-uri, mai puțin spațiu.
   - Teste duplicate / acoperite de altele: listate și scoase doar unde e evident (nimic șters fără
     să rămână aceeași verificare în altă parte); raport în plan.
   - Opțional `cargo nextest` dacă e instalat (paralelism per test, nu per binar), cu fallback la
     `cargo test`.
3. **53c — `target/` ținut mic, fără compilări de la zero.**
   - `sweep.sh` devine curățenie țintită: păstrează pentru fiecare crate doar variantele folosite
     de ultimele compilări (după mtime față de un marcaj pus la începutul ultimei verificări),
     șterge restul din `deps/`, `incremental/` (sesiuni vechi), `.fingerprint/`; ștergerea completă
     rămâne doar peste 60 GB.
   - Clippy din `check.sh` fără compilare incrementală (`CARGO_INCREMENTAL=0` pe `check-host` /
     `check-windows`): ~5 GB mai puțin, viteza la fel (verifică doar ce s-a schimbat).
   - `target/release` și `target/x86_64-pc-windows-gnu` (ale lui `build.sh`) nu se ating.
   - Țintă: `target/` stabil la ~10–15 GB.
4. Măsurat înainte / după: durata `check.sh` (rece și cald), a comparației de snapshot-uri,
   mărimea `target/`, timpul unei iterații obișnuite (o schimbare într-un modul).

## Reguli
- Nicio verificare pierdută: aceleași teste (mutate, nu șterse, cu excepția duplicatelor dovedite),
  aceleași snapshot-uri pixel cu pixel.
- Merge pe macOS, Linux și Windows (Git Bash) ca și acum.

## 53b — ce s-a făcut (măsurat 2026-11-03, M-series, 12 nuclee)
- Măsurat per binar (fiecare binar de test rulat singur, `--report-time`): 75 s în total,
  cel mai lent binarul aplicației (22 s: testele serverelor de limbaj, 43, **unul după altul**
  sub un `Mutex`, ~17,6 s), apoi `ui-components` 6,5 s, `database-console` 6 s,
  `task-manager` 5,3 s, exportul explorer-ului 4,9 s, `database` 4,2 s, `ui-workbench` 3,2 s.
- Testele serverelor (aplicația) rulează unul lângă altul: serverul fals și ce a văzut sunt ale
  rădăcinii proiectului testului (nu ale procesului); doar cele care aleg serverul PHP (o
  setare a procesului) rulează singure printre cele cu fișiere PHP (`RwLock`); grația navigării
  a testelor e a firului testului (`thread_local`), nu a procesului. Timeout-ul unei întrebări în
  teste 3 s (era 2: la limită sub încărcare).
- `TextSystem::with_locale`: fonturile sistemului căutate o dată pe proces (o copie a listei
  pentru fiecare sistem de text nou; aplicația face unul — testele sute).
- Bucle mari: `metric_card` refolosește sistemul de text (6 s → 0,2 s); cele 470 000 de
  layout-uri ale Task Manager-ului în șase teste paralele; meniul (`menu_drawer`) desenat cu un
  singur sistem de text pe test; cele 13 exporturi de 50 000 de rânduri pornite deodată;
  indexul de 5 000 de tabele construit o dată (`OnceLock`) pentru cele două teste ale lui.
- Așteptări reale scurtate: SQLite primește `busy_timeout_ms` în opțiunile țintei (implicit 5 s),
  testul tranzacției consolei așteaptă 50 ms (5,2 s → ~0,1 s); fișierul „schimbat de alt program”
  din proiectele remote primește o oră de modificare cu 3 s înainte în loc de `sleep(1100)`;
  procesul oprit cu tot ce a pornit: 0,4 s / 0,7 s în loc de 1 s / 1,5 s. Celelalte `sleep`-uri
  rămase sunt așteptări „nu se întâmplă nimic în X” (nu au condiție de așteptat) sau pași de
  animație, în binare care nu mai sunt pe drumul critic.
- Testele de integrare: ale explorer-ului (6 fișiere) într-un singur binar `tests/it/main.rs`
  (`protocol.rs` rămâne separat: e și programul driverului, `harness = false`); restul crate-urilor
  au câte un singur fișier de integrare. 123 → 118 binare.
- Instabile reparate: `a_silent_servers_card_signature_and_declaration_dont_wait_for_its_timeout`
  (termenul comparat cu ceasul după un cadru care sub încărcare a consumat grația: verificat
  doar cât ține grația; la sfârșit, doar grația hover-ului, nu orice alt termen al cadrului);
  `downloads.rs` (o descărcare oprită de un test eșuat rămânea blocată pentru totdeauna, iar
  `install` din testele următoare aștepta după ea: sursa testului anterior e eliberată la
  `fresh_downloads`, iar o descărcare oprită renunță după 20 s); plus două găsite pe drum: lista
  consolei rămânea la „Se citește schema…” când citirea se termina între întrebare și citirea
  generației (generația citită acum înainte de întrebare), și fișierul de licență lăsat de o
  rulare veche cu același pid.
- Duplicate: căutate (corpuri de test identice după normalizare, 3 405 teste): singurele perechi
  sunt `queries_compile` (11 limbaje) și `each_mode_tries_what_libpq_does` (MySQL / PostgreSQL),
  fiecare testând codul propriului crate — nu sunt duplicate; nimic șters.
- `cargo nextest`: nu e instalat, deci nefolosit (binarul `protocol`, fără harness, ar trebui
  oricum rulat separat).
- Rezultat: testele 75 s → 41 s (rulate binar cu binar); cel mai lent binar 22 s → 7,8 s;
  `check.sh` cald 79 s → 45 s; o iterație (un fișier al unui modul atins) 84 s → 48 s.

## 53c — ce s-a făcut
- `sweep.sh` nu mai golește `target/` la o limită, ci șterge doar ce nu mai folosește nimeni:
  fiecare job din `check.sh` își salvează mesajele JSON ale lui cargo
  (`--message-format=json-render-diagnostics`: diagnosticele arătate ca înainte în loguri)
  sub `target/.sweep/run`; după o verificare completă reușită, hash-urile unităților folosite
  (construite sau găsite proaspete) devin lista `live`, iar începutul ei marcajul `stamp`. La
  începutul verificării următoare, în fiecare director de profil al verificărilor
  (`target/debug`, `target/check-host/debug`, `target/check-windows/{,x86_64-pc-windows-gnu/}debug`):
  - din `deps/`, `.fingerprint/`, `build/`: intrările cu hash-ul în afara listei și neatinse de
    la marcaj (ce ai construit tu între timp, `cargo test -p …`, rămâne — până la verificarea
    de după, dacă n-ai mai folosit);
  - din `incremental/`: cache-ul unui crate neatins de la marcaj și de o zi, și ale cărui
    obiecte nu le leagă nicio unitate folosită (macOS: obiectele din `deps/` sunt legături
    către cele din cache; în rest decide doar vechimea — un crate neconstruit de mult se compilează
    o dată fără cache când se schimbă); cu totul în `check-host` / `check-windows`;
  - din `deps/*.rcgu.o` (macOS le ține lângă binar pentru informațiile de depanare, cu nume noi la
    fiecare compilare): cele care nu mai sunt în niciun cache păstrat (ale unei compilări vechi).
    Verificat: fiecare obiect la care trimit binarele de test (`nm -pa`, OSO) există după curățenie.
  - Ștergerea completă (`--now`, sau peste 60 GB — `EDITOR_TARGET_LIMIT_GB`) golește doar
    directoarele verificărilor: `target/release` și `target/x86_64-pc-windows-gnu` (ale lui
    `build.sh`) nu se ating niciodată (înainte `cargo clean` le ștergea și pe ele).
    `EDITOR_SWEEP_DRY=1` doar listează. Doar unelte de bază (`find`, `ls -f`, `awk`, `sort`,
    `comm`, `xargs`): macOS, Linux, Git Bash; pe Windows regula obiectelor nu rulează.
    ~3 s pe verificare (listări `ls -f`, nu un `stat` pe fișier).
- Clippy-ul din `check.sh` (`check-host`, `check-windows`) cu `CARGO_INCREMENTAL=0`: 3 GB de
  cache incremental mai puțin, aceeași viteză (fiecare schimbare e verificată o singură dată acolo).
- Măsurat (aceeași mașină, `check.sh` complet):

  |                                   | înainte (53b neînceput) | după 53b | după 53c |
  |-----------------------------------|-------------------------|----------|----------|
  | rece (`target/` gol)              | 8 min 05 s              | —        | 5 min 05 s |
  | cald (nimic schimbat)             | 79 s                    | 45 s     | 39 s     |
  | iterație (un fișier al unui modul)| 84 s                    | 48 s     | 45 s     |
  | `target/` după rece               | 10 GB                   | —        | 7,7 GB   |
  | `target/` după câteva iterații    | 11 GB → 14 GB (crește)  | —        | 7,8–8,7 GB (stabil) |
  | binare de test                    | 123                     | 118      | 118      |

  Variantele lăsate de un `cargo test -p app` / `-p quality` (alte unificări de feature-uri,
  +1 GB) rămân la verificarea imediat următoare și pleacă la cea de după.

## Stare
- [x] 53a — renunțat: comparațiile de snapshot-uri nu se mai rulează
- [x] 53b
- [x] 53c
