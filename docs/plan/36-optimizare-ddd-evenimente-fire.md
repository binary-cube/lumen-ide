# 36 — Optimizare: DDD, evenimente, cozi, fire

O analiză a codului pe patru direcții:
- straturile DDD;
- evenimentele și cozile;
- firele și ce rulează pe firul UI;
- viața modulelor și cache-urile.

Constatările de mai jos le-am verificat în cod; căile sunt relative la `crates/`.

## Ce e deja bine (nu se schimbă)

- Fiecare proiect are bus-ul lui de evenimente, iar `ProjectData` se înlocuiește întreg la
  deschiderea altui proiect.
- `EventBus::publish` eliberează lacătul înainte să cheme handler-ele, iar lacătele nu
  sunt imbricate într-un fel care să ducă la blocaje.
- Fluxul de indexare folosește corect `kernel::queue`. Indexerul construiește indexul fără
  lacăt și îl schimbă la final.
- Datele de limbaj se încarcă la cerere și se eliberează (`OnDemand`). Potrivirea fuzzy are
  un singur helper.
- Editorul își ține în cache forma textului, culorile și marcajele. Bucla de evenimente
  așteaptă, nu interoghează în gol.
- Domeniul are dependențe curate, iar `domain/clippy.toml` interzice fișierele și firele.

## 36a — Scurgeri între proiecte și bug-uri mici

1. **Ferestrele de jos rămân cu datele proiectului vechi.**
   - Deschiderea altui proiect în aceeași fereastră înlocuiește `ProjectData`, dar nu
     atinge `tool_windows`.
   - `restore_tool_windows` înlocuiește doar ferestrele salvate în sesiunea nouă.
   - Ce nu e în sesiune păstrează vederile vechiului proiect: git log, compare, istoric
     local, terminal.
   - **De făcut:** fiecare vedere ține `ProjectId`-ul care a creat-o; la schimbarea
     proiectului se închid cele care nu sunt ale lui (`workbench/files.rs:23`,
     `tool_windows.rs:470`).
2. **Ștergerea unui fișier îi închide tab-urile doar în grupul curent**
   (`entry_events.rs:85`). Redenumirea trece deja prin toate grupurile; ștergerea trebuie
   să facă la fel (`all_tabs_mut`).
3. **Setările țin modelul semantic după închidere.**
   - `settings_view::close()` nu eliberează `embedder` și vectorii.
   - Eliberarea modelului la inactivitate (`app/src/semantic.rs`) nu are efect cât îl
     ține fereastra de setări.
4. **Calculul vectorilor (embeddings) nu se oprește la închiderea proiectului.**
   `indexer/src/search.rs:229` primește `stop` și îl verifică între fișiere.
5. **Registre statice per proiect, curățate doar „când se nimerește”:**
   - `WAITING` din `remote-projects/session/service.rs:130`;
   - `vcs-git/status.rs:20`, `quality/lib.rs:43`;
   - `NUMBER` din `terminal-panel/lib.rs:42`, care crește la fiecare deschidere de proiect.

   Se scot printr-un `Drop` legat de abonamentele proiectului. `SESSIONS`
   (`remote-projects/session.rs:959`) caută după id, nu după cale: două ferestre cu
   același folder nu trebuie să se amestece.

**Teste:** câte unul pe fiecare punct (schimbarea proiectului, ștergerea cu split,
închiderea setărilor, oprirea calculului vectorilor, curățarea registrelor).

## 36b — Firul UI: fără parsări și disc la tastare

1. **Un singur parser PHP sau JS pentru toată aplicația**, păzit de un `Mutex`
   (`lang-php/src/intelligence.rs:282`, `lang-javascript/src/intelligence.rs:158`).
   - Indexarea tuturor proiectelor, worker-ul de simboluri și firul UI stau la rând la el.
   - **De făcut:** câte un parser pe fir (`thread_local!`). Parsarea se împarte atunci pe
     nuclee, iar UI-ul nu mai așteaptă indexarea.
2. **`refresh_symbols_now` parsează tot fișierul pe firul UI, ținând lacătul de scriere
   al indexului** (`workbench/intelligence.rs:159`).
   - **De făcut:** extragerea se face înainte; lacătul se ține doar pentru `set_file`.
   - Unde se poate, se folosește rezultatul worker-ului `symbols`.
3. **Completarea, la fiecare tastă de declanșare** (`workbench/intelligence.rs:287`):
   - copiază tot textul;
   - scanează tot fișierul după `include` cu un regex;
   - citește de pe disc fișierele incluse, ținând indexul blocat;
   - compilează 3 regex-uri la fiecare `->` (`lang-php/intelligence/context.rs:130`).

   **De făcut:**
   - regex-uri compilate o dată, cu o verificare ieftină înainte;
   - include-urile rezolvate, ținute în cache după cale și dată;
   - completarea pe un worker, după versiunea documentului; rezultatele vechi se aruncă.
4. **Schimbările de pe disc se aplică pe firul UI** (`workbench/tree.rs:9-80`).
   - Se recitesc folderele, iar fișierele deschise se citesc și se decodează.
   - La un `git checkout` sau un `composer install`, UI-ul îngheață.
   - **De făcut:** listarea și citirea merg pe pool-ul proiectului; în `poll_fs` se
     aplică doar rezultatul.
5. **Mutarea unei instrucțiuni (Alt+Shift+↑/↓) reparsează tot documentul**
   (`editing.rs:118`). Se folosește arborele pe care worker-ul de sintaxă îl are deja.
6. **Dialogul de redenumire citește fișiere întregi la fiecare tastă**, ținând indexul
   (`refactor_ports.rs:165`).
   - Ordinea corectă: eliberezi lacătul, păstrezi doar primele 3 conflicte, apoi citești
     doar rândul lor.
7. **Salvarea face până la trei copii întregi ale textului**
   (`document/application/storage.rs:52`, `document.rs:265`).
   - Verificarea `represents` merge pe bucăți și doar pentru Latin-1; codarea se face tot
     pe bucăți.
   - Autosave-ul (`files.rs:374`) scrie pe un worker.
8. **Salvarea unei imagini transformate rulează pe firul UI** (`viewer-image/lib.rs:581`).
   Trece pe pool-ul `DECODES`.
9. **Alocări și apeluri de sistem la fiecare cadru:**
   - `current_project()` → `recent::describe` → `home_dir()` (`overlays.rs:71`);
   - titlurile grupurilor de tab-uri și butoanele ferestrelor de unelte (`paint.rs:66, 279`);
   - antetul panoului de proiect cu `i18n::t` (`project-tree/panel.rs:650`);
   - jurnalul remote, care clonează 500 de transferuri sub lacăt
     (`remote-projects/log_view.rs:157`).

   Toate se țin în cache până se schimbă ce arată.
10. **Disc pe firul UI în locuri mici:**
    - `is_dir()` pe fiecare proiect recent (`app/src/recent.rs:70`; pe Windows, o cale de
      rețea lipsă blochează secunde);
    - `exists()` pe strămoși în meniul Copy (`files.rs:604`);
    - `fs::metadata` pe fiecare imagine din Markdown (`viewer-markdown/layout.rs:170`);
    - `is_dir()` în `WorkbenchHost` (`domain/workbench/host.rs:96`);
    - `head_id` / `branch` cu gix (`workbench/vcs.rs:89`).

    Se calculează în fundal sau se iau din ce știm deja.

**Teste:** unitare pe fiecare mutare. Comportamentul (completare, mutare, redenumire,
salvare) rămâne identic: aceleași teste trec, iar capturile ies identice.

## 36c — Evenimente și cozi: fiecare lucru o singură dată

1. **Deschiderea unui proiect îl citește de trei ori.**
   - `relist_dir(root)` publică `EntriesChanged{root}`, deci indexarea parcurge tot
     proiectul.
   - Pornirea watcher-ului dă `EntriesRescan`, adică încă o parcurgere completă, pe lângă
     cea de la pornirea indexerului.
   - `SearchState::update` citește și calculează hash-ul fiecărui fișier fără să verifice
     înainte mărimea și data (`indexer/search.rs:141`).
   - **De făcut:**
     - `EntriesChanged` se publică doar când un serviciu chiar a listat ceva în plus (un
       proiect remote);
     - `update` sare peste fișierele cu aceeași mărime și dată, ca `index_file`.
