# Ferestrele de unelte: înregistrate, nu scrise de mână

Data: 2026-10-22. Regula e trecută în `CLAUDE.md` („Tool windows are registered, never
hardcoded”) și e lege pentru tot ce urmează (bazele de date incluse).

## De ce

Azi, același lucru — un buton pe marginea ferestrei și ce se deschide din el — se face în
patru feluri:

| Ce | Cum e azi | Unde |
|---|---|---|
| Git, Remote (stânga jos) | un **meniu** „pentru bara de unelte”; butonul rulează primul lui rând | `menus.rs` (`tool_menus`, `tool_command`), modulele |
| Structură, Task Manager (dreapta) | `ToolPanelProvider` (id, iconiță, titlu, `create`) | `ui-components/tool_panel.rs`, `side_panel.rs` |
| Proiect, Terminal (stânga), Canvas (dreapta) | scrise direct în aplicație | `paint.rs`, `input.rs` (`tool_button`), `terminal.rs` |
| Conținutul ferestrei de jos | `show_in_panel`; titlul din `tooltip.{id}`, iconița căutată în meniuri | `tool_windows.rs` |

Terminalul e cel mai legat: aplicația ține direct `self.terminal` (≈ 50 de locuri, 12
fișiere: desenare, clicuri, selecție, linkuri, copy / paste, taste, tab-uri cu pin /
redenumire, sesiune, director de lucru, Task Manager), plus `terminal_visible` și
`bottom_for_terminal` care se ascund reciproc cu ferestrele modulelor.

## Ținta

Un singur contract, `ToolWindowProvider` (`ui-components/tool_window.rs`), înregistrat la
`register` de oricine are un buton pe margine — modul sau aplicația însăși:

- `id`, `icon`, `title` (tooltip și titlul ferestrei), `dock` (`Left` / `Bottom` /
  `Right`), `order`, `toggle_command` (scurtătura din tooltip);
- `content()` — ce face butonul:
  - `Panel` — un panou (`ToolPanel`) făcut la prima afișare (`create`);
  - `Views { command }` — vizualizări deschise de modul ca tab-uri ale ferestrei
    (`show_in_panel`); fără niciuna, butonul rulează `command`, și apare doar cât e activă
    (Git: în repo; Remote: în proiect remote);
  - `Command { command, tab }` — nicio fereastră: butonul rulează comanda (canvas-ul),
    aprins cât tab-ul activ e o vizualizare `tab`.
- Meniul de click dreapta = `MenuContribution` cu același id (rămâne cum e).

Aplicația doar descoperă și așază: un dock pe fiecare latură, aceleași butoane, tooltip-uri,
antete (iconiță, titlu, tab-uri, acțiuni) pentru toate. Înregistrarea costă doar descrierea:
nimic nu se creează înainte de prima afișare (încărcarea leneșă rămâne).

## Pașii (un commit după fiecare; verificare vizuală + `check.sh`)

### Pasul 1 — contractul și dreapta / stânga-jos din registru
- [x] `ToolWindowProvider` + `Dock` + `ToolContent` în `ui-components` (fără dependențe noi).
- [x] Aplicația: `support.tools` (sortate după dock, order) în locul lui `support.panels`;
      butoanele din dreapta și cele de jos din stânga construite din registru; tooltip-ul
      din `title()` + scurtătura lui `toggle_command`; antetul ferestrei de jos (iconiță,
      titlu) din registru.
- [x] Mutate: Structură, Task Manager (`Panel`, dreapta), Git, Remote (`Views`, jos),
      Canvas (`Command`, dreapta).
- [x] Șterse: `ToolPanelProvider`, `tool_menus`, `MenuPlace::ToolStrip` folosit drept
      buton (rămâne doar pentru meniul de click dreapta), ramurile `"canvas"` din
      `tool_button` și tooltip.
- Verificare: butoanele apar la fel (ordine, iconițe, aprinse), tooltip-urile cu
  scurtături, Git / Remote doar unde au sens, click dreapta pe Git / Remote dă meniul,
  Structura și Task Managerul se deschid / închid, canvas-ul se deschide.

