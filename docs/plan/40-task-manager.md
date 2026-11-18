# 40 — Task Manager ca tab în editor, responsive (planul 40)

Cerut: „Aș vrea să refac Task Manager-ul ca tab în editor, să-i schimb UI-ul să fie mult mai
frumos și bine gândit ca tab de editor, dar și gândit responsive, pentru că cineva poate să
facă split la tab și să redimensioneze split-ul — ca pe web, trebuie să fie responsive.”

## Context: ce avem acum

- **Modulul `task-manager`** (`crates/modules/tools/task-manager`, ~2 000 de rânduri) e un
  **panou lateral** (`ToolContent::Panel`, `Dock::Right`, ordinea 20, iconița `cpu`,
  `PANEL = "tasks"`). Îl deschid comanda `tasks.open` (`workbench_model::SHOW_RESOURCES`,
  meniul Window), butonul din banda din dreapta și cele trei elemente din bara de stare
  (CPU, RAM, GPU — `status.rs::push_resource_items`, fiecare `clickable(SHOW_RESOURCES)`).
- **Ce arată:**
  - titlul, comutatorul „Proiectul acesta / Toate proiectele” (`ds::segmented`), numărul de
    fire;
  - cinci carduri-histogramă (CPU, Memorie, GPU, Rețea, Disc) pe ultimul minut — ale
    aplicației sau ale părții selectate („Arată toată aplicația” revine);
  - filtrul, capetele de coloană care sortează, apoi lista: „Operații” (în curs și ultimele
    10 terminate, cu bară de progres), apoi grupurile (Aplicația, Conexiuni, Module — sau,
    pe toate proiectele, câte unul pe proiect, „Comune”, Module), care se pliază;
  - coloanele numerice pleacă de la dreapta când nu încap (`view::columns`), detaliile trec
    sub nume.
- **Colectorul** (`collector.rs`) rulează pe firul lui (`task-manager`) cât e panoul vizibil:
  o dată pe secundă citește `ResourceSource::snapshot()` / `snapshot_all()`, timpii firelor
  (`process_stats::threads`), arborii proceselor pornite de părți
  (`process_stats::ProcessTrees`), calculează ratele (`Rates`, `Sampler`) și trimite un
  `Table` întreg. Ascuns, panoul primește `ToolPanel::parked`: colectorul se oprește, sursa
  e ținută slab, iar `AppResources::watched()` (numărul de referințe tari) oprește și
  completarea părților în aplicație.
- **Contractul părților** (`workbench_model::resources`): `ResourceUse` (nume, proprietar,
  memorie, GPU, fire, grup `APP` / `SERVICES` / `MODULES`, trafic, disc, CPU măsurat,
  `activities`, `processes`), `ResourceReporter`, `DiskIo`, `CpuMeter`, `Traffic`,
  `Activities` (cu `Outcome::Stopped`, dar fără cerere de oprire), `ModuleThreads`,
  `ModuleCounters`.
- **Probleme constatate în cod**, de rezolvat odată cu redesign-ul:
  - `Recent::record` clonează tot istoricul (`HashMap<String, Series>`) la fiecare
    secundă, iar `paint_graphs` îl clonează și îl transformă în `Vec` **la fiecare cadru**;
    `items()` / `shown()` / `operations()` alocă și ele la fiecare cadru și la fiecare tastă.
  - Seriile folosesc culorile VCS (`vcs_added`, `vcs_modified`, `vcs_deleted`) — semantic
    greșit și fără contrast verificat.
  - Lățimea e tratată doar la nivel de coloane și de carduri; nu există un layout pur testat
    pe lățimi, iar înălțimea mică nu e tratată deloc.
  - Captura actuală (`EDITOR_SNAPSHOT_TASKS=1`) măsoară procesul real: valorile diferă de la
    o rulare la alta.

### Cum se deschid azi vederile în tab (pe ce construim)

- `ViewHost::open_view(title, extension, data)` → `Workbench::open_virtual(<root>/<title>.<ext>)`
  (`crates/app/src/workbench/viewers.rs`): dacă un tab cu aceeași cale există **în grupul
  curent**, îl activează; altfel cere `ViewerProvider`-ul care răspunde `ViewerKind::Replace`
  pentru extensie și îi dă `data` prin `Viewer::set_data`. Exemple: `view-diagram`
  (`insight/diagrams`), `view-canvas`, `view-data-*` (explorer), `view-local-history`.
- Iconița tab-ului vine dintr-un `languages::FileIconRule` pe extensie (exemplu:
  `database/diagram`, `FileMatcher::Extension(...)`).
- Butonul din bandă al unei vederi-tab: `ToolContent::Command { command, tab: Some(EXT) }`
  (canvas): rulează comanda și e aprins cât e activ un tab cu extensia aceea.
- **Split:** `split_tab` deschide aceeași cale într-un grup nou; pentru o vedere virtuală
  asta înseamnă **o a doua instanță de `Viewer`** (alt `provider.open`). Fiecare grup își
  pictează vederea cu `ViewerPaint::bounds` la fiecare cadru — redimensionarea split-ului
  înseamnă doar alte `bounds`, fără niciun eveniment separat.
- `Viewer` **nu** are echivalentul lui `ToolPanel::parked`: o vedere dintr-un tab inactiv
  pur și simplu nu mai e pictată, dar `poll()` îi e chemat în continuare (toate grupurile).
  Fereastra ascunsă are `window_state.occluded`.
- Ce oferă `ui-components`: `ds` (carduri `surface`, `segmented`, butoane, `search_field`,
  `progress_bar`, `empty_state`, `loader`, `badge`, `tooltip`), `Tip` / `tooltip_at`,
  `tree` (chevroane, `TreeList` virtualizat pe rânduri egale), `splitter`, `scrollbar`,
  `tag_colors` (8 culori cu nuanțe dark/light, contrast ≥ 3:1), `shortcuts`,
  `data_grid::toolbar` (layout pur cu „…”, memoizat). **Nu există** o componentă de
  grafic, un layout generic de bară cu „…” (cel din grilă e legat de uneltele ei) și nici
  o potrivire generică a coloanelor după prioritate.
- Renderer-ul desenează doar quad-uri (rotunjite, cu margine), glife și imagini: graficele
  se fac din quad-uri (bare și trepte), ca histogramele de acum și muchiile din canvas.

## De ce

Panoul lateral are ~340 px și e gândit pentru o coloană îngustă: cardurile, tabelul și
detaliile se înghesuie unul sub altul, iar graficele unei părți înlocuiesc graficele
aplicației. Ca tab, Task Manager-ul are loc pentru o pagină adevărată — rezumat, tabel,
detaliu alături — dar un tab poate fi pus într-un split și tras până la 250 px. Deci nu ajunge
„mai mare”: trebuie un layout care se recompune pe lățime și înălțime, ca o pagină web
responsive, cu puncte de rupere explicite și testat la fiecare lățime.

## Principii

- **Rămâne un modul** (`task-manager`), leneș: nimic nu pornește până la prima deschidere,
  iar firul, istoricul și sursa se eliberează când tab-ul se închide. Construiește doar cu
  `ui-components`, niciodată cu `ui-workbench`.
- **Un tab pe fereastră.** Comanda, bara de stare și butonul din bandă deschid tab-ul sau îl
  aduc în față, în orice grup s-ar afla. Split-ul rămâne permis (e cerința însăși): a doua
  vedere e o oglindă cu starea ei de UI, dar **același colector**.
- **Layout pur, memoizat**, testat la fiecare lățime ca `data_grid::toolbar`. Pictarea doar
  citește layout-ul și modelul de vedere; nimic nu se calculează pe cadru dacă nu s-a
  schimbat lățimea, starea sau datele.
- **Colectarea costă doar cât se vede**: pe pauză cât tab-ul e ascuns (alt tab activ în
  grup, fereastra ascunsă), oprită și eliberată când se închide.
- **Fiecare fereastră are proiectul ei**: „Proiectul acesta” vede doar `ProjectId`-ul
  ferestrei; „Toate proiectele” rămâne vederea explicită, ca acum. Nimic din colector nu e
  global între ferestre (hub-ul e pe sursa ferestrei).
- **Componentele care lipsesc se fac reutilizabile** în `ui-components` (grafic, bară cu
  „…”, potrivirea coloanelor, card de metrică), nu în modul.
- **Engleză în cod, textele în `locales/{ro,en}.toml`**, fără numele aplicației în
  identificatori (`view-task-manager`, firul `task-manager`, secțiunea `[tasks]`).

## Deciziile mari (cu motivul)

1. **Tab, nu panou.** `TaskManagerView: Viewer`, extensia virtuală `view-task-manager`,
   furnizorul `task-manager`, `FileIconRule` → iconița `cpu`. Panoul lateral **dispare**
   (recomandarea mea, întrebarea 1): două UI-uri pentru același lucru înseamnă cod dublu și
   un layout care trebuie oricum să meargă la 300 px — iar tab-ul pus în split dreapta face
   exact ce făcea panoul. Butonul din bandă rămâne, ca la canvas:
   `ToolContent::Command { command: tasks.open, tab: Some("view-task-manager") }`.
2. **„Focalizează dacă există” în toate grupurile.** `open_virtual` caută doar în grupul
   curent. Adăugăm în `ViewHost` `focus_view(extension) -> bool` (implicit `false`): caută în
   toate grupurile, activează grupul și tab-ul. Comanda face
   `host.focus_view(EXT) || host.open_view(title, EXT, &ResourceData(host.resources()?))`.
3. **Contractul de vizibilitate.** `Viewer::set_shown(&mut self, shown: bool)` (implicit
   nimic), chemat de aplicație când vederea unui tab devine vizibilă sau ascunsă: alt tab
   activat în grup, tab mutat, grup închis sau unit, fereastra ascunsă (`occluded`), tab
   închis (`false` înainte de drop). Detectarea „după ultimul paint” ar fi o euristică: un
   tab vizibil, dar nepictat o vreme, s-ar opri singur. Contractul e mic și îl pot folosi și
   alte vederi (terminalul, grila care reîmprospătează).
4. **Un colector partajat (hub).** `collector::Hub` per sursă (ținut slab într-o listă a
   modulului, cheia fiind adresa `Arc<dyn ResourceSource>` a ferestrei): fiecare vedere ține
   un `Subscription` (`Arc`) cu flag-ul ei `shown` și scopul ei (proiectul / toate). Firul
   rulează cât există un abonat vizibil, doarme (`Condvar`, fără polling) cât sunt toți
   ascunși și se termină când pleacă ultimul abonat. Două vederi în split = un singur fir, o
   singură măsurare pe secundă. Dacă cele două vederi au scopuri diferite, hub-ul citește
   `snapshot_all` o dată și filtrează pentru cea pe proiect (nu două citiri).
5. **Mesaje delta, istoric în inele.** Firul nu mai trimite tabelul întreg cu tot istoricul:
   trimite `Tick { generation, now: Arc<Frame> }` (valorile de acum, rândurile), iar
   istoricul stă în hub, în inele de capacitate fixă (`VecDeque` prealocat, `f32`). Vederea
   citește istoricul prin `as_slices()` sub un `Mutex` scurt, doar când îl desenează (o dată
   pe tick, nu pe cadru — vezi „Performanță”). Un abonat nou primește imediat istoricul
   existent (nu începe de la zero).