2. **Quality: lucru pe disc în handler, pe firul care publică (UI), și un fir nou pentru
   fiecare țintă** (`quality/lib.rs:102`, `runner.rs:211, 297`).
   - O refactorizare pe N fișiere pornește N procese phpstan deodată.
   - **De făcut:** handler-ul doar pune calea într-o `SharedQueue`, care unește cheile și
     respectă suspendarea din timpul schimbărilor în masă.
   - Planul și rularea se fac pe un `Pool` limitat al proiectului.
3. **Remote: lista de urcat se reface integral la fiecare eveniment**
   (`remote-projects/outgoing.rs:74`, `session.rs:841`).
   - O salvare o declanșează de cel puțin 3 ori, cu toată arborele listat și hash-uit.
   - **De făcut:** actualizare incrementală, după căile din eveniment, printr-o coadă cu
     liniște și `max_wait`.
4. **Git status** (`vcs-git/status.rs:76-137`):
   - pauză fixă de 300 ms;
   - devine „vechi” și pentru `node_modules` sau `var/cache`;
   - nu ține cont de suspendarea în masă;
   - nu vede `git add` făcut din terminal (`.git/index`).

   **De făcut:**
   - coadă cu liniște, `max_wait` și suspendare;
   - folderele generate sunt filtrate;
   - un eveniment nou, „s-au schimbat metadatele repo-ului”, pentru `.git/index`.
5. **Watcher-ul de fișiere** (`app/src/fs_watch.rs:40`):
   - două canale nelimitate, cu câte o intrare pe cale;
   - căile se sortează și se clonează pe firul UI;
   - căile generate ajung la toți consumatorii.

   **De făcut:**
   - un set deduplicat pe firul watcher-ului; peste o limită, un singur „rescan”;
   - filtrarea folderelor generate înainte de publicare.
6. **Un singur „trezește” pentru toate ferestrele** (`platform-winit/lib.rs:86, 653`).
   - Lucrul proiectului A face fereastra proiectului B să-și interogheze toate serviciile.
   - **De făcut:** trezirea poartă id-ul ferestrei, cu un semnal în așteptare per
     fereastră; `ProjectContext.wake` vine din fereastra proiectului.
7. **Refactorizările salvează fișier cu fișier, fără `BulkChangeStarted/Ended`**
   (`refactorings.rs:295-418`). Quality, git și istoricul local reacționează la fiecare
   fișier, iar aplicația reindexează singură pe firul UI.
   - **De făcut:**
     - evenimentele de schimbare în masă în jurul aplicării și al anulării;
     - reindexarea o face indexerul, din `RefactoringApplied`.
8. **Aplicația anunță indexarea în locul modulului.** Verifică `is_busy()` la fiecare
   trezire, publică `IndexingStarted/Finished` în numele lui și reîmprospătează direct
   căutarea (`workbench/intelligence.rs:241-266`, `search.rs:8-35`).
   - O tranziție rapidă ocupat → liber → ocupat se pierde.
   - **De făcut:** modulul de indexare publică evenimentele, iar vederea de căutare se
     abonează la ele.
9. **`files_generation` e actualizat de mână în 3 locuri** și nu vede scrierile din afară.
   - Cache-urile de framework și rute rămân vechi după un `git checkout`.
   - **De făcut:** un singur abonat îl actualizează, din `FileSaved`, `FilesWritten`,
     `EntriesChanged` și `FileRenamed`.
10. **Arborele proiectului e ținut de două ori.** Aplicația are `FileTree`, modulul
    project-tree pe al lui, și ambele recitesc aceleași foldere la același eveniment.
    - **De făcut:** un singur proprietar, sau un cache comun de listări pe folder.
11. **După o salvare, mai mulți consumatori citesc același fișier:** istoricul local,
    indexerul de două ori, quality, iar ecoul watcher-ului îl mai declanșează o dată.
    - **De făcut:**
      - `FileSaved` poartă hash-ul, mărimea și data;
      - ecoul cu aceleași valori se ignoră;
      - istoricul local primește textul de la salvare, nu-l mai citește.
12. **Lucru pornit din desen, în loc de evenimente** (`paint.rs:7-50`, `app/mod.rs:325`):
    - `sync_vcs`, care rulează pe toate tab-urile, câte o dată pentru fiecare grup;
    - sesiunea, construită la 0,5 s cât timp se desenează;
    - autosave-ul, care folosește redesenarea drept ceas.

    **De făcut:** git pornește la `HeadChanged` sau la deschiderea unui tab; sesiunea și
    autosave-ul au cronometre adevărate.

**Teste:** numărăm citirile și execuțiile, cu un `FileSystem` sau un runner fals:
- la deschiderea unui proiect, fiecare fișier se citește o singură dată;
- o refactorizare pe 10 fișiere dă o singură rulare quality per fișier, după final;
- o salvare dă o singură recalculare remote;
- trezirea proiectului A nu interoghează fereastra B.

## 36d — Fire: pool-uri în loc de fire noi

`kernel::work::Pool` există deja: e limitat, are nume, priorități și `Cancel`, iar firele
se opresc când nu au de lucru. Proiectul are deja pool-uri (`ProjectData.jobs`). Totuși,
modulele pornesc fire proprii în 29 de locuri.

1. **Fire noi la fiecare cerere, mutate pe pool cu `Cancel`-ul proiectului:**
   - git log și commit (`log_view.rs:285,331`, `commit_view.rs:61,97`);
   - diagrame, graful canvas, diff-ul compare;
   - vederile de refactorizare (usages, rename, signature, move);
   - lipirea din arbore, indexul de căi;
   - remote (`review_view`, `connect_view`, `transfers.rs:111`, un fir care doar
     doarme);
   - reîncărcarea setărilor la focus (`app/src/config.rs:56`).
2. **Fire care trăiesc cât proiectul, deși stau degeaba:**
   - `local-history`: pornește la deschidere și face curățenie în tot depozitul;
   - `symbols`, `gutter-diff`, `index-queue`.

   Trec pe pool. Istoricul local pornește la primul `FileWillSave`, iar curățenia devine o
   lucrare amânată, cu prioritate mică.
3. **`module-release` și `watchdog` rulează la nesfârșit**, cel din urmă trezindu-se la
   500 ms. Se opresc la ieșire, iar `watchdog` se trezește doar când e ceva de urmărit.
4. **Același worker „ultima cerere pe document” e scris de două ori**
   (`gutter_diff.rs:43`, `symbols.rs:60`). Devine un helper comun în `kernel::queue`
   (`Latest`) peste un pool.
5. **Fiecare versiune de document se parsează de 5 ori** (`symbols.rs:111`: `extract`,
   `level_issues`, `deprecated`, `unused`, `requirements`).
   - Varianta propusă: o singură parsare pe job, prin API-ul `LanguageIntelligence`, care
     primește arborele deja parsat.
6. **Regulă:** un `crates/modules/clippy.toml` interzice `thread::spawn` și
   `Builder::spawn`. Firele care chiar trebuie să trăiască mult (PTY, watcher) primesc
   `#[allow]` cu motivul scris lângă.

**Teste:** după un proiect deschis și închis, nu rămâne niciun fir de-al lui. Task
Manager-ul arată lucrul sub numele pool-urilor.

## 36e — Date partajate și eliberarea memoriei

1. **Indexul de simboluri ca instantanee** (amânat din review-ul 32, e și în TODO).
   - `queries.rs:107` clonează indexul pentru graful canvas. Cât timp clona există,
     fiecare actualizare (`Arc::make_mut`) copiază toate cele patru tabele, uneori la
     fiecare tastă.
   - **De făcut:** cititorii iau un `Arc<SymbolIndex>`, iar scriitorii construiesc
     versiunea următoare și o schimbă (ca un `ArcSwap`). Se poate și cu tabele pe bucăți.
