# 80 — Diagrame: un editor de diagrame, salvate în proiect

**Stare (2026-11-16): aprobat, deciziile utilizatorului la final. De făcut în ordine
(80a → 80j), pe un branch al lui (`diagrams`).**

Cerut 2026-11-16: un modul în care se desenează diagrame „ceva în genul draw.io” (captura:
paleta de forme în stânga, foaia cu grilă în mijloc, proprietățile Diagram / Style în
dreapta, paginile jos), salvate pe disc, în proiect. „Foarte bine scris, foarte optim, pe
componente, evenimente, după cum e structurat proiectul.”

## Ce avem deja (verificat în cod)

- **Vizualizatoare pe tip de fișier**: `ViewerProvider` (`ui-components/src/viewer.rs`):
  `kind(path)`, `open(path)`; `Viewer` are tot ce cere un editor: `paint`, `scroll`, `zoom`,
  `pointer_down` (clickuri, modificatori), `context_menu` (meniul nostru, `ViewMenuEntry`),
  `select_all`, `is_dirty` / `save`, `lockable` / `set_locked`, `status`, `memory`,
  `set_shown`.
- **Pan / zoom, carduri, linii**: canvasul dependențelor (`modules/insight/canvas`) și
  diagrama bazei de date (`modules/database/diagram`): layout calculat o dată, doar ce e în
  vedere desenat, liniile ca puncte dese pe o curbă Bézier. Doar de citit: nimic editabil.
- **Contururi vectoriale rasterizate**: previzualizarea Mermaid (`modules/viewers/markdown`,
  `tiles.rs`) desenează forme (`Seg`: linii, curbe) cu `tiny-skia`, pe firul ei de fundal,
  la scara ecranului, ținute după hash. Renderer-ul GPU desenează doar dreptunghiuri
  rotunjite, text și imagini: o elipsă, un romb, un cilindru, o săgeată curbă trec prin
  rasterizarea asta.
- **Componentele UI**: meniuri (`ViewMenuEntry` + `RowLook`), dropdown (`ds::dropdown`),
  formulare (`form`: secțiuni, câmpuri, căsuțe, liste), selectorul de culori
  (`color_picker`), splittere, câmpuri de text (`ui-kit/text_input`, cursorul comun),
  tooltip-uri, scrollbar-uri (`EdgeScrollbar`), `Deferred` pentru lucrul din fundal.
- **XML**: `roxmltree` e deja permis în domeniu (citire); scrierea e text simplu, cu
  escapare.
- **Salvarea**: un fișier al proiectului se scrie pe un worker, printr-un fișier temporar
  redenumit peste el; schimbările de pe disc vin pe bus-ul proiectului (`fs` watcher).
- **Istoria locală** și **Git** urmăresc orice fișier al proiectului: o diagramă le are gratis.

## Cum fac alții

- **draw.io / diagrams.net**: fișiere `.drawio` (XML `mxfile` → `diagram` per pagină →
  `mxGraphModel` → `mxCell`-uri cu `style="rounded=1;fillColor=#dae8fc;…"` și
  `mxGeometry`), uneori comprimate (deflate + base64 în `<diagram>`); `.drawio.svg` /
  `.drawio.png` (diagrama ascunsă în imagine). Extensiile de editor îl editează direct din
  repo: formatul e de facto standardul diagramelor ținute lângă cod.
- **Excalidraw**: `.excalidraw` (JSON), stil schiță de mână.
- **IDE-urile**: diagramele lor sunt generate (UML din cod), nu desenate; pentru desen,
  pluginul draw.io.

## Regula

1. **Formatul e al lumii, nu al nostru**: fișierele sunt `.drawio` (XML-ul draw.io,
   necomprimat la scriere, comprimat citit și el). Ce desenăm aici se deschide în draw.io și
   în extensiile lui de editor, și invers. Ce nu înțelegem dintr-un fișier (o formă din altă
   bibliotecă, o cheie de stil necunoscută) **se păstrează neatins** la salvare și se
   desenează cât se poate (dreptunghiul ei, cu textul), niciodată pierdut. Diff-urile Git
   rămân mici: ordinea celulelor și a cheilor de stil se păstrează, nimic rescris degeaba.
2. **Domeniul e pur**: un crate de domeniu (`crates/domain/tools/diagram-model`): modelul
   (pagini, celule — forme, legături, grupuri, straturi —, geometria, stilul ca listă
   ordonată de chei), citirea și scrierea `.drawio` (`roxmltree`, `flate` pentru cele
   comprimate), operațiile ca tranzacții reversibile (undo / redo), rutarea legăturilor,
   alinierea la grilă și ghidajele, hit-testing-ul. Fără UI, fără disc, fără fire.
3. **Modulul doar leagă** (`crates/modules/tools/diagram-editor`): `ViewerProvider` pentru
   `.drawio`, vizualizarea, comenzile, meniurile, setările. Nu știe de alte module; ce
   oferă altora trece prin contribuții (o comandă „New Diagram”, un șablon de fișier nou).
4. **Componentele sunt ale tuturor**: ce îi lipsește unei componente comune (un câmp
   numeric cu săgeți, un buton-culoare, o bară de unelte cu grupuri) se adaugă în
   `ui-components`, nu se desenează în modul. Rasterizarea contururilor iese din
   `markdown` într-un crate comun (`vector-tiles`, în `crates/ui` sau `crates/render`), pe care
   îl folosesc și Mermaid, și diagramele.
