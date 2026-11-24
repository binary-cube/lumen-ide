# 97 — Fișierele foarte lungi, analizate și sparte

**Cererea** (TODO, 2026-11-20): fișierele cele mai lungi din `crates/` sunt analizate pe
rând:
- ce face fiecare;
- dacă face optim: nimic pe firul interfeței, nimic refăcut pe cadru, fără copii inutile;
- împărțirea în fișiere mici, pe blocuri cu un rost fiecare (`code.md`).

Nimic din ce se vede nu se schimbă, iar testele rămân verzi după fiecare fișier.

## Cum se face

- **Un fișier pe rând.** Bucățile cu un rost propriu (tipurile primite din afară, desenul
  unei părți, ce e sub mouse) se mută în fișiere ale aceluiași modul, cu un `//!` care spune
  ce sunt. Cel inițial rămâne cu structura și legăturile.
- **Ce se poate folosi și în alte părți trece în codul comun** (cerut 2026-11-24): o funcție
  sau un tip care se repetă, ori de care ar avea nevoie și alte module (calcule de text,
  geometrie, cache-uri, desene de componente), se mută în `kernel`, `ui-components` sau
  crate-ul de domeniu potrivit, cu teste. Cei care îl copiaseră îl folosesc de acolo, iar
  pagina componentei din `docs/ui/` e pusă la zi.
- **Parametrii lungi devin DTO-uri** (cerut 2026-11-24, regula din `code.md`): o funcție
  sau un constructor cu mulți parametri primește un struct cu nume (sau câteva tupluri
  mici de ce merge împreună). Cele 10 `#[allow(clippy::too_many_arguments)]` rămase au fost
  scoase toate în aceeași zi, iar `cargo xtask check-deps` refuză acum orice altul:
  - `RowMove` (mutarea pe rânduri a editorului), `ListSection` (lista dialogului),
    `PopoverLook` + `PopoverSpec` (fereastra barei de stare), `Wanted` + `Going` (descărcările
    fixate, folosite de serverele de limbaj, actualizări și diagrame), `Step` (o cerere
    HTTP dintr-un lanț);
  - în rest, uneltele desenului (`(layer, text, icons)`) și ce ține de același lucru grupate
    în tupluri (cardul notificării, câmpul de căutare, butoanele ferestrelor laterale,
    rândurile editorului HTTP); la bara de derulare excepția nici nu mai era nevoie.
- **Contractele dintre module stau în codul comun** (regula din `modules.md`): ce se găsește
  în fișierul unui modul și ar fi folosit de altul devine o interfață în `contracts`,
  `workbench-model` sau crate-ul de domeniu. Doar date, gândite pentru o zi în care
  modulele de proiect pot trăi separat.
- **Doar mutate, nu rescrise**: aceleași funcții, cu comentariile lor. Vizibilitatea se
  deschide doar cât e nevoie (`pub(super)` între fișierele aceluiași modul).
- **După fiecare:** testele crate-ului, clippy, apoi `./scripts/check.sh`. Ce se observă în
  privința performanței e scris la fișierul respectiv.

## Lista (peste 1200 de rânduri, 2026-11-24)

- `xtask/src/icon_packs.rs` (2280)
- `crates/infra/database/protocol/src/convert.rs` (1748)
- `crates/app/src/snapshot.rs` (1708)
- `crates/infra/database/protocol/src/dto.rs` (1700)
- `crates/modules/ai/agent-sessions/src/panel/mod.rs` (1692)
- `crates/app/src/workbench/mod.rs` (1634)
- `crates/modules/project/remote/src/session.rs` (1630)
- `crates/infra/database/postgres/src/lib.rs` (1610)
- `crates/app/src/commands.rs` (1583)
- `crates/infra/database/mysql/src/lib.rs` (1548)
- `crates/ui/ui-components/src/ds/controls.rs` (1528)
- `crates/ui/ui-components/src/tree_table.rs` (1491)
- `crates/modules/project/remote/src/connect_view.rs` (1467)
- `crates/modules/tools/terminal/src/view.rs` (1445)
- `crates/domain/database/database/src/completion.rs` (1373)
- `crates/render/render-wgpu/src/lib.rs` (1346)
- `crates/ui/ui-workbench/src/settings_view/mod.rs` (1343)
- `crates/modules/database/explorer/src/sources/paint.rs` (1341)
- `crates/modules/database/explorer/src/schema/modal.rs` (1328)
- `crates/modules/tools/http-client/src/request_tree.rs` (1326)
- `crates/modules/database/console/src/profile/graph.rs` (1324)
- `crates/modules/database/explorer/src/sources/form.rs` (1292)
- `crates/modules/database/diagram/src/view/mod.rs` (1287)
- `crates/modules/appearance/decorations/src/art.rs` (1283)
- `crates/modules/viewers/markdown/src/lib.rs` (1275)
- `crates/platform/platform-winit/src/lib.rs` (1272)
- `crates/app/src/app/mod.rs` (1260)
- `crates/ui/ui-components/src/control.rs` (1257)
- `crates/domain/appearance/theming/src/decorations.rs` (1253)
- `crates/modules/project/remote/src/properties_view.rs` (1237)
- `crates/modules/database/console/src/bar.rs` (1236)
- `crates/modules/database/explorer/src/tree.rs` (1233)
- `crates/app/src/workbench/editing.rs` (1232)
- `crates/domain/code/intelligence/src/engine.rs` (1229)
- `crates/modules/tools/services/src/panel.rs` (1216)
- `crates/domain/database/database/src/sql/format/layout.rs` (1210)
- `crates/modules/database/console/src/profile/view/viewer.rs` (1206)
- `crates/modules/viewers/markdown/src/layout.rs` (1201)

