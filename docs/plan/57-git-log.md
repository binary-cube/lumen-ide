# Plan 57 — Git: Log și Changes ca tab-uri fixe, Log-ul complet

## Context
Butonul Git (bara de jos, cu numărul de modificări) deschide azi doar „Changes”; „Git Log”
(`crates/modules/vcs/git/src/log_view*`: ramurile în stânga, lista cu graficul, detaliile în
dreapta) se deschide separat, ca tab care se poate închide. Cererea: butonul deschide Log-ul,
cu „Changes” ca tab lângă el, amândouă fixe (fără ×), și Log-ul să arate / meargă ca un Log de IDE complet.

## Ce facem
- **57a — tab-uri care nu se închid + Git-ul cu Log și Changes.**
  - Generic, în fereastra de jos: o vedere își poate declara `closable: false` (în contractul
    vederilor din `ui-components`, implicit `true`): fără ×, fără „Close” în meniul tab-ului,
    neatinsă de Close Others / Left / Right / All / Close Group și de click-ul din mijloc;
    se poate muta (drag, split) ca oricare. (`services.rs` are deja un `closable` pentru
    serviciile lui — de văzut dacă îl generalizăm pe acela.)
  - Git: la deschiderea ferestrei (butonul), primele două tab-uri sunt **Log**, apoi **Changes**,
    amândouă fixe; butonul arată Log-ul (dacă fereastra era închisă) sau ce era activ. Tab-urile
    de istoric ale unui fișier („Shop.php · History”) rămân închidibile, după ele. Ordinea și
    split-urile se păstrează ca la orice fereastră de jos (planul 56).
- **57b — bara de filtre a Log-ului**: „Text sau hash” (cu `.*` regex și `Cc`
  majuscule), Branch ▾ (ramuri locale / remote / tag-uri, căutare), User ▾ (autorii, „eu”),
  Date ▾ (ultimele 24 h / 7 / 30 de zile / interval), Paths ▾ (foldere / fișiere din proiect),
  sortare (după dată / topologic); filtrele se combină, se văd ca pastile active cu ×, citirea
  se face în fundal (`git log` cu argumentele lor, pe pagini, ca acum), lista arată „nimic găsit”
  cu ce filtre sunt active. Panoul de ramuri: câmp de filtrare, dublu-click pe o ramură = filtrul
  Branch, săgețile de incoming / outgoing (ca „main ↗1”) cu tooltip.
- **57c — detaliile**: în dreapta, sus fișierele commit-ului ca arbore (foldere
  restrânse, stare A / M / D / R colorată, numărul), dublu-click = diferența în editor, meniul
  de click dreapta ca în 52a; jos detaliile (hash, autor, data, mesajul, ramurile care conțin
  commit-ul); „Select commit to view changes” când nu e nimic ales. Mai multe commit-uri
  selectate (Shift / Cmd) → fișierele schimbate în total.

- **Lista fișierelor schimbate ca arbore, peste tot** (detaliile commit-ului din Log, istoricul
  unui fișier — 52a —, Changes): ca în panoul de proiect, nu ceva nou — aceleași rânduri
  (iconițele fișierelor după tip, folderele cu săgeată, liniile de indentare, hover / selecție),
  refolosite dintr-o componentă de arbore din `ui-components` (dacă arborele proiectului e doar
  în app / `ui-workbench`, partea lui de desenare și de navigare se mută în `ui-components` și
  e folosită de ambele, proiectul rămânând identic). În plus față de proiect:
  - folderele cu un singur copil comprimate pe un rând („FrontOffice/AI/Service/Chat”), cu
    numărul de fișiere („3 files”);
  - starea fiecărui fișier: litera A / M / D / R / U și numele colorat după ea (adăugat
    verde, modificat albastru, șters gri tăiat, redenumit cu numele vechi), numărul total
    deasupra („64 files changed”);
  - un câmp de căutare deasupra listei: filtrează după nume / cale (fără majuscule, potrivire
    pe bucăți ca la Go to File), păstrând folderele care au ceva găsit, cu textul găsit evidențiat;
  - comutator Arbore / Listă (lista de azi rămâne, pentru cine o preferă), ținut minte;
  - tastatură ca în proiect (← → strâng / deschid, ↵ deschide diferența); Expand / Collapse All.