5. **Firul UI doar desenează și răspunde**: citirea și scrierea fișierului, rasterizarea
   formelor, căutarea în paletă, exportul — pe workeri (`kernel::work`, `Deferred`); o
   tragere de mouse mută doar numere în model și cere un cadru.
6. **Nimic nu există până nu e folosit; nimic nu se face de două ori**:
   - paleta de forme (sute de miniaturi) se rasterizează doar pentru secțiunile deschise,
     o dată pe temă și scară;
   - o formă se rasterizează o dată pe (tip, mărime, stil, treaptă de zoom), apoi e o
     imagine ținută în cache; textul trece prin sistemul de text comun;
   - doar celulele din vedere se desenează (index spațial: o grilă de găleți, refăcută
     doar la mutări);
   - tabul închis eliberează tot (modelul, imaginile, cache-urile).

## Pași (fiecare cu teste, gate verde, un commit)

1. **80a. Modelul și formatul (`diagram-model`).**
   - Tipurile: `Document { pages }`, `Page { id, name, grid, background, cells }`,
     `Cell { id, parent, kind: Vertex | Edge | Group, value, style: Style, geometry }`,
     `Style` (cheile în ordine, cu `shape`, `fillColor`, `strokeColor`, `rounded`,
     `dashed`, `fontSize`, `edgeStyle`, `endArrow`… tipizate pe măsură ce le folosim, restul
     păstrate ca text).
   - Citirea `.drawio`: necomprimat și comprimat (deflate + base64 + URL-encoding, cum face
     draw.io); limite pe mărime și pe numărul de celule (fișier ostil); id-urile unice.
   - Scrierea: același XML, aceeași ordine, doar ce s-a schimbat diferă.
   - Teste: fișiere reale salvate de draw.io (comprimate și nu), dus-întors identic.

   **Făcut (2026-11-16):** crate-ul `crates/domain/tools/diagram-model` (stratul domeniului;
   `miniz_oxide` permis acolo, pentru paginile comprimate). Un arbore XML care ține ce a citit
   (`xml.rs`): atributele în ordinea lor, fiecare valoare cu textul ei original (scris înapoi
   neatins cât nu se schimbă: entități, numere), elementele necunoscute întregi; scris ca
   draw.io (două spații pe nivel, `<gol />`). Peste el, vederea tipizată: `Document` (prologul
   și ce urmează rădăcinii păstrate; `<mxfile>` sau un `<mxGraphModel>` gol), `Page` (nume,
   grilă, pas, fundal, mărimea paginii), `Cell` (id, părinte, tip — formă / legătură /
   rădăcină sau strat —, textul, stilul, sursa / ținta, geometria, vizibil; un `UserObject` /
   `object` cu proprietățile lui), `Style` (chei în ordine, nume fără valoare, `;` final),
   `Geometry` (cutia; punctele, capetele libere, deplasarea etichetei). O celulă își scrie
   doar ce s-a schimbat față de ce a citit; una nouă, în ordinea draw.io. Paginile
   comprimate (base64 → deflate brut → `decodeURIComponent`) citite, scrise necomprimat.
   Limite: fișier 64 MB, 1 000 000 de elemente, 64 de niveluri, o pagină dezarhivată 64 MB,
   200 000 de celule, fără DTD; id-urile unei pagini unice (altfel refuzat).

   Teste (16): un fișier ca al draw.io dus-întors identic (octet cu octet); citirea
   tipizată (entități, rânduri noi, puncte, capete libere, grup, `UserObject`, strat ascuns,
   Unicode); o formă mutată = un rând schimbat; un stil și un text schimbate = rândul lor;
   ce nu e înțeles păstrat după o schimbare a aceleiași celule; o pagină comprimată (făcută
   cu Python, independent de cod) citită și scrisă necomprimat; un fișier nou și o formă
   nouă; fișiere ostile refuzate (nu XML, alt format, id dublu, prea adânc, DTD, base64 stricat,
   o pagină care se umflă peste limită).

   **Măsurat** (release): 500 de forme + 600 de legături (248 KB): citit în 2,2 ms, scris în
   0,16 ms.

   **Lăsat:** fixture-urile sunt scrise după formatul draw.io, nu salvate de aplicația lui
   (nu am rulat draw.io aici); de verificat pe fișiere reale ale tale când apare vizualizarea
   (80d). O pagină comprimată devine necomprimată la prima salvare după o schimbare (cerut:
   diff-uri citibile).
2. **80b. Operațiile și istoria (`diagram-model`).** Adaugă, șterge, mută, redimensionează,
   schimbă stilul / textul, grupează, aduce în față / trimite în spate, leagă / dezleagă; fiecare
   o tranzacție cu inversa ei; istoria pe document (cu limite, ca la editor: număr și
   octeți), cu tragerile unite într-un singur pas. Copiere / lipire ca XML draw.io (ce
   copiezi din draw.io se lipește aici și invers). Teste pe fiecare operație și pe undo.

   **Făcut (2026-11-16):** `edit.rs`: `Change` (inserează / scoate / înlocuiește o celulă la
   locul ei), `Transaction` (aplicată, dă inversa ei; o schimbare fără loc e sărită, niciodată
   panică), `History` (500 de pași, 32 MB; pașii unui gest cu aceeași cheie uniți, fiecare
   celulă cu starea de la începutul gestului; `seal` la eliberarea mouse-ului; redo golit de o
   editare nouă), `Diagram` (documentul, istoria, versiunea, salvat sau nu). `ops.rs`:
   adaugă, șterge (cu ce ține și legăturile pe ea, cu etichetele lor), mută (o formă din grup
   cu grupul, nu de două ori; punctele legăturilor dintre formele mutate; capetele libere ale
   unei legături alese), redimensionează (cel puțin 1), stil (o cheie pusă / scoasă), text,
   capete, grupează (cutia în jurul lor, locurile lor înăuntru; doar formele unui părinte),
   degrupează, față / spate (ordinea lor între ele păstrată). `clipboard.rs`: copierea ca
   `<mxGraphModel>` (cele alese, ce țin, legăturile dintre ele și etichetele lor), lipirea cu
   id-uri noi (cele luate sărite), legate între ele, capetele rămase în afară libere, în
   stratul dat, mutate cu un pas.

   Teste (11 noi): fiecare operație aplicată, anulată (fișierul înapoi octet cu octet) și
   refăcută; o tragere de cinci pași = un undo; istoria la limită; copierea / lipirea.

   **Lăsat:** o legătură lipită fără capete rămâne fără puncte la capete (draw.io le pune
   din forma lăsată în urmă): de completat la editarea legăturilor (80f).
