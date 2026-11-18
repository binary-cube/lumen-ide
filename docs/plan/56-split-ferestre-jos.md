# Plan 56 — Split în ferestrele de jos (Terminal și celelalte cu tab-uri)

## Context
Editorul are grupuri (split): `crates/app/src/workbench/groups.rs` (`EditorGroups`: arborele
`split_tree::SplitTree`, separatorii cu mâner `Splitter`, tab-ul tras între grupuri și spre
margini, „Split Right / Down”, „Split and Move Right / Down”, unirea grupurilor, grupul curent).
Ferestrele de jos care deschid vederi ca tab-uri (Terminal „Local”, „Local (2)”, „+”; istoricul
Git; Remote „To upload”…) au o singură bandă de tab-uri și o singură vedere afișată. Cererea:
split la tab-urile lor, cu toate funcțiile de la editor.

## Ce facem
- **Același model ca la editor, refolosit, nu copiat**: arborele de split
  (`split_tree`) și separatorii (`Splitter`) sunt deja generali — fereastra de jos primește
  propriile grupuri (fiecare cu banda lui de tab-uri și vederea activă), în aceeași fereastră.
  Partea comună cu `EditorGroups` (arborele, separatorii, tragerea, ținta de drop, pastila
  trasă) se scoate într-o bucată refolosită de ambele (editorul rămâne identic).
- **Funcții**, ca la editor:
  - meniul tab-ului: Split Right, Split Down, Split and Move Right, Split and Move Down,
    plus Close, Close Others / Left / Right, Close All (pe grupul lui);
  - tragerea unui tab: în alt grup (între tab-uri, cu reordonare — planul de drag din
    `docs/plan/55`/commit-ul de reordonare), spre o margine a unei vederi (zona de drop
    evidențiată: stânga / dreapta / sus / jos → split acolo), afară din fereastră → nimic;
  - separatorii: trași cu mâner (trei puncte, ca la editor), dublu-click = egalează;
  - grupul curent: click / focus în vedere; `+` deschide un tab nou în grupul curent
    (la Terminal: un shell nou, în folderul grupului curent);
  - un grup golit dispare; ultimul tab închis lasă fereastra goală ca acum;
  - tastatură: aceleași comenzi ca la editor pentru grupuri (mută focusul între grupuri,
    închide grupul) acolo unde fereastra are focusul.
- **Generic**: o fac o dată, în banda / fereastra de jos, pentru toate ferestrele care deschid
  vederi ca tab-uri; modulele nu știu de split (doar își dau vederile).
- **Păstrat**: layout-ul (arborele, proporțiile, ce tab e în ce grup, ordinea) se salvează cu
  fereastra și revine la repornire; identitățile vederilor rămân UUID v4.
- **Terminal**: fiecare vedere e un shell independent; redimensionarea unui split anunță shell-ul
  doar la mărimea finală (ca în 55m); focusul tastaturii merge la vederea grupului curent.
- **Cost**: nimic în plus cu un singur grup; separatorii și ținta de drop doar cât se trage.

## Pași (câte un commit)
- **56a** — partea comună scoasă din `EditorGroups` (editorul identic, testele lui neschimbate).
- **56b** — grupurile în fereastra de jos: split din meniul tab-ului, separatori, grup curent,
  `+` în grupul curent, păstrarea layout-ului.
- **56c** — tragerea tab-urilor între grupuri și spre margini (zone de drop), tastatura.

## Stare
- [x] 56a — `workbench/splits.rs`: separatorii unui arbore de split (`SplitHandles`: aranjare, mâner, tragere, hover), pragul de tragere și zona de drop, folosite de grupurile editorului (identic) și, din 56b, de ferestrele de jos.
- [x] 56b — fereastra de jos are grupuri (`ToolWindow::groups`, arborele și `SplitHandles` din 56a): meniul tab-ului (Split Right / Down — la ferestrele care fac vederi, un shell nou acolo —, Split and Move Right / Down, Close, Close Others / Left / Right / All pe grupul lui), separatorii trași, dublu-click = egalează, grupul curent (click în vedere sau pe tab; tastatura la vederea lui; `+` al fiecărui grup deschide acolo), grupurile golite dispar, layout-ul (arborele, proporțiile, grupul fiecărui tab, tab-ul activ al fiecăruia, grupul curent) salvat cu fereastra (`SavedToolWindow::split / group / actives`, `SavedView::group`). Fiecare vedere e pictată la mărimea grupului ei (terminalul își spune mărimea doar la final, ca în 55m). Snapshot: `EDITOR_SNAPSHOT_TERMINAL_SPLIT=right|down`.
- [x] 56c — tab-ul tras afară din banda lui (revine între timp la locul lui; pastila lângă cursor): între tab-urile altui grup (linia de inserare; cele fixate rămân primele), în mijlocul vederii altui grup (la capăt), pe o margine a unei vederi (jumătatea evidențiată: stânga / sus — grupul nou înainte, `SplitTree::split_before` —, dreapta / jos — după; și pe grupul lui, dacă mai are tab-uri); afară din fereastră, mijlocul vederii lui sau Esc: rămâne unde era. Tastatura: editorul nu avea comenzi de grup, așa că sunt noi și comune: Next Group / Previous Group (⌘⌥⇧PgDn / PgUp; ale ferestrei de jos când are tastatura, altfel ale editorului) și Close Group (paleta; grupul curent al ferestrei de jos).
