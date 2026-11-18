# 74 — Membrii dependențelor citiți la cerere

**Stare (2026-11-11): revizuit (comprimate în RAM); pe branch-ul `index-members-on-demand`; toți pașii făcuți (74a–74g); de unit în `main` când decide utilizatorul.**

Cerut 2026-11-10 („da, fă totul să fie super bine, cu planuri și tot ce ai nevoie”; „poate
să faci branch separat”). Urmează planul 73 (memoria indexului), unde s-a măsurat pe o copie
Laravel (8.251 de fișiere cu simboluri, 8.189 în `vendor`):

- indexul de simboluri ~22,5 MB din 30,6 MB ai proiectului indexat, aproape tot din `vendor`;
- **82% din simboluri sunt membrii claselor** (54.817 din 67.169): metode, proprietăți,
  constante, folosite doar la `->` / `::` pe o clasă, la moștenire, la verificări;
- la utilizator (~70.000 de fișiere PHP în 3 proiecte) indexul are ~250 MB.

## Ce avem (verificat în cod)

- `SymbolIndex` (domain/intelligence) ține fiecare fișier întreg (`Arc<FileSymbols>`) și trei
  tabele: după FQN (tipuri, funcții), membrii după clasă, după nume (toate).
- Membrii sunt ceruți prin API-ul indexului în puține locuri: `members_of`,
  `declared_members`, `all_members` (engine, PHP infer/checks, rename, inlay, graph,
  refactorings), dar mulți cititori iau fișierul întreg cu `index.file(path)` (inferența PHP
  pentru `use`-urile și namespace-ul unui membru, hover, diagrame, rename).
- **Completarea rulează sincron pe firul UI** (`Workbench::update_completion` →
  `code_intelligence::complete` sub lock-ul de citire al indexului). Hover-ul și definițiile
  la fel. Verificările (metode necunoscute, argumente) rulează pe firul simbolurilor.
- Fișierele din biblioteci sunt deja „văzute de proiect”: `keep_public` scoate privatele
  (`PathMark::Library` sau `kernel::in_dependencies_or_git`).
- Cache-ul de pe disc (`<project dir>/index/symbols.bin`, `infra/indexer/src/codec.rs`,
  formatul 9) e **un singur blob**, decodat întreg la pornire (`decode` → `Vec<CachedFile>`):
  nu se poate citi un singur fișier din el.

## Reguli

- **Ce vede utilizatorul rămâne:** completarea, hover-ul, definițiile, verificările dau
  aceleași rezultate. Nicio verificare falsă („metodă necunoscută”) cât un membru încă nu e
  citit.
- **Nimic pe firul UI:** discul se citește doar în fundal. Firul UI ia doar ce e deja în
  memorie; ce lipsește se cere, iar rezultatul vine ca la lista târzie a unui server (se
  alătură listei arătate, cu generația lui).
- Doar fișierele de bibliotecă (aceeași regulă ca `keep_public`) sunt ținute „subțiri”;
  fișierele proiectului rămân întregi.
- Ce e citit stă într-un cache cu limită (`kernel::cache`, LRU pe clase), golit de „Clear
  and Rebuild Caches” și înlocuit cu proiectul (`ProjectData`).
- Un commit pe pas, `./scripts/check.sh` verde, măsurat înainte / după cu
  `project_index_memory` (cifrele în commit și aici).

## Revizuit: comprimate în RAM, nu citite de pe disc (2026-11-10)

Cerut de utilizator: „ia în calcul și compresie de memorie RAM”. Măsurat pe copia Laravel:

| simbolurile, în RAM | |
|---|---|
| azi (toate decodate) | ~24 MB (22,5 + rotunjirea alocatorului) |
| forma codată a cache-ului (fără pointeri, fără alocări mici) | 11,6 MB |
| comprimată pe fișier (deflate rapid, blocuri de ~1,4 KB) | **~35% din cea codată** |
| comprimată întreagă (doar ca reper) | 22% |

Un bloc mic se decomprimă și se decodează în zeci de microsecunde: membrii unei clase se pot
decoda **sincron, chiar pe firul UI**, când sunt ceruți. Dispar citirea de pe disc,
așteptarea, lista „târzie” și citirea din timp (vechii 74c, 74d) — mai simplu, fără riscul
unei completări incomplete, cu un câștig apropiat (~24 → ~8,5 MB estimat aici).

Încă un lucru găsit: **worker-ul indexerului ține și el fiecare fișier întreg** (`CachedFile`,
ca să rescrie cache-ul). Dacă doar indexul ar ține fișierele subțiri, memoria n-ar scădea:
amândoi trebuie să țină același bloc comprimat.