3. **80c. Rasterizarea comună (`vector-tiles`).** Mutată din `markdown/src/tiles.rs` într-un
   crate comun: contururi (linii, Bézier, arce), umplere, linie (grosime, întreruptă),
   umbră, pe un pool de fundal, cu cache după cheie. Mermaid trece pe ea fără nicio
   schimbare vizibilă (snapshot-urile lui la fel). Forme: dreptunghi (rotunjit), elipsă,
   romb, paralelogram, hexagon, triunghi, cilindru, nor, document, proces, actor, notă,
   săgeți (ca în paleta „General” din captură); restul draw.io, pe loturi, după.

   **Făcut (2026-11-16):** crate-ul `crates/ui/vector-tiles` (stratul `ui`): conturul (`Seg`,
   `Line`, `rounded`, `ellipse`, `polygon`, `flowing`) și rasterizarea în plăci de măști
   alfa (`tile`, `grid`, `Mask`), mutate din `markdown`. `tile` primește acum orice listă de
   forme (`Shape { segs, fill, line }`) și mărimea scenei, nu scena Mermaid; Mermaid
   reexportă tipurile și trece prin ele (`markdown/src/tiles.rs` doar adaptează și își
   ține `draw`, legat de cache-ul lui). Testele Mermaid (88) trec neschimbate, inclusiv cel
   care compară plăcile cu desenul direct: nimic vizibil schimbat. Formele (`shapes.rs`,
   `ShapeKind` + `outline`): dreptunghi rotunjit, elipsă, romb, paralelogram, hexagon,
   triunghi, cilindru, nor (`mxCloud`), document, proces, actor (`mxActor`), notă, săgeată,
   săgeată dublă — fiecare un corp (umplut, conturat) și detalii (conturate: marginea
   cilindrului, barele procesului, colțul notei), după geometria draw.io. Verificate
   desenate (o imagine a tuturor) și prin teste: în cutia lor, mijlocul plin, colțurile
   goale unde forma nu ajunge, o cutie fără loc fără panică.

   **Lăsat:** potrivirea numelor din stilul draw.io (`shape=cylinder3`, `ellipse`…) cu
   formele e a modulului (80d). Măsurarea „imagine pe formă vs. plăci pe foaie” se face la
   80d, pe vizualizarea reală.