Peste 800 de rânduri mai sunt încă vreo 90; vin după acestea.

**Stadiul (2026-11-24):** toate cele de mai sus sunt sparte, în afară de
`xtask/src/icon_packs.rs` (2280), unealta care face pachetele de iconițe: n-a fost încă
luată (nu e cod al aplicației). Alte fișiere de cod peste 1200 de rânduri nu mai sunt; peste
1200 au rămas doar câteva fișiere de teste (`ui-components/tests/unit/ds.rs` 1767,
`app/tests/unit/workbench.rs` 1550, `remote/tests/unit/session.rs` 1515 și altele cinci
între 1203 și 1283). Peste 800: 91 de fișiere de cod.

## Făcut

- **`ui-workbench/src/editor_view/mod.rs`** (1372 → 794):
  - `decorations.rs`: tipurile date editorului din afară (culori, decorații, bec, lens,
    hint-uri, erori);
  - `highlights.rs`: ce se desenează în spatele textului (rândul curent, culorile pe rânduri,
    selecțiile, erorile, legătura, potrivirile, rigla);
  - `hits.rs`: ce e sub mouse (hover, linia, coloana și byte-ul de la un punct, locul
    cursorului).

  Tot aici, mutarea pe rânduri primește un DTO (`RowMove`) în loc de șapte parametri.

  Performanță: desenul nu face nimic per cadru care s-ar putea păstra. Culorile se
  împart pe rânduri o dată pe cadru, iar rândurile încadrate se calculează o dată pe linie
  (`wrap_loaded`, plan 96).
- **`app/src/workbench/mod.rs`** (1634 → 1276):
  - `window_layout.rs`: unde stă fiecare parte a ferestrei (dock-urile, editoarele,
    panourile, bara de jos);
  - `appearance.rs`: paleta, fundalul, fontul, rendererul;
  - `scope_color` și `ScopeStyle` s-au mutat în `syntax.rs`, lângă restul sintaxei, cu
    comentariul pus înapoi deasupra funcției lui;
  - `paint_viewer` s-a mutat în `viewers.rs`.

  Structurile ferestrei au rămas: câmpurile lor sunt citite din zeci de fișiere vecine, iar
  mutarea ar cere vizibilitate deschisă peste tot.
- **`db-protocol/src/convert.rs`** (1748 → `convert/mod.rs` 71): enum-urile la fel de o
  parte și de alta și limbajul consolei au rămas; restul, pe secțiunile pe care le avea deja:
  - `capabilities.rs`: ce poate un driver și ce rulează consola (rezultate, planuri,
    tranzacții, rutine, erori);
  - `connection.rs`: câmpurile conexiunii și ținta;
  - `reading.rs`: ce se citește (nodurile arborelui, tabele, structuri, valori, pagini);
  - `changing.rs`: ce se scrie (schimbările rândurilor, ale structurii);
  - `scripts.rs`: scripturile și sursele refacerii unui tabel (SQLite);
  - `profile.rs`: profilurile (protocolul 3), cu macro-ul lor `same_fields`.
- **`db-protocol/src/dto.rs`** (1700 → `dto/mod.rs` 50): `Versions` și ajutoarele lui serde
  (`yes`, `is_true`) au rămas, restul e reexportat (`pub use …::*`), deci `dto::X` merge ca
  înainte:
  - `handshake.rs`: versiunile, ce spune driverul că poate, câmpurile conexiunii și ținta;
  - `reading.rs`, `changing.rs` (cu scripturile și refacerile), `console.rs` (protocolul 2),
    `profile.rs` (protocolul 3, comentariul formatului mutat în `//!`).
