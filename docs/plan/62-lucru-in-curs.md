# Plan 62 — Întrebat înainte de închidere când încă rulează ceva

## Context
Cerere: la închiderea ferestrei, la Close Project (`a1e06621`, `Final::CloseProject`) și la
ieșirea din aplicație, dacă încă rulează ceva — un program pornit în terminal, un upload /
sync remote, o interogare sau un export de date, descărcarea unui server de limbaj, un Git
push / pull / fetch, o operație lungă pe fișiere — utilizatorul e întrebat înainte. Totul
printr-un API generic, pe care orice modul îl folosește; nicio verificare scrisă în app.

## Ce există azi
- **Task Manager-ul** știe deja ce rulează: fiecare modul contribuie un `ResourceReporter`
  (`workbench_model::resources`) ale cărui părți (`ResourceUse`) au operațiile în curs
  (`Activities`: upload, download, sync, export, copiere, ștergere, index, istoric — cu
  progres și „Stop” prin `act`), procesele pornite și acțiunile pe care le oferă
  (`PartAction`: închide sesiunea terminalului, oprește operația).
- **Terminalul** știe cine are terminalul (`pty::foreground`, 54b): shell-ul la prompt sau
  un program pornit din el (Unix: `tcgetpgrp`; Windows: cel mai tânăr copil al shell-ului).
- **Închiderea** (`app/closing.rs`): fișierele nesalvate întrebate pe rând (`Closing`),
  apoi `Final` (fereastra, ieșirea, proiectul, alt proiect în aceeași fereastră).

## Design
- **O singură sursă**: `ResourceReporter` primește `running()` — „ce rulează acum și s-ar
  opri la închidere”, sincron și ieftin (cerut doar la închidere, niciodată pe cadru). Implicit
  îl scoate din operațiile în curs ale propriilor părți (`running::of_parts`), deci remote,
  exporturile, descărcările serverelor de limbaj, operațiile arborelui raportează fără cod
  nou; un modul îl suprascrie pentru ce nu e operație (programul din terminal, interogarea
  din consolă, Git).
- **`workbench_model::running`** (domeniu, pur): `RunningWork` (titlu, detaliu, modulul,
  proiectul, felul operației / iconița, progresul, de când) cu **impactul opririi**
  (`StopImpact`: `LosesWork` — se pierde ceva, `Interrupts` — se reia, `Background` — nu
  se întreabă) și **cum se oprește** (`StopVia`: aceeași acțiune ca în Task Manager, făcută
  de raportorul ei prin `act`; fără ea, lucrul nu poate fi oprit sigur și se termină singur).
  `CloseScope` spune ce se închide (proiectul ferestrei — plus ce e al aplicației doar dacă
  e ultima fereastră —, doar proiectul la Close Project, totul la ieșire); `to_ask` filtrează
  și ordonează, `grouped` grupează pe proiect și modul; `CloseAnswer` (`StopAndClose`,
  `Wait`, `Cancel`); `RunningPrompt` e ce primește modalul.
- **Modalul** e al unui modul (`running-work`, vederea `view-running-work`, deschisă cu
  `open_modal`): lista pe proiecte și module, cu iconițe și progres; butoanele „Oprește și
  închide”, „Așteaptă” (lista rămâne, se reîmprospătează la ~0,5 s și fereastra se închide
  singură când nu mai rămâne nimic; „Anulează” renunță), „Anulează”. Se închide doar cu
  ×, Esc sau butoanele (×/Esc = Anulează). Răspunsul ajunge în app ca
  `AppRequest::RunningAnswer`. Fără modul (dezactivat), app-ul cade pe dialogul simplu.
- **Ordinea**: întâi lucrul în curs, apoi fișierele nesalvate. Întrebarea despre lucrul în
  curs nu schimbă nimic până la răspuns: „Anulează” lasă fereastra exact cum era (taburile
  încă deschise). Invers, după ce fișierele au fost salvate / aruncate și taburile închise,
  un „Anulează” ar lăsa fereastra golită. „Așteaptă” → la final se întreabă tot de
  fișierele nesalvate (nu pot fi hotărâte singure).
- **Ieșirea cu mai multe ferestre**: fereastra care iese întreabă o dată de tot (toate
  proiectele + ce e al aplicației); celelalte întreabă apoi doar de fișierele lor.

## Raportori
- **terminal**: shell-urile cu un program în prim-plan (nu cele la prompt) — `LosesWork`,
  oprit prin închiderea sesiunii; numele programului (macOS `proc_name`, Linux
  `/proc/<pid>/comm`, Windows numele executabilului copilului).
- **remote**: upload / download / sync / ștergeri (implicitul, din `Activities`).
- **database-console**: interogările care rulează (`Runner::snapshot().running`), oprite cu
  `Runner::stop`; **database-explorer**: exporturile (implicitul).
- **git**: fetch (`Interrupts`), pull / push (`LosesWork`), fără oprire (un git oprit la
  jumătate lasă `index.lock`).
- **language-servers**: descărcările (implicitul; `Download` → `Interrupts`).
- Indexarea și istoricul local: `Background` (nu se întreabă).

## Interogările ca operații (62b)
- `ActivityKind::Query`, iar `Activity` primește `detail` (sursa, baza, consola) și `text`
  (textul întreg, în tooltip); `Activities::describe` / `discard`.
- **Consola**: fiecare rulare (și Explain / profilul) e o operație în `runner::QUERIES`:
  prima linie a textului, sursa · baza · consola, durata live, rândurile aduse la final;
  „Stop” e chiar `Runner::stop` (anularea driverului: cererea de anulare PG, `KILL QUERY`
  MySQL, întreruperea SQLite). Terminate / oprite / eșuate rămân printre cele recente cu
  rezultatul lor. Întrebarea de la închidere le vede prin implicitul `of_parts` — raportorul
  special al consolei a dispărut (o singură evidență).
- **Database Explorer**: ce așteaptă utilizatorul (citirea unei pagini, totalul, salvarea
  modificărilor, schimbarea structurii) e o operație a proiectului (`SessionWorker::track`),
  oprită ca anularea vederii; ce se termină bine sub o secundă nu se păstrează (derularea nu
  umple lista). Exporturile erau deja.
- **Proprietarii**: operațiile arborelui raportau `project.tree` (modulul e `project-tree`),
  deschiderea remote `remote.opening`: acum id-ul modulului; căutarea numelui modulului e și
  tolerantă (`same_module`: `.`, `-`, `_` la fel).
- Git rămâne „nu poate fi oprit” (nedecis).

## Pași
1. Domeniul: `running.rs` (+ `ResourceReporter::running`), teste pure.
2. Raportorii în module (terminal + `pty`, consolă, git), teste.
3. Modulul `running-work` (vederea modalului), localizări ro/en, teste.
4. App: `closing.rs` întreabă înainte de `close_all_tabs` (fereastră, Close Project, alt
   proiect aici, ieșire), răspunsurile, așteptarea; teste (cele trei butoane, ieșirea cu
   două ferestre, Close Project doar cu ale proiectului).

## Stare
- [x] Plan
- [x] Domeniu și raportori
- [x] Modal și app
- [x] Interogările ca operații (62b)