4. **80d. Vizualizarea, doar de citit (`modules/tools/diagram-editor`).** `ViewerProvider` pentru
   `.drawio`; foaia cu grilă (opțional „Page View”: marginile paginii), pan (spațiu + tragere,
   rotița), zoom (⌘ + rotița, ⌘0, „fit”), formele și legăturile desenate, textul în ele,
   paginile ca taburi jos. Fișierul citit pe worker; schimbat pe disc (Git, alt editor): citit
   din nou, cu dialogul obișnuit dacă tabul are schimbări nesalvate. Snapshot.

   **Făcut, vizualizarea (2026-11-16):** modulul `crates/modules/tools/diagram-editor` (id
   `tool.diagram-editor`, viewer `drawio`): un `.drawio` se deschide ca tab (după nume, nimic
   citit ca să decidă), fișierul citit pe worker-ul `diagram-read` (mărimea verificată
   înainte; un fișier stricat își spune motivul). `look.rs`: stilul draw.io → forma cu
   proporțiile ei (`arcSize`, `size`, `fixedSize`…), culorile (`none` față de „implicit”,
   care urmează tema), linia, fontul, alinierea; o formă necunoscută e cutia ei, cu textul.
   `label.rs`: textul HTML (`html=1`) cu rândurile lui, fără tag-uri, entitățile citite.
   `drawing.rs`: pagina desenată o dată pe versiune și temă — formele la locul lor absolut
   (în grup, în swimlane), straturile ascunse sărite, legăturile tăiate la marginea formelor
   (elipsă inclusă), drepte sau în unghi drept, prin punctele lor, cu capetele draw.io
   (classic, block, open, oval, diamond; goale cu `endFill=0`), etichetele pe legături la
   locul lor; textul implicit închis pe o umplere deschisă (ca negrul draw.io). `tiles.rs`:
   plăcile pe worker-ul `diagram-tiles`, la trepte de o jumătate de octavă (întinse între
   ele, cea completă de dinainte desenată sub cea care vine), cel mult 96, cele nefolosite
   de mult eliberate; fără waker (teste, snapshot) făcute pe loc. `camera.rs`: zoom în jurul
   mouse-ului (5%–800%), încadrarea la deschidere (cel mult 100%), ⌘0. Pan cu rotița și
   prin tragerea foii; paginile ca taburi jos; grila (din 4 în 4 linii când ar fi prea dese);
   fundalul paginii. Textele desenate de sistemul de text, într-un strat al lor peste
   plăci. `Viewer::file_changed` (nou, în `ui-components`): aplicația îl cheamă pentru tabul
   unui viewer care îi ține locul fișierului când fișierul se schimbă pe disc; diagrama se
   recitește pe worker, același text = nimic schimbat. Testele (15) și un snapshot al
   fixture-ului (elipsă, romb, legături cu săgeți, linie întreruptă, cilindru, notă, swimlane,
   textele lor).

   **Măsurat** (release): 500 de forme + 600 de legături: desenul făcut în 0,6 ms (pe firul
   UI, la o schimbare); 32 de plăci la 100% în 426 ms pe worker (~13 ms una; pe ecran se cer
   6–8), 14,6 MB de măști pentru toată foaia la 100%.

   **Făcut, fereastra Diagrams (2026-11-16):** un `ToolWindowProvider` (`diagram-files`, în
   dreapta, ultimul din grupul de sus, după baza de date — mutat acolo la cererea
   utilizatorului, 2026-11-17; iconița lui în stilul Lucide): lista din
   `<project dir>/diagrams/` — folderele întâi, apoi diagramele (după nume, fără `.drawio`),
   fără fișiere ascunse, fără legături urmate, cel mult 8 niveluri și 5000 de intrări —,
   citită pe worker-ul `diagram-list`; un folder pliat / depliat la click, o diagramă
   deschisă ca tab; fără diagrame: unde stau. Modulul își declară folderul `diagrams` ca al
   utilizatorului în folderul proiectului (`ProjectService::own_dirs`): watcher-ul îl aude,
   lista se recitește la orice schimbare din el. Teste (3) și un snapshot.

   **Lăsat:** crearea, redenumirea, ștergerea, mutarea, căutarea în fereastră — la 80i. Dialogul
   „fișierul s-a schimbat, ai modificări nesalvate” — la editare (80e). Un `.drawio` cu
   `.lumen/.gitignore` care ascunde `diagrams/` (`*`): la crearea primei diagrame (80i), cu un
   ajutor comun care doar adaugă `!diagrams/` și `!diagrams/**` (și pe care îl folosește și
   modulul de marcaje, care acum suprascrie fișierul).
5. **80e. Editarea.** Selecția (click, ⇧ / ⌘, dreptunghi de selecție), mutarea cu ghidaje și
   alinierea la grilă, mânerele de redimensionare, textul editat în formă (dublu click,
   câmpul nostru de text, cu cursorul comun), ștergerea, săgețile de tastatură (⇧: pas
   mare), duplicarea (⌘D), undo / redo, copierea / lipirea, salvarea (⌘S; tabul marcat
   nesalvat), blocarea (read-only, ca un text).

   **Făcut, selecția și gesturile (2026-11-16):** `edit.rs` (pur, în unitățile diagramei): ce
   e sub mouse (o legătură aproape întâi — sunt subțiri —, altfel forma de deasupra), ce ține
   o bandă întreg, mânerele unei cutii și cutia pe care o face unul tras (laturile lui pe
   grilă, niciodată peste cealaltă latură), o mutare aliniată la laturile / mijloacele
   celorlalte forme (cu ghidajele afișate), altfel la grilă. `view/gesture.rs`: o apăsare
   alege (un mâner al singurei forme alese, o celulă — ⇧ / ⌘ o adaugă sau o scoate —, foaia
   goală: o bandă; ⌥: pan); o tragere arată doar unde ar ajunge (contururile, ghidajele,
   banda), ca draw.io, iar eliberarea face schimbarea: un pas al istoriei (nimic rasterizat
   cât se mișcă mouse-ul). Tastele: Delete / Backspace șterg (cu ce țin și legăturile lor),
   săgețile mută cu un punct (⇧: un pas de grilă), Esc renunță. ⌘Z / ⇧⌘Z (aplicația le dă
   acum unei vederi care îi ține locul fișierului), ⌘A. Salvarea: peste fișier, printr-un
   fișier temporar (`files::write_atomic`), raportul ei de pe disc nu schimbă nimic; tabul
   nesalvat cât e. Blocat: se alege, nu se schimbă. Schimbat pe disc sub modificări nesalvate:
   ele rămân, utilizatorul e anunțat. Plăcile: după o editare, setul întreg de dinainte rămâne
   desenat până vin cele noi (fără pâlpâire, fără amestec). Teste (8 noi) și un snapshot
   cu totul ales.

   **Făcut, textul și clipboard-ul (2026-11-16):** `view/text.rs`: dublu click (sau ↩ pe
   singura celulă aleasă) deschide câmpul pe mai multe rânduri al componentelor
   (`MultilineField`) peste formă (la mijlocul legăturii), cu textul ei cum se vede (tag-urile
   HTML lăsate deoparte), ales întreg; el primește tastele, scrisul, undo-ul, clipboard-ul; Esc,
   un click în afară, altă pagină sau blocarea îl închid și textul e pus — un pas, nimic dacă
   n-a schimbat; un text HTML scris înapoi ca HTML (escapat, rândurile `<br>`), unul simplu ca
   atare. `view/clipboard.rs`: ⌘C dă XML-ul draw.io al celor alese (cu legăturile dintre ele);
   ⌘V îl lipește cu id-uri noi în primul strat, la un pas de unde era, și le alege; ⌘X: copiat
   de aplicație, apoi tăiat de vedere (aplicația i-l dă acum unei vederi care îi ține locul
   fișierului); ⌘D: copiat și lipit într-un pas (la fel, dat vederii, nu textului ascuns). Teste
   (3 noi) și un snapshot cu câmpul deschis.