### Pasul 2 — zona de jos: o singură regulă
- [x] Zona de jos arată o singură fereastră (oricare); dispar `terminal_visible`,
      `bottom_for_terminal` și ascunderile reciproce; terminalul e (încă) un caz al acestei
      reguli, nu o excepție în jurul ei.
- Verificare: terminal ↔ Git ↔ Remote se înlocuiesc corect; înălțimea zonei se păstrează;
  terminalul rulează mai departe când e ascuns; sesiunea (vizibil / ascuns) se păstrează.

### Pasul 3 — terminalul ca modul pe contract
- [x] Contractul vizualizărilor (`Viewer`) primește, cu implementări implicite, ce-i trebuie
      terminalului și oricărei vizualizări viitoare: taste brute cu modificatori,
      copy / paste (textul selectat), selecție cu mouse-ul (număr de clicuri), linkuri la
      ⌘ + hover, stare salvată în sesiune (text opac, citit doar de modul), directorul
      proiectului.
- [x] Fiecare shell devine o vizualizare (un tab al ferestrei „terminal”); modulul
      `terminal-panel` înregistrează fereastra (`Views { command: "terminal.new" }`).
- [x] Tab-urile ferestrei de jos capătă, generic, pin / redenumire / „închide celelalte”
      (meniul de click dreapta pe tab) — terminalul le folosește, oricine poate.
- [x] Dispar din aplicație `self.terminal`, `terminal.rs`, `Focus::Terminal` (focusul e al
      ferestrei de jos), ramurile `"terminal"`.
- Verificare (lista completă, nimic pierdut): deschide / închide / tab nou / închide tab /
  `exit` ascunde zona; selecție (clic, dublu-clic, tragere) + copy / paste; ⌘+clic pe
  `fișier:linie:coloană` și pe URL; scroll; pin / redenumire / închide celelalte; tab-urile
  redeschise la pornire (sesiune); directorul = rădăcina proiectului (sau al fișierului
  fără proiect); Task Manager (detaliu „N sesiuni”); scurtătura de afișare; Windows.
- Verificat: live (capturi) — pornire în directorul proiectului, comandă tastată, antetul
  comun cu „+”, cursorul plin cu tastatura, `exit` închide tabul și ascunde fereastra;
  teste — ⌥F12 / „+” / ⌘⇧T, redenumire, pin, închide celelalte, sesiunea salvată și
  redeschisă (și formatul vechi `[terminal]` citit), selecția cu dublu-clic și linkurile
  `fișier:linie` (vizualizarea), clippy pentru Windows. De încercat de mână: copy / paste,
  ⌘+clic pe un link și scroll-ul în istoricul shell-ului (aceleași căi generice, netestate
  cu mouse-ul real).

### Pasul 4 — proiectul și canvas-ul
- [x] **4a.** Butonul panoului proiectului vine din registru (fereastra proprie a aplicației,
      `ToolContent::Builtin`: stânga, prima, ⌘1); fără proiect cere unul (ca înainte); bara
      din stânga construită din registru; un clic pe orice buton trece pe aceeași cale
      (`toggle_tool`). Testat.
- [x] **4b. Canvas-ul, modul întreg.** Portul de interogări (`ProjectQueries`) primește
      ce e general, nu o metodă pentru canvas: o copie ieftină a indexului de simboluri,
      limbajul unui fișier, lista fișierelor și o „generație” (se schimbă cu fișierele: cache-
      urile modulelor). Modulul `canvas` face totul la el: comanda ⌘⇧D (activă cu un proiect),
      tabul, graful construit pe firul lui, catalogul în cache, recentrarea la clic pe un nod.
      Din aplicație dispar `open_canvas`, `request_graph`, cache-urile grafului / catalogului,
      `ViewerAction::Focus` și regula `ready`; aplicația doar implementează portul.
- Arborele proiectului ca modul: pasul 6 (mai jos).