6. **Graficele din quad-uri.** `ui_components::chart`: arie în trepte (o bară de umplere cu
   alfa mic și o treaptă de 1,5 px sus pe fiecare coloană de pixeli), sau bare (ca acum)
   pentru sparkline-uri foarte mici. Mai multe eșantioane decât pixeli → agregare pe
   coloane (max pentru CPU / rate, ultima valoare pentru memorie), memoizată pe
   `(generația seriei, lățimea)`. Nu facem linii antialiasate (renderer-ul nu are căi).
7. **Responsive pe clase, cu histerezis.** Clasa de lățime (vezi „Layout”) se schimbă doar
   când lățimea trece pragul cu cel puțin 8 px logici față de clasa curentă — fără „pâlpâit”
   cât se trage separatorul pe lângă un prag. Funcția e pură: primește clasa anterioară.
8. **Detaliul nu mai înlocuiește rezumatul.** Selecția unui rând deschide un **panou de
   detaliu** (dreapta pe lat, jos pe mediu, pagină cu „‹ Înapoi” pe îngust); cardurile de sus
   arată mereu toată aplicația.
9. **Acțiunile sunt ale proprietarului părții.** Task Manager-ul nu omoară procese după PID
   pe cont propriu: o parte își declară acțiunile (`PartAction`), iar reporterul ei le
   execută (`ResourceReporter::act`). Așa terminalul își închide sesiunea curat, formatter-ul
   e oprit prin `process::Running::stop`, iar git-ul nu poate fi omorât în mijlocul unei
   scrieri.

## Organizarea pe fișiere

```
crates/modules/tools/task-manager/src/
  lib.rs            modulul: furnizorul, comanda, butonul din bandă, iconița, setările
  settings.rs       Setting<T>-urile modulului și SettingsContribution
  collector/        hub.rs (abonații, firul, pauza), sample.rs (Sampler, Rates, add_programs),
                    history.rs (inelele), frame.rs (Frame, Row, ProcessRow)
  model/            view_model.rs (grupare, sortare, filtru, selecție, rânduri vizibile),
                    format.rs (texte formatate o dată pe tick), groups.rs
  layout.rs         layout-ul pur al paginii (clase, regiuni, coloane), memoizat
  view/             mod.rs (TaskManagerView: Viewer), toolbar.rs, summary.rs, table.rs,
                    detail.rs, keys.rs, menu.rs, paint helpers
crates/ui/ui-components/src/
  chart.rs          Sparkline, TimeChart, agregarea pe coloane, scala, citirea la hover
  overflow.rs       bara cu „…”: elemente cu priorități, ce încape, ce intră în meniu
  columns.rs        coloanele unui tabel după prioritate, cu alegere și fixare de utilizator
  metric_card.rs    cardul de metrică (etichetă, valoare, subtext, sparkline, stare)
```

Testele, ca peste tot, în `tests/unit/` oglindind `src/` (`tests/unit/layout.rs`,
`tests/unit/collector/hub.rs`…), iar cele care trec prin aplicație în
`crates/app/tests/unit/workbench/`.

## Contracte noi sau schimbate

- **`ui-components::Viewer::set_shown(bool)`** — implicit gol (decizia 3).
- **`workbench_model::ViewHost::focus_view(extension) -> bool`** — implicit `false`.
- **`workbench_model::resources`:**
  - grupuri noi, constante ca acum: `TERMINALS`, `INDEXING`, `PROGRAMS` (procese externe:
    git, formatter-e și checker-e), `LANGUAGE_SERVERS` (rezervat; vezi mai jos). Raportorii
    existenți se mută: terminalul → `TERMINALS`, indexul proiectului → `INDEXING`, „Git” și
    „Formatters and checkers” → `PROGRAMS`; conexiunile rămân `SERVICES`.
  - `ResourceUse::actions: Vec<PartAction>`, cu
    `PartAction { id: &'static str, kind: ActionKind, target: ActionTarget, confirm: bool }`,
    `ActionKind` = `Stop | Restart | Release | Disconnect`, `ActionTarget` = `Part |
    Process(u32) | Activity(u128)`. Eticheta vine din `kind` (localizată în modul), deci
    datele rămân `Eq`.
  - `ResourceReporter::act(&self, part: &PartRef, action: &str, target: ActionTarget) ->
    ActOutcome` (implicit `Unsupported`), `PartRef { owner, name, project }`. Trebuie să fie
    rapid și să nu blocheze: munca grea pornește pe firul modulului; rezultatul ajunge ca
    notificare (`ProjectContext.notifier`).
  - `ResourceSource::act(...)` — aplicația (`AppResources`) îl trimite reporterului care a
    raportat partea (după `owner`); pentru părțile aplicației îl tratează ea.
  - `Activities::stoppable(id, Arc<AtomicBool>)` și `Activity::stoppable: bool`: o operație
    care verifică un flag de oprire îl înregistrează; `ActionTarget::Activity(id)` îl setează,
    iar operația se termină cu `Outcome::Stopped`.
- **`process-stats`:** `ProcessTrees::sample` dă și folosirea fiecărei rădăcini, iar
  `process_name(pid) -> Option<String>` citește numele programului (macOS `proc_name`,
  Linux `/proc/<pid>/comm`, Windows `QueryFullProcessImageNameW`), memorat pe PID cât trăiește
  procesul.

**Serverele de limbaj:** `lsp-client` e doar un adaptor; niciun modul nu pornește azi un
server LSP. Grupul `LANGUAGE_SERVERS` și `ActionKind::Restart` sunt pregătite în contract,
dar nu apar în UI până nu raportează cineva o parte în grupul acela. Planul nu adaugă un
„Repornește” fără un server real în spate.

## Ce arată tab-ul (designul)

### Bara de sus (toolbar, 40 px)

De la stânga: comutatorul de scop (**Proiectul acesta / Toate proiectele**), gruparea
(**Tip / Proiect / Listă** — „Proiect” doar pe toate proiectele), filtrul (`search_field`,
cu numărul de potriviri), apoi în dreapta: **Pauză / Reia** (îngheață ce se vede; istoricul
continuă în hub, la reluare datele sunt la zi), intervalul și durata istoricului (meniu mic),
„⋯”. Ce nu încape intră în „⋯” prin `ui_components::overflow`, în ordinea: intervalul,
gruparea, pauza, scopul; filtrul se strânge la o iconiță de lupă care se deschide peste bară
sub 360 px. Sub bară, discret: „124 de fire · la fiecare secundă · ultimul minut”.

### Rezumatul (cardurile)

Patru carduri de metrică (`metric_card`), mereu ale aplicației întregi:

| Card | Valoarea | Subtextul | Graficul |
|---|---|---|---|
| CPU | `3,4%` (dintr-un nucleu) | `din 12 nuclee · max 18% în ultimul minut` | arie în trepte, plafon minim 5% |
| Memorie | `412 MB` | `din care neatribuită 38 MB` | arie, scala de la 0 |
| GPU | `48 MB` | `memoria ferestrelor` | arie (cardul lipsește dacă GPU-ul nu se știe) |
| I/O | `↓↑ 12 KB/s` | `disc 1,2 MB/s` | două serii: rețea și disc, cu legendă |

- Un clic pe card sortează tabelul după metrica lui (inelul de focus rămâne pe card).
- Hover pe grafic: linia verticală și tooltip-ul cu valoarea exactă și momentul („acum 12 s:
  3,4%”); hover pe valoare: minim / medie / maxim pe fereastra de istoric.
- Pragurile (CPU > 80% dintr-un nucleu timp de 10 s, memoria crescând continuu 60 s) colorează
  subtextul în `warning` **și** adaugă o iconiță — nu doar culoarea.

### Tabelul părților

- **Grupuri** (pliabile, cu totaluri CPU și memorie pe antet): **Operații** (dacă există,
  sus), **Aplicația** (nucleul: interfața, fișierele deschise, randarea, sintaxa, „alte
  fire”, „memorie neatribuită”), **Indexare**, **Terminale**, **Programe externe**
  (git, formatter-e), **Conexiuni** (baze de date, proiecte remote), **Servere de limbaj**
  (doar când există), **Module**. Pe „Toate proiectele” cu gruparea „Proiect”: un grup pe
  proiect (după `ProjectId`), apoi „Comune”, apoi „Module”, ca acum.
- **Rândurile copil:** o parte cu procese (`processes`) se desface (chevron) în rânduri de
  proces: `PID · nume · CPU · memorie · disc`, cu descendenții adunați, ca acum.
- **Coloane** (cu prioritatea de păstrare; 0 nu pleacă niciodată):

  | Coloana | Prioritate | Lățime (px logici) | Note |
  |---|---|---|---|
  | Nume (iconița grupului, numele, badge proiect pe „toate”) | 0 | flexibilă, min 150 | tăiat cu „…”, numele întreg în tooltip |
  | CPU | 1 | 64 | bară fină în celulă, proporțională |
  | Memorie | 2 | 76 | |
  | Rețea | 3 | 80 | |
  | Disc | 4 | 80 | |
  | Proiect (doar pe „toate”, gruparea nu e „Proiect”) | 4 | 120 | |
  | GPU | 5 | 72 | |
  | Tendință (sparkline CPU pe rând) | 6 | 72 | doar pe lat |
  | Fire | 7 | 48 | |
  | Detalii | 8 | flexibilă, 160–360 | |

  Coloanele care nu încap pleacă de la prioritatea cea mai mare; **coloana după care se
  sortează rămâne mereu** (ia locul ultimei vizibile). Un „⋯” la capătul antetului deschide
  alegerea coloanelor: fiecare cu bifă, cele ascunse din lipsă de loc marcate „nu încape”;
  una bifată explicit e fixată (împinge altele afară), una debifată nu mai apare deloc.
  Alegerea se salvează (setarea `COLUMNS`).
- **Densitate:** 32 px pe lat, 26 px compact; numerele aliniate la dreapta, cu lățimea
  rezervată pentru cel mai lat text al coloanei (fără să „danseze” la fiecare secundă).
- **Sortare:** clic pe antet (din nou: invers), săgeata în antet; numele A→Z, numerele
  descrescător. Operațiile urmează ordinea lor (în curs, apoi terminate).
- **Selecție:** un rând; selecția supraviețuiește tick-urilor (după cheia rândului); o parte
  dispărută lasă detaliul cu „Partea nu mai rulează” și ultimele ei date, nu închide brusc
  panoul.
- **Stări goale:** „Se adună datele…” (loader, primul tick), „Nimic nu se potrivește
  filtrului” (cu „Șterge filtrul”), „Pe pauză” (bandă subțire sus, cu „Reia”).

### Panoul de detaliu

- **Antet:** iconița, numele, proprietarul (modulul), proiectul, grupul (badge), starea
  (rulează / pe pauză / oprit), ×.
- **Statistici:** acum / medie / maxim pe fereastra de istoric, pentru CPU, memorie, GPU,
  rețea, disc (doar cele pe care partea le raportează).
- **Grafice** (`chart::TimeChart`): CPU, memorie, GPU, I/O — cu axă discretă (0, maxim
  rotunjit „frumos”), linii de grilă, citire la hover. 2×2 în panouri de peste 420 px, una
  sub alta sub această lățime.