6. **80f. Legăturile.** Trase dintr-un punct de conexiune al formei (punctele arătate la
   hover, ca în draw.io) la alta; rămân lipite când formele se mută; stilurile: drepte,
   ortogonale (rutate în jurul formelor), curbe; capetele (săgeată, romb, cerc, nimic);
   eticheta pe legătură; punctele de control mutate de mână.

   **Făcut (2026-11-16):** `view/connect.rs`: peste o formă, punctele ei de conexiune
   (mijloacele laturilor); unul tras peste altă formă: o legătură nouă între ele (stilul
   draw.io implicit: ortogonală), aleasă; pe foaie: capătul ei liber acolo (pe grilă). O
   legătură aleasă își arată capetele, punctele și mijloacele dintre ele: un capăt tras pe o
   formă se leagă de ea (pe foaie: liber), un punct se mută, un mijloc pune un punct nou; o
   linie punctată arată unde, fiecare un pas. Legăturile rămân lipite (traseul se face din
   forme la fiecare desen). Curbele (`curved=1`): o curbă netedă prin puncte (Catmull–Rom).
   `view/menu.rs` (click dreapta; ce e sub mouse ales întâi): linia (dreaptă / în unghi drept /
   curbă — două chei, un pas), capătul (niciunul, săgeată, triunghi, săgeată deschisă, cerc,
   romb), în față / în spate, grupează / degrupează, duplică, șterge. În model: `ops::update`
   (o celulă schimbată în mai multe feluri, un pas) și `ops::set_geometry`. Teste (6 noi).

   **Lăsat:** punctele fixe de ieșire draw.io (`exitX` / `entryX`): legăturile noi pleacă
   din marginea cea mai apropiată, nu din punctul ales; eticheta pe legătură se scrie prin
   dublu click pe legătură (80e), nu trasă de-a lungul ei.
7. **80g. Paleta (stânga).** Căutarea („Type / to search”), secțiunile pliabile (General,
   Misc, Arrows, Flowchart, Entity Relation, UML…), miniaturile rasterizate doar pentru
   secțiunile deschise; tragerea pe foaie sau clickul (formă pusă în mijlocul vederii);
   „Scratchpad”-ul ca secțiune a proiectului (`<project dir>/diagrams/scratchpad.drawio`).

   **Făcut (2026-11-16):** `palette.rs`: catalogul ca date, în ordinea aleasă — General (16
   forme), Flowchart (7), Entity Relation (3), UML (4), Arrows (2) —, fiecare cu stilul, mărimea
   și textul din paletele draw.io; căutarea după titlu (tradus) prin toate secțiunile;
   miniaturile unei secțiuni deschise făcute pe worker-ul `diagram-palette`, o dată pe temă și
   scară (desenate la mărimea formei și micșorate: proporțiile în pixeli — marginea
   cilindrului, colțul notei — rămân ale ei). `view/palette.rs`: paleta în stânga foii (196
   px): câmpul de căutare („Search shapes”; ↩ adaugă prima formă găsită, Esc golește apoi
   lasă), secțiunile pliabile (General deschisă), rotița o derulează când mouse-ul e peste
   ea, numele formei în tooltip. O formă: click — în mijlocul vederii; trasă — unde e lăsată
   (conturul ei arătat pe drum); colțul pe grilă, aleasă, un pas. Găsit pe drum: cu
   `fixedSize=1` fără `size`, draw.io ia 20 de pixeli (nu fracția 0,2) — paralelogramul și
   hexagonul ieșeau dreptunghiuri. Teste (4 noi) și un snapshot.

   **Lăsat:** „Scratchpad”-ul (o secțiune a proiectului în `diagrams/scratchpad.drawio`):
   are nevoie de crearea fișierelor din 80i; ascunderea paletei — la butoanele barei (80h).
8. **80h. Proprietățile (dreapta) și bara de unelte (sus).**
   - Panoul cu taburi: **Diagram** (grila și pasul ei, „Page View”, fundalul, umbra,
     mărimea paginii, portret / peisaj) când nu e nimic selectat; **Style** (umplere,
     linie, umbră, colțuri, text: font, mărime, aliniere, culoare) și **Arrange**
     (poziție, mărime, aliniere, distribuire, ordine) pentru selecție. Formularul nostru,
     selectorul nostru de culori, dropdown-ul nostru.
   - Bara: zoom (dropdown-ul nostru cu procentele), undo / redo, ștergere, față / spate,
     umplere, linie, umbră, stilul legăturii, inserare, tabel. Panourile se ascund
     (butoanele din dreapta barei), ca în draw.io.

   **Făcut (2026-11-16):** în `ui-components` (cum cere regula: ce lipsește unei componente se
   adaugă acolo) — `ds::number_field` (− / valoare / +, valoarea clicabilă ca s-o scrii) și
   `ds::swatch_row` (mostrele: tema, nimic tăiat, culorile; cea aleasă încercuită). În model:
   `Change::Setting` (setările paginii — grila, pasul, fundalul — schimbări ale istoriei, cu
   inversa lor) și `ops::set_page_setting`. `view/toolbar.rs`: bara de sus — paleta da / nu,
   undo / redo, șterge, în față / în spate, zoom − / valoare / „toată pagina” / +, proprietățile
   da / nu (dreapta); un buton care n-are pe ce acționa e cenușiu. `view/properties.rs`:
   panoul din dreapta (240 px), făcut la fiecare desen din ce e în memorie. Fără nimic ales:
   Diagram (grila, pasul ei, fundalul). Cu celule alese: Style (umplere, linie — mostrele
   paletei draw.io, „implicit” și „fără” —, grosimea, întreruptă, rotunjită; textul: mărimea,
   îngroșat, cursiv, alinierea, culoarea) și, pentru o formă, Arrange (stânga, sus, lățime,
   înălțime). Fiecare control un pas; un număr: − / + sau scris (↩ îl pune, Esc nu). Foaia
   între paletă și panou, sub bară. Teste (3 noi pe vedere, 2 pe componente, 1 pe model) și
   un snapshot.

   **Lăsat:** umbra, mărimea paginii (portret / peisaj, „Page View”), distribuirea și
   alinierea mai multor forme între ele, fontul (familia), selectorul de culori complet (doar
   mostrele paletei); stilul legăturii în bară (e în meniul ei, 80f).