- **`app/src/snapshot.rs`** (1708 → `snapshot/mod.rs` 704), lângă fișierele de pași care
  existau deja în folder:
  - `options.rs`: citirea variabilelor `EDITOR_SNAPSHOT_*` în `Options` (structura a rămas
    în `mod.rs`: câmpurile ei sunt citite de toți pașii);
  - `setup.rs`: ce se pregătește înainte (serverele de limbaj false, descărcările, secretele
    în memorie, `prepare`), cu așteptarea modelului semantic;
  - `capture.rs`: `render`, desenul în PNG;
  - `actions.rs`: pașii mici din care se fac ceilalți (cadre, așteptări, tastare, căutări,
    trageri, împărțiri, canvas, hover).

  Pașii mari (`run`, `editor_steps`, `view_steps`, `window_steps`) au rămas în `mod.rs`: sunt
  ordinea în care se face o captură.
- **`agent-sessions/src/panel/mod.rs`** (1692 → 713):
  - `tool_panel.rs`: tot `impl ToolPanel` (citit, desenat, mouse, taste, meniuri, comenzi);
  - `menus.rs`: meniul agenților, al unei sesiuni, mutarea într-un folder, copierea;
  - `rename.rs`: redenumirea pe loc și folderul nou;
  - `filter.rs`: ce arată lista (căutare, favorite, agent, foldere) și filtrul agenților;
  - `frame.rs`: panoul desenat întreg (căutarea, filtrul, lista), rândurile rămânând în
    `paint.rs`.

  Testele au primit importurile pe care le luau înainte din `mod.rs` (`ToolPanel`, `Label`…).
- **`remote-projects/src/session.rs`** (1630 → `session/mod.rs` 470), lângă `service.rs`,
  `reconnect.rs`, `permissions.rs`, `resources.rs`, fiecare un `impl State` al lui:
  - `operations.rs`: limitele, operațiile în curs, jurnalul transferurilor, anunțurile,
    lucrările trimise firului sesiunii;
  - `server_changes.rs`: mutări, redenumiri și ștergeri pe server, încărcarea automată,
    manifestul păstrat;
  - `listing.rs`: folderele serverului așa cum le vede arborele;
  - `uploads.rs`: încărcările după salvare, ce așteaptă, schimbările locale, ce se trimite;
  - `sending.rs`: ce trimite utilizatorul din lista de ieșire, comparat, aruncat, adus.
- **`db-postgres/src/lib.rs`** (1610 → 348) și **`db-mysql/src/lib.rs`** (1548 → 277), la
  fel amândouă:
  - `connect.rs`: `impl Driver` (conectarea, TLS după `sslmode`, sesiunea deschisă);
  - `session.rs`: tot `impl Session` (561, respectiv 455 de rânduri: un trait nu se poate
    împărți între fișiere);
  - `reading.rs`: catalogul citit (tabele, structura, nodurile arborelui);
  - `running.rs` (Postgres): instrucțiunile consolei (simplu sau prin cursor, paginile,
    notificările, istoricul, savepoint-ul);
  - `rows.rs` și `profiling.rs` (MySQL): rândurile venite de la server (tipul, valoarea,
    fluxul către sink) și profilurile (contoarele înainte și după, verificarea că o scriere
    se poate anula).

  `dialect()` al sesiunii a devenit `pub(crate)`: altfel `self.dialect()` din `session.rs`
  ar fi ales metoda trait-ului cu același nume.
- **`app/src/commands.rs`** (1583 → `commands/mod.rs` 535): id-urile, tabelul `SPECS`, ce
  cer comenzile și etichetele au rămas; restul e reexportat (`commands::X` neschimbat):
  - `menu_bar.rs`: meniurile barei, secțiunile, submeniurile, arborele lor;
  - `recent.rs`: File › Open Recent;
  - `tool_tabs.rs`: meniul unui tab al ferestrei de jos;
  - `tab_menu.rs`: meniul unui tab al editorului și rândurile listate de module;
  - `menu_icons.rs`: iconițele comenzilor (numit așa ca să nu ascundă crate-ul `icons`);
  - `entries.rs`: comenzile după id, tastele, meniurile ca rânduri.
- **`ui-components/src/ds/controls.rs`** (1528 → 386, doar butoanele), fișiere surori în `ds/`
  reexportate ca celelalte:
  - `toggles.rs`: switch, checkbox, radio, cardul radio;
  - `choices.rs`: pastilele, celula unei alegeri, rândul cu ✓;
  - `segmented.rs`, `stepper.rs`, `search.rs` (câmpul de căutare, stilurile câmpurilor,
    butonul ×).
- **`remote-projects/src/connect_view.rs`** (1500 → 795), lângă fișierele pe care le avea
  deja (`fields`, `owner`, `paint`, `transfer`, `view`):
  - `steps.rs`: formularul citit într-o conexiune (sau de ce nu se poate), butoanele pașilor
    (înainte, înapoi, test, autentificare, salvare), conectorul cu parola păstrată;
  - `suggestions.rs`: folderele sugerate cât se scrie o cale (locale, pe un worker; ale
    serverului), tastele listei, o sugestie aleasă;
  - `browser.rs`: folderele serverului răsfoite (pasul 2), excluderile, descărcarea pornită.

  `save` a devenit `pub(super)`: altfel `self.save()` din `view.rs` ar fi ales metoda unui
  trait cu același nume. Testele importă sugestiile din `suggestions`.