### Pasul 5 — bara de sus
- [x] Butoanele din dreapta barei de sus (căutare, setări — și ce vor aduce modulele)
      înregistrate la fel: id, iconiță, titlu (tooltip), ordine, comanda (scurtătura din
      tooltip). Aceeași regulă, alt loc (`Dock::Top`: aplicația le înregistrează ca pe ale ei;
      bara desenează ce e înregistrat).
- Verificare: butoanele apar la fel, tooltip-uri cu scurtături, pe Windows lângă butoanele
  ferestrei (minimizează / maximizează / închide).

### Pasul 6 — arborele proiectului ca modul

Inventar (2026-10-22): ~1.000–1.300 de linii în aplicație (~20 de fișiere; cele mai multe în
`tree.rs`, `file_ops.rs`, `listing.rs`, `files.rs`, `input.rs`, `search.rs`, `moving.rs`), plus
`project_tree_view.rs` (500) și `file_search.rs` (420) din `ui-workbench`. Domeniul (`workspace`:
arborele, listarea, clipboard-ul, ștergerea, căutarea de căi) e deja reutilizabil.

#### Lista de verificare (ce face azi panoul — nimic nu se pierde) — bifată la 6f (2026-10-22)
- [x] Panoul: cardul, antetul (titlu, iconiță) cu acțiunile Nou, Locate, Expand all,
      Collapse all, Ascunde (+ tooltip-urile lor); lățimea cu separatorul (tragere, dublu-clic
      = implicit, limitele 250 px – 60 %); ⌘1 / butonul (fără proiect: „Deschide proiect”).
- [x] Arborele: desenare doar a rândurilor vizibile, ghidaje, chevron / iconițe de folder și
      fișier (regulile modulelor), selecție / hover / ținta de drop, nume lungi tăiate cu
      tooltip, marcajele modulelor (remote) cu tooltip, intrările „absente” (norișor).
- [x] Expand / collapse cu citire leneșă; listarea cu ce știu modulele (`ListedFs`,
      `merge_listing`); `relist` cerut de module; deschiderea unui fișier absent (adus de
      modul, apoi deschis; eșecul spus).
- [x] Căutarea de fișiere din panou: câmpul (⌘P, arată panoul), rezultatele (foldere întâi,
      litere potrivite evidențiate, „se indexează” / „nimic”), ↑↓ PgUp PgDn ↵ Esc, ×, un folder
      → arătat în arbore, `fișier:linie:coloană` → deschis acolo; golită la deschiderea
      proiectului.
- [x] Fișier / folder nou direct în arbore: părintele (folderul / folderul fișierului /
      rădăcina), validarea (gol, `/`, `..`, caractere interzise, existent → chenar roșu),
      ↵ creează (și deschide fișierul), Esc anulează, un clic oriunde anulează.
- [x] Ștergerea: Delete / ⌘⌫ / meniul, confirmarea (un fișier / un folder / mai multe), la
      coș în fundal, taburile fișierelor șterse închise, `EntriesDeleted`, mesajul.
- [x] Copy / Cut / Paste de fișiere și foldere: ⌘C / ⌘X / ⌘V cu arborele activ și din meniu;
      sursele (fișierele din managerul de fișiere al sistemului, clipboard-ul intern, căi ca
      text); interdicțiile (folder în el însuși); indexerul pe pauză; progresul în bara de
      stare (clic = anulare); la cut taburile urmează fișierele (`FileRenamed`); arborele
      reîmprospătat, ultimul element arătat.
- [x] Drag & drop în arbore: prag de 6 px, rădăcina nu se mută, ținta (folderul de sub mouse
      sau al fișierului), nu în el însuși / unde e deja, pastila trasă; la drop → Refactor ›
      Move (previzualizarea modulului de refactorizare).
- [x] Meniul de click dreapta: New › (File, Folder), Cut / Copy / Paste, Move (F6), Copy Path,
      Copy Path / Reference…, Reveal în managerul de fișiere, Compare with › (fișierul activ /
      clipboard), Delete; plus meniurile modulelor (Git, Local History, Remote) pe ținta aleasă
      (`menu_target`).