## Reguli
Totul în modulul Git (ui-components, nu ui-workbench), fără nume de aplicație, ro / en,
Windows; citirile git în fundal; fără scrieri git (doar citiri); teste pe un repo temporar.

## Stare
- [x] 57a — `Viewer::closable` (implicit `true`): fără ×, fără Close, neatinse de Close Others / Left / Right / All; Close Group le mută în grupul vecin (singure, rămân). Butonul Git deschide Log, apoi Changes (fixe), istoricele după ele. (Click-ul din mijloc nu închide nimic în fereastra de jos: nu există.)
- [x] 57b — `LogFilter` (version-control: referințe, text / hash cu regex și majuscule, autori, interval, căi, ordine) aplicat de adaptorul git la parcurgere (gitoxide, pe pagini, în fundal); un filtru care scoate commit-uri desenează ce a găsit ca o singură linie. Bara: câmpul „Text sau hash” (se aplică după o pauză de tastare / ↵), pastilele Branch / User / Date / Paths / ordinea cu listele lor (căutare, ↑ ↓ ↵), × pe cele active, „nimic găsit” cu filtrele. Panoul de ramuri: câmp de filtrare, click = selecție, dublu-click = filtrul Branch, ↗ / ↙ față de upstream cu tooltip. Datele intervalului sunt în UTC.
- [x] 57c — `ui_components::tree`: `EntryLayout` (geometria rândurilor proiectului), `entry_icon`, `FileIcons` (cache-ul iconițelor), folosite de panoul proiectului (identic, verificat pe imagini) și de `files_tree` (Git): arborele fișierelor schimbate (foldere comprimate cu numărul, starea colorată, șters tăiat, redenumit cu numele vechi, căutare fuzzy cu evidențiere, Arbore / Listă ținut în dimensiunile proiectului, ← → ↵, Expand / Collapse All) în detaliile Log-ului (sus fișierele, jos commit-ul cu ramurile care îl conțin; „Select a commit…”; ⇧ / ⌘ = mai multe, fișierele împreună), în istoricul unui fișier și în Changes (meniul de click dreapta ca în 52a).

## După
- [x] Log și Changes (și istoricele fișierelor, după cale) se refac la pornire, în ordinea, grupurile și split-urile lor, cu tab-ul activ: vederile Git sunt `restorable`; sesiunea ține și numele după care modulul le găsește (dacă tab-ul a fost redenumit) și ce le trebuie ca să fie refăcute (`Viewer::memento`: calea istoricului); citesc din repository abia când se arată; fără repository, tab-ul pleacă.
- [x] Filtrul Date în ora locală (fusul utilizatorului, cu ora de vară): intervalul de la miezul nopții local al primei zile până la cel de după ultima; data commit-ului (detalii, adnotările blame) tot locală.
- [x] Separatorul dintre fișierele schimbate și commit, în detalii, se trage (mânerul cu trei puncte; dublu-click: înapoi la implicit), proporția ținută în dimensiunile proiectului.
- [x] Log și Changes se recunosc după un nume propriu, același în orice limbă (`git.log`, `git.changes`: `Viewer::panel_key`, `show_in_panel_keyed`), nu după titlul tab-ului: o sesiune salvată în altă limbă (sau veche, doar cu titlul) sau un tab redenumit nu mai aduce un al doilea Log.
- [x] Filtrul Date cu un selector de interval refolosibil (`ui_components::DateRangePicker`): presetări (Oricând, Azi, Ieri, Ultimele 7 / 30 de zile, Luna aceasta / trecută, Anul acesta), câmpurile De la → Până la în formatul limbii, două luni alăturate (una când e îngust), azi încercuit, intervalul colorat, viitorul estompat, tastatură, Renunță / Aplică; zilele în ora locală.
