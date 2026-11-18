# 23. Ce aduc modulele, nu app-ul (M2)

Cererea: din review-ul de arhitectură, punctul M2 — app-ul încă știe pe nume de unele module
și le folosește direct. Regula rămâne: **fiecare modul se înregistrează și vine la pachet cu
ce îi trebuie** (comenzi, rânduri de meniu, view-uri, reacții); app-ul doar descoperă și
așază. Un modul oprit nu lasă nimic în urmă (niciun rând de meniu mort).

## Azi (inventar, 2026-10-24)

Ce e scris de mână în app:

1. **Meniul editorului** (`app/src/commands.rs` `editor_context_menu`): Rename, Change
   Signature, Move, Find Usages (`refactoring`), Show Diagram (`diagrams`), Compare with
   Clipboard (`compare`), plus ale app-ului: Go to Declaration, Cut / Copy / Paste.
2. **Meniul arborelui** (`tree_context_menu`): New › (File, Folder), Cut / Copy / Paste,
   Copy Path, Reveal, Delete (`project-tree`), Move (`refactoring`), Compare with › clipboard
   (`compare`); ale app-ului: Compare with › active file, Copy Path / Reference….
   Particularități: scurtăturile afișate (⌘X / ⌘C / ⌘V, ⌘⌫ / Delete) nu sunt ale comenzilor,
   eticheta „Reveal in Finder / Show in Explorer” depinde de sistem, „Compare with” are
   etichete proprii și apare doar când are sens (fișier, alt fișier activ, text în clipboard).
3. **Meniul tab-ului** (`tab_context_menu` în `app/src/app/menus.rs`): Compare with ›
   clipboard (`compare`).
4. **View-uri deschise de app**: redenumirea fișierului unei clase (PSR-4) și Move-ul la
   tragerea în arbore deschid view-urile `refactoring` (`rename_file_as_class`,
   `open_move_view`); „Search in Project” deschide view-ul `indexing`
   (`open_project_search`).
5. **Bara de modificări git din gutter**: click → app rulează `vcs_git::CHANGE_ACTIONS`.
6. Mărunte: `indexing::paths::LIMIT` în `queries.rs`; snapshot-urile (`snapshot.rs`) rulează
   comenzi de module prin constantele lor.

Ce există deja și se păstrează: `MenuContribution` (`workbench-model`) — submeniuri
(`Git ›`, `Local History ›`) după un separator, rânduri filtrate de `enabled()`;
`CommandContribution` cu icon; `ToolWindowProvider`.

## Ținta

- Meniurile de click dreapta (editor, arbore, tab) se compun **numai din contribuții**:
  ale modulelor și ale app-ului (ca ferestrele-unealtă: „a module's or the app's own”).
- Aceeași ordine și grupare ca azi (verificat cu snapshot-urile `editor-menu`,
  `tree-menu`, tab-menu).
- Un modul dezactivat → rândurile lui dispar; nimic din app nu se schimbă.
- O **verificare în `cargo xtask check-deps`**: în `app/src` numele crate-urilor de module
  apar doar în `modules.rs` (unde se încarcă) și în harness-ul de snapshot — ca regula
  să rămână așa și pe viitor.

## Pași

### 1. Rânduri inline în meniuri (domeniu)
- `MenuContribution` primește `style`: `Submenu` (ca acum) sau `Inline { section, order }`.
- `MenuSection` (în `workbench-model`, ordinea fixă a secțiunilor): `New`, `Refactor`,
  `Navigate`, `Clipboard`, `Path`, `Compare`, `Danger`; între secțiuni — separator.
  Secțiunile `New` și `Compare` se arată ca submeniuri („New ›”, „Compare with ›”), cu
  rândurile tuturor modulelor care contribuie la ele.
- Un rând poate avea **eticheta lui** (`label_key`) și **scurtătura afișată** (ex. ⌘X în
  arbore), altfel cele ale comenzii.
- `CommandHandler::menu_label(host) -> Option<String>` (implicit `None`): eticheta care
  depinde de context („Reveal in Finder” / „Show in Explorer”).
- `MenuPlace::Tab` nou (meniul tab-ului).
- Teste în `workbench-model`: ordinea secțiunilor, separatoare, submeniurile secțiunilor.