- [x] Focusul: un clic pe rând = arborele activ (⌘C / X / V / ⌫ / Delete pe fișiere); tastarea
      întoarce la editor.
- [x] Reîmprospătarea la schimbări pe disc (watcher): rândurile, indexul de fișiere marcat
      vechi, generația fișierelor (fără `.git` / `.lumen`).
- [x] Locate: butonul, Alt+F1, breadcrumbs, căutarea în proiect, rezultatul căutării de
      fișiere — expandează și centrează rândul.
- [x] Scroll (vertical), cursorul mână, ordinea clicurilor față de restul ferestrei.
- [x] Deschiderea proiectului: arborele nou, watcher-ul, serviciile modulelor, `ProjectOpened`.
- [x] Windows: clippy, clipboard-ul de fișiere, coșul, „Reveal in Explorer”.
  Verificat: testele din `tests/unit/workbench/tree.rs` (prin aplicație, pe panoul
  modulului), `project-tree/tests/unit/{panel,operations}.rs`, snapshot-uri (arborele,
  căutarea, tragerea). Găsite și reparate la verificare: ⌘C / ⌘X / ⌘V pe textul câmpului din
  panou (ajungea în editor), tragerea în fereastra reală (`drag_pending` știa doar de arborele
  vechi), „se indexează”, PgUp / PgDn, clic în afară anulează intrarea nouă, cursorul text și
  plasarea lui la clic / dublu clic, F6 pe selecția panoului, indexul marcat vechi la orice
  schimbare, și o regresie de performanță evitată (panoul primește interogări fără să
  pornească indexarea; o cere doar când caută: `ToolPanelAction::RefreshProject`).

#### Sub-pașii (un commit fiecare; captură + `check.sh`)
- [x] **6a. Plasa de siguranță, întâi.** Teste pentru ce n-are niciunul azi: ștergerea (cu
      confirmarea), fișierul / folderul nou (validare, creare, anulare), reîmprospătarea de pe
      disc, deschiderea unui fișier absent, drag & drop (ținta, interdicțiile), focusul
      arborelui (⌘C / X / V / ⌫). Trec înainte de orice mutare și după fiecare sub-pas.
- [x] **6b. Operațiile pe fișiere ca use case-uri** (în afara `Workbench`): copiere / mutare /
      ștergere / creare, cu progres și anulare, în domeniul `workspace` + infra (`fs-local`),
      folosite azi de aplicație — mâine de modul, prin port.
- [x] **6c. Contractul panourilor din stânga** (forma hotărâtă înainte de cod):
  - **6c-1. Panourile pe ambele laturi.** Aplicația ține câte un panou pe latură
    (`Dock::Left` / `Dock::Right`, `ToolContent::Panel`), cu același container: antetul din
    registru (iconiță, titlu) plus butoanele declarate de panou
    (`ToolPanel::header_buttons` / `header_action`), separatorul cu lățimea lui, focusul,
    mouse-ul, tastele, scroll-ul, hover-ul și tooltip-urile duse la panou; ⌘C / ⌘X / ⌘V /
    ⌫ cu panoul activ → `ToolPanel::command("copy" | "cut" | "paste" | "delete")`.
  - **6c-2. Cererile către aplicație, prin host.** Ce-i trebuie arborelui și nu e al lui:
    `WorkbenchHost::confirm` (o întrebare, răspunsul după), clipboard-ul de fișiere
    (`clipboard_files` / `set_clipboard_files`), progresul în bara de stare
    (`set_progress`, clic = anulare), pauza indexerului (`pause_indexing`), ținta
    meniului (`set_menu_target`), deschiderea unui fișier la o linie (prin acțiunile
    vizualizărilor: `OpenFile`), „Reveal” (`AppRequest::Reveal`).
  - **6c-3. Aplicația ascultă, nu e chemată.** Taburile urmează fișierele mutate
    (`FileRenamed`) și se închid la cele șterse (`EntriesDeleted`) dintr-un abonament la
    evenimente; schimbările de pe disc (watcher-ul aplicației, folosit și de Git și de
    taburi) se publică (`EntriesChanged`), iar arborele — oriunde ar fi — le ascultă.
  - Fiecare parte are deja un utilizator (aplicația, azi) și teste; modulul le folosește în
    6e.
