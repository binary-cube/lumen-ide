# Plan 63 — Proiecte remote: sincronizare automată pornită / oprită, urcare la alegere

## Context (ce există)
- `crates/modules/project/remote`: sesiunea (`session.rs`, `session/service.rs`) urcă la salvare;
  `outgoing.rs` citește în fundal ce e de urcat, cu motivul (`Why::Waiting` — n-a putut pleca,
  `Changed` — modificat aici, `New` — făcut aici), pentru numărul de pe butonul Remote și tab-ul
  „De urcat” (`pending_view.rs`: „Upload all”, „Upload” pe rând); conflictele sunt deja
  detectate (`Standing::Diverged` → `Mark::Conflict`, `ask_conflict`: compară / al meu / al
  serverului); transferurile au paralelism și reîncercări (setări), pool-ul prudent la FTP
  (`14505bce`); setările conexiunii se editează (`16f6a514`); „ce rulează” la închidere
  (`ce516b78`).

## Ce facem
- **63a — comutatorul și lista.**
  - Setare per proiect remote „Urcă automat pe server la salvare” (implicit pornit, ca azi), în
    formularul conexiunii și ca buton în antetul ferestrei Remote (comutator vizibil, cu starea).
  - **Oprit:** nimic nu pleacă singur. Salvările, fișierele noi, redenumirile și ștergerile se
    adună în „De urcat” (cu motive noi: `Renamed { from }`, `Deleted`), persistent (supraviețuiesc
    repornirii, în `.lumen/` al proiectului — date ale proiectului, nu ale aplicației).
  - **Pornit:** ca azi; lista ține doar ce n-a reușit.
- **63b — lista „De urcat”, ca un panou de modificări.**
  - Arbore (componenta comună de arbore cu căutare, ca la Git — `ui_components::tree`) sau listă,
    cu starea fiecărui fișier (A / M / D / R, colorat), numărul total, căutare.
  - Selecție multiplă; acțiuni: Urcă (selecția / folderul / tot), Arată diferența față de server
    (comparația existentă, citind fișierul de pe server în fundal), Renunță (readu de pe server),
    Exclude (nu mai urca acest fișier / aceste tipare).
  - Ștergerile urcate doar explicit, cu confirmare (numărul și lista).
  - Înainte de urcare, fișierele schimbate și pe server între timp → conflict. Un fișier urcat
    singur: dialogul existent. **În bloc (hotărârea utilizatorului, 2026-11-06):** întâi se
    verifică pe server toate, nu pleacă nimic; dacă sunt conflicte, se marchează în listă și
    utilizatorul alege: „Rezolvă întâi conflictele” (dialogul primului, celelalte din listă)
    sau „Urcă acum celelalte N”.
- **63c — în arborele proiectului și în editor.**
  - Semn pe fișierele și folderele care au ceva de urcat (decorare discretă, ca la Git; un folder
    poartă semnul dacă ceva de sub el are), click dreapta → Remote › Urcă / Diferență / Renunță
    (pentru fișier, selecție sau folder).
  - Tab-ul fișierului: o acțiune „Urcă” în bara de sus a editorului când fișierul are ceva de urcat
    și sincronizarea e oprită.
  - Badge-ul de pe butonul Remote (`ds::count_badge`) cu numărul; avertisment (galben) dacă sunt
    conflicte.
- Urcarea în bloc ca operație în Task Manager și în dialogul „ce rulează” (deja prin
  `ResourceReporter`); progres, anulare; erorile pe fișier rămân în listă cu motivul.

## Reguli
Modul remote (fără `ui-workbench`), domeniul `remote` pur pentru starea listei; teste pe
servere false / containere proprii `editor-test-*` (niciodată `remote-*-demo` ale
utilizatorului); ro / en; Windows.

## Stare
- [x] 63a
- [x] 63b
- [x] 63c