## Pași (revizuiți)

1. **74a. Cache-ul de pe disc pe blocuri** (formatul 10): fiecare fișier un bloc cu lungimea
   înainte, poziția lui cunoscută (`Block`), un bloc citibil singur. *Făcut, mai jos.*
2. **74b. Blocul comprimat** (formatul 11): blocul unui fișier e forma lui codată, comprimată
   (`miniz_oxide`, deja în proiect, Rust pur). Același bloc e și pe disc, și în RAM.
   **Făcut.** Măsurat pe copia Laravel (`block_times`): cache-ul pe disc 11,6 → 4,2 MB;
   citit întreg la pornire 69 ms; un bloc singur: mediana 6,5 µs, media 8,5 µs, cel mai rău
   0,45 ms (un fișier de 37 KB cu 2.474 de simboluri) — decodat la prima cerere, fără să se
   simtă, chiar pe firul UI.
3. **74c. Fișierele de bibliotecă ținute comprimate.** În `SymbolIndex`: tipurile și funcțiile
   lor decodate (completarea numelor, definițiile, `top_level` nu decodează nimic), restul
   în blocul comprimat, decodat o dată la prima cerere (`OnceLock` pe fișier: referințele
   date rămân valabile), întors de `file(path)` și de căutările membrilor ca azi. Worker-ul
   ține blocul (nu simbolurile): rescrierea cache-ului îl copiază ca atare. Fișierele
   proiectului rămân decodate.
   **74c făcut** (74c1 indexul, 74c2 indexerul). Măsurat pe copia Laravel, pornire din cache:
   proiectul indexat 30,6 → 26,2 MB (−14%; −22% față de 33,7 MB la începutul planurilor
   73–74); indexul de simboluri 11,6 MB măsurat (12,7 estimat: fișierele 9,1 — clasele
   decodate, blocurile ~3,5, numele membrilor ~0,6 —, tabelele 3,7). Mai puțin decât ținta
   (~15 MB pe tot proiectul): tabelele țin încă o intrare pentru fiecare membru din
   biblioteci (de căutat după listele de nume ale fișierelor împachetate: ~2,5 MB), iar
   indexerul ține încă o dată blocurile.
   **Tabelele, mai mici** (după 74g): un fișier împachetat are în tabela membrilor o singură
   intrare pe clasă (`ALL_MEMBERS`: membrii ei, din fișierul decodat la cerere), nu una pe
   membru; în tabela după nume, doar tipurile și funcțiile lui — `by_name` îi găsește
   membrii după listele de nume. Măsurat: proiectul indexat 26,2 → 23,6 MB; tabelele 3,7 →
   1,2 MB (după nume 2,1 → 0,5, membrii 1,1 → 0,2); indexul de simboluri 11,6 → 9,1 MB.
   **Față de început (33,7 MB): −30%.**
4. **74d. Ce s-a decodat se poate da înapoi**: „Clear and Rebuild Caches” și odihna
   proiectului golesc formele decodate ale bibliotecilor (rămân blocurile).
5. **74g. Bibliotecile recunoscute în orice proiect, și arătate** (cerut: „nu te raporta doar
   la `vendor`: pot deschide un proiect Node sau altele; trebuie identificate și marcate”).
   Azi o cale e bibliotecă dacă trece printr-un folder de dependențe declarat de un modul
   (`ProjectDirs.dependencies`: PHP `vendor`, JavaScript `node_modules`) sau e într-un folder
   marcat de utilizator „Library”. De făcut:
   - **o singură decizie** „e bibliotecă?” (marcajul utilizatorului întâi — „Normal” o
     anulează —, apoi regulile modulelor, apoi ce s-a găsit în proiect), folosită de
     indexer, de compresie, de căutare, de verificări;
   - **fiecare modul își raportează dependențele** (cerut: „fiecare modul e bine să-și
     raporteze dependințele”): numele fixe, ca azi (`ProjectDirs.dependencies`), și, unde
     managerul le poate muta, cum le găsește — o contribuție care citește, în fundal, la
     deschiderea proiectului, fișierul managerului (PHP: `config.vendor-dir` din
     `composer.json`). Aplicația doar adună ce spun modulele, nu știe de niciun manager;
     fără modulul unui limbaj, dependențele lui nu mai sunt recunoscute, iar un modul nou
     își aduce folderele lui. De adăugat la modulele pe care le avem: JavaScript
     (`bower_components`, `jspm_packages`, `.yarn/unplugged`), Lua (`lua_modules`);
   - **arătate în arbore** ca un folder marcat „Library” (aceeași culoare, eticheta
     „bibliotecă (automat)”, în tooltip de ce: „vendor-dir din composer.json”); meniul de
     marcaje le poate schimba, ca azi;
   - teste: un proiect Node (doar `node_modules`), unul PHP cu `vendor-dir` propriu, un
     folder marcat „Normal” sub `vendor`.
   **74g făcut.** `kernel::ProjectDirs.found_by` (un `DependencyFinder` pe managerul unui
   modul: fișierul lui și o funcție pură de la textul lui la foldere), `kernel::paths::
   set_found` / `found` / `dependency_at`; `in_dependencies` știe și de folderele găsite (toți
   cei care întreabă: căutarea, verificările PHP, ordinea completării, rename-ul). Indexerul
   citește fișierele managerilor la fiecare build și când unul se schimbă (dacă folderele
   s-au mutat: build din nou); marcajul „Normal” scoate un folder din biblioteci. PHP:
   `config.vendor-dir` din `composer.json`; JavaScript: `bower_components`, `jspm_packages`;
   Lua: `lua_modules`. Arborele arată folderele găsite ca „Library”, cu eticheta „bibliotecă
   (automat)”. Teste: kernel, PHP, indexer, arbore.