9. **80i. Fișiere noi, export, integrarea.** „New › Diagram” (comandă, meniul arborelui):
   un `.drawio` gol, deschis; exportul ca SVG și PNG (pe worker, „Save as…”); previzualizarea
   unui `.drawio` în Markdown (`![](arch.drawio)` arată diagrama), dacă iese ieftin; ce
   ocupă (memoria tabului) în Task Manager.

   **Făcut, fișierele (2026-11-16):**
   - **`files::ignore::let_through`** (crate-ul comun `files`): un modul adaugă doar
     rândurile lui `!…` care lipsesc din `.gitignore`-ul folderului proiectului. Fișierul e
     făcut cu `*` dacă lipsește și nu îl mai suprascrie nimeni. Modulul de marcaje trece pe el:
     înainte suprascria fișierul și ar fi șters excepțiile altora.
   - **`create.rs`**: o diagramă nouă (o pagină goală; „Untitled.drawio”, „Untitled
     2.drawio”…) sau un folder nou („New folder”…), pe worker, cu `diagrams/` lăsat în Git
     (`!diagrams/`, `!diagrams/**`).
   - **Comanda `diagram.new`** („New Diagram”, în paleta de comenzi): diagrama e făcută în
     `diagrams/` și deschisă. Nu e în bara de meniu: meniul File e plin (15 rânduri, limita
     testului).
   - **Fereastra Diagrams**: butoanele „New Diagram” / „New Folder” fac în folderul clicat
     ultima dată. Meniul unui rând are:
     - redenumirea în rând (↩: aplicația redenumește, tabul urmează; `.drawio` păstrat);
     - „Move to Folder ›” (aplicația mută);
     - ștergerea (coșul sistemului, pe worker; ștergerea anunțată pe evenimentele
       proiectului).
   - **Teste**: 5 noi. Ștergerea nu e încercată pe disc, fiindcă ar ajunge în coșul real.

   **Lăsat:**
   - mutarea prin tragere și căutarea după nume în fereastră (meniul acoperă mutarea);
   - „New › Diagram” în meniul arborelui: nicio secțiune „New” a lui nu e deschisă încă
     modulelor.

   **Făcut, exportul (2026-11-16):**
   - **Unde**: meniul de click dreapta pe foaie, fără nimic ales: „Export as SVG… / PNG…”.
     Calea se alege în dialogul „Save As” al sistemului (`AppRequest::PickSavePath`), cu
     numele fișierului propus (și al paginii, când are mai multe).
   - **Cum**: pagina e desenată din nou pe un worker (`diagram-export`), în culorile draw.io
     (alb, linii și texte negre; fundalul paginii, dacă are), cu 10 px de margine. Când e gata,
     un toast spune unde s-a scris (sau de ce n-a mers).
   - **`texts.rs`**: așezarea unui text (paragrafe, rupte la lățimea cutiei, aliniate) scoasă
     din vedere și folosită de vedere, SVG și PNG: o etichetă se rupe la fel peste tot.
   - **SVG** (`export::svg`): un `<path>` pentru fiecare contur, un `<text>` pentru fiecare
     rând (escapat), fundalul etichetelor de pe legături.
   - **PNG** (`export::png`): chiar desenul vederii — tile-urile formelor (`vector_tiles`) și
     glifele textelor (un `TextSystem` al workerului, eliberat la final) — compus pe CPU în
     două straturi, ca în vedere, și encodat cu `tiny-skia`. Scara e 2× (clar pe ecrane
     dense), mai mică dacă latura ar trece de 8192 px.
   - **De ce nu `render-software`**: stratul render-driver nu e permis modulelor.
   - **Teste**: 7 noi; `EDITOR_DIAGRAM_SHOTS=<dir>` scrie exporturile fixture-ului, ca să fie
     privite. Verificat: PNG-ul și SVG-ul (prin Quick Look) arată ca vederea.

   **Lăsat:**
   - previzualizarea unui `.drawio` în Markdown (`![](arch.drawio)`): ar cere modulului
     Markdown să știe diagramele (un contract nou); de cântărit separat;
   - memoria tabului în Task Manager;
   - exportul unei selecții sau al tuturor paginilor (doar pagina deschisă);
   - textele HTML bogate (`<b>` într-o etichetă): desenate ca text simplu, ca în vedere.
