# Plan 51 — Meniurile aplicației, regândite (grupuri, ordine, submeniuri)

## Context
Meniurile sunt „stufoase rău de tot”: și bara nativă de pe macOS ({app}, File, Edit, Code,
Refactor, Find, Navigate, Git, Window), și drawer-ul de meniu (plan 49) — aceeași listă,
`commands::menus()` (`crates/app/src/commands.rs`). Azi modelul e plat:
- `CommandSpec.menu: Option<&'static str>` — doar numele meniului;
- ordinea = ordinea din `SPECS`, apoi comenzile modulelor în **ordinea în care sunt înregistrate
  modulele** (`app/src/modules.rs`), nu după sens;
- separatorii = o listă globală a app-ului (`SEPARATOR_AFTER`, 11 id-uri): un modul nu poate
  pune un separator și nu-și poate alege locul;
- `platform_api::MenuItem` are doar `Command` / `Separator`: niciun submeniu, niciun item nativ
  macOS (Services, Hide…), niciun meniu Help.

Nu scoatem nimic. Regândim gruparea, ordinea, separatorii, câteva submeniuri și meniurile de
top, după convențiile editoarelor de cod bine organizate și macOS HIG.

## 1. Inventarul de azi
Scurtăturile sunt ale presetului implicit (`keymaps.golden`); `Mod` = ⌘ pe macOS / Ctrl în
rest. „│” = separator. Sursa: *app* = `SPECS` din `commands.rs`, altfel modulul.

**{app}** (4 rânduri, niciun separator)
| id | Etichetă (en) | Scurtătură | Sursa |
|---|---|---|---|
| app.about | About {app} | — | app |
| app.settings | Settings… | Mod+, | app |
| app.theme_studio | Theme Studio… | — | theme-studio |
| app.quit | Quit {app} | Mod+Q | app (mutat forțat la final) |

**File** (12 rânduri)
| id | Etichetă | Scurtătură | Sursa |
|---|---|---|---|
| file.new | New File | Mod+N | app |
| window.new | New Window | Mod+Shift+N | app |
| file.open | Open File… | Mod+O | app |
| project.open | Open Project… │ | Mod+Shift+O | app |
| file.save | Save | Mod+S | app |
| file.save_as | Save As… │ | Mod+Shift+S | app |
| tab.close | Close Tab | Mod+W | app |
| app.clear_caches | Clear and Rebuild Caches | — | app |
| tab.reopen_closed | Reopen Closed Tab | Mod+Shift+T | app |
| history.show | Show Local History | — | local-history |
| remote.open | Open Remote Project… | — | remote |
| project.reindex | Reindex Project | — | indexing |

**Edit** (17 rânduri)
| id | Etichetă | Scurtătură | Sursa |
|---|---|---|---|
| edit.undo | Undo | Mod+Z | app |
| edit.redo | Redo │ | Mod+Shift+Z, Mod+Y | app |
| edit.cut / copy / paste | Cut / Copy / Paste | Mod+X / C / V | app |
| edit.select_all | Select All │ | Mod+A | app |
| edit.duplicate | Duplicate Line or Selection | Mod+D | app |
| edit.delete_line | Delete Line │ | Mod+Backspace | app |
| edit.select_next_occurrence | Add Selection for Next Occurrence | Ctrl+G, Alt+J | app |
| edit.select_all_occurrences | Select All Occurrences | Mod+Shift+L | app |
| edit.unselect_occurrence | Unselect Occurrence | Ctrl+Shift+G, Alt+Shift+J | app |
| edit.add_cursor_up / down | Add Caret Above / Below | Mod+Alt+Up / Down | app |
| edit.extend_selection / shrink_selection | Extend / Shrink Selection | Alt+Up / Alt+Down | app |
| compare.clipboard | Compare with Clipboard | — | compare |

**Code** (21 rânduri, 2 separatori)
| id | Etichetă | Scurtătură | Sursa |
|---|---|---|---|
| code.complete | Complete Code | Ctrl+Space | app |
| code.show_actions | Show Context Actions | Alt+Enter, Mod+. | app |
| code.comment | Comment with Line Comment │ | Mod+/ | app |
| code.move_statement_up / down | Move Statement Up / Down | Mod+Shift+Up / Down | app |
| code.move_line_up / down | Move Line Up / Down | Alt+Shift+Up / Down | app |
| code.fold / code.unfold | Collapse / Expand │ | Mod+- / Mod+= | app |
| code.fold_all / unfold_all | Collapse All / Expand All | Mod+Shift+- / Mod+Shift+= | app |
| sql.format_statement | Reformat Statement | — | sql |
| lsp.choose_engine | Choose the File's Language Engine… | — | lsp |
| lsp.format | Format with Language Server | Shift+Alt+F | lsp |
| lsp.restart | Restart Language Servers | — | lsp |
| lsp.show_log | Show Language Server Log | — | lsp |
| lsp.download | Download the File's Language Server | — | lsp |
| lsp.use_built_in | Use the Built-in Engine for the File's Language | — | lsp |
| quality.format_file | Reformat File | Mod+Alt+L | quality |
| quality.check_file | Check File | — | quality |
| quality.format_project | Format Project | — | quality |
| quality.check_project | Check Project | — | quality |