6. **74f. Piesa reutilizabilă** (cerut: „după să facem și pe la alte module, dacă iese
   bine”): ce ține ceva comprimat și îl decodează o dată, la prima cerere (`Packed<T>`:
   octeții, `OnceLock` pe forma decodată, golit la purjare), în `kernel::cache`, cu codecul
   dat de cel care îl folosește. Apoi, pe rând, măsurat întâi: istoria terminalului (~18 MB
   pe un terminal plin, celule de 24 B), alte cache-uri reci.
   **74f făcut.** `kernel::cache::Packed<T>` (`new`, `bytes`, `get` — decodat o dată și
   păstrat —, `once` — decodat fără să fie păstrat, pentru cine parcurge tot —, `unpacked`)
   și `kernel::cache::compress` / `decompress` (deflate rapid, `miniz_oxide` mutat în
   kernel). Indexul (`SymbolIndex`) și codecul indexerului le folosesc; o decodare eșuată nu
   se păstrează (se încearcă din nou). Teste în kernel.
   **74d, acoperit fără cod nou:** „Clear and Rebuild Caches” cere reindexarea
   (`ReindexRequested`), iar proiectul nou / odihnit e un index nou: formele decodate se duc
   cu el. Clasele decodate cât se lucrează rămân până la următorul build (puține: doar cele
   cerute).
7. **74e. Măsurat și terminat**: `project_index_memory` înainte / după; teste că
   completarea, hover-ul, verificările, rename-ul dau aceleași rezultate pe o clasă din
   `vendor`; timpul de decodare al unei clase (ținta: sub 0,1 ms); cifrele în TODO și 73.
   **74e făcut.** Testul `a_library_kept_packed_answers_as_when_kept_whole` (PHP): același
   proiect cu bibliotecile ținute întregi și împachetate dă aceleași răspunsuri la
   completare, hover, definiție, rename și verificări — membri proprii, moșteniți, dintr-un
   trait, ai unui enum, `@property` / `@method`, o metodă necunoscută pe o clasă închisă,
   trimiterea statică a Eloquent spre `Builder` și un mixin.
   Cifrele finale, copia Laravel (`project_index_memory`, pornire din cache): **proiectul
   indexat 33,7 → 23,6 MB (−30%)**; indexul de simboluri ~22,5 → 9,1 MB; cache-ul de pe disc
   11,6 → 4,2 MB. Decodarea unui fișier: mediana 6,5 µs, media 8,5 µs; **cel mai rău
   0,45 ms**, peste ținta de 0,1 ms, dar o singură dată pe fișier (un fișier de 37 KB cu
   2.474 de simboluri, de neobservat la tastare): decodarea pe clasă nu se justifică.

## Riscuri

- **Decodarea pe firul UI:** o clasă mare din `vendor` (sute de metode) într-un bloc de zeci
  de KB: de măsurat (74e); peste țintă, decodarea pe clasă, nu pe fișier.
- **Cititorii care iau `index.file(path)`** pentru un fișier de bibliotecă: primesc fișierul
  întreg ca azi (decodat la prima cerere); cei care parcurg toate fișierele (`files()`)
  ar decoda tot — trecuți pe tipuri și funcții acolo unde nu le trebuie membrii, câte un
  test.
- **Cache-ul de pe disc schimbat de altă fereastră / reindexare:** blocul din RAM e al
  indexului, nu citit de pe disc: nu depinde de fișier.
