# 18. Task Manager: discul, operațiile și toate proiectele

Cererea (6c din planul 11): în Task Manager și **mișcările pe disc** (citiri / scrieri), **operațiile**
(transferuri, lipiri, indexare) **pe proiect**, și o **vedere globală** cu toate proiectele
deschise (toate ferestrele), grupate și pliabile, cu filtru și sortare — totul optim, raportat
de module (ele știu ce fac), adunat și desenat de modulul `task-manager`.

## Azi (inventar, 2026-10-22)

- Contractul: `ResourceUse` (nume, proprietar, memorie, GPU, detaliu, fire, grup, trafic de
  rețea) raportat de aplicație (`task_data.rs`: părțile ei, pe fereastră) și de module
  (`ResourceReporter`: indexarea, remote, terminalul, Task Manager-ul însuși). Rețeaua se
  numără cu `Traffic` (contori atomici, citiți de colector).
- CPU-ul unei părți = CPU-ul firelor ei, **după nume**. Firele au aceleași nume la toate
  proiectele (`indexer`, `remote-upload`…): cu două proiecte deschise, fiecare rând de
  proiect arată **CPU-ul amândurora** — nu se poate împărți pe proiect după nume.
- Discul: **nimic** măsurat.
- Vederea: părțile ferestrei curente plus ce raportează modulele (care știu de toate
  proiectele: indexarea, remote) — un amestec, fără proiect pe rând; grupele App / Servicii /
  Module, pliabile; filtru, sortare, grafice pe ultimul minut.

## Identitatea proiectului: UUID v4, nu numele / calea

Azi proiectele se recunosc după rădăcină (calea) — sesiunile Remote, indexurile deschise,
căutările recente — iar un rând din Task Manager după nume. Amândouă sunt eronate: două
proiecte pot avea același nume, un folder poate fi redenumit / mutat, iar aceeași cale poate
fi închisă și redeschisă (alt proiect, altă stare).

- **`ProjectId(Uuid)`** (v4, `kernel`), dat **la fiecare deschidere** a proiectului, ținut în
  `ProjectData` și dat modulelor (`ProjectContext.id`, `ProjectView.id`, `host.project_id()`):
  se naște cu proiectul deschis și dispare cu el, ca magistrala lui de evenimente. Nu e scris
  nicăieri: unul salvat în proiect (`.lumen/`) ar ajunge în git (două copii deschise = același
  id), unul salvat în datele aplicației ar fi tot după cale.
- Tot ce ține starea pe proiect (sesiunile Remote, indexurile deschise, căutările recente,
  operațiile, contorii Task Manager-ului) o ține **după id**; calea rămâne doar ce e: unde e
  proiectul pe disc (și ce se arată).
- Task Manager-ul grupează după id; numele (și calea, în tooltip) doar se afișează.

## Ținta

```
domain (workbench-model::resources)
  ResourceUse + project (ProjectId, None = al aplicației; numele doar afișat), read / written (octeți, crescători),
                cpu (timp de CPU măsurat de modul, crescător), activities
  DiskIo        contori atomici: octeți citiți / scriși, fișiere (ca Traffic)
  CpuMeter      timpul de CPU al lucrului făcut pentru o parte (măsurat pe firul care lucrează:
                kernel::cpu::thread_time — macOS clock_gettime(THREAD_CPUTIME), Windows
                GetThreadTimes), adunat atomic
  Activity      o operație în curs: id (UUID v4), proiect, fel (urcare, descărcare, copiere,
                mutare, ștergere, indexare, istoric), ce (fișierul / folderul), progres
                (făcut / total, octeți sau fișiere), început; și ultimele terminate
module         își numără discul / CPU-ul / operațiile pe proiect (știu ce fac)
app            registrul ferestrelor (fiecare își înscrie sursa; slab, se curăță singur)
task-manager   colectorul (pe firul lui, doar cât e deschis) + vederea
```

## Vederea

- Sus, alegerea: **Proiectul acesta** / **Toate proiectele** (ținută minte).
- „Toate”: un grup pliabil pe proiect (numele, culoarea / iconița proiectului, totalurile lui:
  CPU, memorie, disc, rețea), în el părțile lui; la sfârșit **Comune** (interfața, randarea,
  modulele, ce nu ține de un proiect). „Acesta”: ca azi, plus coloanele noi.
- Coloane noi: **Disc** (citire / scriere pe secundă, cu graficul pe ultimul minut, ca rețeaua);
  CPU-ul pe proiect din `CpuMeter` acolo unde modulul îl măsoară (firele rămân pentru restul).