- **Fire:** numele firelor numărate pentru parte, cu CPU-ul fiecăruia (colectorul le are
  deja).
- **Procese:** PID, program, CPU, memorie, cu acțiunea „Oprește” acolo unde partea o oferă.
- **Operații:** cele ale părții, cu bară de progres, viteză, timp rămas, și „Oprește” pentru
  cele `stoppable`.
- **Acțiuni:** butoanele părții (`ds::secondary_button`, cel distructiv `danger_button`).
  Cele cu `confirm: true` întreabă printr-un dialog (×, Esc sau butoanele; niciodată clic
  în afară), rezultatul vine ca notificare (toast).

### Acțiunile oferite (în 40g)

| Partea | Acțiunea | Ce face | Există azi baza? |
|---|---|---|---|
| Terminal (o sesiune) | Închide sesiunea | terminalul își închide tab-ul și shell-ul (`PtySession` drop) | da, în modulul terminal |
| Formatters and checkers | Oprește programul | `process::Running::stop` / `kill_tree` pe PID-ul lui | da (`runner::RUNNING`) |
| Operații `stoppable` (transfer remote, export) | Oprește operația | setează flag-ul; `Outcome::Stopped` | flag-ul de oprire se adaugă (contract nou) |
| Modul cu date încărcate | Eliberează datele | `ModuleRegistry::release` pentru un modul (azi doar `release_idle` pe toate) | aproape: se adaugă eliberarea unuia |
| Sesiuni de bază de date | Închide sesiunile inactive | catalogul închide sesiunile fără tab deschis | de verificat în explorer; altfel rămâne fără acțiune |
| Git | — | nicio oprire: o scriere întreruptă lasă `index.lock` | intenționat |
| Server de limbaj | Repornește | — | nu există încă servere LSP |

## Layout-ul responsive (pur, `layout.rs`)

`layout(spec: &LayoutSpec, previous: Option<WidthClass>) -> PageLayout`, cu
`LayoutSpec { width, height, scale, has_gpu, detail_open, scope_all, grouping, columns_user,
sorted_by, text_widths }` și `PageLayout { class, toolbar, summary, table, detail, splitter,
columns, row_height, list_mode }` (dreptunghiuri în pixeli fizici, ca grila). Memoizat într-un
`kernel::Memo` pe `LayoutSpec` (lățimea rotunjită la pixel); recalculat doar când se schimbă
lățimea, înălțimea, scala sau starea care contează. Lățimea și înălțimea sunt ale zonei
vederii (`ViewerPaint::bounds` / scala), deci split-ul și fereastra se tratează la fel.

### Clasele de lățime (px logici)

| Clasa | Lățimea | Rezumatul | Tabelul | Detaliul |
|---|---|---|---|---|
| `Wide` | ≥ 1080 | 4 carduri pe un rând, 96 px, sparkline 40 px | toate coloanele care încap, 32 px, tendința | în dreapta, 360 px (tras 300–520), separator `splitter` |
| `Medium` | 760–1079 | 4 carduri pe un rând, 80 px | 32 px, fără tendință | jos, 45% din înălțime (min 220 px), separator orizontal; dacă înălțimea < 560: pagină |
| `Narrow` | 480–759 | carduri 2×2, 68 px (3 carduri fără GPU: pe un rând) | compact, 26 px, coloanele pleacă după prioritate, „⋯” în antet | jos dacă înălțimea ≥ 640, altfel pagină |
| `Compact` | 320–479 | o bandă de 4 „pastile” (valoare + micro-sparkline 28 px), 2×2 sub 400 px | **listă**: rânduri de 2 linii (numele și metrica sortată mare, la dreapta; dedesubt celelalte metrici, mici); sortarea dintr-un meniu „Sortează după…” în bară | pagină cu „‹ Înapoi” |
| `Minimal` | < 320 | o linie de text: `CPU 3,4% · 412 MB` | listă de 1 linie: nume + metrica sortată | pagină |

- **Înălțimea:** sub 420 px, rezumatul devine banda din `Compact` oricare ar fi lățimea; sub
  260 px dispare (rămâne linia de text din bară).
- **Detaliul pe pagină** (îngust sau scund): selecția cu Enter / clic deschide pagina în
  locul tabelului, „‹ Înapoi” (sau Esc) revine la același rând, același scroll.
- **Panoul de detaliu își are propriul layout** (pur, aceeași funcție pe lățimea panoului):
  grafice 2×2 peste 420 px, altfel 1 coloană; statisticile pe 2 coloane peste 360 px.
- **Separatorul** ține raportul (nu pixelii) cât se redimensionează pagina; minimul tabelului
  e 320 px, al detaliului 280 px — sub suma lor detaliul trece jos sau pe pagină (după
  tabel).
- **Histerezis:** schimbarea clasei cere 8 px dincolo de prag față de clasa curentă.
- **Toolbar-ul** folosește `overflow::fit` (decizia „ce intră în ⋯”) cu aceeași logică ca
  grila: ce nu încape pleacă în ordinea fixă, iar ce încape iar în locul rămas revine.

### Invariante testate pe fiecare lățime 200–2400 (pas 1 px) și înălțimi 200–1600 (pas 20 px)

- nicio regiune nu iese din `bounds` și nicio pereche nu se suprapune;
- clasa e monotonă în lățime (fără histerezis); cu histerezis, o mișcare dus-întors de ±7 px
  lângă un prag nu schimbă clasa;
- o lățime mai mare nu arată niciodată mai puține coloane;
- coloana sortată e mereu vizibilă; numele are mereu cel puțin minimul lui;
- coloanele vizibile + cele din „⋯” = toate coloanele prezente (nimic pierdut);
- ce e în „⋯”-ul barei n-ar fi încăput (nimic ascuns degeaba);
- rândurile au mereu cel puțin 24 px logici (ținte de clic) la orice scală (1×, 1,5×, 2×).

## Performanță și resurse

- **Pe firul UI, la un tick (o dată pe secundă):** modelul de vedere se reface (rânduri
  vizibile, totaluri de grup, texte formatate: `3,4%`, `412 MB`), iar seriile pentru grafice
  se agregă pe coloanele de pixeli; buget ≤ 0,3 ms la 200 de părți, ≤ 1 ms la 1 000.
- **Pe cadru (hover, scroll, redimensionare):** doar pictare din ce e memorat. Rândurile se
  desenează doar cele vizibile (`TreeList`). Textele formatate și modelate se refolosesc
  (sistemul de text le ține), layout-ul vine din `Memo`. **Nicio alocare în codul nostru la
  un cadru identic** — măsurat într-un test de integrare cu un alocator care numără (al
  doilea paint identic alocă 0 în modul, după încălzirea scenei).
- **Redimensionare fluidă:** în timpul tragerii separatorului se refac doar layout-ul (pur,
  O(coloane + regiuni)) și agregarea graficelor pentru lățimea nouă (memoizată pe lățime);
  graficele mari folosesc cel mult o coloană de date pe pixel fizic.
- **Firul colectorului:** ≤ 2 ms pe măsurare; doarme cât toți abonații sunt ascunși
  (`Condvar`), se termină la ultimul abonat; sursa e ținută slab între măsurări (ca acum),
  deci `AppResources::watched()` rămâne adevărat doar cât se vede ceva.
- **Memoria istoricului:** 5 serii `f32` pe parte. La 60 de eșantioane: ~1,2 KB pe parte;
  la 15 minute cu pas de 1 s: ~18 KB pe parte, ~0,7 MB la 40 de părți — plafonat la 1 MB
  (pasul se mărește automat dacă ar trece). Părțile dispărute sunt uitate după 30 s (detaliul
  lor rămâne până se închide). „Clear and Rebuild Caches” golește agregările și textele
  memorate (`Purge`), nu și istoricul măsurat.
- **Cadre:** vederea nu animă nimic continuu; cere un cadru la tick (waker-ul existent) și la
  interacțiune. Loader-ul primului tick folosește `is_loading` (ritmul loader-ului, nu al
  ecranului).
- **Windows:** numele proceselor prin `QueryFullProcessImageNameW`, fără `wmic` / PowerShell;
  testele de layout nu depind de platformă; clippy pentru Windows în `check.sh`.

## Setări (`crates/modules/tools/task-manager/src/settings.rs`)

| Setarea | Secțiune / cheie | Tip | Implicit | Validare |
|---|---|---|---|---|
| `HISTORY` | `[tasks] history` | `i64` (secunde) | 60 | 60, 300, 900 |
| `INTERVAL` | `[tasks] interval` | `i64` (secunde) | 1 | 1, 2, 5 |
| `COLUMNS` | `[tasks] columns` | `String` (`cpu,memory,-gpu,…`) | `""` (după prioritate) | id-uri cunoscute, `+` fixat / `-` ascuns |
| `DETAIL_RATIO` | `[tasks] detail_ratio` | `i64` (procente) | 30 | 15–60 |

Citite și scrise doar prin serviciul de setări (`SettingChanged` ajunge la hub pentru
interval și istoric). Fereastra de setări primește un bloc „Task Manager” (Aspect sau
Instrumente) prin `SettingsContribution`: durata istoricului și intervalul, ca
`SettingsRow::Segmented`. Scopul („Toate proiectele”) rămâne ținut cât rulează aplicația,
ca acum (nu e o setare).

## Temă și accesibilitate

- **Culorile seriilor** din `tag_colors` (albastru CPU, verde memorie, violet GPU, portocaliu
  rețea, cyan disc), în nuanța temei; `chart::series_color(palette, Series)` le dă, iar un
  test verifică contrastul ≥ 3:1 față de fundalul cardului în ambele teme și ≥ 4,5:1 pentru
  text (`text` / `text_muted` pe `surface`).
- Nicio informație doar prin culoare: pragurile au iconiță, săgeata de sortare are formă,
  rândurile selectate au și marginea de focus.
- **Tooltips** (`Tip`, cu tastele comenzii când are): fiecare buton iconiță, fiecare antet de
  coloană (numele întreg și ce măsoară — „CPU: procente dintr-un nucleu, poate trece de
  100%”), fiecare valoare tăiată, fiecare punct de grafic (valoarea exactă cu unitate și
  momentul), fiecare card (min / medie / max).
- **Tastatura:**
  - ↑ / ↓, Page Up / Page Down, Home / End: selecția în tabel (rândurile de grup incluse);
  - → desface grupul / partea cu procese, ← o pliază sau urcă la grup;
  - Enter: deschide detaliul (pe pagină în clasele înguste), Esc: golește filtrul → închide
    detaliul → deselectează (în ordinea asta);
  - tastarea merge în filtru (ca acum); ⌘F / Ctrl+F îl focalizează;
  - Tab / ⇧Tab: bara → rezumat → tabel → detaliu, cu inel de focus vizibil;
  - comenzi remapabile prin `ViewAction` (ca în diagrame): „Pauză / Reia (Task Manager)”,
    „Sortează după CPU / Memorie / …”, „Alege coloanele…”, „Oprește (Task Manager)” (cu
    confirmare), toate în paletă.
- **Meniul contextual** pe rând (`Viewer::context_menu`): acțiunile părții, „Copiază rândul”
  (TSV), „Copiază PID-ul”, „Arată în detaliu”, „Ascunde coloana” pe antet.

## Pașii