2. **`purge` e gol la 23 din 26 de vederi**, deci eliberarea ferestrelor ascunse după
   5 minute nu face nimic.
   - Se implementează unde vederea ține date: compare, diagrame, imagine, task manager,
     istoric local, changes, structure, project-tree.
   - Panourile laterale parcate primesc același cronometru.
3. **„Clear and Rebuild Caches” nu ajunge la module** (`app/mod.rs:283`). Primește
   `ModuleManager::purge()`, care eliberează:
   - datele modulelor;
   - gramaticile compilate;
   - indexurile pachetelor de iconițe;
   - vectorii setărilor.
4. **Cache-uri pe lângă `kernel::cache`:**
   - `scope_colors` (`workbench/mod.rs:504`);
   - `glyph_keys` (`render/text/lib.rs:179`);
   - `file_icons` (`project-tree/panel.rs:613`);
   - vectorii setărilor, calculați acum pe firul UI la prima tastă.

   Trec pe `GenerationalCache` / `Memo`, iar vectorii se calculează în fundal.
5. **Serviciile de proiect pornesc toate la deschidere** (quality, vcs-git, istoric
   local). Nu adăugăm o politică de activare în manifest: serviciile devin leneșe și
   pornesc la primul eveniment care le privește. Decizia se notează în
   `03-sistem-module.md`.

**Teste:** după „Clear caches”, memoria raportată de module scade. O vedere ascunsă și
purjată se redeschide identic, verificat cu capturi.

## 36f — DDD: regulile în domeniu, porturi în aplicație

1. **Refactorizările:** regulile stau în aplicație (`workbench/refactorings.rs:61-489`,
   `moving.rs:31-82`):
   - un simbol din bibliotecă nu se redenumește;
   - stilul numelui;
   - PSR-4 (fișierul se redenumește odată cu tipul);
   - aplicarea și anularea, cu amprente;
   - ținta mutării.

   Pe deasupra, `std::fs::rename` e chemat direct. Toate se mută în crate-ul de domeniu
   `refactor` ca use case-uri peste porturile `DocumentStorage`, `EntryStore` și o
   interogare a indexului. Aplicația dă doar textele nesalvate.
2. **Nivelurile de limbaj și framework-urile** (`app/src/levels.rs`, `frameworks.rs`):
   regulile de rezolvare trec în domeniu, iar store-urile TOML vin prin trăsături (trait),
   date din `main.rs`.
3. **`Workbench` primește porturile, nu tipurile concrete:**
   - `FileSystem`, `SessionStore`, settings store, recent files;
   - un `FileWatcher`: watcher-ul `notify` se mută din `app/fs_watch.rs` în `fs-local`.

   Atunci `Workbench` se poate testa cu falsuri, fără disc.
4. **Modulele ocolesc porturile:**
   - `project-tree` folosește `LocalEntries` și `std::fs`;
   - `vcs-git/changes_view` și `local-history/view` citesc de pe disc, deci ignoră
     textul nesalvat;
   - `canvas`, `indexing`, `binary-files`.

   Se trec pe `EntryStore` / `FileSystem` din `ProjectContext` și pe `host.file_text`.
5. **`WorkbenchHost` a devenit un port-de-toate** (`domain/workbench/host.rs:233-319`). Se
   împarte pe capabilități: editor, refactorizare, vederi.
6. **Puritatea domeniului:**
   - `Instant::now()` în `resources.rs` și `extensions/lib.rs` devine un ceas dat din
     afară;
   - `is_dir()` din punctul 36b.10 dispare.
7. **`markup` și `stylesheet` sunt pure, dar stau în `infra`.** Se mută în `domain`, ca să
   primească verificările de puritate.
8. **Euristici de limbaj în aplicație:**
   - `path_in_quotes` și `multiline_signature` (`links.rs:298`);
   - `tab_reference` (`files.rs:536`).

   Trec în `code-intelligence`.
9. **Setările quality** (din TODO): un `SettingFamily<T>` cu cheia după limbaj, în
   `settings-model`. Înlocuiește cheile scrise ca text în `quality/settings.rs`.

**Teste:** use case-urile mutate își iau testele cu ele (se mută, nu se rescriu) și
primesc teste noi cu porturi false. Comportamentul rămâne identic.

## 36g — Funcționalități care ies din aplicație în module

1. **Git gutter și blame** (`app/workbench/vcs.rs`, `gutter_diff.rs`, câmpurile git din
   `ProjectData`) trec în `vcs-git`, printr-un punct de extensie generic: decorații de
   gutter și adnotări pe rând.
2. **Căutarea semantică** (`app/semantic.rs`, secțiunea scrisă de mână în setări) trece în
   modulul `indexing`, cu `SettingsContribution` proprie.
3. **„Compară cu alt tab”:**
   - acum e meniu și comandă scrise în aplicație (`menus.rs:107`, `COMPARE_PREFIX`);
   - devine contribuția modulului `compare` în `MenuSection::Compare`, printr-o interogare
     a tab-urilor deschise.
4. **Problemele quality:** aplicația ascultă direct `ProblemsFound`, iar starea stă în
   `Workbench`.
   - Devine un eveniment sau o contribuție generică de diagnostice.
   - Starea trece în `ProjectData`.
5. **Aplicația știe nume din module:**
   - firele `git-read`, `git-blame` (`task_data.rs:180`);
   - comenzile `git.log`, `refactor.rename` (`snapshot.rs:616`).

   Fiecare raportează singur prin `ResourceReporter`. Scenariile de capturi devin
   contribuții ale modulelor, sau măcar trec în spatele unui `feature = "snapshot"`, ca
   să nu mai intre în binarul de livrare.

**Teste și capturi:** toate cele 140 de capturi ies identice. Meniurile și setările arată
la fel.

## 36h — Reguli care să nu mai lase să alunece

În `cargo xtask check-deps` și în fișierele clippy:
- domeniul interzice și:
  - `Instant::now`, `SystemTime::now`;
  - `Path::{exists, is_dir, is_file, metadata, canonicalize}`;
  - `std::process::Command`, `std::net`;
  - `OpenOptions`, `fs::copy`, `create_dir`, `remove_dir`;
- modulele: fără fire proprii (36d.6);
- un modul poate folosi doar crate-urile `infra` dintr-o listă permisă, nu orice;
- aplicația: în `workbench/` nu apar tipuri concrete din `storage` / `fs_local`, după 36f.3;
- comenzile și firele altor module, scrise ca text în aplicație, sunt semnalate, nu doar
  căile `crate::`;
- `snapshot.rs` iese din lista de excepții, după 36g.5.

## 36i — Cod comun și împărțirea obiectelor mari

1. **Același rând de arbore e desenat de 4 ori:** chevron, indentare, iconiță, hover
   (project-tree, remote, git branches, task manager). Devine un `TreeList` / `tree_row`
   în `ui-components`, cu aceeași lovire a click-ului și aceleași linii de ghidare.
2. **Filtrarea listelor e în trei feluri:** `to_lowercase().contains`, `ui_kit::fold` și
   fuzzy. Rămâne un singur helper.
3. **`Workbench` are ~125 de câmpuri și ~460 de metode.** Se împarte în structuri cu un
   singur rost:
   - `TabDiagnostics`;
   - `EditorGroups`;
   - `Dialogs`;
   - `HomeScreen`;
   - `ToolArea`;
   - `Chrome` (bara de sus, tint, controalele ferestrei).

   Fiecare se mută ca atare, fără alt comportament. Și `EditorApp` (~76 de câmpuri) se
   împarte la fel.

**Capturi:** identice.

## Ordinea și regulile de lucru

Ordinea propusă: **a → b → c → d → e → f → g → h → i**.
- Primele trei se simt direct: scurgeri, înghețări, lucru dublat.
- Restul fac codul mai curat și mai greu de stricat.
- Fiecare literă e una sau mai multe commit-uri. Fiecare trece de `check.sh`, iar
  capturile ies identice.