**Refactor** (4, niciun separator) — toate din *refactoring*: refactor.rename „Rename…” Shift+F6 ·
refactor.change_signature „Change Signature…” Mod+F6 · refactor.move „Move…” F6 ·
refactor.undo „Undo Last Refactoring”.

**Find** (5): find.open „Find…” Mod+F · find.replace „Replace…” Mod+R │ · find.next „Find Next”
Mod+G, F3 · find.prev „Find Previous” Mod+Shift+G, Shift+F3 · refactor.find_usages „Find
Usages” Alt+F7 (*refactoring*).

**Navigate** (11): navigate.back „Back” Mod+[ · navigate.forward „Forward” Mod+] │ ·
navigate.definition „Go to Declaration” Mod+B · navigate.implementation „Go to Implementation”
Mod+Alt+B · navigate.type_declaration „Go to Type Declaration” Mod+Shift+B ·
navigate.type_hierarchy „Type Hierarchy” Ctrl+H · navigate.line „Go to Line…” Mod+L ·
project.search_files „Go to File…” Mod+P · project.search „Search in Project…” Mod+Shift+F
(implementat de *indexing*) · file.reveal_in_project „Select in Project View” Alt+F1 ·
diagram.show „Show Diagram” Mod+Alt+Shift+U (*diagrams*).

**Git** (15, niciun separator) — toate din *git*: git.commit „Commit…” · git.pull „Pull” Mod+T ·
git.push „Push” Mod+Shift+K · git.fetch „Fetch” · git.log „Show Git Log” Alt+9 · git.changes
„Show Changes” · git.revert_change „Revert Change” Mod+Alt+Z · git.change_actions „Change
Actions” · git.annotated_commit „Show Annotated Commit” · git.file_history „Show History” ·
git.rollback „Rollback…” Mod+Alt+Shift+Z · git.compare_head „Compare with Last Commit” ·
git.compare_revision „Compare with Revision…” · git.compare_branch „Compare with Branch…” ·
git.annotate „Annotate with Git Blame”.