Fiecare pas e un commit, după `./scripts/check.sh`, cu capturile vechi identice în afară de
cele numite. Capturile Task Manager-ului folosesc o **sursă falsă, deterministă** (sub
feature-ul `snapshot`: părți, procese și serii fixe, ceasul oprit), ca imaginile să fie la fel
la fiecare rulare.

1. **40a — Tab-ul și contractele de deschidere.**
   - **Ce intră:** `Viewer::set_shown` și apelurile din aplicație (activare în grup, mutare,
     închidere și unire de grupuri, `occluded`, închiderea tab-ului); `ViewHost::focus_view`;
     furnizorul `task-manager` pe `view-task-manager`, `FileIconRule` → `cpu`; comanda
     `tasks.open` care focalizează sau deschide (cu `ResourceData`); butonul din bandă devine
     `ToolContent::Command { tab: Some(EXT) }`; elementele din bara de stare rulează aceeași
     comandă (neschimbate); panoul lateral și `ToolPanel`-ul dispar. UI-ul de azi se mută
     ca atare în `Viewer` (redesign-ul vine după), deci pasul e mic și sigur.
   - **Hub-ul colectorului** (decizia 4): un fir pe fereastră, oricâte vederi; pauză pe
     `set_shown(false)`, oprire la ultimul abonat.
   - **Teste (app):** comanda de două ori → un singur tab; cu tab-ul în alt grup, comanda îl
     activează acolo; bara de stare și butonul din bandă îl deschid; split dreapta → două
     vederi, un singur fir `task-manager`; tab inactiv → firul doarme și `watched()` e fals;
     tab închis → firul se termină, sursa e eliberată; fereastra ascunsă → pauză; două
     ferestre (proiecte) nu își văd părțile pe „Proiectul acesta”. Testul
     `a_hidden_task_manager_keeps_nothing_running` se mută pe tab.
   - **Teste (modul):** hub-ul cu o sursă falsă (abonare, pauză, reluare, istoric dat unui
     abonat nou, scopuri diferite citite o dată).
   - **Captură:** `tasks` (panoul) e înlocuită de `task-manager` (tab-ul, UI-ul vechi).
2. **40b — Datele: inele, delta, grupuri, procese.**
   - **Ce intră:** `history.rs` (inele prealocate, plafonul de 1 MB, uitarea după 30 s),
     `Tick` cu `Arc<Frame>`, `HISTORY` / `INTERVAL` / `COLUMNS` / `DETAIL_RATIO` și blocul
     din setări; grupurile noi în `workbench_model::resources` și raportorii mutați
     (terminal, indexare, git, quality); rândurile de proces (`ProcessTrees` pe rădăcină,
     `process_name` pe cele trei platforme); `model/view_model.rs` refăcut doar la tick sau
     la schimbarea stării (generație), cu textele formatate o dată.
   - **Teste:** inelele (capacitatea, ordinea, `as_slices`, plafonul), delta-urile aplicate
     dau același istoric ca tabelul întreg de acum, gruparea pe fiecare scop și grupare,
     sortarea stabilă, filtrul (nume, proprietar, detalii, PID, program), selecția care
     supraviețuiește tick-urilor, o parte dispărută; setările validate; `process_name` pe
     procesul testului însuși (toate platformele).
   - **Capturi:** niciuna schimbată (UI-ul vechi citește modelul nou).
3. **40c — Componentele din `ui-components`.**
   - **Ce intră:** `chart` (`Sparkline`: bare sau arie în trepte; `TimeChart`: axă, grilă,
     plafon, una sau două serii, agregarea pe coloane memoizată, citirea la hover ca
     `Tip`), `overflow` (elemente cu lățimi și priorități → ce se pune, ce intră în „⋯”,
     ordinea meniului), `columns` (potrivirea după prioritate, coloana fixată de sortare,
     alegerile utilizatorului, rezervarea lățimii numerelor), `metric_card`; `series_color`.
   - **Teste:** graficele pe serii goale, de un punct, constante, cu vârfuri, mai lungi
     decât lățimea (agregarea păstrează maximul), două serii; numărul de quad-uri ≤ 2 ×
     lățimea în pixeli; citirea la hover pe fiecare coloană; `overflow` și `columns` pe
     fiecare lățime (fără suprapuneri, nimic pierdut, nimic ascuns degeaba, monotonie);
     contrastul seriilor în ambele teme.
   - **Capturi:** niciuna (componentele nu sunt încă folosite).
4. **40d — Layout-ul pur.**
   - **Ce intră:** `layout.rs` cu clasele, histerezisul, regiunile paginii și ale panoului de
     detaliu, separatorul pe raport, memoizarea.
   - **Teste:** invariantele de mai sus la fiecare lățime și înălțime, la scalele 1, 1,5 și 2;
     pragurile exacte (479 / 480, 759 / 760, 1079 / 1080) cu și fără histerezis; detaliul
     care trece jos sau pe pagină după înălțime; memo-ul care nu recalculează la aceeași
     cheie.
   - **Capturi:** niciuna.
5. **40e — UI-ul nou.**
   - **Ce intră:** bara, rezumatul, tabelul grupat și virtualizat (rânduri de proces,
     coloane, „⋯” cu alegerea coloanelor, modul listă), panoul de detaliu (dreapta / jos /
     pagină), separatorul, stările goale și pauza, tooltips, ambele teme; UI-ul vechi și
     funcțiile lui (`columns`, `paint_*`) se șterg.
   - **Teste (vedere):** clic pe antet, pe card, pe grup, pe rând; detaliul la fiecare clasă;
     „‹ Înapoi” revine la rând și scroll; alegerea coloanelor salvată și aplicată; fiecare
     buton iconiță are tooltip; textele lungi tăiate cu „…”; testul fără alocări la un cadru
     identic; o redimensionare continuă 1200 → 260 px fără panică și fără suprapuneri
     (verificate pe dreptunghiurile hit-test).
   - **Capturi noi** (ambele teme):
     - `task-manager` — tab întreg (1 200 px, `Wide`), o parte selectată, detaliul în dreapta;
     - `task-manager-split` — `SPLIT=right` (~600 px, `Narrow`), detaliul jos;
     - `task-manager-compact` — split cu separatorul tras la ~400 px (`Compact`, listă);
     - `task-manager-minimal` — ~280 px;
     - `task-manager-page` — detaliul pe pagină într-un split îngust;
     - `task-manager-columns` — alegerea coloanelor deschisă;
     - `task-manager-all` — „Toate proiectele”, grupate pe proiect.

     Pentru lățimi exacte, captura primește `EDITOR_SNAPSHOT_TASKS=<scenariu>` și
     `EDITOR_SNAPSHOT_SPLIT_AT=<px logici>` (poziția separatorului primului split).
6. **40f — Tastatura, meniurile, comenzile.**
   - **Ce intră:** navigarea completă (lista de mai sus), focusul vizibil, `ViewAction`-urile
     remapabile, meniul contextual pe rând și pe antet, copierea.
   - **Teste:** fiecare tastă în fiecare clasă (în listă ← / → pe grupuri, Enter pe pagină),
     ordinea Esc, tastarea în filtru, comenzile din paletă ajung la vederea activă și nu la
     o oglindă din alt grup, meniul contextual rulează acțiunea.
   - **Capturi noi:** `task-manager-menu` (meniul contextual pe un rând).
7. **40g — Acțiunile.**
   - **Ce intră:** `PartAction`, `ResourceReporter::act`, `ResourceSource::act`,
     `Activities::stoppable`; acțiunile din tabelul de mai sus pentru terminal, quality,
     operațiile remote și export, eliberarea datelor unui modul, sesiunile de bază de date
     dacă explorer-ul o permite curat; dialogul de confirmare și toast-urile de rezultat.
   - **Teste:** domeniul (acțiunile egale, rutarea după proprietar, `Unsupported` implicit),
     fiecare modul care oferă o acțiune (cu procese false: un `sleep` pornit de test, oprit,
     arborele lui dispare), o operație oprită se termină `Stopped`, dialogul se închide doar
     prin ×, Esc sau butoane, git nu oferă nimic.
   - **Capturi noi:** `task-manager-stop` (dialogul de confirmare peste tab).
8. **40h — Bugetele, Windows, Stare.**
   - **Ce intră:** măsurătorile (tick la 200 și 1 000 de părți, pictarea, redimensionarea),
     scrise în Stare; verificarea pe Windows (nume de procese, fire, DPI 1,5×); rândul din
     indexul planurilor.
   - **Teste:** bugetele ca teste în release (`#[ignore]` în debug, ca 39a): tick ≤ 1 ms la 1 000
     de părți, layout ≤ 50 µs, agregarea graficelor ≤ 0,2 ms la 900 de eșantioane.

## Teste (strategia)

- **Pur, fără UI:** inelele, delta-urile, modelul de vedere, layout-ul (toate lățimile),
  `overflow`, `columns`, agregarea graficelor, rutarea acțiunilor.
- **Vederea, cu `TextSystem` real și scenă software:** hit-test-uri, tooltips, taste, stări.
- **Aplicația:** deschiderea / focalizarea în orice grup, split-ul cu un singur fir,
  vizibilitatea, închiderea care eliberează, ferestrele și proiectele separate.
- **Capturi:** cu sursa falsă, deterministă, în ambele teme; restul capturilor identice.
- **Fără git pe repo-uri reale**, fără servere pornite de teste; procesele de test sunt
  `sleep` / `timeout` locale, oprite la final.

## Riscuri

- **`set_shown` uitat într-un drum al aplicației** (un grup unit, un tab tras între grupuri):
  firul ar rula ascuns. Mitigare: aplicația calculează vizibilitatea dintr-un singur loc
  (după layout-ul grupurilor, o dată pe cadru, comparând cu starea anterioară) și o anunță
  doar la schimbare; testul enumeră toate operațiile pe tab-uri.
- **Oglinzile în split** pot diverge (sortări diferite) — e voit: ca două ferestre de browser.
  Comenzile din paletă merg doar la vederea activă.
- **Procese omorâte greșit:** acțiunile trec doar prin proprietar, cu confirmare la cele
  distructive; Task Manager-ul nu are „kill după PID” generic.
- **Clase care pâlpâie la tragerea separatorului:** histerezisul și testul dus-întors.
- **Capturi nedeterministe:** sursa falsă; nicio măsurare reală în captură.

## Ce nu facem acum

- Panoul lateral ca a doua formă a Task Manager-ului (vezi întrebarea 1).
- Servere de limbaj și „Repornește serverul”: așteaptă un modul care pornește un server LSP.
- Linii antialiasate și zoom pe axa timpului în grafice; exportul istoricului (CSV).
- Istoric persistat între porniri sau mai lung de 15 minute.
- Mutarea barei grilei (`data_grid::toolbar`) pe `ui_components::overflow` — se poate face
  ulterior, fără grabă (grila are deja testele ei).
- Profilarea per funcție (eșantionarea stivelor) și „ce fișier a încetinit indexarea”.
- Oprirea programelor git.

## Întrebări (cu recomandarea mea)

**Hotărâte** (2026-11-01, la „go”, vezi „Decizii” mai jos): 1 — doar tab-ul, panoul lateral
dispare; 2 — **nu** (contrar recomandării de mai jos): tab-ul nu se redeschide la pornire și
nu intră în sesiune; 3 — ca recomandarea.