- **Nimic din ce avem nu se pierde:** mutările se fac ca atare, cu testele existente
  mutate odată cu codul și cu teste noi pentru ce se schimbă ca structură.
- Ce se dovedește prea scump sau fără câștig se trece în TODO, cu motivul.

## Stare

- **36a, gata.**
  - Ferestrele de jos se închid la deschiderea altui proiect; sesiunea lui le-a păstrat
    deja pe cele care revin.
  - Ștergerea închide tab-urile în toate grupurile.
  - Setările eliberează modelul semantic și vectorii la închidere.
  - Calculul vectorilor se oprește între fișiere când proiectul se închide.
  - Registrele per proiect (quality, git, aprobarea remote) își scot intrarea printr-un
    `Subscription::on_end`, nou în `kernel`; numerele de shell nu se mai păstrează pentru
    proiectele fără shell-uri.
  - `SESSIONS` din remote rămâne căutat după cale: întrebarea e „în ce folder e fișierul”,
    iar două ferestre cu același folder au aceleași fișiere pe disc.
- **36b, gata.**
  - Parserele PHP și JS sunt câte unul pe fir (`thread_local!`), fără lacăt comun.
  - `refresh_symbols_now` extrage simbolurile înainte să blocheze indexul.
  - Tipul unei variabile (`->`) folosește regex-uri compilate o dată.
  - Arborele proiectului recitește în fundal (firul `tree-list`) când s-au schimbat mai
    mult de 4 foldere afișate. Loturile rămân în ordine: o listare veche nu o acoperă pe
    una nouă.
  - Conflictele la redenumire: indexul e eliberat înainte de citire și se citesc doar
    primele 3 rânduri.
  - Salvarea codează direct din bucățile buffer-ului (fără copia întreagă), iar verificarea
    Latin-1 se face tot pe bucăți.
  - Jurnalul remote copiază doar rândurile vizibile. Descrierea proiectului curent se
    face o dată, la deschidere, nu la fiecare cadru.
  - Indexarea anunțată de aplicație numără construcțiile terminate
    (`ProjectIndex::builds_finished`): una pornită și terminată între două verificări nu
    se mai pierde. Era cauza testului instabil `a_rebuild_and_another_project_start_clean`
    (punctul 36c.8, făcut aici).
  - **Lăsate, cu motiv:**
    - completarea pe worker: după regex-uri, costul rămas e mic (contextul și variabilele
      locale), iar o listă asincronă ar schimba felul în care răspunde la tastare;
    - mutarea instrucțiunii pe arborele worker-ului de sintaxă: e o acțiune rară, iar
      parserul nu mai e comun;
    - salvarea imaginii și autosave-ul pe un worker: `Viewer::save` e sincron, iar
      salvarea e o acțiune cerută explicit;
    - arborele propriu al aplicației și `reload_written`: citesc doar folderele încărcate
      de aplicație (rădăcina) și tab-urile deschise, deci au o limită;
    - `is_dir` pe proiectele recente și `exists` în meniul Copy: rulează doar la deschiderea
      meniului, nu la fiecare cadru (trecut în TODO pentru căile de rețea pe Windows).
- **36c, gata.**
  - Deschiderea proiectului nu mai anunță rădăcina ca schimbată. Arborele aplicației se
    reîncarcă singur, fără anunț, așa că indexurile nu mai parcurg din nou tot proiectul.
    `SearchState::update` sare peste fișierele cu aceeași mărime și dată.
  - Quality: handler-ul doar notează salvarea. Planul (căutarea în `PATH`) și rulările se
    fac pe un pool `quality` cu cel mult 2 programe deodată, nu pe câte un fir per rulare.
    Salvările dintr-o schimbare în masă așteaptă finalul ei, iar ce n-a pornit se anulează
    la închiderea proiectului.
  - Git status nu citește cât timp rulează o schimbare în masă; citește o dată, la final.
  - Watcher-ul ține căile în seturi deduplicate, cu limită de 4096. Peste limită se
    renunță la ele și se cere o recitire completă (`EntriesRescan`).
  - Trezirea e per fereastră (`Window::waker`). Ce privește toată aplicația (setările,
    monitorul de resurse, modelul semantic, a doua pornire) folosește `app_waker` și le
    trezește pe toate.
  - Refactorizările (aplicare și anulare) sunt o singură schimbare în masă. Simbolurile
    tab-urilor deschise se extrag înainte de lacătul indexului.
  - Generațiile fișierelor (`Generations`) au un singur proprietar, un abonat la
    evenimentele proiectului. Văd și scrierile făcute de alte programe, nu și folderele
    generate.
  - **Lăsate, cu motiv:**
    - lista de urcat din remote se citea deja o dată la câteva cereri (o citire pe rând,
      cererile apropiate unite), cu hash doar pe fișierele cu mărimea sau data mutată;
    - git status nu mai urmărește `.git/index`: status-ul își poate rescrie singur
      indexul, deci ar intra într-o buclă;
    - folderele generate rămân anunțate arborelui, care le arată;
    - arborele aplicației ține doar rădăcina (marcajele din meniuri), deci recitirea lui
      rămâne mică;
    - lucrul pornit din desen (`sync_vcs`, sesiunea la 0,5 s, termenul autosave-ului) e
      doar comparații ieftine, care pornesc ceva doar când chiar e nevoie.
- **36d, gata.**
  - `kernel::work::run(nume, job)` rulează lucrul pe un pool comun cu acel nume: limitat,
    iar firele lui se opresc când n-au de lucru. L-au primit graful, diff-ul compare,
    diagramele, indexul de căi, vederile de refactorizare, lipirea, git (log, commit,
    status în fereastra de commit), remote (plan, aplicare, descărcare) și numărarea
    istoricului local.
  - `kernel::Latest` ține ultima cerere pe cheie și o rulează pe un pool. Îl folosesc
    worker-ul de simboluri și diff-ul pentru gutter, care nu mai țin câte un fir cât
    trăiește proiectul.
  - Istoricul local folosește un pool per proiect (un job pe rând, în ordine). Curățenia
    se face înainte de prima versiune păstrată, nu la deschiderea proiectului. CPU-ul se
    numără pe job.
  - Watchdog-ul doarme cât nu se tratează niciun eveniment.
  - `LanguageIntelligence::analyse` dă simbolurile și toate analizele dintr-o singură
    parsare (PHP și JS). JS nu mai face un parser nou la fiecare analiză.
  - `crates/modules/clippy.toml` interzice `thread::spawn` și `Builder::spawn`. Firele
    care trebuie să trăiască singure au excepție, cu motivul scris lângă: cozile remote,
    sesiunea de navigare, reîncercarea care așteaptă, colectorul Task Manager-ului, coada
    indexării și detectarea PHP.
  - **Lăsat:** `module-release` se trezește o dată pe minut, ca să elibereze modulele
    nefolosite.
- **36e, gata.**
  - Vizualizatorul de imagini are `purge`: imaginea decodată și copiile mici (RAM și GPU)
    se eliberează și se decodează din nou când imaginea e arătată.
  - Tab-urile nevăzute de 5 minute își eliberează și ce ține vizualizatorul lor (imagine,
    Markdown), nu doar sintaxa.
  - „Clear and Rebuild Caches” cheamă `ModuleManager::purge`: datele modulelor și
    gramaticile compilate nefolosite se eliberează pe loc.
  - Vectorii setărilor se fac în fundal. Până sosesc, căutarea găsește după cuvinte, apoi
    caută din nou și după sens.
  - Activarea modulelor e notată în `03-sistem-module.md`: toate se activează, dar lenea
    e la date și la serviciile de proiect.
  - **Lăsate, cu motiv:**
    - indexul de simboluri ca instantanee rămâne în TODO: se copiază doar cât graful
      canvas ține o clonă, iar scrierile vin acum din fundal, nu de pe firul UI;
    - `purge` la compare, diagrame, task manager și istoric nu se face: compare ține
      texte care nu se pot citi din nou (taburi nesalvate), iar celelalte țin date mici
      sau le eliberează deja când se închid;
    - `scope_colors` și `glyph_keys` rămân pe `HashMap`: au o limită naturală (scopurile
      temei, glifele folosite) și se golesc la „Clear caches”. `glyph_keys` trebuie să
      rămână în pas cu atlasul GPU, deci nu poate scoate intrări singur.