10. **80j. Review de un agent separat**, apoi repararea celor găsite; „Făcut” / „Lăsat”
    scrise aici; regula diagramelor în `.claude/rules/project-conventions.md`.

    **Făcut (2026-11-16):** două review-uri separate, în paralel (modelul + rasterizarea;
    modulul + aplicația). Fiecare constatare a fost verificată în cod, apoi reparată, cu un
    test care ar fi picat înainte.

    *Modelul (`diagram-model`):*
    - un singur buget pe fișier: elementele tuturor paginilor (comprimate sau nu) se numără
      împreună, iar paginile comprimate se pot dezarhiva la cel mult 256 MB în total — înainte,
      un fișier de 12 KB cu 4 pagini comprimate trecea de limită de 2,4 ori;
    - un atribut scris între ghilimele simple care conține `"` nu mai e păstrat „ca scris”
      (salvat între ghilimele duble, strica fișierul); tab-urile și `\r` sunt scrise ca
      `&#x9;` / `&#xd;`, iar caracterele de control pe care XML nu le permite sunt lăsate afară;
    - `number()` nu mai scrie niciodată `inf` / `NaN`; `group` fără nicio geometrie nu face
      nimic;
    - ungroup mută și punctele legăturilor din grup;
    - „send to back” cu celule din mai mulți părinți: fiecare ajunge înaintea primului frate
      al lui, iar ordinea se calculează direct, în O(n) (înainte O(m²·n));
    - `replace_each` copiază doar celulele pe care le poate schimba (un pas de drag pe o
      pagină mare nu mai copiază toată pagina);
    - `paste` caută id-urile date într-un `HashSet`;
    - `copy` pune celulele din vârf în stratul clipboard-ului, la locul lor pe pagină (un grup
      necopiat nu le mai deplasează în draw.io);
    - istoria își socotește corect memoria (XML-ul păstrat al celulei, punctele).

    *Rasterizarea (`vector-tiles`):* un tile cu mai mult de 8 culori e o singură imagine RGBA
    (1 MB), în loc de 1 MB de ponderi pentru fiecare culoare, păstrați pe fir.

    *Editorul:*
    - un fișier redenumit sau mutat se salvează unde e acum (`Viewer::moved`);
    - ⌘S în timpul editării unui text îl include;
    - ⌘X / ⌘C / ⌘A / ⌘Z / lipirea, cu un număr în curs de scriere în panou sau cu căutarea
      din paletă activă, sunt ale câmpului, nu ale celulelor (înainte, ⌘X ștergea forma);
    - citirile fișierului au un număr: una mai veche care ajunge după una nouă sau după o
      salvare e lăsată (înainte putea readuce conținutul vechi);
    - textele se așază o singură dată pentru fiecare (desen, zoom, font), la primul desen, și
      sunt doar mutate la pan;
    - Esc în timpul unui drag îl oprește și păstrează selecția;
    - miniaturile paletei făcute pentru o temă sau o scară veche sunt aruncate;
    - exportul PNG nu mai poate depăși 8192 px pe latură (refuză geometria infinită);
    - `diagram-palette` și `diagram-export` apar în Task Manager.

    *Fereastra Diagrams:*
    - meniul unui rând și redenumirea țin intrarea după cale, nu după poziție (o relistare
      între timp nu mai schimbă ținta);
    - prima listare pornește la primul desen, pe worker (înainte rula pe firul UI);
    - `.drawio` e recunoscut în orice combinație de majuscule.

    11 teste noi (model 7, rasterizare 1, editor 4 + fereastră 1).

    **Lăsat:**
    - **`.gitignore` scris din două module deodată:** fără blocare, unul își poate pierde
      rândul, dar îl adaugă din nou la următorul apel. O blocare ar însemna I/O sub lock.
    - **`with_descendants`:** O(adâncime·n), limitat de adâncimea de 64.
    - **Ce draw.io scrie rar și se pierde la salvare:** comentariile XML, prefixele de
      spațiu de nume, textul amestecat cu elemente.
    - **Salvarea rulează pe firul UI:** contractul `Viewer::save` e sincron, ca la vizualizarea
      de imagini; o diagramă mare o simte.

## Măsurat înainte de ales (de scris aici la implementare)

- O diagramă de 500 de forme și 600 de legături: memoria tabului, timpul unui cadru la pan
  și la tragerea unei forme (ținta: cadrul sub 4 ms pe CPU), timpul deschiderii.
- Rasterizarea formelor: imagine per formă vs. plăci pe toată foaia (ce face Mermaid) —
  măsurate amândouă la zoom și la pan, alese după cifre.
- Fișierul: dus-întors pe 20 de diagrame reale din draw.io, fără diferențe.

**Măsurat și optimizat (2026-11-16, release, Mac):** două review-uri separate doar pe
CPU / RAM / GPU, apoi măsurători înainte și după. Teste ignorate le refac:
`view::tests::measure_a_large_page`, `texts::tests::measure_wrapping`.

*O pagină de 2.000 de forme și 2.400 de legături, foaia 1600 × 1000 la 2×* (înainte → după):

| Ce | Înainte | După |
|---|---|---|
| un cadru cu toate celulele alese | 30,8 ms | 0,4 ms |
| o mișcare de mouse cu toate alese | 2,6 ms | 0,001 ms |
| un cadru cu o formă aleasă | 96 µs | 55 µs |
| un cadru la 100% | 3,2 ms | 2,0 ms |
| un pas de zoom | 2,65 ms | 1,8 ms |
| un pan | 0,44 ms | 0,016 ms |
| glife trimise GPU-ului la 100% | 2.288 | 1.352 |