1. **Rămâne și panoul lateral, ca opțiune?**
   - Recomand **nu**: tab-ul pus în split dreapta face același lucru, iar layout-ul
     `Compact` / `Minimal` acoperă lățimile panoului. Un singur UI, mai puțin cod, testat o
     dată. Butonul din bandă rămâne și deschide tab-ul.
2. **Se redeschide tab-ul la pornire (`restorable`)?**
   - Recomand **da**, ca terminalul: tab-ul revine unde era, dar colectorul pornește doar
     când tab-ul e vizibil (`set_shown(true)`), deci o pornire cu tab-ul în spate nu costă
     nimic.
3. **Acțiunile distructive cer mereu confirmare?**
   - Recomand **da** pentru oprirea unui program sau a unei sesiuni de terminal (se pierde
     munca), **nu** pentru „Eliberează datele” și „Oprește operația” (reversibile: datele se
     reîncarcă, operația se poate relua) — acolo un toast cu rezultatul ajunge.

## Stare

- **Aprobat** („go”, 2026-11-01), cu deciziile de la sfârșit.
- **40a făcut.** `ui_components::Viewer::set_shown(bool)` (implicit nimic; o vedere pornește
  ascunsă până i se spune) și `Viewer::mirror_data()` (ce primește a doua instanță deschisă
  de un split); aplicația calculează vizibilitatea dintr-un singur loc
  (`Workbench::sync_shown`, înainte de fiecare cadru și la `set_occluded`): tab-ul activ al
  fiecărui grup, cu vederea arătată (nu doar editorul), fereastra neascunsă — anunțată doar
  la schimbare, deci orice drum (activare, tragere între grupuri, split, unire, închidere)
  e acoperit; `Workbench::set_occluded` din evenimentele ferestrei (`Occluded`,
  `Focused(true)`). `ViewHost::focus_view(extension)` (implicit `false`) caută în toate
  grupurile, face grupul curent și activează tab-ul. `ResourceSource::snapshot_both()`
  (implicit cele două citiri; `AppResources` întreabă raportorii modulelor o singură dată).
  Modulul: furnizorul `task-manager` pe `view-task-manager`, `FileIconRule` → `cpu`;
  butonul din dreapta e `ToolWindow { id: "tasks", content: ToolContent::Command { tab:
  Some(EXTENSION) } }` (aprins cât tab-ul e activ); comanda `tasks.open` face
  `focus_view || open_view(…, ResourceData)`; bara de stare rulează aceeași comandă
  (neschimbată). Panoul lateral (`ToolContent::Panel`, `PANEL`, `ToolPanel`) a dispărut;
  UI-ul vechi e acum `impl Viewer for TaskManager` (`view/viewer.rs`), neschimbat altfel.
  Colectorul: `collector/sample.rs` (fostul `collector.rs`, fără fir) și `collector/hub.rs`
  — `Hub` pe adresa sursei ferestrei (o listă slabă), `Subscription` pe vedere (vizibilă,
  scopul ei, waker-ul); firul `task-manager` pornește la prima vedere arătată, doarme pe
  `Condvar` cât toate sunt ascunse, se termină la ultima plecată; sursa e ținută tare doar
  cât e arătată o vedere (deci `AppResources::watched()` e fals cât tab-ul e în spate);
  două scopuri diferite = o singură citire (`snapshot_both`), stări de rate / procese /
  istoric separate pe scop; o vedere arătată (sau care își schimbă scopul) primește imediat
  ultimul tabel al scopului ei; schimbarea scopului cere o citire imediată.
  - **Abateri:** închiderea tab-ului nu cheamă `set_shown(false)` înainte de drop —
    `Subscription::drop` face același lucru (sigur pe orice drum de închidere). Split-ul unei
    vederi virtuale deschide acum a doua instanță prin `open_virtual` (înainte trecea prin
    `open_file`, care nu găsea calea pe disc), cu `mirror_data` a primei. Tab-ul nu e
    `restorable` (decizia 2) — vederile virtuale nu intră oricum în sesiune; testat.
    Captura: cazul `TASKS=1` din `snapshot.rs` deschide acum tab-ul; `scripts/ui-snapshots.sh`
    tot nu o rulează (măsurători reale, nedeterministe) până la sursa falsă din 40e.
  - **Teste:** modulul (contribuțiile: butonul fără panou, furnizorul, iconița; hub-ul pe o
    sursă falsă — ascuns nu pornește nimic, arătat citește și ține sursa, ascuns doarme și o
    eliberează, reluat citește iar; două vederi un fir și ultimul tabel dat celei noi fără o
    citire în plus; scopuri diferite citite o dată prin `snapshot_both`, schimbarea scopului
    cu tabelul lui imediat; ultima vedere plecată oprește firul și eliberează sursa, una nouă
    face alt hub); aplicația (comanda de două ori → un tab; în alt grup → grupul lui devine
    curent; sesiunea fără el; bara de stare și butonul din dreapta îl deschid; split dreapta
    → două vederi, un hub; tab în spate → `shown` 0, sursa eliberată, `watched()` fals; în
    față iar; închis → firul se termină; fereastra ascunsă → pauză; două ferestre → două
    hub-uri, fiecare cu proiectul lui). `a_hidden_task_manager_keeps_nothing_running` mutat
    din `tree.rs` în `tests/unit/workbench/viewers.rs`, pe tab.
  - Modul rapid (cerut): `cargo fmt`, `cargo build -p app` (și cu `--features snapshot`),
    `cargo xtask check-deps`, clippy pentru `task-manager` și pe `x86_64-pc-windows-gnu`,
    testele crate-urilor atinse (task-manager, ui-components, workbench-model, app);
    `check.sh` și capturile nu s-au rulat.
- **40b făcut.** Datele: `collector/frame.rs` — `Frame` (valorile de acum și rândurile, un
  `Arc` pentru toate vederile scopului; fostul `Table` fără istoric), `Line` cu cheia
  calculată o dată (`Line::new`) și `processes: Vec<ProcessRow>` (PID, programul, CPU,
  memorie, disc, cu descendenții adunați), `Tick { generation, now: Arc<Frame> }`.
  `collector/history.rs` — `Ring` (`VecDeque<f32>` prealocat, `as_slices`, `copy_into`,
  `set_capacity`), `Trace` (cinci serii: CPU, memorie, GPU, rețea, disc), `History` pe scop în
  hub: o valoare la fiecare privire, plafonul de 1 MB (peste el pasul crește — o valoare la
  câteva priviri — și inelele se micșorează; revin când pleacă părți), părțile dispărute
  uitate după 30 s, `set_window` la schimbarea setărilor; vederea copiază seriile pentru
  graficele vechi o dată pe privire (sau la schimbarea selecției), sub lacătul scurt —
  `Recent` (care clona tot istoricul la fiecare secundă) și clonarea pe cadru din
  `paint_graphs` au dispărut. Hub-ul trimite `Tick`-uri (ultimul pe scop dat imediat),
  `Subscription::history()` dă istoricul scopului vederii; intervalul și durata vin din
  `collector::configure` (setările). Rândurile de proces: `ProcessTrees::sample` dă deja
  folosirea pe rădăcină; `ProcessRates` (CPU și disc pe PID de la privirea trecută),
  `process_stats::process_name(pid)` (macOS `proc_name`, Linux `/proc/<pid>/comm`, Windows
  `OpenProcess` + `QueryFullProcessImageNameW`, fără `.exe`), memorat de
  `ProcessTrees::name` cât trăiește procesul. Grupurile noi în `workbench_model::resources`:
  `INDEXING`, `TERMINALS`, `PROGRAMS`, `LANGUAGE_SERVERS` (rezervat); mutate: indexul
  proiectului și „Index and search” al aplicației → `INDEXING`, terminalul → `TERMINALS`,
  „Git” (ambele părți) și „Formatters and checkers” → `PROGRAMS`. Modelul de vedere
  (`model/`): `groups.rs` (`Grouping::{Kind, Project, List}` — pe proiect doar pe „Toate
  proiectele” —, ordinea tipurilor, apartenența), `format.rs` (textele scrise o dată:
  `PartTexts`, ale operațiilor), `view_model.rs` (`ViewState`, `ViewModel::build` — operațiile,
  antetele cu totalurile scrise, părțile filtrate și sortate stabil, cheile vizibile —,
  `selection` cu partea dispărută păstrată cu ultimele valori și marcată); vederea îl ține
  într-un `Arc` și îl reface doar când se schimbă privirea sau starea (generațiile), deci
  pictarea și tastele nu mai alocă lista la fiecare cadru; „Clear and Rebuild Caches” golește
  modelul și graficele copiate (`Viewer::purge`), nu istoricul. Setările (`settings.rs`):
  `HISTORY` (60 / 300 / 900), `INTERVAL` (1 / 2 / 5), `COLUMNS` (`cpu,+gpu,-threads`,
  `columns_of`), `DETAIL_RATIO` (15–60) și blocul „Task Manager” în Aspect (istoricul și
  intervalul ca `Segmented`), aplicate hub-urilor prin `apply`. UI-ul vechi citește modelul
  nou și arată și grupurile noi (Indexare, Terminale, Programe externe, Servere de limbaj
  când există); localizările în ambele limbi.
  - **Abateri:** UI-ul vechi tot golește selecția când partea dispare (comportamentul de
    azi); modelul are deja `selection` cu `gone` pentru panoul de detaliu din 40e. Rândurile
    de proces sunt în model și în filtru (PID, program), dar UI-ul vechi nu le desenează
    (40e). `COLUMNS` și `DETAIL_RATIO` sunt declarate și validate, folosite din 40d / 40e.
    Graficele vechi au atâtea bare cât ține inelul (la 15 minute, 900; agregarea pe coloane
    vine cu `chart` în 40c).
  - **Teste:** inelele (capacitatea, ordinea, `as_slices`, `copy_into`, redimensionarea),
    privirile adăugate una câte una dau aceleași serii ca tabelul întreg de dinainte, o parte
    dispărută păstrată 30 s apoi uitată, plafonul (40 de părți × 900 încap; 200 cer un pas,
    sub 1 MB; revin la pas 1), fereastra schimbată; rata programelor și rândul lor (numele
    procesului testului), un frame cu rândurile de proces ale părții; hub-ul (istoricul pe
    scop, urmând scopul vederii); modelul (gruparea pe tip, listă și pe proiect — ca tip pe
    „Proiectul acesta” —, pe toate proiectele după identitate, sortarea stabilă, filtrul pe
    nume / proprietar / detalii / PID / program, selecția care supraviețuiește privirilor și
    partea dispărută, operațiile cu textele scrise); vederea (modelul refăcut o dată pe
    privire sau schimbare, `purge`); setările validate și blocul lor; `process_name` pe
    procesul testului; aplicația (părțile în grupurile lor).
  - Modul rapid: `cargo fmt`, `cargo build -p app`, clippy pentru `task-manager` și
    `process-stats` (și pe `x86_64-pc-windows-gnu`), testele crate-urilor atinse; `check.sh`
    și capturile nu s-au rulat. Capturi care se vor schimba: pagina de setări Aspect (blocul
    nou „Task Manager”); lista Task Manager-ului are grupurile noi (în afara scriptului).
