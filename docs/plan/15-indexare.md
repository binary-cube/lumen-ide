# 15. Indexarea ca modul, prin evenimente cu cozi

Scop: indexarea proiectului (căile, simbolurile, textul / sensul) devine un modul care
**ascultă** ce se întâmplă în proiect, în loc să fie **chemată** de fiecare funcție. Domeniile
își anunță doar faptele lor (un fișier salvat, mutat, șters, schimbat pe disc, o refactorizare
aplicată); modulul le pune într-o coadă a lui, care comasează cererile — aceeași cale cerută de
mai multe ori înainte să fie procesată = o singură reindexare —, și își anunță starea pentru
cine o arată. Nimic nu se pierde din ce face indexarea azi.

## Azi (inventar, 2026-10-22)

Patru bucăți, toate ale aplicației (`crates/app`) sau din infra, legate direct:

| Ce | Unde | Fir | Comasare azi |
|---|---|---|---|
| Indexul de căi (căutarea de fișiere, Go to File) | `workbench/project_files.rs` (`ProjectFiles`, în `ProjectData`) | `file-index`, o parcurgere completă | da: un singur build odată; `mark_stale` doar marchează |
| Indexul de simboluri + text / sens, persistent (`.lumen/index`) | `infra/indexer` (`ProjectIndexer`), pornit din `intelligence.rs` | `indexer` + `index-watch` (watcher propriu) | da: `Changed` cu debounce 300 ms, sortat, fără dubluri; **nu** pentru `Rebuild` |
| Simbolurile taburilor deschise (la fiecare editare) | `symbols.rs` (`SymbolWorker`) | `symbols` | da: ultima cerere pe document |
| Modelul semantic (Model2Vec) | `semantic.rs`, unul pe proces | `embed` | — (încărcat la nevoie, eliberat după 10 min) |

Legăturile directe de azi (de înlocuit cu evenimente):
- Lipirea de fișiere (modulul `project-tree`) **pune indexarea pe pauză** prin gazdă
  (`host.pause_indexing`); cât e pe pauză, schimbările se aruncă, iar la reluare se face o
  reîmprospătare completă.
- Refactorizarea **reindexează sincron, pe firul UI**, fișier cu fișier (`reindex_changed`),
  apoi cheamă `indexer.changed`.
- Watcher-ul arborelui marchează direct indexul de căi (`mark_stale`).
- **Doi watcher-i** pe același proiect: `tree-watch` (aplicația → `EntriesChanged`) și
  `index-watch` (al indexer-ului) — dublă muncă pentru același eveniment de disc.
- `EntriesDeleted` nu e ascultat de indexer (se bazează pe watcher-ul lui);
  `ProjectOpened` nu are niciun ascultător.
- Starea: bara de jos arată „Indexing…” doar după indexer-ul de simboluri; căutarea de
  fișiere își are propriul „se indexează”; Task Manager-ul vede doar CPU-ul firelor, fără
  memoria indexului.
- „Clear and Rebuild Caches” și „Reindex” cheamă direct aplicația; două `Rebuild` la rând
  fac două construiri complete.

## Ținta

```
domenii (workspace, document, refactor…)      ──publică──▶  EventBus
  FileSaved · FileRenamed · EntriesDeleted · EntriesChanged
  RefactoringApplied · BulkChange{start/end} · ProjectOpened
                                                                │ abonare (cât trăiește proiectul)
                                                                ▼
modulul `indexing` (crates/modules/project/indexing)
  ProjectService: pornit la deschiderea proiectului, oprit cu el
  CoadaComasată (kernel::queue): căi unice + „tot proiectul” care le înghite;
       așteaptă liniștea (debounce) dar nu mai mult de un plafon; ține cât e deschis un BulkChange
  ├─ indexul de căi      (fostul ProjectFiles)
  ├─ indexul de simboluri / text   (infra/indexer, fără watcher propriu)
  └─ publică: IndexingStarted · IndexingProgress{done,total,scope} · IndexingFinished
       ▲ ascultate de: bara de stare, căutarea de fișiere, Task Manager (ResourceReporter)
```

Reguli:
- **Domeniile nu știu de modul.** Publică doar faptele lor; nimeni nu mai cheamă „pune pe
  pauză” sau „reindexează” direct (în afară de comanda explicită a utilizatorului).
- **Coada comasează.** O cale deja în coadă și neprocesată nu se mai adaugă; o cerere „tot
  proiectul” le înghite pe toate; un `BulkChange` deschis (o lipire de 1.000 de fișiere) ține
  coada până se închide → un singur lot.
- **Un singur watcher** pe proiect (cel care publică `EntriesChanged`).
- **Nimic greu pe firul UI**; abonările doar pun în coadă (`EventBus` livrează sincron).
- **Starea e anunțată**, nu citită prin `is_busy()` din toate părțile.
- Indexul se eliberează când nu e folosit (ca azi), se golește complet la alt proiect
  (`ProjectData`), se poate reconstrui din „Clear and Rebuild Caches”.