- **`ui-components/src/tree_table.rs`** (1491 → `tree_table/mod.rs` 455):
  - `cells.rs`: din ce e făcut tabelul (coloane, rânduri, celule, cerneluri), reexportat;
  - `input.rs`: ce e sub mouse, apăsarea, rotița, tastele, cursorul, tooltip-urile, tabelul
    ca un `Control`;
  - `paint.rs`: desenul (antetul, rândurile din vedere, fiecare celulă după felul ei) și
    `number_text`, reexportat.
- **`terminal-panel/src/view.rs`** (1445 → `view/mod.rs` 447):
  - `viewer.rs`: tot `impl Viewer` (506 rânduri, un singur trait);
  - `grid.rs`: grila desenată (culorile fiecărei celule, glifele, selecția, cursorul);
  - `selection.rs`: mouse-ul peste grilă (celula, selecția trasă dincolo de margine,
    legătura și fișierul numit);
  - `shells.rs`: shell-urile pornite (procesele, proiectele, cele vii numărate) și terminalele
    găsite după cheie (redenumite, întrebate dacă mai rulează), reexportate `pub(crate)`.

- **`database/src/completion.rs`** (1373 → `completion/mod.rs` 443): tipurile sugestiilor,
  `complete`, `Completer` cu ce e comun (rangul, numele, cuvintele cheie cerute),
  `alias_for` și `quote` au rămas:
  - `values.rs`: literalele unei coloane enum unde i se scrie valoarea;
  - `tables.rs`: schemele, tabelele și vederile, cele legate după `JOIN`, condițiile `ON`
    întregi din chei;
  - `columns.rs`: coloanele (după `alias.`, necalificate, lista unui `INSERT`, `USING`,
    `SELECT *` desfăcut);
  - `functions.rs`: funcțiile oferite și semnăturile lor (`builtin_signature`, `signature`,
    reexportate);
  - `keywords.rs`: cuvintele cheie pe locuri și motoare, cuvintele rezervate.

  Testele au primit `TableKind`, pe care îl luau înainte din importurile fișierului.
- **`render-wgpu/src/lib.rs`** (1346 → 476): contextul comun (`GpuContext`, `SharedState`),
  `WgpuRenderer` cu crearea lui și `Drop` au rămas; fișiere surori, folosite prin `use super::*`
  ca celelalte ale crate-ului:
  - `instances.rs`: ce primește GPU-ul la un cadru (globalele, instanțele de dreptunghiuri,
    glife și imagini, loturile, pașii, bufferele);
  - `pipelines.rs`: pipeline-urile unui format, grupul de legături al atlasului,
    `create_pipeline`;
  - `surface.rs`: suprafața ferestrei (configurată, refăcută, dată înapoi cât e ascunsă,
    imaginea următoare cerută) și octeții imaginilor ei;
  - `renderer.rs`: tot `impl Renderer` (desenul unei scene, capturi, ce se eliberează).

  Câmpurile structurilor mutate citite din `drawer.rs` / `context.rs` au devenit `pub(crate)`.
- **`ui-workbench/src/settings_view/mod.rs`** (1343 → 472), lângă fișierele care existau:
  - `model.rs`: ce primește fereastra și ce raportează (temele, iconițele, modulele,
    framework-urile, blocurile contribuite, `SettingsAction`), reexportat;
  - `text.rs`: căutarea, câmpul scurtăturilor, câmpul unui bloc scris, o scurtătură
    înregistrată, rezultatele parcurse;
  - `input.rs`: mouse-ul (clicul, hover-ul, sliderul și bara trase, lista unui dropdown,
    controalele ferestrei);
  - `paint.rs`: desenul (fereastra, conținutul arătat cât se derulează, bara, tastele din
    subsol) și derularea ținută în ce s-a desenat.
- **`database-explorer/src/sources/paint.rs`** (1341 → `sources/paint/mod.rs` 233): desenul
  ferestrei și cardul tras au rămas:
  - `list.rs`: lista surselor (filtrul, bara de unelte, rândurile, unde cade una trasă,
    insigna de producție);
  - `parts.rs`: bucățile mici din dreapta (etichete, câmpuri, dropdown-uri, butoane, titluri,
    note, cipul moștenit, culorile, un rând cu bifă);
  - `general.rs`: antetul sursei alese și tab-ul General (câmpurile driverului, folderul,
    parola);
  - `tabs.rs`: celelalte tab-uri (opțiuni, SSL, scheme, verificări, avansat).