### 2. Meniul editorului din contribuții
- `refactoring` contribuie Rename, Change Signature, Move (Refactor), Find Usages
  (Navigate); `diagrams` — Show Diagram (Navigate); `compare` — Compare with Clipboard
  (Clipboard); app-ul — Go to Declaration (Navigate), Cut / Copy / Paste (Clipboard).
- `editor_context_menu` din `commands.rs` dispare; app-ul compune: inline pe secțiuni,
  apoi submeniurile (ca acum).
- Snapshot `editor-menu` identic.

### 3. Meniul arborelui și al tab-ului din contribuții
- `project-tree` contribuie New › File / Folder, Cut / Copy / Paste (cu ⌘X…), Copy Path,
  Reveal (`menu_label` după sistem), Delete (⌘⌫ / Delete); `refactoring` — Move (F6);
  `compare` — Compare with › clipboard (în arbore și pe tab); app-ul — Compare with ›
  active file, Copy Path / Reference….
- Condițiile de azi (`TreeTarget`) trec în `enabled()` (din `host.context()`: fișier sau
  folder, alt fișier activ, text în clipboard). `TreeTarget` dispare.
- Snapshot-urile meniurilor de arbore / tab identice.

### 4. Redenumirea și mutarea fișierelor, preluate de module
- Un punct de extensie nou, `FileRefactoring` (`workbench-model`): `rename(host, path,
  new_name) -> bool`, `moved(host, paths, destination) -> bool` — un modul poate prelua o
  redenumire / mutare (refactoring: clasa PSR-4, Move cu referințele); niciunul → operația
  simplă de azi.
- `rename_file_as_class` și `open_move_view` ies din app; logica de context (`rename_context_at`,
  `move_context`) rămâne în app, dată prin `WorkbenchHost` (cum e deja pentru comenzi).
- Testul `renaming_a_class_file_renames_the_class` trece prin contribuție.

### 5. „Search in Project” al modulului `indexing`
- Comanda și handler-ul ei trec în `indexing`; ce îi trebuie (interogările proiectului,
  căutarea semantică, textul selectat pe un rând) vine prin `WorkbenchHost`.
- `open_project_search` iese din app; scurtătura rămâne aceeași (același id de comandă).

### 6. Click pe bara de modificări
- Punct de extensie `ChangeBarAction` (`workbench-model`): `clicked(host, line) -> bool`;
  `vcs-git` îl contribuie (acțiunile lui de azi). Niciunul → comparația cu ultimul commit,
  ca acum.

### 7. Mărunte și paza
- `indexing::paths::LIMIT` → `workspace` (domeniul fișierelor proiectului).
- `snapshot.rs`: comenzile modulelor după id (`CommandId("git.log")` din registru), nu
  prin crate-ul modulului.
- `cargo xtask check-deps`: refuză `<crate-de-modul>::` în `app/src` în afară de
  `modules.rs` și `snapshot.rs`.

## Stare (2026-10-24)

- [x] 1–2. Rânduri inline (`ContextMenuItem`, `MenuSection`, `arrange`); meniul editorului.
- [x] 3. Meniul arborelui; „Compare with › Clipboard” al tab-ului (`MenuPlace::Tab`).
- [x] 4. `FileRefactoring`: redenumirea fișierului unei clase și mutarea, ale modulului
  refactoring (fără el: operația simplă).
- [x] 5. „Search in Project” — o comandă a workbench-ului **implementată** de indexing
  (`CommandImplementation`): locul în meniul Navigate și scurtăturile rămân ale app-ului.
- [x] 6. `GutterAction`: click-ul pe bara de modificări rulează comanda modulului git.
- [x] 7. `FILE_LIMIT` în `workspace`; adaptoarele porturilor de refactorizare
  (`refactor_ports.rs`) în app, unde se leagă; `RENAME_SYMBOL` în `workbench-model`;
  modulele locale ale app-ului redenumite (`problems`, `refactorings`) ca un nume să fie
  mereu al crate-ului; **`check-deps` refuză** un crate de modul numit în `app/src` în afară
  de `modules.rs` și `snapshot.rs`.

## Riscuri

- Meniurile se schimbă vizual dacă ordinea / separatoarele nu ies la fel → fiecare pas cu
  snapshot-urile meniurilor comparate cu referința (`ui-snapshots.sh compare`).
- Scurtăturile: id-urile comenzilor nu se schimbă (keymap-urile salvate rămân valabile).
- Fiecare pas e un commit separat, cu `check.sh` verde.