## Pașii (un commit fiecare; `check.sh`; „go” între ei)

- [x] **15a. Plasa de siguranță.** `tests/unit/workbench/intelligence.rs`, prin aplicație:
      salvarea, un fișier făcut / redenumit / șters pe disc (declarațiile și căutarea de
      fișiere îl urmează), o lipire de 40 de fișiere (indexate toate, indexarea nelăsată pe
      pauză), Clear and Rebuild Caches, alt proiect (nimic din cel vechi). Refactorizarea
      („numele nou găsit imediat”) avea deja test (`renames_a_method_everywhere_and_undoes_it`).
      „O lipire nu pornește N reindexări” se măsoară în 15d (contorul cozii).
      Găsit: un fișier făcut pe disc înainte ca watcher-ul arborelui să fie activ (imediat
      după deschiderea proiectului) nu ajunge niciodată în indexul de căi (o rescriere nu-l
      marchează vechi); indexer-ul de simboluri acoperă golul (reîmprospătare când watcher-ul
      lui devine activ), cel de căi nu — de închis în 15d (un singur watcher, cu
      reîmprospătare la pornire).
- [x] **15b. Coada comasată** (`kernel::queue`): `Coalescing<K>` (timpul dat din afară,
      testată fără așteptări) și `SharedQueue<K>` pentru un fir care doarme până e gata un
      lot. O cheie deja în așteptare nu se mai adaugă; „tot” le înghite; așteaptă liniștea
      (`quiet`) dar nu mai mult de `max_wait`; un lot deschis (`hold(id)`) ține tot până se
      închide (sau până trece `hold_limit`, ca un lot uitat să nu blocheze), după care
      ceasurile pornesc din nou; `QueueStats` (cerute / comasate / loturi) pentru Task Manager
      și teste.
- [x] **15c. Evenimentele lipsă**, în domeniile lor: `BulkChangeStarted / Ended { id }`
      (UUID v4) și `IndexingStarted / Progress / Finished { index: Paths | Symbols }` în
      `workspace::events`; `refactor::events::RefactoringApplied { changed, renamed }`.
      Publicate de cine face lucrul: lipirea (început / sfârșit, și când nu pornește),
      refactorizarea (la aplicare și la anulare), indexurile de azi (pornit / gata; progresul
      vine cu modulul). Deocamdată **pe lângă** apelurile directe (pauza, reindexarea
      sincronă), ca nimic să nu se strice între commit-uri; acestea dispar în 15d–15f.
- [x] **15d. Modulul `indexing`** pentru simboluri / text. Portul `workbench_model::
      {IndexProvider, ProjectIndex}`: aplicația cere indexul proiectului de la modul și doar
      îl citește (declarațiile, căutarea, starea, modelul semantic, Reindex). Modulul pornește
      `ProjectIndexer` fără watcher și fără debounce proprii (`Options`) și îl ține la zi din
      evenimente prin coadă (`feed.rs`, firul `index-queue`): `FileSaved`, `FilesWritten`
      (nou: ce scrie oricine pe disc), `FileRenamed`, `EntriesDeleted / Changed` (un folder:
      tot proiectul revăzut), `RefactoringApplied` (bandă urgentă; fișierele schimbate doar
      în editor — `unsaved` — rămân ale taburilor până la salvare), `EntriesRescan` (nou:
      watcher-ul abia pornit — închide golul găsit la 15a), loturile. Un singur watcher
      (`index-watch` nu mai pornește). Pauza la lipire a dispărut (gazdă, modulul arborelui).
      Reindex cerut de două ori înainte să pornească: o dată. Task Manager: „Index: proiect”
      (fișiere, cât a comasat coada din cât s-a cerut), firele lui. Măsurat: o lipire de 40
      de fișiere = cel mult 3 reindexări (era câte una pe fișier, prin pauză + reîmprospătare
      completă).
      Găsit și reparat: refactorizarea scria în index simbolurile din taburi, apoi îi cerea
      indexer-ului să recitească de pe disc — care le suprascria cu textul vechi (nesalvat).
- [x] **15e. Indexul de căi în modul** (`paths.rs`, fostul `ProjectFiles`), prin același
      port (`ProjectIndex`: `ensure_paths`, `find_paths`, `files`, `peek_*`,
      `take_paths_built`, `release_idle_paths`); marcat vechi de evenimente (intrări făcute /
      șterse / mutate, reverificare, lot încheiat). Indexul se deschide și fără limbaje care
      declară (doar căile). „Reindex Project” e comanda modulului și publică
      `ReindexRequested`, ca și „Clear and Rebuild Caches”; după orice construire, taburile
      deschise își pun din nou declarațiile din textul lor (nesalvat).
      Reparat la cererea ta (scalabil): un folder schimbat nu mai cere „tot proiectul” —
      firul cozii îl parcurge și reindexează doar fișierele din el; doar un folder dispărut
      care avea fișiere indexate cere revederea întregului proiect; un `Makefile` nu mai
      cere nimic. Găsit și reparat: magistrala de evenimente e comună ferestrelor —
      evenimentele cu rădăcină (reverificare, reindexare) și cele pe căi sunt filtrate pe
      proiect (un lot al altui proiect poate întârzia puțin coada, niciodată nu pierde).
