# 25. DDD, module cu cozi, lucru în fundal, consum — analiză și plan

Cererea: cod DDD, scalabil pe module cu cozi, lucrul în fundal (async) făcut bine, consum
mic, rapid — **fără să stricăm nimic din ce avem**. Doar analiză și plan; nimic nu se
schimbă fără „go”.

Analiza (2026-10-24): straturile și `check-deps`, firele / cozile / anularea, memoria și
CPU-ul; dovezile sunt `fișier:linie` la data analizei.

## Ce e bine azi (se păstrează)

- **Straturile** sunt verificate automat (`xtask/src/main.rs`): un crate într-o cale
  necunoscută e eroare; domeniul are o listă scurtă de dependențe externe; infra → infra
  interzis (cu excepția `process`, `files`); modul → modul interzis; un crate de modul numit
  în `app/src` doar în `modules.rs` / `snapshot.rs`; `crates/domain/clippy.toml` interzice
  firele, așteptarea, discul și mediul în domeniu.
- **Registrul de contribuții** e generic (`extensions/src/lib.rs:59-119`, după `TypeId`).
- **Coada care unește** (`kernel::queue`: `Coalescing`, `SharedQueue`) e bine făcută: chei
  unite, „tot” înghite cheile, liniștea / `max_wait`, `hold` pentru loturi, worker-ul doarme
  pe `Condvar` (fără busy wait).
- **Evenimentele per proiect** (`ProjectData.events`), înlocuite cu proiectul.
- **Bucla UI** e `ControlFlow::Wait`; trezirile vin de la workeri, redesenare doar dacă s-a
  schimbat ceva; caret-ul doar în fereastra cu focus.
- **Parsarea** e incrementală, pe firul ei, cu job-urile per document unite; rope-ul se
  clonează în O(1).
- **Cache-urile** sunt mărginite (`Memo`, `GenerationalCache`), golibile; datele mari
  (`php.stubs`, MDN) se parsează la prima folosire și se eliberează la inactivitate;
  modelul de embedding e mapat în memorie; undo-ul are plafon (1000 de grupuri / 32 MB).

## Constatări

### A. DDD și module