- **36f, făcut în parte.**
  - `markup` și `stylesheet` s-au mutat în `crates/domain`, unde primesc verificările de
    puritate.
  - `path_in_quotes` și `multiline_signature` s-au mutat în `code-intelligence`, cu
    testele lor.
  - Regulile nivelurilor de limbaj (`code_intelligence::levels`) sunt în domeniu: alegerea
    manuală, apoi cea detectată, apoi cea mai nouă, plus excepțiile pe fișier. Aplicația
    doar citește și scrie `project.toml`.
  - `WorkbenchHost::context` nu mai întreabă discul (un tab arată mereu un fișier).
  - **Verificate, fără schimbare:**
    - framework-urile aveau deja regulile în domeniu (`choose_framework`,
      `active_framework`); aplicația doar le leagă de stocare;
    - compararea din git changes și din istoricul local e, intenționat, cu fișierul de pe
      disc;
    - lipirea din arbore lucrează pe discul local;
    - `Instant::now` din domeniu doar măsoară durate (activități, activare), nu decide
      nimic.
  - **Rămase pentru un plan separat (în TODO):** regulile refactorizărilor mutate în
    `refactor`, `Workbench` cu porturi în loc de tipuri concrete, împărțirea lui
    `WorkbenchHost` și `SettingFamily` pentru quality. Fiecare atinge zeci de fișiere și
    merită pașii lui.
- **Lista de proiecte recente, gata.** Folderele se verifică în fundal (`recent-check`).
  Lista arată ce s-a văzut ultima dată și se actualizează după verificare.

## Pașii rămași (cerut: „totul super organizat”)

Ordinea: **36f2 → 36f3 → 36f4 → 36g → 36i → 36h**. Fiecare e un commit, după `check.sh`
și cu capturile identice. Mutările se fac ca atare, iar testele se mută odată cu codul.

- **36f2 — refactorizările în domeniu.** Din `app/workbench/refactorings.rs` și
  `moving.rs` trec în crate-ul `refactor`, ca use case-uri:
  - contextul redenumirii (un simbol din bibliotecă nu se redenumește, stilul numelui,
    PSR-4);
  - aplicarea unui plan și anularea lui, cu amprente și fișierele sărite;
  - ținta mutării.

  Totul merge prin porturi: textul fișierelor (deschise sau de pe disc), scrierea, mutarea
  pe disc. Aplicația dă doar implementările: tab-urile deschise și `fs_local`.
  `std::fs::rename` și `create_dir_all` ies din `Workbench`.
- **36f3 — porturi în `Workbench`.**
  - `Workbench` primește un `Ports` (`FileSystem`, `SessionStore`, settings store pentru
    proiect, fișierele recente), făcut în `main.rs`.
  - Watcher-ul (`app/fs_watch.rs`) se mută în `fs-local`, în spatele unui
    `workspace::FileWatcher`.
  - Testele pot da falsuri.
- **36f4 — `WorkbenchHost` pe capabilități.** Metodele făcute pentru module anume
  (refactorizare, compare, teme, tip la cursor) trec în trăsături separate (editor,
  refactorizare, vederi), pe care modulele le cer doar pe cele de care au nevoie.
- **36g — funcții mutate din aplicație în module:**
  - git gutter și blame trec în `vcs-git`, printr-un punct de extensie generic
    (decorații de gutter, adnotări pe rând);
  - căutarea semantică trece în `indexing`, cu `SettingsContribution` proprie;
  - „compară cu alt tab” devine contribuția modulului `compare` (`MenuSection::Compare`);
  - problemele quality devin un eveniment sau o contribuție de diagnostice, cu starea în
    `ProjectData`;
  - firele și comenzile modulelor, scrise acum ca text în aplicație (`task_data.rs`,
    `snapshot.rs`), ies de acolo: fiecare modul se raportează singur, iar scenariile de
    capturi trec în spatele unui `feature`.
- **36i — cod comun și obiecte mari.**
  - `TreeList` / `tree_row` în `ui-components`, folosit de project-tree, remote, git
    branches și task manager;
  - un singur helper de filtrare a listelor;
  - `Workbench` și `EditorApp` împărțite în structuri cu un singur rost (`EditorGroups`,
    `Dialogs`, `HomeScreen`, `ToolArea`, `Chrome`, `TabDiagnostics`).
- **36h — regulile, la final**, ca să păzească tot ce s-a mutat:
  - domeniul interzice și `Path::exists`/`is_dir`/`metadata`, `process::Command`,
    `std::net`, `fs::copy`/`create_dir`/`remove_dir`/`OpenOptions`;
  - modulele pot folosi doar crate-urile `infra` dintr-o listă;
  - în `app/workbench` nu mai apar tipuri concrete din `storage` sau `fs_local`;
  - comenzile și firele modulelor nu mai apar ca text în aplicație.


## Stare (pașii rămași)

- **36f2, gata.**
  - Contextul redenumirii e în `refactor::application`: `rename_target` (un simbol din
    bibliotecă nu se redenumește, stilul numelui și sigiliul, fișierul redenumit cu tipul
    după PSR-4), `in_library` (folosit și de Change Signature) și `type_named_after`
    (tipul numit după fișier). Limbajele vin prin portul `Languages`; aplicația îl dă cu
    `ModuleLanguages` (modulele de limbaj).
  - Aplicarea și anularea planului sunt în `refactor::applying`: `apply_plan`,
    `undo_plan`, amprentele, fișierele sărite și `Undo` (fostul `RefactoringUndo`).
    Fișierele vin prin portul `ProjectFiles` (textul deschis, editarea lui, citirea și
    scrierea de pe disc, mutarea). Aplicația îl dă cu `WorkbenchFiles`: tab-urile și
    scrierea ca la salvare, cu `FileWillSave` / `FileSaved`.
  - Ce nu s-a putut face (scriere, redenumire, restaurare) se întoarce ca `Problem`.
    Aplicația îl scrie în jurnal cu același text ca înainte.
  - Mutarea pe disc e `rename_entry`, prin portul `Entries` (ocupat, folder nou,
    redenumire). Îl implementează `fs_local::LocalFileSystem`, deci `std::fs::rename` și
    `create_dir_all` au ieșit din `Workbench`. `move_file` face doar ce ține de
    aplicație: tab-urile, arborele și `FileRenamed`.
  - Ținta mutării e în `refactor::moving`: `classes_in` (clasele numite după fișier, din
    index), `movable` și `moves_somewhere`.
  - `Workbench` doar leagă lucrurile: lotul de schimbări, reindexarea,
    `RefactoringApplied` și mesajul. Evenimentele și mesajele sunt aceleași, în aceeași
    ordine.
  - Teste noi în domeniu, cu porturi false: `tests/unit/applying.rs` și cazuri noi în
    `application.rs` și `moving.rs`. Testele aplicației au rămas și trec.
  - `refactor` depinde acum de `code-intelligence` (domeniu → domeniu).
  - **Lăsat:** `reindex_changed` rămâne în aplicație, pentru că ține de index, de
    tab-uri și de bus. Întrebarea „e folder?” de acolo și din `move_context_for` trece
    în spatele porturilor la 36f3.