- **`database-explorer/src/schema/modal.rs`** (1328 → `schema/modal/mod.rs` 352): tipurile,
  `SchemaView` cu deschiderea, câmpurile, mutarea focusului și `snapshot` au rămas:
  - `controls.rs`: lista deschisă și câmpul cu tastele ca `Control`-uri, ordinea lor;
  - `viewer.rs`: tot `impl Viewer` (cadrul, tastele, textul, comenzile, mouse-ul);
  - `plan.rs`: planul cerut workerului cât se schimbă formularul, ce se pierde, rularea;
  - `choices.rs`: alegerile și sugestiile formularului, bifele.

  `suggesting` și `choosing` (erau `pub(super)`, citite de testele lui `schema`) au devenit
  `pub(in crate::schema)`, ca să rămână vizibile acolo de un nivel mai jos.
- **`http-client/src/request_tree.rs`** (1326 → `request_tree/mod.rs` 483): tipurile, citirea
  listei, rândurile filtrate, plierea, selecția și ce se cere din afară au rămas:
  - `changes.rs`: colecțiile numite pe rândul lor, cererile făcute, redenumite, mutate,
    copiate sau șterse pe un worker;
  - `paint.rs`: desenul (căutarea, rândurile, câmpul numelui, butoanele, subsolul);
  - `input.rs`: mouse-ul și tastele (rândul sau butonul de sub cursor, tragerea într-un
    folder, textul, tooltip-ul);
  - `menus.rs`: meniurile de click dreapta și comenzile lor.
- **`database-console/src/profile/graph.rs`** (1324 → `graph/mod.rs` 296): constantele,
  `GraphView` cu potrivirea în vedere și zoom-ul au rămas:
  - `layout.rs`: așezarea (cardurile, cele pliate, muchiile după rânduri), reexportată;
  - `input.rs`: mouse-ul (cardul sau controlul de sub el, apăsarea, panoramarea, tooltip-ul,
    săgețile);
  - `paint.rs`: desenul (cardurile din vedere, muchiile, controalele din colț, legenda);
  - `card.rs`: cardul unui pas (fonturile, liniile, desenul, cel pliat, textul tooltip-ului),
    `Fonts` și `card_tip` reexportate.
- **`database-explorer/src/sources/form.rs`** (1292 → `sources/form/mod.rs` 315): tipurile,
  `Editor` cu crearea, selecția și sursa curentă au rămas:
  - `in_folders.rs`: folderele (rândurile listei, făcute, redenumite, colorate, mutate,
    șterse), `ListRow`, `Place`, `Item` reexportate; numit așa ca să nu ascundă
    `database::folders`;
  - `inputs.rs`: câmpurile sursei (ce schimbă o editare, URL-ul ținut la pas, ce se încarcă
    și se scrie înapoi, opțiunile, fișierele alese);
  - `choices.rs`: alegerile (dropdown, switch, read-only, producție, politica parolei,
    schemele și bazele arătate);
  - `changes.rs`: sursele adăugate, copiate, șterse, mutate, problemele, `Plan`-ul.
- **`database-diagram/src/view/mod.rs`** (1287 → 457), lângă `filter.rs` și `paint.rs`:
  - `viewer.rs`: tot `impl Viewer`;
  - `reading.rs`: graful citit pe un worker, ce se arată, așezarea făcută în fundal;
  - `edges.rs`: muchia aleasă sau de sub cursor, fereastra ei, o cheie virtuală acceptată,
    muchiile parcurse cu tastele;
  - `navigation.rs`: locurile prin care s-a trecut și înapoi, un tabel ales și vecinii lui,
    ce face un clic, meniul unui card.
- **`theme-decorations/src/art.rs`** (1283 → `art/mod.rs` 221): `Canvas` și ajutoarele lui
  au rămas, restul e reexportat (`art::X` neschimbat):
  - `garland.rs`: ghirlanda (firul, becurile, mărgelele, steagurile, arcul și bucata de pe
    lateral);
  - `pieces.rs`: decorațiunile de sezon (globul, scânteia, particulele, marginile strălucirii,
    colțul);
  - `effects.rs`, `figures.rs`: efectele (puncte luminoase, bule, aurora, valuri, soare,
    ploaie, nori, ceață, fluture) și cele desenate ca forme (baloane, inimi, lilieci, pete,
    linii, halouri, orizontul orașului).

  Comentariile-separator cu numărul planului (istorie) n-au mai fost păstrate.
- **`viewer-markdown/src/lib.rs`** (1275 → 438): modulul, providerul, `MarkdownViewer` cu
  butoanele din colț și controalele au rămas:
  - `viewer.rs`: tot `impl Viewer` (545 de rânduri, un singur trait);
  - `pressing.rs`: o apăsare și ce urmează (butoane, bife, plieri, legături, selecția
    începută și întinsă, derularea la margine, meniul textului);
  - `building.rs`: așezarea construită (pe loc sau pe un worker), luată, instalată.