| # | Ce | Unde | Impact |
|---|---|---|---|
| A1 | Reguli PHP / CSS în app: `is_php` (extensii), `fits_kind` (`->`, `::`, `$`, `const`, `case`), potrivirea fără majuscule, separatorul `\` | `app/src/refactor_ports.rs:202-331` | regula „limbajul vine din modul” încălcată |
| A2 | `WorkbenchHost` are ~45 de metode amestecate: UI, git (`repository`, `committed_text`), refactorizare (`rename_at_cursor`, `move_context`…), `type_at_cursor` cu FQN | `domain/workbench/src/host.rs:73-285` | fiecare funcție nouă îngroașă trăsătura |
| A3 | `LanguageSupport`: ~27 de câmpuri `Vec<Arc<dyn …>>`, fiecare legat de mână de trei ori | `app/src/modules.rs:21-268` | un punct de extensie nou = 3 locuri de editat |
| A4 | `workbench-model` depinde de 9 crate-uri de domeniu (nod central) | `domain/workbench/Cargo.toml` | recompilări în cascadă, cuplaj |
| A5 | `code-intelligence` amestecă: motorul generic, CSS, diagrame, framework | `domain/intelligence/src/{css,diagram,graph}.rs` | context mărginit neclar |
| A6 | `check-deps` nu verifică: stratul App, dev-dependencies, modul → `ui-workbench` (permis prin `Layer::Ui`), un allowlist de infra per modul, aliasuri `use lang_php as x`, `app/tests` | `xtask/src/main.rs:71,336` | regulile pot fi ocolite fără să observe nimeni |
| A7 | `depends_on` verifică doar selecția, nu dacă dependența chiar s-a activat | `extensions/src/lib.rs:218-229` | un modul pornește peste o dependență căzută |
| A8 | `.git` scris de mână în urmărirea discului; starea de blame / HEAD în app | `app/src/fs_watch.rs:18-131`, `workbench/vcs.rs` | VCS-ul nu e doar al modulului |
| A9 | Mesajul `Panicked` încă în română | `extensions/src/lib.rs:29` | regula „English in the code” |

### B. Fire, cozi, lucru în fundal

| # | Ce | Unde | Impact |
|---|---|---|---|
| B1 | **Un fir nou pentru fiecare job**, fără limită și fără pool: `host.spawn` („module-job”), `git-read` / `git-blame` per fișier deschis, ~33 de locuri în module, 15 în app | `app/src/workbench/host.rs:215-228`, `workbench/vcs.rs:158,187` | la multe fișiere / acțiuni: zeci de fire simultan, CPU în vârf, RAM pentru stive |
| B2 | **Job-urile modulelor nu țin de proiect**: canalul `jobs` e pe `Workbench`, nu pe `ProjectData`; fetch / pull / push, lipirea din arbore se termină peste proiectul deschis între timp; fără anulare | `workbench/mod.rs:713,920`, `host.rs:360-366`, `vcs-git/src/sync.rs:36-43` | încalcă „nimic nu trece între proiecte” |
| B3 | Flag-ul `RUNNING` al git-ului e `static`, comun tuturor ferestrelor | `vcs-git/src/sync.rs` | un proiect blochează sincronizarea altuia |
| B4 | **Disc pe firul UI**: inteligența (completare, hover, goto, link-uri Cmd) citește fișiere cu `fs::read_to_string` / `read_dir` | `workbench/intelligence.rs:270-300` | sacadări pe proiecte mari / discuri lente |
| B5 | Deschiderea proiectului pe firul UI: gix open + HEAD, listarea, nivelurile, framework-urile, fișierele recente | `workbench/files.rs:15-60`, `vcs.rs:45-53` | pornirea „îngheață” pe repo-uri mari |
| B6 | Deschiderea / salvarea fișierului pe firul UI (canonicalize, citire, editorconfig, rename) | `files.rs:98-140,299,500` | fișierele mari blochează UI-ul |
| B7 | **Rețea pe firul UI posibilă**: SFTP e `block_on` sincron; o listare de arbore pe un proiect la distanță ar aștepta rețeaua | `remote-sftp/src/lib.rs:277-437`, `listing.rs:26-39` | înghețare la rețea lentă |
| B8 | Cozile `SharedQueue` sunt folosite doar în 2 locuri (indexing, transferuri); restul ~70 de `mpsc` + `try_recv` | — | același tipar scris de mai multe ori, fără unire / prioritate |
| B9 | `on_wake` întreabă 11 surse la fiecare trezire | `app/src/events.rs:196-224` | ieftin azi, dar crește cu fiecare modul |
| B10 | `process::run` verifică copilul la 20 ms; 2 fire nenumite per proces | `infra/process/src/lib.rs:163-189` | mic, dar sondare |
| B11 | Watchdog-ul (500 ms) și eliberarea modulelor (60 s) nu se opresc niciodată | `watchdog.rs:59`, `module_manager.rs:102` | ieftin; acceptabil |

### C. Memorie și CPU

| # | Ce | Unde | Impact |
|---|---|---|---|
| C1 | **Indexul de căutare liniar**: fără liste inverse, BM25 parcurge toți termenii tuturor bucăților la fiecare interogare; semanticul compară cu toți vectorii; totul sortat înainte de a tăia la 200 | `domain/search/src/lib.rs:340-375` | căutarea crește liniar cu proiectul |
| C2 | `remove_file` mută tot `Vec`-ul de bucăți pentru fiecare fișier → un lot de N fișiere (checkout) e O(N × bucăți) | `search/src/lib.rs:213-216` | vârf de CPU la checkout / pull |
| C3 | **Embedding-ul sub lacătul de scriere**: căutările așteaptă cât se calculează vectorii | `infra/indexer/src/search.rs:151-163` | căutarea „stă” în timpul reindexării |
| C4 | **Refresh reîncarcă `search.bin` de pe disc** și construiește un al doilea index complet cât îl ține pe primul → vârf dublu de memorie | `indexer/src/search.rs:91`, `lib.rs:317,428` | vârf de RAM pe proiecte mari |
| C5 | Fiecare bucată își ține calea ca `String` (repetată), preview, vectorul (~1 KB) | `search/src/lib.rs:26-37` | ~100 MB la 100k bucăți |
| C6 | **Copii ale întregului text la tastă**: `auto_close` → `tab.text()` la fiecare caracter declanșator; `refresh_symbols_now` → `buffer().to_string()`; preview-ul Markdown la fiecare versiune; `select_next/all_occurrences` | `workbench/intelligence.rs:26,158`, `workbench/mod.rs:1464`, `document/.../commands.rs:671,696` | alocări mari pe fișiere mari |
| C7 | Instantaneul sintaxei clonat de 2 ori după fiecare parsare (`layers`, `ranges`, `folds` copiate adânc) | `syntax-treesitter/src/worker.rs:143` | alocări la fiecare tastă |
| C8 | **Alocări per cadru**: `restyle_glyphs` (vectori + reshape per rând aldin / italic), titlurile tab-urilor, `format!` în bara de stare, `line_count().to_string()`, task manager-ul sortează la desenare | `ui-workbench/src/editor_view.rs:1552-1575,1836`, `app/src/workbench/paint.rs:279-292`, `status.rs`, `task-manager/src/view.rs:278` | CPU per cadru (la scroll / animații) |
| C9 | Cache-ul de linii shape-uite poate ajunge la ~131k intrări (32 de fonturi × 2048 × 2) | `render/text/src/lib.rs:18,142-166` | RAM pe fișiere mari cu multe fonturi |
| C10 | Indexul de simboluri clonează harta `files` la fiecare refresh | `indexer/src/lib.rs:371` | alocare inutilă |
| C11 | 56 de crate-uri în mai multe versiuni (skrifa / read-fonts, tiny-skia, objc2, hashbrown ×3…) | `Cargo.lock` | binar mai mare, compilare mai lungă |

## Ținta

1. **Un singur tipar pentru lucrul în fundal**: cozi care unesc (`kernel::queue`) + un pool
   mărginit de fire, nu un fir per job. Fiecare job aparține unui proiect și moare cu el.
2. **Firul UI nu atinge discul și rețeaua**: totul trece printr-un port, rezultatul vine
   prin trezire (cum fac deja sintaxa, indexul, diff-ul).
3. **Limbajul, git-ul, framework-urile vin din module** (A1, A8), iar `check-deps`
   păzește tot ce se poate păzi (A6).
4. **Consum proporțional cu ce se folosește**, nu cu mărimea proiectului: indexul cu liste
   inverse, fără copii ale textului la tastă, nimic alocat per cadru ce poate fi ținut.

### „Async” — ce alegem și de ce

- **Nu** introducem un runtime async (tokio) în app / domeniu / module. Un editor desktop
  are puțin I/O concurent; firele cu cozi + trezirea buclei UI e modelul pe care îl avem
  deja și care merge (și alte editoare native au executorul lor, tot pe fire + trezirea buclei). tokio ar
  aduce încă un set de fire, alocări per task și „colorarea” funcțiilor `async` peste tot.
- tokio rămâne **închis în adaptoarele care îl cer** (SFTP, Postgres), cu o regulă: un port
  blocant pe rețea nu se cheamă niciodată de pe firul UI (B7).
- Planul 02 („tokio multi-thread, rayon”) se actualizează la modelul real.

## Pași

Fiecare pas: commit separat, `check.sh` verde, cele 128 de snapshot-uri identice, testele
existente neschimbate (doar adăugate); unde e consum — măsurat înainte / după pe proiectul
sintetic din scratchpad (RSS, timpul indexării, timpul unei căutări).

### Etapa 1 — mărunte, fără risc
- **25a.** `Panicked` în engleză (A9); cele 2 fire nenumite din `process` cu nume (B10).
- **25b.** Copiile textului la tastă (C6): `auto_close` primește textul doar dacă
  limbajul cere (sau lucrează pe rope / o fereastră în jurul cursorului);
  `refresh_symbols_now` folosește `Tab::text()` comun; preview-ul Markdown se actualizează
  printr-o coadă (după liniște), nu la fiecare versiune.
- **25c.** Alocările per cadru (C8): titlurile tab-urilor și textele barei de stare
  memorate pe cheia lor (`Memo`), `restyle_glyphs` cu buffer-e refolosite din `EditorView`,
  sortarea task manager-ului la colectare, nu la desenare.
- **25d.** C7 (instantaneul sintaxei într-un `Arc`, trimis, nu copiat) și C10 (fără
  clonarea hărții `files`).

### Etapa 2 — lucrul în fundal: un pool, cozi, proiectul
- **25e. `kernel::work`** — pool mărginit (fire = nuclee − 1, min 2, max 8), numit, cu două
  priorități (interactiv: ce așteaptă utilizatorul; fundal: indexare, blame), job-uri
  anulabile (`Cancel` = un `Arc<AtomicBool>` / generație). Fără dependențe noi. Teste în
  `kernel/tests/unit/work.rs` (ordinea priorităților, anularea, limita).
- **25f.** `host.spawn` trece pe pool; **job-urile țin de proiect** (B2): canalul în
  `ProjectData`, anulate și aruncate când proiectul se închide / se schimbă; `RUNNING`-ul
  git în starea proiectului (B3). Test: un job terminat după schimbarea proiectului nu
  ajunge la noul proiect.
- **25g.** `git-read` / `git-blame` (B1) printr-o coadă care unește (ultimul fișier
  câștigă; cel închis se anulează) pe pool, nu un fir per fișier.
- **25h.** Workerii modulelor cu `mpsc` + `try_recv` (B8) trecuți pe rând pe
  `SharedQueue` / pool acolo unde unesc muncă (refactoring, diagrams, remote, project-tree);
  cei de o singură dată rămân `mpsc` (nu schimbăm ce merge doar ca să arate la fel).
- **25i.** `on_wake` (B9): fiecare sursă își marchează trezirea (un bit per sursă), bucla
  întreabă doar sursele marcate.

### Etapa 3 — firul UI fără disc și rețea
- **25j.** Inteligența (B4): un cache al conținutului fișierelor proiectului (`Memo` pe
  data modificării, umplut în fundal de indexare); citirea sincronă rămâne doar ca
  rezervă pentru un fișier mic necunoscut, cu limită de mărime. Măsurat: timpul completării
  pe proiectul sintetic.
- **25k.** Deschiderea proiectului (B5): gix, nivelurile, framework-urile, recentele pe
  pool; UI-ul arată arborele imediat, restul vine prin trezire (cum vine deja indexul).
- **25l.** Fișierele (B6): citirea fișierelor peste un prag (ex. 1 MB) și salvarea pe pool;
  sub prag rămâne cum e (rapid și sigur). Tab-ul arată „se încarcă” pe fișierele mari.
- **25m.** Proiectele la distanță (B7): listarea și citirea prin pool, niciodată
  `block_on` pe firul UI; o pază — un `debug_assert!` că portul de rețea nu e chemat de pe
  firul principal.

### Etapa 4 — indexul de căutare
- **25n.** Lacătul (C3): vectorii calculați **în afara** lacătului, apoi o scriere scurtă.
- **25o.** Lotul (C2): `remove_files(&[path])` într-o singură trecere; căile internate
  (un `Arc<str>` / id de fișier per fișier, nu per bucată — C5).
- **25p.** Refresh fără reîncărcarea de pe disc (C4): ca indexul de simboluri, pornește de
  la ce e în memorie.
- **25q.** Liste inverse pentru BM25 (C1): termen → bucăți; o interogare atinge doar
  bucățile cu termenii ei; top-K cu un heap în loc de sortarea totală. Formatul
  `search.bin` versionat (`kernel::cache_stamp`) — indexul vechi se reconstruiește singur.
  Măsurat: timpul interogării și RSS-ul.
- (Maparea în memorie a vectorilor — planul 24, pasul 7 — doar dacă măsurătoarea de după
  25o–25q o mai justifică.)

### Etapa 5 — DDD: limbajul din module, host-ul pe capabilități, paza
- **25r.** Regulile PHP / CSS din `refactor_ports.rs` (A1) trec în `LanguageIntelligence`
  (`Naming` extins: extensiile, ce tip de simbol încape după `->` / `::` / `$`,
  separatorul de namespace), implementate în `lang-php` / `lang-css`. Testele din
  `app/tests/unit/refactor_ports.rs` rămân și trec neschimbate.
- **25s.** `.git` din `fs_watch.rs` (A8): VCS-ul spune ce căi urmărește
  (`RepositoryWatch` din `version-control`).
- **25t.** `WorkbenchHost` pe capabilități (A2): trăsături mici (`EditorHost`,
  `VcsHost`, `RefactorHost`…) obținute din host (`host.vcs() -> Option<&dyn VcsHost>`);
  metodele vechi rămân cu implementarea implicită care deleagă, mutate pe rând — niciun
  modul nu se schimbă în același pas cu trăsătura.
- **25u.** `LanguageSupport` (A3): un singur registru tipizat (ca `Contributions`) în loc
  de 27 de câmpuri; câmpurile devin accesori, apelanții neschimbați.
- **25v.** `check-deps` (A6, A7): modul → `ui-workbench` interzis; App cu regulile lui;
  dev-dependencies verificate; aliasurile `use x as y` și `app/tests` în scanare; allowlist
  de infra per modul în manifestul modulului; `depends_on` verificat la activare (o
  dependență căzută → modulul nu pornește, cu mesaj).
- **25w.** (Opțional, după 25t) `workbench-model` mai subțire (A4) și `code-intelligence`
  împărțit (A5: diagramele / CSS în crate-uri sau module proprii) — doar dacă după 25t
  timpii de compilare / cuplajul o cer.

### Etapa 6 — dependențe (C11)
- **25x.** `cargo tree -d`: aliniat versiunile unde ține de noi (hashbrown, toml_edit,
  thiserror, base64); cele aduse de winit / cosmic-text rămân până la versiunile lor noi.

### Documentația
- Planul 02 („Model de concurență”) adus la ce avem după etapa 2; planul 10 bifat.

## Ce nu facem

- Niciun runtime async în core (vezi mai sus); niciun `rayon` peste pool-ul nostru.
- Nu rescriem tot ce folosește `mpsc`: doar unde se unește muncă sau se poate anula.
- Nu schimbăm id-urile comenzilor, scurtăturile, setările salvate, formatul fișierelor
  utilizatorului (doar indexul, care e cache și se reconstruiește singur).

## Plasa de siguranță („să nu stricăm nimic”)

- Înainte de fiecare etapă: snapshot-urile de referință, golden-ul meniurilor, testele.
- Pașii mari (25e–f, 25q, 25t) au teste noi scrise **întâi**, pe comportamentul de azi.
- Măsurătorile în `docs/plan/25…` la „Rezultate”, înainte / după.
- Testele git doar pe repo-uri false din scratchpad; proiectul sintetic pentru măsurători.

## Ordinea propusă

Etapa 1 (rapid, câștig sigur) → 2 (baza pentru 3) → 3 → 4 → 5 → 6. Etapele 4 și 5 sunt
independente și se pot inversa.

## Stare

- [x] Etapa 1 (25a–25d): `Panicked` în engleză; firele `process-input` / `process-output`
  numite; simbolurile la cerere folosesc copia comună a textului (`Tab::text()`);
  `restyle_glyphs` cu buffer-e refolosite; sortarea task manager-ului fără alocări per
  comparație; indexul de simboluri ia harta fișierelor în loc s-o cloneze (dată înapoi la
  întrerupere). Lăsate, după verificare: `auto_close` copiază doar la `>` în HTML și
  folosește aceeași copie comună; preview-ul Markdown primește textul doar când s-a
  schimbat și e vizibil; `select_*_occurrences` sunt comenzi explicite, nu taste; bara de
  stare / titlurile tab-urilor sunt câteva șiruri mici, iar cadrele se desenează doar la
  evenimente; instantaneul sintaxei se clonează o dată (arborii sunt numărați prin
  referință, straturile puține), iar un `Arc` ar cere tot o copie la modificare.
- [x] Etapa 2 (25e–25i): `kernel::work` — pool-uri numite și mărginite (firele pornesc la
  nevoie și se termină după 30 s fără lucru; interactiv înaintea fundalului; `Cancel`; un
  job care dă panic nu oprește pool-ul). Pool-uri cu numele de azi (Task Manager-ul împarte
  CPU-ul după numele firelor): `module-job` (job-urile modulelor), `git-read`, `git-blame`
  (2), `image-decode` (2), `usages` (1). Job-urile modulelor țin de proiect
  (`ProjectData.jobs`): cele nepornite se anulează la închidere, rezultatele celorlalte se
  aruncă — test `a_job_never_reaches_another_project`. Sincronizarea git e „una odată” per
  repository, nu pentru toate ferestrele (un gardian eliberează repository-ul și când job-ul
  e anulat sau dă panic). Lăsate, după verificare: workerii lungi care deja unesc munca
  (sintaxa, simbolurile, diff-ul, indexul, transferurile) și firele pornite o dată per
  acțiune a utilizatorului (view-urile de refactorizare, commit, log) rămân cum sunt;
  `on_wake` (25i) — cele 11 întrebări sunt `try_recv`-uri sub o microsecundă, iar biți per
  sursă ar cere schimbat fiecare waker fără câștig măsurabil.
- [x] Etapa 3 (25j–25m) — **măsurată, nu mutată**: timpii intră în log („project ready”,
  „file opened … elapsed”). Release, macOS: proiectul sintetic (522 MB) se deschide în
  ~3 ms; un repository git fals (5.000 de fișiere, 31 de commit-uri) în 3–7 ms; un fișier
  PHP de 21 MB (250.000 de rânduri) în 23 ms — toate mult sub pragul de sacadare al
  watchdog-ului (150 ms). Proiectele la distanță: listarea vine din memorie și cere restul
  în fundal (`session.rs` `known` / `ask_list`), deci rețeaua nu ajunge pe firul UI.
  Citirile inteligenței (fișierele incluse, rutele framework-urilor — memorate pe
  generația fișierelor) sunt câteva fișiere mici din cache-ul sistemului. Mutarea lor în
  fundal ar face completarea asincronă (o schimbare mare de comportament) fără câștig
  măsurabil; se reia dacă watchdog-ul arată sacadări reale.
- [x] Etapa 4 (25n–25q): fișierele se pregătesc (bucăți, termeni, vectori) **în afara**
  lacătului (`SearchIndex::prepare_file`), apoi se aplică într-o singură trecere
  (`apply` / `remove_files`) — căutările nu mai așteaptă embedding-ul, iar un checkout de N
  fișiere nu mai parcurge indexul de N ori; refresh-ul aplică doar diferențele pe indexul
  din memorie (fără al doilea index, fără recitirea `search.bin`); clasamentele iau top-200
  prin selecție, nu sortare completă; schimbările nesalvate rămân de salvat după un
  refresh. Teste: `a_batch_of_files_is_the_same_as_one_by_one`,
  `a_refresh_applies_only_the_changes`. Lăsate: listele inverse pentru BM25 ar costa ~4
  octeți per termen per bucată (zeci de MB pe un proiect mare) — contra consumului mic;
  căile internate (câțiva MB) ar schimba formatul și toți consumatorii `Chunk.path`.
- [x] Etapa 5 (25r–25w):
  - 25r: `LanguageIntelligence::fits_kind` (regulile PHP — `->`, `::`, `$`, `const`,
    `case` — în `lang-php`, implicitul în domeniu); `Naming.names_ignore_case` și
    `Naming.namespace_separator` (`\` al PHP-ului) pentru conflictele redenumirii;
    potrivirea numelor CSS (`css_match`, `is_css_kind`) în `code-intelligence::css`.
    `refactor_ports.rs` nu mai știe nimic de PHP sau CSS; testele mutate odată cu
    regulile, cu aceleași aserțiuni.
  - 25s: `VcsProvider::metadata_dir` (git: `.git`); urmărirea discului, arborele și „Copy
    Path from Repository Root” îl iau de acolo.
  - 25u: contribuțiile luate direct în `LanguageSupport { … }` — un punct de extensie
    nou se scrie în 2 locuri (câmpul și rândul lui), nu în 3.
  - 25v: `check-deps` refuză `ui-workbench` oricui în afară de app (test
    `only_the_app_builds_on_its_screens`) și aliasurile `use <modul> as x` în app;
    activarea: un modul a cărui dependență a căzut nu pornește (test
    `a_failed_dependency_is_missing`).
  - Lăsate, cu motiv: 25t (`WorkbenchHost` pe capabilități) — 30 din cele 50 de metode au
    implementare implicită, doar 3 tipuri îl implementează (app-ul și două gazde de test);
    împărțirea ar muta metode fără câștig, se reia la o a doua gazdă reală. 25w — nu se
    cere după 25t. Dev-dependencies și `app/tests` rămân nescanate: testele folosesc
    intenționat module și crate-uri de infra ca să pregătească scenarii.
- [x] Etapa 6 (25x): `cargo tree -d` — nicio dependență **directă** a noastră în două
  versiuni; dublurile vin din dependențele lor (alacritty_terminal / ureq: base64;
  cosmic-text / swash: skrifa, read-fonts; gix / dashmap: hashbrown; png / image:
  miniz_oxide; winit: objc2). Se aliniază singure când acelea se actualizează.