- **36f3, gata.**
  - `Workbench::new(support, ports)` primește un `Ports` (`workbench/ports.rs`): discul
    (`Disk`, adică `workspace::FileSystem` + `document::DocumentStorage` +
    `EditorConfigSource` + `refactor::applying::Entries`), urmărirea fișierelor
    (`workspace::FileWatching`), sesiunile (`workbench_model::SessionStore`), fișierele
    recente (`workspace::RecentFiles`, port nou) și settings store-ul proiectului (o
    funcție care dă un `SettingsStore` pentru `project.toml`). `Ports::local()` e în
    `app/src/ports.rs`, în afara lui `workbench`, și e făcut în `main.rs`. Fiecare fereastră
    nouă primește același `Ports` prin `AppWindows`.
  - `workspace::FileSystem` are acum `kind` (implicit: din listarea folderului), `stamp`
    (mărimea și data modificării) și `canonical`. `LocalFileSystem` le răspunde direct, iar
    `ListedFs` le trimite mai departe la disc.
  - Watcher-ul s-a mutat din `app/fs_watch.rs` în `fs-local/src/watch.rs`, cu testul lui.
    Stă în spatele lui `workspace::FileWatcher` (`drain_changed`, `drain_written`,
    `take_rescan`, `take_vcs_changed`), pornit prin `FileWatching::watch` (`LocalWatching`).
    Aplicația ține un `Box<dyn FileWatcher>` și nu mai depinde de `notify`.
  - `storage::RecentFilesFile` implementează `RecentFiles` peste `recent_files::load/save`.
  - Întrebările „e folder?” din `reindex_changed` și `move_context_for` (și din
    `move_entry`, `open_path`, meniul intrării), „există?”, `is_file`, data și mărimea
    fișierului (`disk_stamp`), `canonicalize` și redenumirea tab-ului (`is_taken` +
    `rename`) trec prin port. În `app/workbench` nu mai e niciun `std::fs`, `fs_local`
    sau `storage`.
  - Teste: un test al aplicației cu falsuri (watcher și fișiere recente:
    `the_workbench_follows_the_ports_it_is_given`), `kind` implicit în `workspace`,
    `kind` / `stamp` / `canonical` în `fs-local`. Restul testelor folosesc `Ports::local()`.
  - **Lăsat:** `app/queries.rs` (în afara lui `workbench`) încă face un `LocalFileSystem`
    pe loc. `levels.rs` și `frameworks.rs` își iau store-urile singure, nu din `Workbench`.
    Rămân pentru regula de la 36h.
- **36f4, gata.**
  - `WorkbenchHost` (`workbench-model`, `host.rs`) a rămas nucleul: proiectul (rădăcina,
    identitatea, `context`, `target_path`, depozitul, `project_storage`, setările lui,
    `language_of`, `relist`), bus-ul, întrebările (`confirm`, `choose`), lucrul în fundal
    (`spawn`), mesajele și progresul, meniul și comenzile (`set_menu_context`,
    `view_command`, `request_command`, `keyed_view`, `editor_focused`).
  - Capabilitățile sunt trăsături separate, în `host/capabilities.rs`, supertrăsături ale
    lui `WorkbenchHost`:
    - `ViewHost`: vederile modulelor (tab, grupul de jos, fereastra de jos, modal, overlay),
      `compare`, ferestrele de unelte (`toggle_tool`, `show_side_panel`, `tell_panel`,
      `set_tool_badge`);
    - `EditorHost`: textul editorului (`file_text`, `edit_active`, `save_active`,
      `committed_text`, `annotated_commit`, `toggle_annotations`, `type_at_cursor`);
    - `RefactoringHost`: redenumirea, Change Signature, mutarea (`move_context*`,
      `rename_type_of_file`), aplicarea și anularea planului;
    - `ServicesHost`: `queries`, `semantic`, `resources`, `themes`;
    - `SystemHost`: clipboard-ul sistemului, file managerul, `open_project`.
  - Toate metodele capabilităților au implementări implicite („nu e aici”), afară de
    `open_view`. Falsurile din teste (`project-tree`, `vcs-git`) implementează doar ce
    folosesc, iar restul e `impl EditorHost for Host {}`.
  - Aplicația le implementează în blocuri separate (`app/workbench/host.rs`). Modulele
    apelează metodele tot prin `host` (pe un `dyn WorkbenchHost` se văd și metodele
    supertrăsăturilor), deci nu s-a schimbat nimic în ele. O funcție care are nevoie doar
    de o capabilitate o poate primi prin upcasting (`&mut dyn RefactoringHost`).
  - **Lăsat:** am ales supertrăsăturile, nu accesori `host.refactoring() -> Option<…>`,
    pentru că `CommandHandler::enabled` primește `&dyn WorkbenchHost` și citește din
    capabilități (`committed_text`, `annotated_commit`, `clipboard_text`, `resources`).
    Cu accesori ar fi trebuit perechi `&` / `&mut`. Asta înseamnă că un modul poate apela
    orice capabilitate: compilatorul nu verifică ce folosește fiecare modul.
- **36g (git), gata.**
  - Punctul de extensie e în `workbench-model` (`decorations.rs`): un modul contribuie un
    `LineDecorator`, care se deschide cu proiectul (`DecoratorContext`: identitatea,
    rădăcina, bus-ul, `wake`, depozitul) și dă un `ProjectDecorations`. Acesta trăiește cât
    proiectul, în `ProjectData.decorations`.
    - Înainte de fiecare cadru, aplicația îi spune ce documente se văd (`sync` cu un
      `ShownDocument`: id, cale, versiune, text). La trezire face `poll`, iar după o
      schimbare în `.git` cheamă `vcs_changed`. Tab-urile închise le scoate cu `keep`.
    - Aplicația ia de la el doar tipuri generice: `DocumentDecorations` (`LineMark` cu
      `MarkKind`, `LineAnnotation`), lățimea adnotărilor, tooltip-ul adnotării, `settled`
      (pentru capturi) și `label` (ramura din bara de sus).
  - În `vcs-git` (`decorations.rs`, plus `gutter_diff.rs` mutat ca atare) stă tot ce era în
    `app/workbench/vcs.rs`:
    - textul din commit și blame-ul, citite în fundal pe pool-urile proprii ale proiectului
      (`git-read`, `git-blame`), oprite cu proiectul;
    - liniile schimbate (`gutter-diff`, pe `kernel::Latest`) și adnotările;
    - `HEAD` și ramura, iar la un `HEAD` nou publică `HeadChanged`, ca înainte.
  - Comenzile modulului (Revert, Rollback, Annotate, commit-ul adnotat) citesc aceeași
    stare, găsită după proiect și după documentul activ. Din `EditorHost` au ieșit
    `committed_text`, `annotated_commit` și `toggle_annotations`. A intrat
    `active_document` (`None` când un meniu lucrează pe altă intrare, ca înainte).
  - Rândul „Git” din Task Manager (firele, discul, memoria) îl raportează modulul, cu
    `ResourceReporter`. `ui-workbench` nu mai depinde de `version-control`. Din
    `ProjectData` și `Tab` au ieșit câmpurile git, iar din aplicație `compare_with_head`
    (un clic pe bară rulează acțiunea modulului; capturile rulează `git.compare_head` și
    `git.annotate`).
  - Teste noi în `vcs-git`: `tests/unit/decorations.rs` (barele, o versiune nouă,
    adnotările și tooltip-ul, `HEAD` nou anunțat, fără depozit). `changes.rs` folosește
    starea reală a modulului, peste depozitul fals.
  - **Lăsat:** depozitul îl deschide tot aplicația (`VcsProvider`, `host.repository()`,
    `metadata_dir` pentru watcher). E punctul de extensie existent, iar comenzile git îl
    folosesc peste tot.
  - Clicul pe o bară compară din nou cu `HEAD` când acțiunile modulului nu-l iau (ca
    înainte): `ProjectDecorations::mark_clicked` dă comanda (git: `git.compare_head`), iar
    aplicația o rulează. Test: `a_click_on_a_change_bar_compares_with_the_last_commit`.
  - Adnotările rămân per tab: starea e ținută după `DocumentId`, iar fiecare tab are
    documentul lui (tab-ul din alt grup e o oglindă cu id propriu), deci e același lucru
    ca înainte, fără nimic în plus.