Cauzele:
- **selecția**: căutată liniar pentru fiecare celulă, de mai multe ori pe cadru; acum o
  mulțime, iar pentru câteva celule, comparare directă;
- **textele**: se așezau și cele până la 2.000 px în afara foii; acum doar cât poate ajunge
  textul, calculat o dată pe desen;
- **panoul**: își căuta celula de ~10 ori pe cadru; acum un index, ținut pe versiune.

Cadrul fără schimbări rămâne de 16 µs.

*Așezarea textelor:* 2.000 de etichete la o mărime nouă costau 136 ms rupte cuvânt cu cuvânt
și costă 89 ms dintr-o singură modelare pe paragraf. Lățimea vine din glife; aproape de
limită, sau în scrieri de la dreapta la stânga, rândul e modelat exact. Un test arată că
rândurile sunt identice în ~2.000 de combinații (mărimi, lățimi, accente, emoji, arabă).

*O editare:* refăcea toate tile-urile din vedere (24–40, adică 45–65 ms pe worker și ~11 MB
încărcați pe GPU). Acum se refac doar cele atinse de schimbare; celelalte trec la noul desen,
identice pixel cu pixel (testat). Tile-urile unui desen înlocuit își eliberează texturile
imediat, nu după 20 s. Un tile terminat cere un cadru doar dacă se vede.

*Modelul, 10.000 de celule* (bench separat):

| Ce | Înainte | După |
|---|---|---|
| celulele clonate | 22 MB, 62 alocări / celulă | 15 MB, 29 alocări / celulă |
| un stil | 445 B în 12 alocări | 130 B în 2 |
| mutarea unei forme | 6,1 ms | 0,4 ms |
| o proprietate pe tot selectat | 185 ms | 10,5 ms |

Cum:
- stilul e un singur text cu pozițiile intrărilor;
- celula nu-și mai copiază câmpurile la citire, ci le compară cu XML-ul păstrat;
- textul brut al unui atribut se păstrează doar când diferă de valoare;
- `move_by` copiază doar ce se mișcă;
- `set_style` și `group` folosesc o mulțime;
- `copy` folosește un index.

*Restul:*
- **exportul PNG**: compune tile cu tile și encodează direct, deci nu mai ține măștile
  paginii și nici o a doua copie a imaginii;
- **tile-urile RGBA**: își lasă pixelii din RAM după încărcare;
- **citirea**: amprenta fișierului se calculează pe worker (textul nu mai vine pe firul UI);
- **Task Manager**: memoria e socotită unde e (RAM până la încărcare, apoi GPU), iar modelul
  după greutatea reală, o dată pe versiune;
- **„Clear and Rebuild Caches”**: golește și textele așezate și miniaturile.

*Zoom-ul (cerut de utilizator, 2026-11-16):* textele se așază la trepte de 1/12 de octavă
(`texts::text_step`). Glifele au mărimea treptei, cu cel mult ~3% față de cea exactă;
rândurile stau unde ar sta la mărimea exactă (pozițiile lor întinse până acolo). Ultimele 3
trepte rămân așezate, așa că un zoom înainte și înapoi nu reașază nimic. Un pinch (cadre de
câte 1%) costa ~1,9 ms pe cadru, pentru că fiecare cadru reașeza textele; acum costă
0,15 ms. Exportul SVG / PNG rămâne la mărimea exactă. Verificat într-un snapshot.

**Lăsat:**
- **un pas al istoriei care păstrează doar geometria**: o schimbare mai mare a modelului;
  pașii s-au micșorat deja odată cu celulele;
- **salvarea pe un worker**: contractul `Viewer::save` e sincron;
- **cutiile formelor ținute pe desen pentru tăierea pe tile-uri**: ~0,1 ms pe tile, pe worker.

## Deciziile utilizatorului (2026-11-16)

- **Formatul: `.drawio`.** Da.
- **Unde stau: `<project dir>/diagrams/`** (`.lumen/diagrams/` în proiect), organizate pe
  **foldere**, ca arborele proiectului și ca listele pe care le-am făcut deja (sursele de date,
  sesiunile agenților). Folderele sunt foldere reale pe disc (le vede și Git, și draw.io).
  - Se adaugă o fereastră a ei, **Diagrams** (un `ToolWindowProvider`, cu butonul ei pe
    margine): arborele cu folderele și diagramele, „New Diagram” / „New Folder”,
    redenumire în rând, ștergere (în coș, ca în arbore), mutare prin tragere sau
    „Move to Folder ›”, căutare după nume. Click: diagrama se deschide ca tab.
  - Un `.drawio` din altă parte a proiectului se deschide la fel (din arbore); fereastra
    arată doar ce e în `diagrams/`.
  - `<project dir>/` are deja `.gitignore`-uri pe module (ce e personal nu se comite);
    `diagrams/` **se comite** (sunt documentele proiectului): de verificat că nicio regulă
    nu-l ascunde.
- **Bibliotecile de forme**: după „General”, în ordinea propusă (Flowchart, Entity Relation,
  UML, Arrows).
- **Numele crate-ului modulului**: `modules/insight/diagrams` există deja (diagramele de
  clase generate din cod), deci modulul nou e `modules/tools/diagram-editor` (crate
  `diagram-editor`), domeniul `diagram-model`.

Pasul 80d primește fereastra Diagrams (lista, deschiderea); 80i, crearea / mutarea /
folderele.