**Window** (19, 2 separatori)
| id | Etichetă | Scurtătură | Sursa |
|---|---|---|---|
| tab.close_others | Close Others | — | app |
| tab.close_saved | Close Saved Tabs | — | app |
| tab.close_all | Close All Tabs │ | — | app |
| tab.next / tab.prev | Next Tab / Previous Tab │ | Mod+Shift+] / [, Ctrl+(Shift+)Tab | app |
| view.toggle_project | Project | Mod+1, Alt+1 | app |
| view.zoom_in / zoom_out | Zoom In / Zoom Out | Mod+Shift+= / Mod+Shift+- | app |
| view.zoom_reset | Actual Size (100%) | Mod+Shift+0 | app |
| view.full_screen | Full Screen | F11 | app |
| view.switch_theme | Switch Theme | Mod+Alt+T | app |
| notifications.close | Close Notifications | — | app |
| notifications.recent | Show Recent Notifications | — | app |
| view.debug_overlay | Diagnostic Overlay | F12 | app |
| view.toggle_structure | Structure | Mod+7, Alt+7 | structure |
| view.canvas | Dependency Canvas | Mod+Shift+D | canvas |
| tasks.open | Task Manager | — | task-manager |
| view.toggle_terminal | Terminal | Alt+F12, Ctrl+` | terminal |
| view.toggle_services | Services | Mod+8, Alt+8 | services |

**Pierdut**: terminal.new „New Terminal Tab” cere meniul „View”, care nu e în `MENU_ORDER` —
nu apare nicăieri.

Total: ~108 rânduri în 9 meniuri; Code 21, Window 19, Edit 17, Git 15 — peste orice prag
de lizibilitate.

### Ciudățenii găsite
1. **Code amestecă trei lumi**: editare (complete, comment, move), pliere, formatare/verificare
   și infrastructura serverelor de limbaj (restart, log, download, alegerea motorului) — 21 de
   rânduri cu doar 2 separatori; după „Move Line Down” urmează direct „Collapse”.
2. **Formatarea, numită în patru feluri, în trei locuri**: „Reformat Statement” (sql, primul),
   „Format with Language Server” (lsp, la mijloc), „Reformat File” / „Format Project” (quality,
   la final). „Reformat Statement” apare pentru orice fișier, nu doar SQL.
3. **File**: „Close Tab │ Clear and Rebuild Caches │ Reopen Closed Tab” lipite (curățarea
   cache-urilor între două comenzi de taburi); „Reindex Project” și „Clear caches” sunt
   mentenanță, nu fișier; „Open Remote Project…” e departe de „Open Project…” (vine ultimul,
   după ordinea modulelor).
4. **Window e un sertar**: taburi (închidere, comutare), ferestre-unealtă (Project, Structure,
   Terminal, Services, Task Manager), zoom, ecran complet, temă, notificări, overlay-ul de
   diagnostic și pânza dependențelor. Nu există meniul **View**.
5. **Ferestre-unealtă incomplete**: Project / Structure / Services / Terminal / Task Manager sunt
   în Window, dar **Database** (`database.window`) și **Git** (fereastra de jos) nu — deși toate
   sunt `ToolWindowProvider` înregistrate; lista e scrisă de mână, contrar regulii „tool windows
   are registered, never hardcoded”.
6. **Edit**: 17 rânduri; cele 5 comenzi de cursoare multiple / apariții plate; „Compare with
   Clipboard” lipit de „Shrink Selection”. „Copy Path / Reference…” (Mod+Alt+C) nu e în
   niciun meniu (locul lui obișnuit e Edit).
7. **Git**: 15 rânduri fără separator, ordine întâmplătoare (Rollback între istoric și comparări,
   Annotate ultimul); „Change Actions” și „Show Annotated Commit” sunt acțiuni de gutter (pe
   rândul cursorului) puse ca comenzi globale; „Show History” se confundă cu „Show Local
   History”. Meniul contextual Git (`git_menu()`) are deja grupuri bune — bara le ignoră.
8. **Navigate** conține „Search in Project…” (e o căutare, locul ei e în Find), iar „Show
   Diagram” e lipit la coadă.
9. **{app}**: fără separatori; lipsesc itemii standard macOS (Services, Hide {app}, Hide Others,
   Show All); pe Windows / Linux drawer-ul arată o categorie „{app}” (About / Settings / Quit),
   neconvențional acolo (convenția: File › Settings / Exit, Help › About).
10. **Niciun meniu Help**: pe macOS câmpul de căutare în meniuri (Help › Search) apare doar
    dacă există un meniu Help marcat ca atare — cu atâtea comenzi, l-am vrea.
11. **Scurtături care se bat** (de verificat / decis): Zoom In / Zoom Out au exact scurtăturile
    lui Expand All / Collapse All (Mod+Shift+= / Mod+Shift+-) în toate presetele — ambele apar în
    meniu cu același accelerator. Pe Windows, Find Next (Ctrl+G) și „Add Selection for Next
    Occurrence” (Ctrl+G primul) la fel. Presetele care leagă Replace și de Mod+H (`keymaps.rs`)
    s-ar bate pe macOS cu ⌘H „Hide {app}” când îl adăugăm.
12. **Ordinea depinde de înregistrarea modulelor**, iar separatorii de o listă globală a app-ului:
    un modul nou ajunge mereu la coada meniului, lipit de ce era acolo.
13. **Etichete lungi / neomogene**: „Choose the File's Language Engine…”, „Use the Built-in
    Engine for the File's Language”, „Download the File's Language Server”; „Show Git Log” /
    „Show Language Server Log” / „Show Local History” / „Show History” fără un tipar.

## 2. Structura propusă
Meniuri de top (ordinea obișnuită a IDE-urilor): **{app}** (doar macOS) · **File** · **Edit** ·
**View** · **Find** · **Navigate** · **Code** · **Refactor** · **Git** · **Tools** · **Window** ·
**Help**. Fiecare meniu = secțiuni (grupuri de 2–5, separator automat între ele), cel mult
~13 rânduri vizibile. „›” = submeniu. *Nou* = rând care azi nu e în niciun meniu (comandă
existentă), *nativ* = item predefinit macOS (muda), opțional = de confirmat cu utilizatorul.

**{app}** (macOS; HIG) — 1. About {app} · 2. Settings… ⌘, · 3. Services › *nativ* ·
4. Hide {app} ⌘H, Hide Others ⌥⌘H, Show All *nativ* · 5. Quit {app} ⌘Q.
Pe Windows / Linux nu există: About → Help, Settings… și Quit (Windows: „Exit” / „Ieșire”) →
ultimele secțiuni din File.

**File** (10–12)
1. New File ⌘N · New Window ⇧⌘N
2. Open File… ⌘O · Open Project… ⇧⌘O · Open Remote Project… · Open Recent › (opțional, *nou*:
   proiectele recente, aceeași listă ca meniul Dock; cheia `menu.recent_projects` există)
3. Save ⌘S · Save As… ⇧⌘S
4. Close Tab ⌘W · Reopen Closed Tab ⇧⌘T
5. Show Local History
6. (Windows / Linux) Settings… Ctrl+,
7. (Windows / Linux) Exit / Quit {app} Ctrl+Q

**Edit** (13)
1. Undo · Redo
2. Cut · Copy · Paste · Copy Path / Reference… ⌥⌘C (*nou*) · Compare with Clipboard
3. Select All · **Selection ›** — a) Extend Selection ⌥↑, Shrink Selection ⌥↓ │ b) Add Selection
   for Next Occurrence, Unselect Occurrence, Select All Occurrences │ c) Add Caret Above, Add
   Caret Below
4. Duplicate Line or Selection ⌘D · Delete Line · Move Line Up · Move Line Down

**View** (8, *nou*)
1. **Tool Windows ›** — generat din `ToolWindowProvider`-ele înregistrate (titlu, iconiță,
   scurtătura comenzii de toggle), pe dock-uri cu separator între ele, în ordinea butoanelor:
   Project ⌘1, Structure ⌘7 │ Terminal, Git (*nou*), Services ⌘8, Task Manager │ Database (*nou*)
   — un modul nou cu fereastră apare singur aici.
2. **Appearance ›** — Switch Theme ⌥⌘T · Theme Studio… (mutat din {app}: e despre temă, iar
   meniul aplicației rămâne cel standard)
3. Zoom In · Zoom Out · Actual Size (100%)
4. Full Screen F11 (pe macOS, opțional, ⌃⌘F ca HIG)
5. Show Recent Notifications · Close Notifications

**Find** (6)
1. Find… ⌘F · Replace… ⌘R
2. Find Next ⌘G · Find Previous ⇧⌘G
3. Search in Project… ⇧⌘F (mutat din Navigate) · Find Usages ⌥F7

**Navigate** (11)
1. Back ⌘[ · Forward ⌘]
2. Go to File… ⌘P · Go to Line… ⌘L
3. Go to Declaration ⌘B · Go to Implementation ⌥⌘B · Go to Type Declaration ⇧⌘B
4. Type Hierarchy ⌃H · Show Diagram ⌥⇧⌘U · Dependency Canvas ⇧⌘D (mutat din Window)
5. Select in Project View ⌥F1

**Code** (12)
1. Complete Code ⌃Space · Show Context Actions ⌥↩
2. Comment with Line Comment ⌘/ · Move Statement Up · Move Statement Down
3. **Folding ›** — Collapse ⌘-, Expand ⌘= │ Collapse All, Expand All
4. Reformat File ⌥⌘L · Reformat SQL Statement (doar într-un fișier SQL, vezi §3) · Reformat with
   Language Server ⇧⌥F · Reformat Project
5. Check File · Check Project

**Refactor** (4)
1. Rename… ⇧F6 · Change Signature… ⌘F6 · Move… F6
2. Undo Last Refactoring

**Git** (13) — aceleași grupuri ca meniul contextual Git
1. Commit… · Show Changes · Show Git Log ⌥9
2. Pull ⌘T · Push ⇧⌘K · Fetch
3. Annotate with Git Blame · Show File History · **Compare With ›** (Last Commit, Revision…,
   Branch…) · Rollback… ⌥⇧⌘Z
4. Revert Change ⌥⌘Z · Change Actions… · Show Annotated Commit (cele pe rândul cursorului)

**Tools** (6, *nou*)
1. Data Sources and Drivers… (*nou*) · New Terminal Tab (azi pierdut)
2. **Language Servers ›** — Choose Language Engine…, Use Built-in Engine │ Download Language
   Server │ Restart Language Servers, Show Language Server Log
3. Reindex Project · Clear and Rebuild Caches… (mutate din File, aici lângă
   celelalte unelte)
4. Remote › (opțional, *nou*: pull / push / sync ale proiectului remote, `remote.*`, doar cu un
   proiect remote)

**Window** (5 + nativ)
1. Minimize ⌘M · Zoom *nativ* (macOS)
2. Next Tab · Previous Tab
3. Close Other Tabs · Close Saved Tabs · Close All Tabs
4. Bring All to Front + lista ferestrelor *nativ* (macOS, ca azi: `windows: true`)

**Help** (*nou*)
1. (macOS) câmpul de căutare în meniuri *nativ* (`set_as_help_menu_for_nsapp`)
2. Keyboard Shortcuts… (opțional, *nou*: Settings la pagina de scurtături) · Diagnostic Overlay F12
3. (Windows / Linux) About {app}

Fiecare comandă de azi are un loc; nimic dublat (Task Manager doar în View › Tool Windows,
toggle-urile ferestrelor doar acolo).

### Etichete (id-urile rămân)
| id | en azi → propus | ro propus |
|---|---|---|
| sql.format_statement | Reformat Statement → **Reformat SQL Statement** | Formatează instrucțiunea SQL |
| lsp.format | Format with Language Server → **Reformat with Language Server** | Formatează cu serverul de limbaj |
| quality.format_project | Format Project → **Reformat Project** | Formatează proiectul |
| lsp.choose_engine | Choose the File's Language Engine… → **Choose Language Engine…** | Alege motorul limbajului… |
| lsp.use_built_in | Use the Built-in Engine for the File's Language → **Use Built-in Engine** | Folosește motorul inclus |
| lsp.download | Download the File's Language Server → **Download Language Server** | Descarcă serverul de limbaj |
| git.file_history | Show History → **Show File History** | Arată istoricul fișierului (Git) |
| git.change_actions | Change Actions → **Change Actions…** | Acțiunile modificării… |
| tab.close_others | Close Others → **Close Other Tabs** | Închide celelalte taburi |
| app.clear_caches | Clear and Rebuild Caches → **Clear and Rebuild Caches…** (are confirmare) | Golește și reconstruiește cache-urile… |
| app.quit (Windows) | Quit {app} → **Exit** | Ieșire |

În submeniuri un rând poate avea o etichetă scurtă de meniu (cheia opțională
`menu_label.<id>`: „Compare With › Last Commit / Revision… / Branch…”); paleta de comenzi,
editorul de scurtături și filtrul drawer-ului folosesc eticheta întreagă.

## 3. Modelul
Declarativ, în `command-model` (domeniu pur) și `platform-api`:

```rust
/// Where a command sits in the menu bar: a section, and its place inside it.
pub struct MenuAt { pub section: &'static str, pub order: u16 }
pub struct CommandSpec { id, label, menu: Option<MenuAt>, shortcuts }

/// A group of rows (separators come between sections, never declared).
pub struct MenuSectionSpec {
    pub id: &'static str,          // "code.format"
    pub menu: &'static str,        // "Code" (a top-level menu or a submenu id)
    pub order: u16,                // its place in that menu
    pub platforms: Platforms,      // e.g. app.settings: {app} on macOS, File elsewhere
}
/// A row opening more rows: it has sections of its own.
pub struct SubmenuSpec { pub id: &'static str, pub section: &'static str, pub order: u16,
                         pub label_key: &'static str, pub icon: &'static str }
```
- Meniurile de top, secțiunile și submeniurile **app-ului** sunt declarate o dată în
  `commands.rs` (`MENUS`, `SECTIONS`, `SUBMENUS`; dispar `MENU_ORDER`, `SEPARATOR_AFTER`, mutarea
  forțată a lui Quit). Id-urile secțiunilor sunt constante publice în `workbench-model::menus`
  (`sections::CODE_FORMAT`…), ca modulele să le numească fără șiruri scrise de mână.
- **Un modul își pune comanda numind o secțiune** (`menu: Some(MenuAt { section:
  sections::CODE_FORMAT, order: 30 })`), niciodată o poziție; dacă are nevoie de grupuri proprii
  (Git, Tools › Language Servers) le contribuie (`MenuSectionContribution` /
  `SubmenuContribution`). Ordinea e cea declarată, nu a înregistrării modulelor.
- Se reutilizează ideea din meniurile contextuale (`MenuSection` + ordine, `rows_in`): secțiunile
  goale dispar, separatorii doar între secțiuni arătate, un submeniu cu un singur rând se arată
  inline, unul gol dispare, un meniu de top gol dispare (Git fără modulul lui).
- **Rânduri condiționate** (opțional): `MenuAt.when` — „Reformat SQL Statement” doar la SQL,
  ca `MenuWhen` de la meniurile contextuale; în meniul nativ ca rând dezactivat, în drawer ascuns.
- **Ferestrele-unealtă**: View › Tool Windows e un submeniu *generat* din `ToolWindowProvider`
  (dock, order, titlu, iconiță); rândul rulează toggle-ul ferestrei (`toggle_command`) sau, fără
  el, comanda generică „arată fereastra `<id>`” pe care app-ul o știe deja. Toggle-urile modulelor
  nu mai declară `menu`.
- `platform_api::MenuItem` primește `Submenu { title, icon, items }` și `Native(NativeItem)`
  (`Services`, `Hide`, `HideOthers`, `ShowAll`, `Minimize`, `Zoom`, `BringAllToFront`; doar macOS,
  drawer-ul le ignoră); `Menu` primește `help: bool` (pe lângă `windows`).
- **muda** (`platform-winit/src/menu.rs`): construire recursivă (`muda::Submenu` în submeniu),
  `PredefinedMenuItem::{services, hide, hide_others, show_all, minimize, maximize,
  bring_all_to_front}`, `set_as_help_menu_for_nsapp` pentru Help.
- **Calea unei comenzi**: `menu_of(id)` devine `menu_path(id) -> Vec<String>` („Code › Folding”):
  detaliul din paleta de comenzi (`app/input.rs`) și grupul din editorul de scurtături
  (`commands.rs:687`) o arată întreagă.
- Teste: fiecare `MenuAt` numește o secțiune cunoscută; niciun id de două ori; niciun meniu de
  top peste 15 rânduri vizibile; arborele meniurilor fixat într-un `menus.golden` per platformă
  (macOS / Windows / Linux), cu toate modulele.

### Drawer-ul (al treilea nivel)
Azi: panoul categoriilor (300 px) + panoul itemilor (300 px). Un rând-submeniu are chevron „›”
la dreapta (fără scurtătură). Variante:
- **A — al treilea panou** (propus): la hover / → / ↵ pe rând se deschide un panou alăturat,
  lărgindu-se lin ca al doilea (aceeași animație, același „triunghi de intenție”, aceeași
  întârziere de hover), titlul lui = rândul („Folding”), aliniat cu rândul care l-a deschis;
  ← se întoarce un nivel. Când fereastra e prea îngustă pentru 3 × 300 px, al treilea panou îl
  acoperă pe al doilea, cu un antet „‹ Code › Folding” (clic / ← = înapoi). E exact modelul
  meniului nativ și al meniurilor contextuale (care au deja `ViewMenuEntry::Submenu`).
- B — grup inline expandabil (acordeon): fără lățime în plus, dar mută rândurile de sub mouse
  la deschidere și nu se împacă cu deschiderea la hover; respins.
- C — submeniurile aplatizate sub titluri mici: aduce înapoi listele lungi; respins.

**Filtrul**: caută și în submeniuri; un rezultat își arată calea întreagă, stinsă, înaintea
etichetei: „Code › Folding › Collapse All”, „Tools › Language Servers › Restart Language
Servers”; calea se scurtează (elipsă la mijloc) înaintea etichetei când nu încape. Scrisul
potrivește și titlul submeniului („folding” → toate rândurile lui). Scurtăturile la dreapta ca azi.

## 4. Pași (câte un commit)
- **51a — modelul**: `MenuAt`, secțiuni, submeniuri, `MenuItem::Submenu` / `Native`, `Menu.help`;
  `commands::menus()` construit din secțiuni; modulele migrate pe secțiuni; muda recursiv;
  drawer-ul afișează temporar submeniurile inline; `menu_path`. **Aceeași structură ca azi**
  (secțiunile reproduc meniurile actuale): `menus.golden` scris acum, snapshot-uri identice.
- **51b — noua structură**: meniurile View / Tools / Help, secțiunile și submeniurile de mai sus,
  mutările (Search in Project, Reindex / Clear caches, Theme Studio, Dependency Canvas, toggle-urile
  ferestrelor), View › Tool Windows generat, maparea Windows / Linux ({app} → File / Help, Exit),
  itemii nativi macOS, rândurile *noi* (Copy Path / Reference, Data Sources, New Terminal Tab,
  Database / Git în Tool Windows), etichetele noi și locale ro / en, iconițele meniurilor noi
  (View, Tools, Help; adăugate în set dacă lipsesc). Scurtăturile care se bat (§1, pct. 11):
  decise cu utilizatorul, apoi `keymaps.golden`. `menus.golden` actualizat; snapshot-urile cu
  drawer-ul refăcute.
- **51c — submeniurile în drawer**: al treilea panou (varianta A) cu fallback-ul îngust, navigarea
  de tastatură pe trei niveluri, filtrul cu căi, subsolul de taste. Snapshot-uri noi
  (`EDITOR_SNAPSHOT_MENU_DRAWER=keys:…` până într-un submeniu; `filter:fold`; fereastră îngustă;
  dark / light). Măsurat: deschiderea și un cadru cu trei panouri (ca la 49c).

## Decizii (user, 2026-11-03: „go” pe propunere)
- Structura propusă (secțiunea 2) aprobată ca atare.
- Zoom In / Out păstrează Mod+Shift+= / Mod+Shift+-; Expand All / Collapse All trec pe
  Mod+Shift+NumPad+ / Mod+Shift+NumPad-.
- Windows: Ctrl+G rămâne doar Find Next; „Add Selection for Next Occurrence” trece pe Alt+J.
- macOS: în preset-urile unde Replace e pe ⌘H, Replace trece pe ⌘R (⌘H = Hide).
- Ordinea: după planul 50 și după reparația flicker-ului din drawer.

## 51d — rânduri dezactivate, nu ascunse (cerut de utilizator după 51c)
Un rând al cărui context lipsește se arată stins și nu rulează (nici din tastele lui): fără
proiect, tot meniul Git e stins, nu dispare. Declarativ și ieftin:
- `command_model::Requires` — `Project`, `Repository`, `Tab`, `TextFile`, `FileOnDisk`,
  `Language(id)`, `ClosedTab`, `BackHistory`, `ForwardHistory`, `OtherTabs` — și
  `MenuContext` (starea ferestrei: proiect, depozit, câte taburi, tabul activ — text, pe disc,
  limbajul, modificat —, un tab închis, istoricul înapoi / înainte); `satisfies` / `missing`.
- Comenzile app-ului: tabelul `commands::REQUIRES`; ale modulelor:
  `CommandHandler::requires` (`workbench_model::Requiring` îl declară fără să atingă
  handler-ul), apoi `enabled(host)` (starea proprie a modulului) — app-ul nu știe de niciun
  modul.
- `Workbench::menu_context` (fără să citească textul), `row_state(id)` → `Enabled` /
  `Disabled(motivul)`, `disabled_rows`. `EditorApp::sync_menu_states` după fiecare eveniment
  compară contextul cu ultimul și doar când diferă recalculează rândurile și le trimite
  meniului nativ (`Window::set_menu_states` → muda `set_enabled`; păstrate peste un
  `set_menus`); doar fereastra cu focus (bara e a aplicației). Drawer-ul le citește la
  deschidere: rând stins (icon, etichetă, scurtătură estompate), nu se rulează la clic / ↵,
  tastatura ajunge la el și arată motivul în locul scurtăturii („Deschide întâi un proiect”,
  „Proiectul nu are un depozit Git”…); filtrul îl găsește, tot stins. Tastele: `command()`
  nu rulează ce nu se aplică (după rutarea panourilor: ⌘⌫ în arbore rămâne a lui; ⌘⇧T în
  terminal face alt shell). Paletă de comenzi nu există (lista „aceleași taste” rămâne).

| Comandă | Activă când |
|---|---|
| Save, Save As, Close Tab, Find, Find Next / Previous, Close Saved / All Tabs | un tab deschis |
| Replace, Go to Line, Comment, Folding ›, Duplicate / Delete / Move Line, Move Statement, Selection › (toate), Complete Code, Show Context Actions, Go to Declaration / Implementation / Type Declaration, Type Hierarchy | un fișier text în editor |
| Reopen Closed Tab | un tab închis (în terminal: alt shell, oricum) |
| Copy Path / Reference… | fișierul activ e pe disc |
| Search in Project…, Go to File… | un proiect |
| Select in Project View | un proiect și fișierul pe disc |
| Back / Forward | un tab vizitat înainte / după, încă deschis |
| Next / Previous Tab, Close Other Tabs | alt tab decât cel activ |
| Git: Commit…, Show Changes, Show Git Log, Pull, Push, Fetch; Tool Windows › Git | un proiect într-un depozit Git |
| Git: Annotate, Show File History, Compare With ›, Rollback…, Revert Change, Change Actions…, Show Annotated Commit | + fișierul pe disc (apoi ce cere modulul: Revert Change doar cu modificări față de commit) |
| Reindex Project, Dependency Canvas | un proiect |
| Show Local History | fișierul pe disc, în proiect |
| Reformat File, Check File | un proiect și fișierul pe disc |
| Reformat Project, Check Project | un proiect |
| Reformat SQL Statement | un fișier SQL |
| Reformat with Language Server, Choose Language Engine…, Use Built-in Engine, Download Language Server | un fișier text (și ce cere modulul: un server care formatează, un limbaj cu motoare) |
| Restart Language Servers, Show Language Server Log | un proiect |
| Rename…, Change Signature…, Move…, Find Usages, Show Diagram | un fișier text pe disc |
| Compare with Clipboard | un fișier pe disc (modulul) |
| Tool Windows › Remote | un proiect remote (modulul) |
| Task Manager | măsurătorile pornite (modulul) |
| restul (New, Open, Settings, Quit, Undo / Redo / Cut / Copy / Paste / Select All, Zoom, Full Screen, notificările, Data Sources…, New Terminal Tab, Clear and Rebuild Caches…, Tool Windows › Project / Terminal / Services / Structure / Database) | mereu |

## Stare
- [x] 51a — modelul declarativ (secțiuni, submeniuri, itemi nativi), aceeași structură.
  `command_model::menus`: `MenuAt { section, order }` (în `CommandSpec.menu`), `MenuSectionSpec`
  (id, meniu, ordine, `Platforms` All / MacOs / Others — aceeași secțiune declarată o dată pe
  sistem, în alt meniu), `SubmenuSpec` (rând care deschide secțiuni; gol → dispare, un rând →
  inline). `workbench_model::menus::{bar, sections}`: id-urile meniurilor și ale secțiunilor
  app-ului. `commands.rs`: `MENUS` / `SECTIONS` / `SUBMENUS` / `NATIVE` (au dispărut
  `MENU_ORDER`, `SEPARATOR_AFTER`, mutarea forțată a lui Quit), `MenuTree` (separator doar între
  secțiuni cu rânduri, ordinea declarată — la egalitate întâi app-ul), `menus_on(System)`
  (macOS / Windows / Linux), `menu_path` / `menu_place` (paleta, editorul de scurtături),
  contribuțiile modulelor (`MenuSectionSpec` / `SubmenuSpec` prin `ctx.contribute`, Git își
  aduce secțiunea). `platform_api`: `MenuItem::Submenu`, `MenuItem::Native { item, label }`
  (`NativeItem`: Services, Hide, HideOthers, ShowAll, Minimize, Zoom, BringAllToFront),
  `Menu.help`. muda: bara construită recursiv, itemii predefiniți, `set_as_help_menu_for_nsapp`;
  meniurile contextuale native (Windows / macOS) construiesc și ele submeniuri. Drawer-ul arată
  temporar submeniurile inline. Verificat: `menus.golden` (arborele pe cele trei sisteme, cu
  toate modulele, în testul keymaps-urilor) scris din codul vechi și identic cu cel nou (doar
  grupul lui `terminal.new` în Scurtături: „View” brut → gol, ca înainte nicăieri în meniuri);
  toate capturile identice (`task-manager*` și `sql-file-attached` light — timpul interogării
  — zgomot cunoscut).
- [x] 51b — noua structură, etichetele, locale, Tool Windows generat, Windows / Linux.
  Meniurile: {app} (doar macOS: About │ Settings │ Services › │ Hide / Hide Others / Show All │
  Quit) · File · Edit (Selection ›) · View (Tool Windows ›, Appearance ›) · Find · Navigate ·
  Code (Folding ›) · Refactor · Git (Compare With ›, secțiunile modulului) · Tools (Language
  Servers ›, al modulului lsp) · Window (Minimize / Zoom │ taburi │ închideri │ Bring All to
  Front) · Help (marcat Help: căutarea macOS) — ca în §2; pe Windows / Linux Settings și Quit
  (Windows: „Exit” / „Ieșire”) la finalul lui File, About în Help. View › Tool Windows e făcut
  din `ToolWindowProvider`-ele înregistrate (`commands::set_tool_windows`; pe dock-uri, în
  ordinea butoanelor, fără bara de sus și fără o fereastră a cărei comandă are loc propriu:
  pânza, în Navigate): Project │ Terminal, Services, Git, Remote │ Structure, Task Manager,
  Database; un rând rulează toggle-ul ferestrei sau `tool-window:<id>` (Git, Remote:
  `toggle_tool`). Rânduri noi: Copy Path / Reference…, Data Sources and Drivers…, New Terminal
  Tab, Database / Git / Remote în Tool Windows. Etichetele și meniurile noi în ro / en (id-urile
  aceleași); etichete scurte în submeniu (`[menu_label]`: Compare With › Last Commit / Revision…
  / Branch…). Iconițe noi: `wrench` (Tools), `circle-help` (Help); View: `eye`. Scurtăturile
  (decise): Expand / Collapse All pe ⌘⇧ NumPad+ / NumPad− (`NumpadAdd` / `NumpadSubtract`;
  `KeyEvent.numpad` de la winit, afișate „NumPad+”), Zoom rămâne pe ⌘⇧= / ⌘⇧−; o scurtătură doar
  pentru un sistem se scrie `mac:…` / `pc:…` (`command_model::shortcut_on`, `Layers.mac`): Add
  Selection for Next Occurrence / Unselect Occurrence au ⌃G / ⌃⇧G doar pe macOS (pe Windows /
  Linux Ctrl+G e doar Find Next, ele Alt+J / Alt+Shift+J); Replace în VS Code / Zed / Sublime:
  ⌥⌘F, ⌘R pe macOS (⌘H = Hide), Ctrl+H în rest. Scurtăturile salvate de utilizator neatinse.
  `keymaps.golden` (al macOS-ului) și `menus.golden` actualizate; conflictele cunoscute rămase:
  ⌘⇧N (New Window / Go to File în JetBrains), ⌘G (Find Next / Go to Line), Ctrl+Shift+−
  (Navigate Forward / Zoom Out, VS Code pe Windows). Opționale lăsate: Open Recent ›, Keyboard
  Shortcuts…, Tools › Remote ›, ⌃⌘F, rândurile condiționate (`MenuAt.when`).
- [x] 51c — submeniurile în drawer, filtrul cu căi, snapshot-uri. Varianta A: un rând cu
  submeniu (chevron „›”, fără scurtătură) îl deschide ca al treilea panou — la hover după
  aceeași pauză (`HoverIntent` al doilea, cu triunghiul spre al treilea panou), la clic, la → /
  ↵; drawer-ul se lărgește lin cu încă 300 px, rândurile apar cu același fade; titlul și primul
  rând aliniate cu rândul care l-a deschis (urcate cât trebuie să încapă); rândul lui „deschis”
  (marcajul accentului) cât tastatura e acolo. Sub 3 × 300 px + 24 (fereastra îngustă) panoul
  acoperă submeniul, cu un antet „‹ Cod › Pliere” (clic sau ← = înapoi). `DrawerNav`: al treilea
  nivel (`NavFocus::Sub`, `open_sub`, `show_sub`, `hover_sub_item`, `leave_sub`), ↑ ↓ / ← → / ↵
  pe trei niveluri, subsolul cu tastele lui (→ deschide pe un rând cu submeniu). Filtrul caută și
  în submeniuri, potrivind și titlul lor („plier” → toate rândurile din Cod › Pliere), calea
  întreagă stinsă înaintea etichetei (`Cod › Pliere › Restrânge tot`), scurtată la mijloc când nu
  încape. Meniurile app-ului: un nivel de submeniuri în drawer (mai adânc, inline). Capturi noi
  (dark / light): `menu-drawer-<meniu>` pentru fiecare din cele 12 meniuri, `menu-drawer-third`
  (Vizualizare › Ferestre-unealtă), `-third-folding` (Cod › Pliere, aliniat cu rândul),
  `-third-narrow` (900 × 600, peste al doilea), `-filter-fold` (filtrul „plier”); schimbate ca
  urmare: `menu-drawer-short`, `-matrix` (rândurile-submeniu cu chevron); comparația completă
  cu referința oprită la cererea utilizatorului (capturile grupului `chrome` randate și
  privite). Măsurat
  (release, 1200 × 800 cu un fișier): deschiderea 0,30 ms, un cadru al ferestrei cu trei panouri
  ~66 µs (fără drawer ~57 µs); `MenuDrawer` 1,4 KB plus meniurile. Meniul nativ macOS (Services,
  Hide…, Help cu căutarea) nu se vede în capturi: verificat doar prin construcție (muda) și
  `menus.golden`.
- [x] 51d — rândurile fără context dezactivate, nu ascunse (tabelul de mai sus). Teste:
  cerințele contra contextului (`command_model`), contextul și stările pentru nimic deschis /
  proiect fără Git / proiect Git cu un fișier (un depozit făcut într-un folder temporar),
  drawer-ul refuzând un rând stins (clic, ↵, filtru), stările date rândurilor native
  (`platform-winit`, `menu::states`). Capturi noi (dark / light): `menu-drawer-git-none`
  (nimic deschis: tot Git stins, „Compară cu ›” și el), `-git-reason` (motivul pe rândul ales),
  `-git-repo` (proiectul-exemplu: Git activ, doar „Arată commit-ul adnotat” stins — fără
  adnotare); `ui-snapshots.sh` primește `-` ca fișier: fereastra goală. Meniul nativ macOS
  (`set_enabled`) neverificat vizual; pe Windows nu există bară nativă (doar drawer-ul).
  Limită: starea proprie a unui modul care se schimbă fără context (Revert Change după
  tastare) e citită din nou la schimbarea contextului (inclusiv „modificat”), la deschiderea
  drawer-ului și la apăsarea tastei.