- **`remote-projects/src/properties_view.rs`** (1237 → 538), lângă `octal.rs`, `viewer.rs`,
  `checksum.rs`:
  - `paint.rs`: desenul ferestrei, cu dimensiunile ei (importate înapoi în părinte, citite
    de `viewer.rs` și `checksum.rs`);
  - `lists.rs`: listele proprietarului, grupului și algoritmilor (ce oferă, ce e ales);
  - `applying.rs`: mărimea unui folder măsurată în fundal, schimbarea făcută, OK.
- **`database-console/src/bar.rs`** (1236 → `bar/mod.rs` 517):
  - `menus.rs`: meniurile barei (Run, tranzacția, modul, setările, sursa, locul);
  - `paint.rs`: desenul barei și insignele notițelor;
  - `toolbar.rs`: tot `impl EditorToolbar`.
- **`app/src/workbench/mod.rs`, a doua oară** (1276 → 1005): structurile au rămas (câmpurile
  lor sunt citite din zeci de fișiere vecine):
  - `construct.rs`: `Workbench::new` și ce primește fereastra de la aplicație după aceea
    (waker-ul, clipboard-urile, temele, cifrele resurselor);
  - `results.rs`: `MouseResult`, alegerea din lista proiectelor făcută rezultat, cursorul
    sistemului pentru ce cere o vedere (reexportate).
- **`app/src/workbench/editing.rs`** (1232 → 372): tastarea, plierea, comentariile, mutarea
  codului și tastele date panourilor și vederilor au rămas:
  - `editor_keys.rs`: tastele de editare (întâi ferestrele de deasupra, apoi editorul:
    completarea, mișcarea, comentariile de documentație, expandările);
  - `text_fields.rs`: câmpul care are tastatura (al cui e, tastele cursorului, selecția pusă
    din afară), cu `InputOwner`;
  - `copy_paste.rs`: copierea, tăierea, lipirea, Select All, textul compus de IME;
  - `find_bar.rs`: bara de căutare (deschisă cu selecția, tastele ei, potrivirea următoare).

  Metodele mutate al căror nume apare în alt fișier au primit `pub(super)`.
- **`app/src/app/mod.rs`** (1260 → 609), lângă fișierele care existau (toate prin
  `use super::*`):
  - `frame.rs`: cadrul desenat (`render`), ce l-a cerut numărat cât se înregistrează un
    profil, cadrul sărit, decorațiile ferestrei, diagnosticul redimensionării (`ResizeTest`);
  - `chords.rs`: combinația de taste a unui eveniment (AltGr păstrat ca text);
  - `floating.rs`: stratul de deasupra paginilor.

  Importurile lor (`chord`, `ResizeTest`, `FLOATING_SHADOW`) sunt scrise explicit în cei care
  le folosesc: un `use` privat al părintelui nu ajungea la vecini prin `use super::*` când
  fișierul de unde vine îl importă tot așa (ciclu de glob-uri). Testele importă la fel.
- **`platform-winit/src/lib.rs`** (1272 → 454): `WinitWindow`, `Handler` și deschiderea
  ferestrei au rămas:
  - `window.rs`: tot `impl Window for WinitWindow` (dialogurile, clipboard-ul, wakerii,
    butoanele barei, file manager-ul) și `reveal_in_explorer`;
  - `events.rs`: tot `impl ApplicationHandler` (bucla reluată și în așteptare, evenimentele
    trimise din afară, ale fiecărei ferestre) și tastele în termenii API-ului.
- **`ui-components/src/control.rs`** (1257 → `control/mod.rs` 581): contractul `Control`,
  `Controls` și câmpul de text ca un `Control` au rămas, restul reexportat
  (`control::X` neschimbat):
  - `bars.rs`: `Thumb`, `ScrollThumb`, `Scrolled`, `BarThumb`, `Divider`;
  - `wrappers.rs`: `Absent`, `Option<C>`, `OneClick`, `Shown`;
  - `views.rs`: `TabView`, `Embedded`, `AskedMenu`.
- **`theming/src/decorations.rs`** (1253 → 498), fișiere noi în folderul care exista deja
  (`effect.rs`, `presets.rs`), reexportate:
  - `fields.rs`: opțiunile unui strat descrise (felul valorii, limita, listele fiecărui fel),
    valorile și erorile, numerele și culorile scrise ca într-o temă;
  - `layer_spec.rs`: `impl LayerSpec` (citit și scris pe opțiuni, zona, costul, limitele);
  - `resolve.rs`: decorațiile declarate de o temă (legături, condiții, combinate cu ce
    extinde) și rezolvate;
  - `line.rs`: `encode` / `decode` (un strat pe un rând, în setări).