- **40c făcut.** Componentele noi din `ui-components`, încă nefolosite de modul:
  `chart.rs` — `Series` (CPU / memorie / GPU / rețea / disc) și `series_color` (albastru,
  verde, violet, portocaliu, cyan din `tag_colors`, în nuanța temei), `Samples` (cele două
  felii ale unui inel, necopiate), `Plot` (valorile pe coloane: cea mai nouă la dreapta,
  `Reduce::Max` păstrează vârful, `Reduce::Last` nivelul; `fill` refolosește memoria),
  `PlotCache` (refăcut doar la altă generație a seriei, alt număr de coloane sau alt istoric;
  `Purge`), `nice_ceiling` / `scale_for` (plafonul „frumos”, cu minim), `sparkline` (bare sau
  arie în trepte: umplere slabă + treaptă de 1,5 px, rulajele egale unite, trepte verticale
  doar când coloana are ≥ 3 px — deci cel mult două quad-uri pe coloana de pixeli),
  `time_chart` (grila, plafonul scris sus, una sau două serii, la hover linia și câte un punct
  pe serie) și `Plot::reading` (coloana de sub mouse, valoarea, cât de demult). `overflow.rs` —
  bara cu „…” generică (`OverflowItem`: lățime, minim pentru unul flexibil, stânga / dreapta,
  ordinea în care pleacă, ordinea din meniu; `fit`: pleacă pe rând până încape, apoi revine ce
  încape iar, cel mai dorit întâi; `fits` fals când nici ce rămâne nu încape). `columns.rs` —
  coloanele după prioritate (`ColumnSpec`: prioritatea, 0 nu pleacă; lățimea sau intervalul
  unei flexibile; `reserve` — cel mai lat text, ca numerele să nu „danseze”; `Choice::{Auto,
  Pinned, Hidden}`; `fit`: pleacă prioritatea cea mai mare, cele fixate doar când nu mai e
  altceva, cea sortată niciodată; ce rămâne lărgește flexibilele, prima ia restul;
  `dropped` / `hidden` pentru alegerea coloanelor; `cramped`). `metric_card.rs` — cardul
  (eticheta, valoarea mare, linia de sub ea — peste prag în `warning` **și** cu iconița
  `alert` —, sparkline-ul jos, legenda pentru două serii, hover, marginea accent când tabelul e
  sortat după el, inelul de focus; scund: eticheta și valoarea pe un rând) și forma de pastilă
  (`CardForm::Pill`); `CardAreas` (valoarea și graficul, pentru tooltips și citire).
  `ds::draw_ellipsized`: un text tăiat cu „…” desenat fără nicio alocare (gliful care încape,
  apoi „…”, ambele modelate o dată de sistemul de text).
  - **Abateri:** monotonia barei cu „…” nu e „mai lat = cel puțin la fel de multe elemente”
    (o regulă de priorități n-o poate ține: la 415 px încap două elemente mici în locul unuia
    mai dorit care la 416 px intră); testul cere în schimb că o bară mai lată nu schimbă
    niciodată un element pe unul mai puțin dorit. Contrastul textului (`text` / `text_muted`
    ≥ 4,5:1) se verifică în 40e, pe temele reale (în `ui-components` nu sunt teme încărcate);
    aici, culorile seriilor ≥ 3:1 pe fundalul închis și pe cel deschis, toate diferite.
  - **Teste:** graficele (serie goală, un punct, constantă — un singur rulaj —, cu vârfuri, mai
    lungă decât lățimea: maximul păstrat / ultimul nivel; două felii = una; ≤ 2 quad-uri pe
    coloana de pixeli la zece lățimi și trei istorice, bare și trepte; citirea pe fiecare
    coloană, cea mai nouă la margine; două serii cu citire; scala; cache-ul), `overflow` pe
    fiecare lățime 380–1 400 (fără suprapuneri, în bară, nimic pierdut, nimic ascuns degeaba,
    monotonia de mai sus), ordinea și meniul, revenirea unuia îngust, prea îngust; `columns` pe
    fiecare lățime cu cinci sortări (fără suprapuneri, nimic pierdut, numele cu minimul lui,
    sortata vizibilă, mai lat = cel puțin la fel de multe), ordinea plecării, fixate și ascunse,
    rezervarea numerelor și creșterea flexibilelor, prea îngust; cardul (în dreptunghi la orice
    lățime și formă, pragul cu semnul lui, legenda și valoarea lungă tăiată, inelul și marginea);
    `draw_ellipsized`.
  - Modul rapid: `cargo fmt`, clippy pentru `ui-components`, testele lui, `cargo build -p app`;
    `check.sh` și capturile nu s-au rulat. Capturi: niciuna schimbată (componentele nu sunt
    încă folosite).
- **40d făcut.** `layout.rs` (pur): `WidthClass::{Minimal, Compact, Narrow, Medium, Wide}`
  cu `BREAKPOINTS` 320 / 480 / 760 / 1080 px logici, `class_of` (fără histerezis) și
  `width_class(width, previous)` (schimbarea cere 8 px dincolo de prag, în ambele sensuri);
  `ColumnId` (numele, CPU, memoria, rețeaua, discul, proiectul, GPU, tendința, firele,
  detaliile — cu prioritățile și lățimile din tabelul planului și id-urile din
  `[tasks] columns`), `present` (proiectul doar pe „Toate proiectele” negrupat pe proiect,
  GPU-ul când se știe, tendința doar pe `Wide`); `Tool` (scopul, gruparea, filtrul, pauza,
  intervalul, „Sortează după…” în modurile listă) așezate prin `overflow::fit` (pleacă
  intervalul, gruparea, pauza, scopul; filtrul se strânge la 140 px și devine lupă sub
  360 px); `LayoutSpec` (zona vederii și scala, GPU-ul, detaliul deschis, scopul, gruparea,
  alegerile coloanelor, coloana sortată, lățimile textelor numerelor și ale uneltelor,
  raporturile detaliului în dreapta și jos) → `PageLayout` (clasa, bara și uneltele ei,
  lupa, linia de stare, rezumatul — `SummaryForm::{Cards, Pills, Line, None}` cu cardurile
  lui: 4 pe un rând de 96 / 80 px, 2×2 de 68 px — 3 pe un rând fără GPU —, pastilele
  (2×2 sub 400 px), linia de text pe `Minimal` —, tabelul cu antetul, rândurile și „⋯”-ul
  alegerii coloanelor, coloanele potrivite prin `columns::fit` cu rezervarea textelor,
  înălțimea rândului — 32 / 26 compact / 44 pe două linii / 30 pe una, niciodată sub 24 px
  logici —, `ListMode::{Table, TwoLines, OneLine}`, locul detaliului `DetailPlace::{Right,
  Below, Page}`, zona lui, separatorul cu orientarea lui și `DetailLayout`). Înălțimea: sub
  420 px banda de pastile, sub 260 niciun rezumat (rămâne linia de stare); detaliul în dreapta
  pe `Wide` (raportul lui, 300–520 px, tabelul ≥ 320), jos pe `Medium` de la 560 px înălțime
  și pe `Narrow` de la 640 (45%, cel puțin 220 px), altfel pagină în locul rezumatului și al
  tabelului. `detail_layout` (grafice 2×2 peste 420 px, cifrele pe două coloane peste 360).
  `LayoutCache` (un `kernel::Memo` pe `LayoutSpec` — fără date pe heap, deci copierea cheii nu
  alocă —, clasa găsită devine `previous` pentru următoarea, `Purge`).
  - **Abateri:** marginea paginii e aceeași (12 px) pe toate clasele cu tabel, ca o coloană să
    nu plece la schimbarea clasei; `columns::fit` (40c) aduce înapoi, după plecarea pe
    priorități, coloanele care încap iar (cea mai dorită întâi) — fără asta, la trecerea în
    `Wide` tendința (6) putea scoate și firele (7), deci mai puține coloane la o lățime mai
    mare. Monotonia coloanelor se cere cu detaliul închis: deschis în dreapta, pe `Wide`
    detaliul ia locul lui din tabel (voit). Pe pagină, detaliul ia și locul rezumatului
    (mai mult loc pe un ecran îngust sau scund).
  - **Teste:** invariantele la fiecare lățime 200–2 400 (pas 1 px) și înălțime 200–1 600
    (pas 20 px), la scalele 1, 1,5 și 2 (în bounds, fără suprapuneri între regiuni și între
    carduri, uneltele în bară, separate, niciuna pierdută, rândurile ≥ 24 px logici, antetul,
    „⋯”, coloanele fără suprapuneri și înainte de „⋯”, nimic pierdut, sortata vizibilă, numele
    ≥ 150 px; clasa monotonă și coloanele mai multe pe lățime) și, la fiecare 7 px, cu
    detaliul deschis și pe „Toate proiectele” sortat după memorie; pragurile exacte (319 / 320,
    479 / 480, 759 / 760, 1 079 / 1 080) cu și fără histerezis, dus-întors cu ±7 px, salturile
    mari; fiecare clasă cu rezumatul, rândurile, tendința și locul detaliului; înălțimea
    (banda, nimic, linia de stare, jos / pagină la 560 / 640, pagina în locul tabelului,
    detaliul jos ≥ 220 px); detaliul în dreapta pe raport și în limite, separatorul între
    ele; ordinea uneltelor puse în „⋯” și lupa sub 360 px; memo-ul (o singură calculare pe
    aceeași cheie, histerezisul prin el, `Purge`); `detail_layout`; coloanele ascunse /
    fixate și coloana proiectului.
  - Modul rapid: `cargo fmt`, clippy pentru `task-manager`, testele lui și ale
    `ui-components`, `cargo build -p app`; `check.sh` și capturile nu s-au rulat. Capturi:
    niciuna (layout-ul nu e încă folosit de vedere).