- O secțiune **Operații**: ce rulează acum (progres, viteză, cât a mai rămas), pe proiect, și
  ultimele terminate (estompate, cu durata și ce s-a mutat); clic pe una → ce o deține
  (jurnalul Remote, fișierul…).
- Filtrul și sortarea merg pe tot (și pe operații); totul responsive (panoul îngust).
- Antetul: memoria și discul procesului cum le vede sistemul (`proc_pid_rusage` /
  `GetProcessIoCounters`), pentru comparație cu suma părților.

## Optim

- Contori atomici `Relaxed` în module (fără blocări pe drumul fierbinte); măsurarea CPU-ului e
  două citiri de ceas pe lucru (nu pe octet).
- Colectorul citește doar cât Task Manager-ul e deschis (în orice fereastră), o dată pe
  secundă, pe firul lui; nimic pe UI thread. Operațiile terminate: cel mult 50 pe proiect,
  apoi se uită.
- Registrul ferestrelor ține referințe slabe: o fereastră închisă dispare singură; datele unui
  proiect nu ajung în altul (cheia e `ProjectId`).

## Pașii (un commit fiecare; `check.sh`; „go” între ei)

- [x] **18a. Identitatea.** `ProjectId` (UUID v4) la fiecare deschidere, în `ProjectData`,
      `ProjectContext`, `ProjectView`, `host.project_id()`; stările ținute azi după cale
      (sesiunile Remote, indexurile deschise, căutările recente) trec pe id; teste (două
      proiecte cu același nume, același folder redeschis).
      *Făcut:* `kernel::ProjectId` (și în `IndexContext`, `ContextProvider::add_facts`);
      sesiunile Remote, vederile lor (jurnal, „De urcat”, revizuire), comenzile și dialogurile
      lor — după id; căutările recente — după id, uitate când proiectul se închide (cu indexul
      lui). Rămâne după cale doar ce e o întrebare despre disc („în ce proiect e fișierul
      ăsta”). Reparat pe drum: panoul proiectului primea datele noi doar dacă se schimba
      calea — același folder redeschis îl lăsa cu magistrala proiectului vechi.
- [x] **18a′. Contractul.** `project`, `read` / `written`, `cpu`, `activities` pe `ResourceUse`;
      `DiskIo`, `CpuMeter` (cu `kernel::cpu::thread_time` pe macOS / Windows / Linux),
      `Activity`; teste.
      *Făcut:* `kernel::cpu::thread_time` (macOS / Linux `clock_gettime`, Windows
      `GetThreadTimes`, fără dependențe), `kernel::id::new_uuid`; în `resources`: `project`,
      `read` / `written`, `cpu`, `activities` pe `ResourceUse`; `DiskIo`, `CpuMeter`
      (`measure`), `Activities` (start / advance / end, ultimele 50 terminate pe proiect,
      `forget` la închidere), `Activity` (UUID v4, fel, ce, unitate, progres, sfârșit);
      `ResourceSnapshot.projects` (`OpenProject`: id, nume, rădăcină) și `disk`.
- [x] **18b. Modulele raportează.** Indexarea (fișierele citite, CPU-ul pe proiect, progresul
      ca operație), remote (descărcările scriu, urcările citesc; fiecare transfer o operație),
      istoricul local (versiunile scrise), aplicația (salvările, lipirile / copierile /
      ștergerile cu progres — azi doar în bara de stare — ca operații, pe proiect).
      *Făcut:* **indexarea** — indexatorul (`infra`) numără fișierele citite, cache-ul scris,
      CPU-ul firului lui (fiecare proiect are firul lui: CPU-ul e al proiectului), construcțiile
      și fișierele lor; modulul face din ele partea proiectului și operația „Indexare” (în
      fișiere). **Remote** — urcările citesc, descărcările scriu (în același loc cu rețeaua);
      un transfer e o operație în octeți, o revizuire în fișiere (și în paralel: legate prin
      identitatea operației, nu prin fir), oprită → „oprită”. **Istoricul local** — versiunile
      scrise (comprimate, numărate de magazia lui), fișierele salvate citite, CPU-ul firului
      lui (fără operații: câte una la fiecare salvare ar umple lista). **Arborele** — lipirile
      (copiere / mutare, în fișiere, octeții citiți și scriși) și ștergerile ca operații ale
      proiectului; înregistrarea proiectului ținută de un abonament al lui (dispare cu el).
      **Aplicația** — salvările ca scrieri ale proiectului (partea „Fișiere deschise”); firele
      `paste`, `trash`, `local-history` scoase din partea ei (le raportează modulele: CPU-ul
      nu se numără de două ori).