- **`project-tree/src/panel.rs`** (1234 → `panel/mod.rs` 309):
  - `tool_panel.rs`: tot `impl ToolPanel`;
  - `listing.rs`: proiectul ținut la zi (interogările, schimbările urmărite, folderele citite
    pe firul listării, „Expand all”, cererile);
  - `search.rs`: căutarea panoului (rezultatele, unul ales, câmpul ei);
  - `entries.rs`: fișierul sau folderul nou numit pe loc și redenumirea.

  Testele au primit importurile pe care le luau din `panel.rs` (`ToolPanel`, `ProjectView`…).
- **`database-explorer/src/tree.rs`** (1233 → `tree/mod.rs` 449): tipurile și sursele au
  rămas:
  - `reading.rs`: ce e deschis și ce trebuie citit (ordinea cererilor, răspunsurile, o sursă
    citită din nou, uitarea);
  - `rows.rs`: rândurile arătate (folderele, sursele, bazele serverului, rândurile de
    încărcare, `nearest` reexportat);
  - `table_rows.rs`: tabelele pe scheme și grupuri, coloanele, indecșii, cheile străine.
- **`code-intelligence/src/engine.rs`** (1229 → `engine/mod.rs` 151): `Project` și
  ajutoarele mici au rămas, funcțiile publice reexportate:
  - `completion.rs`: `complete`, sugestiile făcute, potrivirile ținute pentru tasta
    următoare (`NARROWED`);
  - `completer.rs`: `Completer` (ce se sugerează pentru fiecare context; câmpurile lui au
    devenit `pub(super)`, e construit în `completion.rs`);
  - `definition.rs`: „mergi la declarație”;
  - `hover.rs`: cardul hover și semnăturile scrise simplu.
- **`services-panel/src/panel.rs`** (1216 → `panel/mod.rs` 556):
  - `tool_panel.rs`: tot `impl ToolPanel`;
  - `tabs.rs`: tab-urile nodului arătat (făcute, vederile lor, închise, fixate), ce cere o
    vedere, meniul unui tab.
- **`database-console/src/commands.rs`** (1214 → `commands/mod.rs` 355): rularea, istoricul,
  oprirea, tranzacția, închiderea, setările și comenzile simple au rămas, restul reexportat:
  - `values.rs`: valorile parametrilor, „Edit Parameters…”;
  - `guarded.rs`: rularea confirmată (pericolele, scrierea pe producție);
  - `explain.rs`: Explain și Explain Analyze;
  - `open_console.rs`: „Jump to Query Console…” și consola nouă;
  - `attach.rs`: un fișier SQL atașat la o sursă sau detașat, cu comenzile lor.
- **`database/src/sql/format/layout.rs`** (1210 → `layout/mod.rs` 188), pe secțiunile pe
  care le avea deja (separatoarele `// ---- … ----` n-au mai rămas):
  - `reading.rs`, `statements.rs`, `queries.rs`, `expressions.rs`, câte un
    `impl Layout` fiecare; metodele chemate dintr-un alt fișier au primit `pub(super)`.
- **`viewer-markdown/src/layout.rs`** (1201 → `layout/mod.rs` 390): elementele desenate,
  ce se poate apăsa, `Layout`, `Caches`, `Drawn`, stilurile și `Builder` au rămas:
  - `images.rs`: imaginile locale (mărimile citite din antet și ștampilele verificate din
    nou pe un worker, pixelii decodați la mărimea arătată), reexportate; câmpurile citite de
    teste au devenit `pub(super)`;
  - `lines.rs`: tot `impl Builder` (textul pus pe rânduri, cuvintele, formulele, regula,
    măsura), cu `Atom` și `Placed`.
- **`database-console/src/profile/view/viewer.rs`** (1206 → 763): a rămas doar
  `impl Viewer` (un singur trait, 758 de rânduri):
  - `frame.rs`: tot ce arată vederea cu părțile așezate, apăsările pe constatări și pe
    detalii, părțile date ca controale (`SPLITTER`, `FILTER`);
  - `parts.rs`: filtrul pașilor și panoul lateral ca `Control`-uri (`PanelRows`).

  `SPLITTER` și `FILTER` se importă explicit în `viewer.rs`: nevăzute, într-un `match` ar fi
  devenit în tăcere variabile care prind orice (compilatorul a spus „unreachable pattern”).

**Cum s-a lucrat** (pentru cele de mai sus): elementele s-au mutat cu scripturi (cu
comentariile și atributele lor), importurile lipsă s-au adăugat după erorile compilatorului.
O metodă privată mutată într-un alt fișier poate fi înlocuită în tăcere de metoda unui
trait cu același nume (`dialect`, `save`): de aceea fiecare metodă mutată, al cărei nume
apare în alt fișier al modulului, a primit `pub(super)` / `pub(crate)`. Un element care era
deja `pub(super)` și coboară un nivel (fișierul devine folder) primește `pub(in crate::…)`
cu modulul de dinainte, altfel testele sau vecinii lui nu-l mai văd.

## Firul interfeței (verificat 2026-11-24)