- **40e făcut.** UI-ul nou (`view/`): `mod.rs` (starea vederii, selecția, detaliul, pauza,
  preferințele, `draw` — fondul, bara, rezumatul, tabelul, detaliul, separatorul peste ele,
  popover-ele), `texts.rs` (etichetele în limba utilizatorului și textele scrise o dată pe
  privire: valorile și liniile cardurilor — CPU „din N nuclee · max X”, memoria „din care
  neatribuită”, GPU-ul „memoria ferestrelor”, I/O „↓↑ rețea · disc” —, min / medie / max pe
  istoric, pragurile — CPU > 80% 10 s, memoria crescând 60 s —, linia de stare „N fire · la
  fiecare secundă · ultimul minut”, cifrele și scalele detaliului, cel mai lat text al fiecărei
  coloane și lățimile uneltelor pentru layout), `toolbar.rs` (scopul și gruparea ca
  segmentate, filtrul cu numărul de potriviri și ×, lupa care îl deschide peste bară sub
  360 px, „Sortează după…” în liste, Pauză / Reia, pastila „1 s · 1 min ▾”, „…”-ul),
  `summary.rs` (cardurile `metric_card` mereu ale aplicației întregi, pastilele, linia de
  text; clic = sortare după metrica lui; citirea pe grafic), `table.rs` (antetul cu săgeata
  sortării și „⋯”-ul alegerii coloanelor, grupurile pliabile cu totalurile, rândurile
  virtualizate — toate la fel de înalte —, iconița grupului, chevronul care desface programele
  unei părți în rânduri de proces cu ghidaj, celula CPU cu bara fină, tendința ca sparkline pe
  rând, proiectul, detaliile; operațiile cu iconița, starea și progresul pe piciorul rândului;
  modurile listă pe două linii și pe una; pauza ca bandă cu „Reia”, „Se adună datele…” cu
  loader, „Nimic nu se potrivește filtrului” cu „Golește filtrul”), `detail.rs` (antetul —
  iconița, numele, proprietarul · proiectul, badge-urile grupului și stării, × sau „‹ Înapoi”
  pe pagină —, banda „Partea nu mai rulează”, cifrele acum / medie / max, graficele
  `time_chart` 2×2 sau una sub alta cu citire, firele cu CPU-ul lor, programele, operațiile
  părții cu bară de progres; derulat), `menu.rs` (popover-ele: „…”-ul cu ce n-a încăput,
  intervalul și istoricul, sortarea, alegerea coloanelor — bifate cele arătate, „nu încape”,
  „fixată”, „După prioritate”; un clic în afară sau Esc închide; alegerea unei coloane îl ține
  deschis), `viewer.rs` (mouse, taste — tastarea în filtru, ↑ / ↓, Enter deschide detaliul,
  Esc: popover → filtru → detaliul → selecția —, rotița pe listă sau pe detaliu, separatorul
  tras pe raport, cursorul, tooltips: fiecare buton iconiță, fiecare antet cu ce măsoară,
  fiecare card cu min / medie / max, fiecare punct de grafic „acum 12 s: 3,4%”, numele tăiate
  întregi). Selecția supraviețuiește privirilor; o parte dispărută rămâne cu ultimele date
  (`last_selected` ținut ca `Arc`). Pauza îngheață ce se vede, ultima privire așteaptă.
  Preferințele (`Preferences`: coloanele, raportul detaliului, intervalul, istoricul) vin din
  setări (`settings::apply` → `set_preferences`) și se scriu prin comanda internă `tasks.save`
  (`settings::save_preferences`, `columns_text` / `choices_of`); intervalul și istoricul
  aleși din pastilă se aplică hub-urilor imediat. Tema: o copie a paletei cu `text_muted`
  adus spre `text` cât să aibă 4,5:1 pe card (`severity::readable`, făcută o dată pe temă).
  Nicio alocare la o repictare identică (`ds::draw_ellipsized`, `ds::segmented_with`,
  layout-ul în `Arc`, texte și grafice memorate, hit-uri `Copy`). Captura: sursa falsă
  `demo.rs` (sub `snapshot` și în teste: părți fixe în fiecare grup cu programe, operații și
  fire, două proiecte, istoric de 60 de priviri din valuri fixe, ceasul oprit), comanda
  vederii `demo:<scenariu>`; `EDITOR_SNAPSHOT_TASKS=wide|split|compact|minimal|page|columns|
  all` (`1` rămâne măsurarea reală) și `EDITOR_SNAPSHOT_SPLIT_AT=<px>` (`place_first_divider`;
  `close_left_panel` pentru tab-ul întreg); cele șapte cazuri în `scripts/ui-snapshots.sh`.
  Iconița nouă `pause`. Localizările în ambele limbi (cheile vechi `summary`, `whole_process`,
  `show_whole` șterse). UI-ul vechi (`view.rs`, `view/paint.rs`, `columns`, `paint_*`) șters.
  - **Abateri:** `SPLIT_AT` e poziția separatorului de la marginea stângă a **ferestrei** (px
    logici), nu a zonei editorului — fără a depinde de lățimea arborelui. Fereastra capturilor
    are 700 px logici înălțime, deci un split îngust nu are loc pentru detaliul jos (pragul de
    640 px): `task-manager-split` arată tabelul `Narrow` cu un rând selectat, fără detaliu (pagina
    are cazul ei); valorile `SPLIT_AT` din script sunt estimate (nerandate) și pot cere ajustare
    la prima randare. Testul fără alocări e un test de unitate cu un alocator global care numără
    doar pe firul testului (nu un test de integrare: sursa falsă e sub `cfg(test)` /
    `snapshot`). Panoul de detaliu nu are încă acțiunile părților (40g) și nici navigarea
    completă cu tastatura, inelul de focus și meniul contextual (40f). Densitatea „32 / 26”
    urmează clasa (nu o alegere a utilizatorului). Cardul de 80 px (`Medium`) pune eticheta și
    valoarea pe un rând, cu linia dedesubt, ca graficul să aibă loc.
  - **Teste:** vederea (sortarea și filtrul, plierea, modelul o dată pe privire, selecția care
    rămâne și partea dispărută cu ultimele valori și detaliul deschis, scopul, operația care
    își selectează partea, `purge`; clic pe antet — din nou invers —, pe card, pe grup, pe rând,
    pe chevron; detaliul la fiecare clasă — dreapta, jos, pagină după lățime și înălțime —;
    „‹ Înapoi” și Esc la același rând și scroll; alegerea coloanelor — ascunsă, salvată prin
    `tasks.save`, aplicată, fixată, după prioritate, un clic în afară doar închide —; pauza;
    „Golește filtrul”; tooltip-ul fiecărui buton iconiță și antet la trei lățimi; numele lung
    tăiat în coloana lui, întreg în tooltip; mânerul separatorului deasupra (`is_on_top`) pe
    dreapta și jos; separatorul tras pe raport și salvat; o redimensionare continuă 1 200 → 260
    px la două înălțimi și două scale fără panică, cu toate hit-urile în vedere și fără
    suprapuneri între regiuni; o repictare identică — 0 alocări — la șapte scenarii);
    setările (preferințele scrise și citite înapoi, raportul în limite); `frame` cu firele
    părților și CPU-ul lor; aplicația (Task Manager-ul pe temele implicite: seriile ≥ 3:1,
    textul și textul estompat ≥ 4,5:1 pe card; eticheta comenzii noi; testele existente ale
    tab-ului); `ui-components` (`severity::readable`, cardul scund cu linia lui).
  - Modul rapid: `cargo fmt`, clippy pentru `task-manager` și `ui-components`, `cargo build -p
    app` (și cu `--features snapshot`), `cargo xtask check-deps`, testele crate-urilor atinse
    (task-manager, ui-components, ui-workbench, icons, i18n, app — cele trei teste ale
    indexului PHP din aplicație au căzut în rularea completă, ca înainte, și trec singure);
    `check.sh` și capturile nu s-au rulat. Capturi noi (nerandate): `task-manager`,
    `task-manager-split`, `task-manager-compact`, `task-manager-minimal`, `task-manager-page`,
    `task-manager-columns`, `task-manager-all`; de schimbat: niciuna dintre cele vechi (Task
    Manager-ul nu era în script).
- **40f făcut.** `view/keys.rs`: focusul (`Focus::{Tool, More, Card, Chooser, Table,
  Detail}`) și cursorul tabelului (`RowRef::{Group, Part, Process, Operation}` — rândul după ce
  arată, deci supraviețuiește privirilor și sortării; locul lui găsit o dată pe model și pe
  schimbare). ↑ / ↓ (din nimic ↓ merge la prima parte), Page Up / Page Down, Home / End prin
  toate rândurile (și antetele de grup, programele, operațiile), partea rândului selectată (a
  programului, a operației), detaliul urmând alături, rândul adus în vedere; → desface grupul /
  programele părții, din nou intră în primul rând; ← pliază sau urcă (programul la parte, partea
  la grup); Enter deschide detaliul (pe pagină în clasele înguste — pagina primește tastele, ↑ /
  ↓ / Page Up / Page Down o derulează) sau pliază grupul; Esc: popover → filtrul → detaliul →
  selecția → tastele înapoi la tabel. Tastarea merge în filtru (`Viewer::key` când filtrul n-are
  tastele, `text_input` când le are); în filtru ← / → / Home / End mută caret-ul, ↑ / ↓ trec prin
  ce găsește. Tab / ⇧Tab: bara (scopul, gruparea, filtrul, „Sortează după…”, pauza, intervalul,
  „…”) → cardurile → „⋯”-ul coloanelor → tabelul → detaliul, în ordinea desenată (`stops`, refolosit);
  Enter face ce face clicul, ← / → schimbă scopul / gruparea / cardul; popover-ele cu ↑ / ↓ / Home
  / End / Enter (rândul tastelor luminat), Tab le închide. Inelul de focus (`ds::focus_ring`, nou
  în `ui-components`, folosit și de `metric_card`) apare doar după o tastă, nu după un clic, cât
  vederea are tastele: în jurul controlului, al cardului, al rândului cursorului. `view/commands.rs`:
  comenzile vederii și meniurile contextuale — pe rând („Arată în detaliu”, „Copiază rândul” — TSV,
  coloanele în ordinea lor, fără tendință —, „Copiază PID-ul” — al programului sau toate ale
  părții; pe grup „Pliază / Desface”; tastele se mută întâi pe rând, fără să deschidă detaliul),
  pe antet („Sortează după X”, „Ascunde «X»” — salvat —, „Alege coloanele…”, „După prioritate”).
  ⌘C copiază rândul (`selected_text`; textul selectat din filtru când filtrul are tastele).
  Acțiunile remapabile (`ACTIONS`, `ViewAction` pe vederea `task-manager`): Pauză / Reia,
  Filtrează (⌘F / Ctrl+F), Alege coloanele…, Sortează după nume / CPU / memorie / rețea / disc /
  GPU / fire, Arată în detaliu, Copiază rândul, Copiază PID-ul — în paletă, localizate
  (`[command]`). Aplicația: o comandă rulată pe vedere (`WorkbenchHost::view_command`) își face
  acum cererea (textul pentru clipboard → `ViewerRequest`, o comandă → `request_command_named`),
  restul așteaptă următoarea acțiune (`held_view_action`). `Viewer::status` spune „Pe pauză”.
  Retușuri după prima privire la capturi: bara fină din celula CPU are o pistă și e o parte din
  cea mai ocupată parte (nu din 100%), deci nu mai apare ca un punct; numerele cu separatorul
  zecimal al limbii (`i18n::decimal`: `3,4%`, `1,2 MB/s`, ca `i18n::bytes`); textele de la
  capătul drept al rândurilor lasă loc barei de derulare (16 px); totalurile grupurilor în liste
  cu fontul UI; cifrele detaliului au „medie · max” toate alături sau toate dedesubt; pe „Toate
  proiectele” operațiile spun și proiectul; sursa falsă are numele și detaliile părților
  localizate (cum le scrie aplicația).
  - **Abateri:** `wants_text` e adevărat doar cât filtrul are tastele (altfel acțiunile vederii
    n-ar avea niciodată tastele — `keyed_view` cere o vedere fără text); un caracter tastat oriunde
    deschide filtrul prin `Viewer::key`. „Oprește (Task Manager)” vine cu acțiunile, în 40g. În
    filtru, ↑ / ↓ trec prin rânduri dar tastele rămân în filtru (ca o listă de căutare). Inelul
    nu se desenează în jurul cardurilor de vederea însăși (cardul își desenează inelul).
  - **Teste:** fiecare tastă la fiecare clasă (1 300 / 900 / 600 / 400 / 280 px: ↑ / ↓ / Home /
    End / Page Up / Page Down, rândul adus în vedere; ← / → pe grupuri și pe programele părții;
    Enter — pagina în clasele înguste, cu tastele ei, Esc la același rând; Enter pe grup pliază),
    ordinea Esc, tastarea în filtru și caret-ul doar acolo, Tab / ⇧Tab în ordinea desenată, inelul
    fiecărui control în vedere, niciun inel după clic, controalele cu Enter și săgeți, popover-ul
    cu tastele; meniul unui rând (intrările, rândul copiat ca TSV și prin ⌘C, PID-urile părții și
    ale programului, detaliul), al unui grup, al unui antet (sortare, ascundere salvată, numele
    nu se ascunde, după prioritate), fiecare acțiune din `ACTIONS` rulată de vedere; aplicația:
    cu tab-ul în split, „tasks.pause” din paletă ajunge doar la vederea cu tastele (nu la oglindă),
    iar „tasks.copy_row” dă textul aplicației (`ViewerRequest(CopyText)`); modulul își declară
    acțiunile `When::View("task-manager")`.
  - **Capturi** (excepție de la modul rapid, cerută): randate doar cele `task-manager*`, ambele
    teme, cu HOME izolat, privite și corectate; valorile `SPLIT_AT` din 40e au ieșit bune (600 /
    400 / 280 / 450 px). Nouă: `task-manager-menu` (meniul pe rândul terminalului, cu Operații și
    Aplicația pliate în scenariul `menu`). Toate opt cazurile s-au schimbat față de 40e (nerandate
    atunci).
  - Modul rapid altfel: `cargo fmt`, `cargo build -p app` (și cu `--features snapshot`), clippy
    pentru `task-manager`, `ui-components`, `i18n`, testele task-manager, ui-components, i18n și
    app (cele trei teste ale indexului PHP au căzut în rularea completă, ca înainte, și trec
    singure).