- **36g (căutarea semantică), gata.**
  - Modelul a trecut din `app/semantic.rs` în modulul `indexing` (`semantic.rs`): descărcarea,
    încărcarea (pe `kernel::work::run("embed")`), eliberarea după 10 minute fără căutări și
    starea pentru UI (`SearchMode`, mutat din `workbench-model` în modul, cu trăsătura
    `SemanticSearch` a căutării în proiect). Folderul de configurare e acum
    `storage::config_dir()` (aplicația îl folosește tot de acolo), deci modelul stă unde
    stătea.
  - Setarea `[search] semantic` e declarată în modul (`settings.rs`, `SEMANTIC_SEARCH`) și
    arătată printr-un `SettingsContribution` (`semantic`, secțiunea „Căutare”). Blocul
    folosește două rânduri generice noi în `workbench-model`: `SwitchCard` (cardul cu
    comutator și insigna de stare, `StatusTone`) și `Notes` (explicațiile). Secțiunea
    „Căutare” din setări arată doar blocurile modulelor; fără ele nu mai apare.
    `SettingsView` nu mai are nimic semantic (`ToggleSemantic`, `paint_search`, câmpurile
    din `SettingsModel`, intrarea scrisă de mână din căutarea setărilor).
  - Indexul ia singur modelul: `ProjectIndex::sync_model` (în locul lui `set_embedder` /
    `is_semantic`) și `release_idle` (căile și modelul, în locul lui `release_idle_paths`).
    Modelul trezește fereastra fiecărui index deschis când e gata sau eliberat
    (`semantic::follow`), ca `app_waker` de dinainte.
  - Căutarea în setări primește modelul printr-un punct de extensie generic,
    `EmbeddingSource` (`workbench-model`, `embeddings.rs`), la fiecare deschidere
    (`Workbench::take_settings_embedder`). Din `ServicesHost` a ieșit `semantic()`; în
    nucleul gazdei a intrat `user_settings()` (butonul „Activează” din căutarea în proiect
    salvează alegerea acolo).
  - Setările editate de mână (citite din nou la revenirea în fereastră) ajung acum și la
    `apply` al blocurilor modulelor (un abonament în `modules::load`), ca valoarea să fie
    tot cea din fișier, ca înainte.
  - Task Manager: firul `embed` și traficul descărcării se văd pe rândul modulului
    „Indexing”, nu pe rândul „Index” al aplicației.
  - Teste: `indexing/tests/unit/settings.rs` (blocul, alegerea salvată, `poll`) și
    `the_search_section_shows_its_switch_card` în `ui-workbench`.
  - **Lăsat:** modelul se eliberează când un index îl verifică (la desenarea unei ferestre
    cu proiect); o fereastră fără proiect care l-a pornit din setări nu-l mai eliberează
    singură. `poll` al blocului ține minte o singură stare pentru toate ferestrele: două
    ferestre cu setările deschise în timpul descărcării nu-și văd amândouă fiecare pas.
- **36g („compară cu alt tab”), gata.**
  - Rândurile cu celelalte tab-uri din „Compară cu ›” al unui tab sunt contribuția modulului
    `compare`, printr-un punct de extensie generic din `workbench-model` (`menus.rs`):
    `ListedMenuItems` (id, iconiță, locuri, secțiune) cu un `MenuList`, care dă rândurile
    când se deschide meniul (`rows`, din `ActionContext`) și ce face un clic (`run`, cu
    argumentul rândului). Id-ul unui rând e `<id>:<argument>` (`compare.tabs:<tab>:<altul>`).
    Aplicația le pune înaintea item-elor secțiunii (clipboard-ul), cu separatorul de
    dinainte, și le rulează din `on_menu_command` (`Workbench::listed_items` / `run_listed`);
    iconița vine din contribuție (`commands::set_listed`).
  - Interogarea gazdei, în nucleu: `WorkbenchHost::tabs()` (`TabInfo`: identitatea
    documentului, titlul, fișierul) — tab-urile grupului curent, în ordine.
  - Comparația o face modulul: `compare::compare_files` (textul din editor, altfel de pe
    disc), folosită și de „Compare with Active File” și de captura `COMPARE`. Din aplicație
    au ieșit `COMPARE_PREFIX`, `COMPARE_TABS` și `Workbench::open_compare`.
  - Tab-ul temei: `close_theme_studio` (care căuta `STUDIO_VIEW`) a devenit
    `close_asking_view`: închide tab-ul activ dacă e o vedere (configuratorul trimite
    `SaveTheme` / `CancelTheme` doar când e el cel activ).
  - Test nou: `a_tab_is_compared_with_another` în `compare`, cu o gazdă falsă.
  - **Diferență:** tab-ul comparației deschise din „Compară cu ›” primește calea virtuală în
    folderul fișierului din dreapta (ca toate comparațiile modulului), nu al celui din
    stânga; se vede doar când cele două fișiere sunt în foldere diferite.
- **36g (diagnosticele), gata.**
  - Problemele găsite de verificatoare sunt acum un eveniment generic din `workbench-model`
    (`diagnostics.rs`): `DiagnosticsFound` (sursa, `DiagnosticsScope::File` / `Project`,
    `Diagnostic`-ele: cale, linie, coloană, `Severity`, mesaj, sursa, regula). Modulul
    `quality` le publică pe bus-ul proiectului (`diagnostics_found`, din `Problem`), iar
    `ProblemsFound` a ieșit din `code_quality`.
  - Starea e `ProjectDiagnostics` (pe sursă, apoi pe fișier; un fișier își înlocuiește
    diagnosticele, un proiect pe toate ale sursei), în `ProjectData.diagnostics`, făcută de
    `ProjectData::new()` cu abonamentul pe bus-ul proiectului: pleacă odată cu proiectul,
    nu mai e resetată de mână în `files.rs`. Din `Workbench` a ieșit câmpul `quality`;
    `poll_quality` e `poll_diagnostics`.
  - În aplicație a rămas doar `workbench/diagnostics.rs` (fostul `problems.rs`): un
    diagnostic ca semn în editor (`mark_of`), cu testele lui.
  - Teste: `workbench-model/tests/unit/diagnostics.rs` (mutat din aplicație, plus „alt
    bus nu ajunge aici”) și `problems_go_out_as_diagnostics` în `quality`.
- **36g (numele modulelor în aplicație), gata.**
  - `task_data.rs` nu mai scrie niciun fir al vreunui modul: după git (36g git) și modelul
    semantic (36g căutarea semantică), rândurile aplicației au doar firele ei (`symbols`,
    `tree-watch`, `syntax`…); fiecare modul își raportează singur rândul
    (`ModuleThreads`, `ModuleCounters`, `ResourceReporter`).
  - Scenariile capturilor au trecut în spatele unui `feature = "snapshot"` al aplicației
    (`snapshot.rs` și metodele `Workbench` folosite doar de ele, cu
    `#[cfg(feature = "snapshot")]`; cele folosite și de teste, cu `any(test, …)`). Fără el,
    `--snapshot` spune că build-ul nu are capturi. `scripts/ui-snapshots.sh` construiește
    singur aplicația cu el; `check.sh` (și CI) rulează clippy și pe aplicație cu el, iar
    testele workspace-ului cu `app/snapshot`. Build-urile de livrare (`build.sh`) rămân fără
    el.
  - Am ales `feature`, nu scenarii contribuite de module: scenariile conduc `Workbench`-ul
    aplicației (cadre, așteptări, desenare, `settings_step`, `type_text` cu `TextSystem`),
    pe care modulele nu-l văd. Ca să fie contribuții, gazda ar fi trebuit să primească un
    API doar pentru capturi, adică mai mult cod în binarul livrat, nu mai puțin. Cu
    `feature`, binarul livrat nu are nici scenariile, nici id-urile comenzilor modulelor
    (`git.log`, `refactor.rename`), nici câmpurile asistentului remote.
  - **Lăsat:** `snapshot.rs` încă numește comenzi și tipuri din module (permis de
    `check-deps`, ca înainte); la 36h, regula poate să-l scutească doar în spatele
    feature-ului.