Un agent separat a căutat ce citește discul, git-ul sau procesele pe firul interfeței (desen,
clic, taste, meniuri, setări). A găsit ~30 de locuri; modulele trimit aproape totul pe
workeri, cele mai grele sunt în aplicație.

**Făcut:**
- **Meniurile nu mai citesc fișiere în `enabled`**: contractul nou `WorkbenchHost::active_path`
  dă doar calea (din memorie; pentru ținta unui meniu, nedeschisă, se uită o dată la
  primii 8 KB să vadă dacă e text). `enabled` din git, compare, HTTP, SQL, calitate,
  refactorizări, istoric local și diagrame îl folosesc pe acesta; textul întreg îl cere
  doar `run`.
- **Comparările citesc în fundal**: git (`compare_file`: versiunea din depozit prin
  `host.spawn`), compare (`compare_files`: contractul nou `EditorHost::file_text_later`
  dă textul deschis din memorie sau un cititor pentru un worker, `FileText`).
- **Arborele proiectului** deschide un folder necitit pe firul lui de listare (`tree-list`),
  deschis când vine; „Expand all” la fel. Unul citit deja se deschide pe loc.
- **Consolele de bază de date**: lista lor (folderul sursei), crearea uneia și numele
  ocupate la redenumire, în fundal.
- **Sugestiile de foldere locale** din fereastra de conectare la distanță: pe un worker, la
  fiecare tastă, arătate doar dacă textul e încă cel scris.
- **Setările calității codului**: ce unealtă are proiectul și ce ar rula o comandă (caută
  în `PATH`) se află în fundal, ținute 10 s (`looked.rs`), cu „Se verifică…” până atunci.
- **Adăugarea unui server de limbaj** scrie fișierul pe un worker („Se adaugă…”).
- **Istoricul local**: butonul de ștergere citește din memorie numărul de versiuni.
- **Exportul unei diagrame din Markdown** (SVG, PNG) se scrie pe un worker.

**Lăsat (cu motivul):**
- **Deschiderea și salvarea fișierelor în aplicație** (`open_file`, restaurarea sesiunii,
  `save_tab`, `apply_plan`/`write_closed`): sunt sincrone și le folosesc zeci de locuri
  care așteaptă rezultatul pe loc (`host.open_file(…)` urmat de `select_active`). Ca să fie
  asincrone, tab-ul ar trebui să apară când vine textul și fiecare apelant să continue
  dintr-o completare. E un pas de proiectare separat; schimbă cum se comportă
  deschiderea, deci se face cu grijă și cu teste pe fiecare apelant.
- **`refresh_symbols_now`**: reanalizează documentul activ doar pentru acțiuni cerute
  explicit (mergi la declarație imediat după o modificare). Ca să fie asincron, acțiunile
  ar trebui să răspundă mai târziu.
- **Redenumirea și ștergerea unei console**: câte un singur fișier, iar evenimentele lor
  trebuie publicate pe firul interfeței (abonații le așteaptă acolo).
- **Mici** (un `stat`, un `exists` la o acțiune rară) și așteptările de la închidere
  (`flush` al sesiunii, salvările în curs): lăsate.

## Recenzia (2026-11-24, un agent separat)

Pe tot ce e necomis: mutările de pe firul interfeței, DTO-urile, fișierele sparte.
DTO-urile și împărțirile au ieșit curate (ordinea argumentelor verificată la fiecare
apelant, liniile fiecărui grup comparate înainte și după). Ce a găsit, verificat și reparat:
- **Setările calității și „Adaugă server” nu se actualizau** după lucrul din fundal:
  blocurile se refac doar când un `poll()` spune că s-a schimbat ceva, iar contribuțiile
  lor n-aveau unul. Acum îl au (`looked::take_looked`, `Draft::told`). Un al doilea „Add”
  apăsat cât se scrie primul nu mai pornește altă scriere.
- **Sugestiile locale venite târziu** redeschideau lista după Esc, după schimbarea pasului
  sau sub câmpul serverului: citirea în curs se uită odată cu lista, iar rezultatul se
  arată doar în pasul și câmpul lui.
- **Arborele**: un al doilea clic pe un folder încă necitit îl lasă închis (test nou). O
  modificare venită cât se citește un folder îl face citit încă o dată.
- **Salvările de diagrame din Markdown** puse una după alta își spun fiecare rezultatul.
- **Golirea istoricului** apăsată înainte de prima numărătoare nu mai spune „0 versiuni”:
  spune că se șterg toate.
- **Ultima fereastră** așteaptă GPU-ul cel mult 2 s la închidere, nu la nesfârșit.
- **Cache-ul împărțirii pe rânduri** ține lista de hint-uri însăși, nu doar adresa ei.

Găsit în afara schimbărilor (era așa și înainte): „Compare with Active” din meniul
arborelui nu se poate activa, pentru că în meniu fișierul activ e chiar ținta meniului.
Rămâne de văzut separat.