- **40g făcut.** Contractul (`workbench_model::resources`): `ResourceUse::actions:
  Vec<PartAction>`, `PartAction { id, kind, target, confirm }` (`new`, `confirmed`), `ActionKind::
  {Stop, Close, Restart, Release, Disconnect}`, `ActionTarget::{Part, Process(u32), Activity(u128)}`,
  `PartRef { owner, name, project }` (`is`), `ActOutcome::{Done, Failed(String), Unsupported}`;
  `ResourceReporter::act` și `ResourceSource::act` (implicit `Unsupported`); `Activities::stoppable(id,
  Arc<AtomicBool>)` / `stoppable_with(id, Fn)`, `Activity::stoppable`, `Activities::stop` / `act`, iar
  `Activities::report` oferă „Oprește” (`STOP_OPERATION`) pentru operațiile care rulează și se pot
  opri. `process::Running::stop(pid)`: rularea programului îl oprește (cu ce a pornit) ca la
  timeout și se termină `Stopped` — nimeni nu omoară după PID din afară. `ModuleRegistry::release(id)` /
  `ModuleManager::release`. Aplicația (`AppResources::act`): pe rândul unui modul, „Eliberează
  datele” (oferit cât modulul are date în memorie) îl face ea; altfel acțiunea ajunge la raportorul
  care a raportat partea (după proprietar și nume; al altuia niciodată). Acțiunile: terminalul —
  „Închide sesiunea” pe fiecare shell (cu confirmare; vederea se încheie prin `closed()`, tab-ul și
  sesiunea cu ea: `CLOSING`); quality — „Oprește” pe fiecare formatter / checker (cu confirmare,
  `RUNNING.stop`); remote — transferurile (`run_all` și aplicarea din revizuire, cu flag-ul vederii)
  se pot opri, se termină `Stopped`, fără confirmare; explorer-ul — exportul se poate opri (anulat
  ca din vederea lui: fișierul temporar dispare) și „Închide sesiunile inactive” (`sweep` cu prag 0,
  pe firul `db-idle`; se redeschid la folosire), fără confirmare; git — nimic, intenționat. Vederea
  (`view/actions.rs`): butoanele părții sub antetul detaliului (cel care pierde muncă în culoarea
  `danger`), câte un buton mic pe fiecare program și pe fiecare operație care se poate opri,
  acțiunile primele în meniul contextual al rândului (pe program și pe operație doar ale lor; id-ul
  rândului `act:<id>:<țintă>`), „Oprește (Task Manager)” în paletă (`tasks.stop`: programul,
  operația sau partea rândului tastelor). Cele cu `confirm` deschid un dialog peste vedere (fundalul
  estompat, titlul „Oprești php-cs-fixer · 5200?” / „Închizi sesiunea …?”, ce se pierde, „Renunță” —
  implicit, ca un Enter rătăcit să nu piardă nimic — și acțiunea în culoarea `danger`, ×): se închide
  doar prin ×, Esc sau butoane; un clic în afară, rotița, meniul contextual și tastele nu ajung la
  nimic din spate; Tab / ← / → trec între butoane. Rezultatul fiecărei acțiuni e o notificare
  (`take_notifications` → toast: reușit, eroare cu motivul, „nu se mai poate”, „partea nu mai
  rulează”), iar hub-ul face imediat o privire (`Subscription::look_now`).
  - **Abateri:** `ActionKind` are și `Close` (eticheta „Închide sesiunea” pentru terminal, nu
    „Oprește”). Dialogul de confirmare e desenat de vedere peste tab-ul ei (un `Viewer` nu poate
    deschide un modal al aplicației), cu aceleași reguli ca modalele. Rutarea întreabă raportorii
    (`report`) cine are partea — o dată, la acțiune, nu pe cadru. Operațiile sunt oprite printr-un
    `Fn` (`stoppable_with`; flag-ul planului e `stoppable`), ca exportul să-și facă pasul de anulare
    imediat. Consolele SQL nu oferă încă acțiuni (n-au fost în tabelul planului).
  - **Teste:** domeniul (acțiunile egale ca date, `Unsupported` implicit la raportor și sursă,
    `PartRef::is`, o operație oprită prin flag și terminată `Stopped`, după care nu mai oferă nimic);
    `process` (un `sleep` listat oprit la cerere: `Stopped`, scos din listă); quality (un `sleep` al
    formatter-ului oprit prin acțiunea părții, cu confirmare, arborele lui dispare); terminalul
    (sesiunea oferită cu confirmare, închisă: vederea se încheie, shell-ul dispare din listă); remote
    (transferul oprit doar pentru proiectul lui, terminat `Stopped`); explorer-ul (sesiunile inactive
    închise, al altui proiect neatins; un export oprit din parte, `Stopped`, ținta neatinsă); git
    (nicio acțiune); `ModuleRegistry::release` (doar al lui); aplicația (rutarea după proprietar,
    eliberarea unui modul, apoi „nimic de eliberat”); vederea (fără confirmare făcută imediat, cu
    toast; eșec și „nu se poate” spuse; partea dispărută; cu confirmare — clic în afară, rotița,
    meniul nu fac nimic, ×, Esc, Enter pe „Renunță” nu fac nimic, Tab + Enter și butonul fac;
    operația și sesiunile oprite din detaliu fără întrebare; meniul rândului cu acțiunile primele,
    „Oprește” din paletă, o acțiune dispărută între timp; id-urile `act:` scrise și citite înapoi).
  - **Capturi** (randate, ambele teme): nouă `task-manager-stop` (dialogul peste tab, formatter-ul
    selectat); schimbate `task-manager`, `-page`, `-all`, `-menu` (butoanele acțiunilor în detaliu /
    meniu: sesiunea terminalului, „Oprește” pe operație).
  - Modul rapid: `cargo fmt`, `cargo build -p app` (și cu `--features snapshot`), clippy pentru
    crate-urile atinse, testele lor (task-manager, workbench-model, extensions, process, quality,
    terminal-panel, remote-projects, database-explorer, vcs-git, i18n, app — cele trei teste ale
    indexului PHP au căzut în rularea completă, ca înainte, și trec singure).
- **40h făcut.** Bugetele, ca teste în release (`#[cfg_attr(debug_assertions, ignore)]`;
  `cargo test -p task-manager --release budget -- --nocapture`, `cargo test -p ui-components
  --release budget`), măsurate pe macOS (Apple Silicon), mediana a 31–101 rulări:
  - modelul de vedere refăcut la o privire (grupat, filtrat, sortat, textele scrise): **~70 µs**
    la 200 de părți (buget 0,3 ms) și **~0,39 ms** la 1 000 (buget 1 ms);
  - istoricul unei priviri pe firul colectorului, 1 000 de părți în inele: **~25 µs** (buget
    1 ms în test, 2 ms în plan);
  - layout-ul unei pagini, la fiecare clasă 280–2 400 px, scala 2, detaliul deschis: **0,6–2 µs**
    (buget 50 µs);
  - agregarea graficelor, 900 de valori pe 28–1 800 de coloane, `Max` și `Last`: **0,8–4,7 µs**
    (buget 0,2 ms);
  - cadrele (scala 2): o repictare identică **~14 µs** pe tab-ul întreg (1 300 px) și **~5 µs** în
    split-ul îngust (400 px); un cadru de redimensionare **~16 / 6 µs**; o privire nouă (modelul
    și textele refăcute) **~60 / 40 µs** — testul cere ≤ 1 / 2 / 2 ms (loc pentru mașini mai
    lente). Fără alocări la o repictare identică (testul din 40e, încă trecut, cu acțiunile și
    inelul de focus).
  - **Windows:** `cargo clippy --workspace --all-targets --target x86_64-pc-windows-gnu` trece curat. Prima rulare a găsit probleme mai vechi, din afara Task Manager-ului, reparate aici: testul din `database-external` nu mai compila (`GridColumn` fără `computed` / `generated`), iar clippy-ul nou semnala în `database-console` un `if` de pliat, un tip prea complex (`TokensKey`, `Told`), clonări inutile în teste, iar în `database-console` / `database` liste de un singur interval (permise local, cu motivul). Numele proceselor (`QueryFullProcessImageNameW`, fără `.exe`), firele
    (`process_stats::threads`) și oprirea programelor (`process::Running::stop` → `taskkill /T` prin
    rularea lor, ca la timeout) sunt compilate și verificate de clippy pe `x86_64-pc-windows-gnu`;
    DPI 1,5× e acoperit de testele de layout (invariantele la scala 1,5) și de redimensionarea
    continuă la 1,5. Nu s-a rulat pe un Windows real (nu e disponibil aici).
  - **Abateri:** bugetul colectorului e testat pe partea lui pură (istoricul); citirea sistemului
    (fire, procese) depinde de mașină și nu are test de buget. Măsurătorile cadrelor nu includ GPU-ul
    (scena software a testelor).
  - Modul rapid: `cargo fmt`, `cargo build -p app`, testele task-manager și ui-components (în debug
    bugetele sunt ignorate; în release trec), clippy pe Windows. `check.sh` nu s-a rulat. Capturi:
    niciuna schimbată.
- **Planul 40 e încheiat** (40a–40h).

## Decizii (de la utilizator, 2026-10-31)

1. **Doar tab în editor**: panoul lateral dispare (butonul din dreapta, comanda și bara de jos deschid / aduc în față tab-ul).
2. **Nu se redeschide la pornire și nu se ține minte** în sesiune: tab-ul Task Manager nu intră în `session.toml`; după repornire nu mai e deschis.
3. **Confirmarea acțiunilor**: recomandarea rămâne (confirmare la oprirea unui program sau a unui terminal, fără confirmare la „eliberează datele”), dacă utilizatorul nu spune altfel la „go”.