- [x] **15f. Evenimentele pe proiect.** Fiecare proiect are magistrala lui
      (`ProjectData.events`, nouă la deschidere, dispărută cu proiectul și cu abonările ei);
      cea comună ferestrelor a dispărut din aplicație. Toate cele 27 de locuri (publicări,
      abonări, serviciile modulelor, panourile, indexul, taburile) folosesc magistrala
      proiectului. Filtrele pe rădăcină din 15e au dispărut: separarea vine din construcție.
      Teste: un modul atașat aude doar proiectul lui, abonările proiectului vechi se termină
      la deschiderea altuia; o lipire lungă într-un proiect nu ține coada altuia. Regula e
      în `CLAUDE.md`.
- [x] **15g. Curățenia.** Indexer-ul nu mai are watcher (`notify` scos), debounce, pauză
      sau `Options`: un singur `start`, iar ce s-a schimbat îi spune coada modulului
      (`changed`, `refresh`, `rebuild`); testul pauzei înlocuit cu „o reîmprospătare găsește
      ce s-a schimbat și parsează doar acel fișier”. Meniul arborelui din aplicație folosește
      id-urile comenzilor modulului (`project_tree::operations`), ramurile moarte de iconițe
      au dispărut. Un test al modulului deschide indexul unui proiect și verifică tot drumul:
      declarațiile, urmărirea prin evenimente, căile, raportul pentru Task Manager.

## Lista de verificare (nimic nu se pierde)

- [x] Indexul persistent (`.lumen/index`), refolosit la pornire (amprente mărime + mtime, apoi
      hash); salvat la 10 s când s-a schimbat.
- [x] Fișierele peste 2 MB și folderele sărite (`skipped_dir`) rămân afară.
- [x] Taburile deschise: simbolurile la fiecare editare (pe firul `symbols`), Go to
      Declaration sincron când e nevoie (`refresh_symbols_now`).
- [x] Refactorizarea: numele nou găsit imediat după aplicare / anulare.
- [x] Căutarea de fișiere: „se indexează” la prima construire, rezultatele vechi servite cât
      se reface indexul.
- [x] Căutarea în proiect: lexical + semantic (cu modelul, când e pornit), refuzată fără index.
- [x] Bara de jos: „Indexing…” cât lucrează; Task Manager: firele + memoria.
- [x] Clear and Rebuild Caches, Reindex, eliberarea după 10 min fără folosire.
- [x] Alt proiect: nimic din cel vechi (abonări, fire, index) nu rămâne.
- [x] Windows: clippy, căile cu `\`, watcher-ul.
  Dovezi: `tests/unit/workbench/intelligence.rs` (salvare, disc, redenumire, ștergere,
  lipire = cel mult 3 reindexări, Clear and Rebuild, alt proiect), `renames_a_method_
  everywhere_and_undoes_it` (refactorizarea, sub 500 ms; fișierele deschise din taburi),
  `a_place_typed_after_a_file_name` și testele panoului (căutarea de fișiere, „se
  indexează”), `indexing/tests/unit/{feed,paths,lib}.rs` (coada, folderele, proiectele
  separate, raportul), `infra/indexer/tests/unit` (cache, amprente, reîmprospătare);
  clippy pentru Windows trece. Ce nu se vede într-un snapshot (rândurile Task Manager-ului,
  „Indexing…” cât lucrează — colectorul și indexarea nu apucă într-un singur cadru) e
  verificat prin teste.

## De hotărât înainte de cod

1. **Refactorizarea și „imediat”.** Azi reindexarea după o refactorizare e sincronă pe firul
   UI ca Go to Declaration să găsească numele nou chiar în cadrul următor. Propun: modulul
   tratează `RefactoringApplied` **fără debounce** (bandă prioritară), iar fișierele deschise
   rămân actualizate sincron de taburi (cum sunt deja). Diferența posibilă: câteva zeci de ms
   pentru fișierele închise.
2. **`BulkChange` vs. debounce simplu.** Un lot explicit (început / sfârșit) e mai sigur decât
   „aștept liniștea” pentru o lipire lungă; propun ambele: lotul ține coada, debounce-ul
   comasează restul.
3. **Indexul de căi în același modul** cu simbolurile (15e) sau un modul separat? Propun
   același modul, două indexuri, o singură coadă.