- **36i (rândurile de arbore), gata.**
  - `ui-components` are acum `tree` (`tree.rs`): `chevron` / `chevron_icon` (în jos deschis,
    la dreapta strâns), `guides` (o linie pe nivel, pe pixel întreg), `highlight` (fundalul
    rotunjit al rândului ales sau de sub mouse) și `TreeList` (rânduri de aceeași înălțime:
    `visible` — ce rânduri se văd la un scroll, `slot_at` / `row_at` — rândul de sub mouse).
  - Le folosesc arborele proiectului (`tree_view.rs`: rândurile văzute, lovirea click-ului
    cu rândul de intrare pentru un fișier nou, ghidajele, chevronul, fundalurile), arborele
    folderului remote (`connect_view/paint.rs`), ramurile git (`log_view/branches.rs`) și
    grupurile din Task Manager (`view/paint.rs`).
  - Geometria rămâne a fiecăruia (înălțimea rândului, indentarea, unde stă chevronul):
    componenta doar desenează și lovește, deci pixelii sunt aceiași. Teste:
    `ui-components/tests/unit/tree.rs`.
- **36i (filtrarea listelor), gata.**
  - Un singur helper, `ui_kit::ListFilter` (`ui-kit/src/filter.rs`, fostul `fold.rs`, cu
    `fold` lângă el): textul scris se taie și se pliază o dată (litere mici, fără
    diacritice), iar `matches` / `matches_any` spun dacă un text al rândului îl conține.
  - Îl folosesc lista de proiecte și cea a proiectului deschis (`projects_picker.rs`),
    editorul de scurtături, versiunile din alegătorul de nivel (`level_picker.rs`) și Task
    Manager-ul (părțile și operațiile).
  - Rămân fuzzy, cu ordonare: căutarea fișierelor, a simbolurilor și cea din canvas.
    Căutarea în setări își păstrează scorul pe cuvinte (tot cu `fold`).
  - **Diferență:** editorul de scurtături, alegătorul de nivel și Task Manager-ul ignoră
    acum și diacriticele („setari” găsește „Setări”), ca lista de proiecte; înainte doar
    literele mici.
- **36i (`Workbench` împărțit), gata.**
  - Câmpurile s-au mutat ca atare în structuri cu un singur rost, fiecare în fișierul ei din
    `app/workbench`:
    - `Chrome` (`chrome.rs`): bara de sus, tint-ul, eticheta proiectului, butoanele ferestrei,
      ecranul complet și apariția barei ascunse (`top_bar_hides`);
    - `Dialogs` (`dialogs.rs`): dialogul deschis, răspunsul, textul și bifa lui (`answered`),
      întrebarea unui modul (`confirming`), tab-ul de jos redenumit;
    - `HomeScreen` (`home.rs`): ecranul gol (`picker`), fișierele lui, lista de proiecte din
      bara de sus și proiectele recente (`pressed_picker`);
    - `ToolArea` (`tool_area.rs`): barele de unelte, panourile laterale și separatoarele lor,
      ferestrele de jos, mărimile lor (`side_on`, `side_at`, `window`, `panel_viewer`,
      redenumirea, fixarea și închiderea vederilor);
    - `EditorGroups` (`groups.rs`): grupurile, cel curent, arborele împărțirii, separatoarele
      și tab-ul tras (`group_at`);
    - `TabDiagnostics` (`tab.rs`), în `Tab`: erorile, semnele și intervalele desenate
      (`set_marks`), problemele de nivel, numele învechite sau nefolosite, cerințele.
  - Metodele publice ale lui `Workbench` (folosite de `EditorApp`) au rămas pe el; în
    structuri au trecut doar cele care ating un singur grup. `Workbench` are acum ~80 de
    câmpuri, nu ~125. Fără alt comportament; testele au rămas și trec.
- **36i (`EditorApp` împărțit), gata.**
  - `EditorApp` avea 38 de câmpuri (nu ~76, cât spunea planul); acum are 12. Grupurile sunt
    în `app/app/state.rs`, mutate ca atare:
    - `WindowState`: fereastra ascunsă, focusul, pipeta, titlul;
    - `Pointer`: mouse-ul, cursorul, tragerea, ultimul clic, unde s-a deschis ultimul meniu
      (și pe ce nod din arbore);
    - `Frames`: renderer-ul, scena, suprapunerea de depanare, latența, diagnosticul de
      redimensionare;
    - `Startup`: splash-ul (`splash_at`: cât s-a arătat și opacitatea) și primul cadru;
    - `Applied`: versiunile temei, limbii și scurtăturilor aplicate, fontul dat;
    - `Pending`: închiderea tab-urilor în curs, întrebarea dialogului, ieșirea cerută.
  - Fără alt comportament; testele au rămas și trec.
- **36h (domeniul fără I/O), gata.**
  - `crates/domain/clippy.toml` interzice acum și `Instant::now`, `SystemTime::now`,
    `Path::{exists, try_exists, is_dir, is_file, is_symlink, metadata, symlink_metadata,
    canonicalize, read_dir}`, `fs::{copy, create_dir, remove_dir, canonicalize,
    symlink_metadata}` și tipurile `OpenOptions`, `process::Command`, `net::{TcpStream,
    TcpListener, UdpSocket}`.
  - Ce le folosea primește acum un ceas: `Activities::new(clock)` (modulele dau
    `Instant::now`) și `ModuleRegistry::set_clock` (dat de aplicație în `builtin()`; fără
    el, duratele sunt zero). Fără excepții în codul domeniului; un test își ia singur
    momentul de start (`#[allow]` pe test).
  - Modulele: pe lângă `thread::spawn` / `Builder::spawn`, interzise și `thread::scope` /
    `spawn_scoped`. Firele scoped din `remote-projects` (`session::threads`,
    `transfers::run`) spun de ce, ca celelalte excepții (Task Manager, `indexing::feed`,
    `lang-php`, sesiunea și transferurile remote).
- **36h (`check-deps`: infra-ul modulelor, porturile, numele modulelor), gata.**
  - Un modul poate depinde doar de adaptoarele dintr-o listă: comune tuturor (`process`,
    `files`, `fs-local`, `storage`, `syntax-ts`, `image-decode`) sau ale funcției lui
    (`embed` / `indexer` → `indexing`, `git` → `vcs-git`, `history-fs` → `local-history`,
    `process-stats` → `task-manager`, `pty` → `terminal-panel`, `remote-ftp` / `remote-sftp`
    / `secrets` → `remote-projects`). `db-postgres`, `lsp-client` și orice adaptor nou rămân
    ale aplicației până intră în listă. Lista e cea de azi, fără mutări.
  - Regulile din surse au trecut în `xtask/src/app_sources.rs` (cu testele lui):
    - în `app/src/workbench/` niciun `storage::` / `fs_local::` (nici `use … as`);
    - textul din aplicație egal cu o comandă (`CommandId("…")`) sau un fir al unui modul
      (`.name("…")`, `work::run("…")`, `Pool::new("…")`, `const …THREAD…`,
      `ModuleThreads(&[…])`, unde `"remote-"` e prefix) e semnalat. Singura excepție:
      `"paste"` din `app/actions.rs` (acțiunea de editare cerută panoului, nu firul
      arborelui), în `NOT_A_MODULE_NAME`;
    - `snapshot.rs` a ieșit din lista de excepții: e scutit orice `mod` declarat după
      `#[cfg(feature = "snapshot")]` (azi doar el), deci doar în build-ul capturilor.
  - Ce rămăsese de la 36f3 trece prin porturi: `Ports` are acum `levels`
    (`ProjectLevelsStore`) și `frameworks` (`ProjectFrameworksStore`), date de
    `Ports::local()`; `levels::load/save` și `ProjectFrameworks::load/set` primesc store-ul.
    `QueriesSnapshot` primește discul (`disk`, discul lui `Ports`) în loc să-și facă un
    `LocalFileSystem`.