- [x] **6d. Vizualizările în modul.** `ProjectTreeView` și `FileSearch` trec din
      `ui-workbench` în `project-tree` (pe `ui-components`, cum cere regula modulelor).
- [ ] **6e. Panoul în modul.** Modulul înregistrează fereastra (stânga, `Panel`, ⌘1),
      comenzile (New, Delete, Copy Path, Reveal…, cu scurtăturile), meniul arborelui
      (`MenuContribution` pe TreeFile / TreeFolder) și face tot ce e în lista de verificare,
      prin port. Pe bucăți, panoul fiind înregistrat abia la final (un singur commit face
      schimbul, fără regresii între commit-uri):
  - [x] **6e-1. Panoul.** `ProjectPanel` (`ToolPanel`): arborele (citit prin
        `ProjectQueries::list_dir`, cu marcajele modulelor), căutarea de fișiere
        (`paths`, `User.php:12` deschis la loc), antetul (butoane în grupuri), intrarea
        nouă tastată în arbore (verificată, făcută prin `entries::create`, deschisă).
        Contractul a primit ce-i trebuia ca să nu pierdem nimic: `ProjectView` (dat la
        deschidere și când se schimbă proiectul / fișierele), `PanelButton.group`,
        `header_icon`, `edit_key_with` (⌥ / ⇧), `tooltip_at`, `cursor_at`.
  - [x] **6e-2a. Contextul acțiunilor.** `ActionContext` (locul, țintele, faptele lor):
        cine arată țintele îl face (arborele: rândul, cu marcajul lui — absent, doar aici,
        schimbat, în urmă, conflict), modulele adaugă ce știu doar ele (`ContextProvider`:
        remote spune `remote.excluded`), comenzile îl citesc (`host.context()`, la
        `enabled` și la `run`). Meniul îl păstrează cât e deschis; o scurtătură îl face din
        focus (selecția panoului / arborelui, altfel fișierul activ). Remote ascunde Pull pe
        ce e doar aici, Push pe ce e doar pe server, totul pe ce e exclus. Modulele nu se
        cheamă între ele: context la întrebări, evenimente la ce s-a întâmplat, porturi
        pentru date.
  - [x] **6e-2b. Operațiile.** Comenzile arborelui ale modulului (aceleași id-uri:
        `tree.*`), pe țintele contextului: New File / Folder (panoul primește cererea și
        tastatura prin `host.tell_panel`), Cut / Copy / Paste (clipboard comun, lipirea pe
        firul ei, progres în bara de jos cu oprire, indexarea pusă pe pauză, `FileRenamed`
        la mutare, intrarea arătată în panou), Delete (întrebat prin `host.confirm`, la
        coș în fundal, `EntriesDeleted`), Copy Path (`host.set_clipboard_text`), Reveal
        (`host.reveal_in_file_manager`); ce nu e pe disc nu se taie / copiază / șterge.
        În panou: ⌘C / ⌘X / ⌘V / ⌘⌫ pe selecție devin comenzile modulului; tragerea, cu
        pastila ei, cere `ToolPanelAction::Move` (refactorizarea Move, cu previzualizare).
        Modulul (`ProjectTreeModule`) e scris, dar încă neînregistrat. Compare with
        Active / Copy Path / Reference… rămân ale aplicației până la schimb (6e-3).
  - [x] **6e-3a. Ce mai trebuia panoului.** Primește magistrala odată cu proiectul
        (`ProjectView { queries, events }`) și ascultă singur `EntriesChanged` (abonament
        per panou: merge și cu mai multe ferestre), aplicat la `poll` (o trezire cerută);
        un folder listat din nou de un modul (`relist`) se anunță la fel. Ce i se spune
        prin `tell_panel` e un tip comun, `EntriesRequest` (Reveal cu / fără deschidere,
        FocusSearch pentru Go to File, NewEntry). Rândul selectat sau folderul intrării noi
        dispărute de pe disc sunt lăsate. Fișierele de pe server se aduc la deschidere prin
        `open_file` (neschimbat).
  - [x] **6e-3b. Schimbul.** `ProjectTreeModule` e înregistrat (stânga, `Panel`); ⌘1 rămâne
        comanda aplicației (fără proiect cere unul) — un panou comutat de o comandă a
        aplicației o lasă pe ea să decidă. Aplicația nu-și mai desenează arborele: Locate,
        Go to File, folderul arătat, intrarea nouă merg la panou (`EntriesRequest`);
        comenzile `tree.*` sunt ale modulului (operațiile vechi ale aplicației șterse, testul
        lor mutat pe comenzile modulului). Panoul ascuns își păstrează starea (folderele
        deschise) până e arătat din nou; primește proiectul chiar la creare; după click pe
        un rând păstrează tastatura (un rezultat al căutării o dă editorului); ascultă și
        `EntriesDeleted` / `FileRenamed`. Testele 6a trec pe panoul modulului; ordinea
        contribuțiilor e păzită de un test (aceeași la fiecare pornire, niciun loc împărțit).
        Verificat vizual (snapshot): arborele și căutarea arată ca înainte.