- [x] **18c. Toate ferestrele.** Registrul surselor, alegerea Acesta / Toate, gruparea pe
      proiect (pliabilă, cu totaluri) și „Comune”; antetul cu discul procesului.
      *Făcut:* fiecare fereastră își înscrie sursa (slab) și proiectul ei (`OpenProject`: id,
      nume, rădăcină); „Proiectul acesta” = părțile proiectului ferestrei (după id) și cele
      comune; „Toate” (`snapshot_all`) = fiecare fereastră, părțile comune o singură dată,
      GPU-ul ferestrelor adunat; cât e cerută, fiecare fereastră își ține datele proaspete (la
      evenimentele oricărei ferestre, cel mult o dată pe secundă; 3 s după ultima cerere —
      apoi nimic). Colectorul: cheia rândului cu id-ul proiectului (două proiecte cu același
      nume nu se amestecă, nici graficele lor), CPU-ul măsurat de module în locul firelor
      comune (un fir comun e „revendicat”, nu adunat primului proiect), ritmurile de disc pe
      parte și ale procesului (`process_stats::disk_io`: macOS `proc_pid_rusage`, Linux
      `/proc/self/io`, Windows `GetProcessIoCounters`). Vederea: comutatorul „Proiectul acesta
      / Toate proiectele” (lângă titlu, sau pe rândul lui când panoul e îngust; ținut minte
      cât rulează aplicația), grupurile la „Toate”: câte unul pe proiect (după id), „Comune”,
      „Module” (pliat).
- [x] **18d. Vederea.** Coloana Disc cu graficul, secțiunea Operații (în curs + terminate),
      filtrul și sortarea peste tot, responsive; snapshot-uri (`EDITOR_SNAPSHOT_TASKS=all`).
      *Făcut:* coloana **Disc** (citire + scriere pe secundă; pleacă prima când nu e loc) și
      cardul ei cu graficul pe ultimul minut (cinci carduri: pe un rând când e loc, altfel
      3 / 2 / 1 pe rând, ultimul singur pe rând ia tot rândul). Secțiunea **Operații**, în
      capul listei, pliabilă: cele în curs (iconița felului, pe ce, procentul, viteza, cât a
      mai rămas după ritmul de până acum, bara de progres), apoi ultimele 10 terminate
      (estompate: „gata în 2 s”, „eșuat” cu motivul, „oprit”), cu proiectul lor la „Toate”;
      filtrul le găsește, sortarea după nume le ordonează. *Diferit de plan:* un clic pe o
      operație selectează partea ei (graficele o urmează), nu deschide jurnalul Remote — o
      operație nu știe (încă) ce vedere o arată. Captura verificată (panoul îngust).
- [x] **18e. Lista de verificare** și Windows. (Găsit și reparat: un panou lateral ascuns e
      păstrat deoparte — Task Manager-ul ascuns își ținea colectorul pornit și fereastra
      „urmărită”, deja dinainte de planul 18. Acum panourile au `parked()`: el își oprește
      colectorul și ține sursa slab; firul colectorului o ține și el slab, deci se oprește
      imediat ce panoul o lasă.)

## De hotărât

0. ~~Identitatea~~ — **hotărât (2026-10-22): un UUID v4 individual la fiecare deschidere**
   (nimic după nume / cale: fără coliziuni între proiectele deschise).

1. Implicit **Proiectul acesta** (ca azi) sau **Toate proiectele**? Propun: proiectul acesta,
   cu alegerea ținută minte.
2. CPU-ul pe proiect **măsurat de module** (`CpuMeter`) acolo unde firele sunt comune — propun
   da: e singurul fel corect; restul rămâne după fire.
3. Operațiile terminate: câte și cât timp? Propun ultimele 50 pe proiect, cât rulează aplicația.
4. Discul procesului din sistem în antet (pentru comparație) — propun da.

## Lista de verificare (nimic nu se pierde)

- [x] Ce arată azi (CPU, memorie, GPU, rețea, graficele, grupele, filtrul, sortarea, textele
      lungi pe rânduri) arată la fel în „Proiectul acesta” (plus discul și operațiile;
      capturile panoului verificate; testele vechi ale vederii trec).
- [x] Bara de stare (CPU / memorie / GPU, clic → Task Manager) neschimbată (neatinsă).
- [x] Nimic nu rulează cât Task Manager-ul e închis sau ascuns (în toate ferestrele): testul
      `a_hidden_task_manager_keeps_nothing_running`; „Toate” expiră la 3 s după ultima cerere.
      În module rămân doar contorii atomici (citiri / scrieri / CPU pe lucrare).
- [x] Windows: contorii (`GetThreadTimes`, `GetProcessIoCounters`) compilați și verificați cu
      clippy pentru `x86_64-pc-windows-gnu`; rularea pe Windows — de încercat de tine.