- [ ] **După 6 (de discutat): indexarea ca modul, prin evenimente cu cozi.** Azi indexul de
      fișiere și cel de simboluri / text sunt servicii ale aplicației (`ProjectFiles` în
      `ProjectData`, `indexer::ProjectIndexer` din infra), chemate direct (`mark_stale`,
      `changed`, `pause` / `resume`). Propunerea: un modul de indexare care ascultă
      evenimentele domeniului (`EntriesChanged`, `FileSaved`, `FileRenamed`,
      `EntriesDeleted`) într-o coadă a lui care comasează cererile (aceeași cale cerută de
      mai multe ori înainte să fie procesată = o singură reindexare; o lipire de 1.000 de
      fișiere = un singur lot), își anunță starea (`IndexingStarted / Progress / Finished`)
      pentru cine o arată (bara de stare, panoul, Task Manager), iar domeniile nu știu de
      modul (doar de evenimentele lor).
- [ ] **După 6:** meniurile sistemului (bara de sus) contribuite de module: azi un modul își
      pune comenzile într-un meniu existent (`CommandSpec.menu`), dar titlurile și ordinea
      lor sunt ale aplicației (`MENU_ORDER`, cu „Git” scris acolo); propunerea: un modul
      își declară meniul (titlu tradus, după care meniu stă), ca ferestrele de unelte.
- [x] **6f. Curățenia din aplicație.** Arborele vechi șters din aplicație: câmpurile
      (`tree_view`, `file_search`, `project_header`, `project_visible`, `tree_press`,
      `tree_drag`), `Focus::Tree / TreeInput / FileSearch`, câmpurile de text ale lor,
      desenarea, clicurile, tooltip-urile, scroll-ul, căutarea veche, `ToolContent::Builtin`.
      Rămân în aplicație, cu rost: `tree.rs` (watcher-ul → `EntriesChanged`, indexul marcat
      vechi, taburile fișierelor scrise de alții), `listing.rs` (`ListedFs`: implementarea
      `ProjectQueries::list_dir`, adusul fișierelor absente la deschidere). De discutat: și
      `project: FileTree` al aplicației (azi doar „proiectul deschis” + marcajele pentru
      meniul unui tab) poate deveni un mâner mai mic al proiectului.

## Riscuri și cum le evit

- **Pierderea unei funcții a terminalului** — lista de verificare de la pasul 3, bifată
  înainte de commit; testele existente ale terminalului rămân verzi.
- **Încărcarea leneșă** — `create` / vizualizările se fac doar la prima afișare; un test
  verifică că înregistrarea nu creează nimic.
- **Sesiunile salvate** — formatul vechi al sesiunii (tab-urile terminalului) citit în
  continuare (alias), ca nimic din ce era salvat să nu se piardă.
- **Windows** — clippy pentru Windows la fiecare pas (`check.sh`).
