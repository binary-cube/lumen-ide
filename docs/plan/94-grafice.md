# 94 — Graficele: o componentă comună, interactivă, și un modul care desenează rezultatele

**Cererea:** din rezultatul unei interogări (tabelul de date) să putem face un grafic:
bare, plăcintă, arie, linie, puncte, bule, bare stivuite, arie stivuită, histogramă. Axa
X, grupurile și axa Y se aleg din coloanele rezultatului, cu opțiuni (în trepte, cu puncte)
și mai multe serii. Graficele să fie un modul separat, cu componente configurabile, bine
scrise și scalabile, cu hover și interacțiune. Aceleași componente să fie folosite și în
Task Manager, în profiler și oriunde mai sunt grafice.

## Unde suntem azi (citit din cod, 2026-11-23)

- **`ui_components::chart`** e singurul cod de grafice, făcut pentru serii de timp de
  sistem:
  - seriile sunt un enum fix (CPU, memorie, GPU, rețea, disc);
  - valorile sunt `f32` într-un inel, desenate din dreptunghiuri (rendererul nu desenează
    curbe), cel mult două pe coloană de pixeli;
  - `sparkline` (bare sau trepte) și `time_chart` (o grilă, o singură etichetă cu maximul,
    fără axa X, fără legendă). La hover are o linie de ghidaj și câte un punct pe serie;
    tooltip-ul îl face apelantul (Task Manager: `reading_tip`, „acum 12 s: 3,4%”).
  - Nu are categorii pe X, valori negative, linii, puncte, plăcintă, stivuire.
- **Desenate de mână, separat:**
  - axa de timp și benzile profilerului (`profiler/src/view/page.rs:296`);
  - graficele din popover-ul barei de stare (fără hover);
  - graficul de ramuri din git (e altceva: rânduri cu benzi, rămâne al lui).
- **Markdown, prin Mermaid** (`modules/viewers/markdown/src/mermaid/`):
  - desenează plăcintă, gantt, cronologie, `xychart` (bare și linii pe două axe), sankey,
    radar, treemap și altele;
  - construiește o scenă de contururi (curbe cubice, arce) plus texte, rasterizată în
    afara firului UI cu `ui/vector-tiles` (tiny-skia), apoi exportabilă în SVG și PNG;
  - acolo sunt singurele linii, arce și felii netede din aplicație. Funcțiile (`ticks`,
    `arc`, paleta `Look`) sunt private modulului și n-au hover.
- **Tabelul de date** (`ui_components::data_grid`):
  - n-are niciun punct prin care un alt modul să-i adauge un buton sau un panou: `Tool` e
    o listă închisă, iar `GridAction` doar Copy, Submit, Export, Command;
  - un rezultat de consolă vine în bucăți de câte 1000 de rânduri (`Arc`); se aduc 500 la
    început, restul la cerere (`fetch_more`), iar rândurile vechi pot fi eliberate pentru
    memorie;
  - tabelul unei tabele din explorer ține doar blocurile vizibile;
  - deci un grafic are la îndemână doar rândurile aduse, dacă nu cere mai multe.
- **Între module:** precedentul e un registru neutru plus o trăsătură, umplute de aplicație
  (`contracts::profiling`, `extractors::catalog`, `agents::AgentProvider`). Pentru „un
  panou peste un rezultat tabelar” nu există încă nimic.

## Ce construim

Trei straturi, fiecare cu o singură treabă:

1. **`charts-model`** (domeniu pur, `crates/domain/viewers/charts-model`; nu știe de ecran):
   - **specificația** unui grafic (`ChartSpec`): felul, seriile (coloana X, coloana de
     grupare, coloanele Y, agregarea: sumă, număr, medie, min, max), opțiunile (în trepte,
     cu puncte, stivuit, procente, scară logaritmică), serializabilă, ca s-o putem păstra;
   - **datele** ca trăsătură (`Tabular`): coloanele (nume, tip: număr, text, dată), câte
     rânduri, o celulă ca număr, text sau moment. Rezultatul consolei, tabelul din
     explorer, Task Manager și un CSV o implementează, fără să se cunoască;
   - **pregătirea:** gruparea, agregarea, sortarea categoriilor, histograma (lățimea
     coșurilor după regula Freedman–Diaconis), stivuirea, rărirea punctelor pentru ecran
     (min și max pe coloană de pixeli, ca `Plot` azi);
   - **scalele:** liniară, logaritmică, de timp (pași „frumoși”: secunde, minute, zile,
     luni), pe categorii; diviziunile „frumoase” (1, 2, 5 × 10ⁿ) și formatarea etichetelor;
   - **geometria:** dreptunghiurile barelor, punctele liniilor, feliile plăcintei, cutiile
     axelor și ale legendei, pentru o mărime dată. E pură, deci testabilă fără ecran;
   - **ce e sub mouse** (`hit`): bara, felia sau cel mai apropiat punct, pe X sau în plan.

2. **`ui_components::charts`** (componenta, înlocuiește și extinde `chart.rs`):
   - **`Chart`**, o parte de formular (`Control`): primește geometria și o desenează:
     - barele, histograma și grila din dreptunghiuri, direct (nimic de rasterizat);
     - liniile, ariile, feliile și punctele ca contururi, prin `vector-tiles`, pe un
       worker, ținute ca imagine după generația datelor și mărime. Hover-ul nu le refă;
     - axele, etichetele și legenda ca text, cu fonturile interfeței.
   - **Interacțiunea**, toată prin contractul `Control`:
     - hover: elementul sub mouse evidențiat, o linie de ghidaj pe X;
     - tooltip-ul: categoria și valoarea fiecărei serii, cu bulina culorii (componenta
       comună de tooltip primește rânduri cu bulină);
     - un clic pe legendă ascunde sau arată o serie;
     - pe un grafic cu X numeric sau de timp: zoom cu rotița, deplasare trăgând, dublu-clic
       înapoi la tot;
     - meniul la clic dreapta: copiază valorile, exportă PNG sau SVG.
   - **Culorile** din temă (`tag_colors`), aceeași ordine a seriilor peste tot, lizibile pe
     ambele teme.
   - **Accesibilitatea:** graficul e un grup numit după titlu, cu un rezumat („bare, 12
     categorii, 2 serii, maxim 4 210”). Elementele sunt noduri citibile cu valoarea lor, cel
     mult câteva zeci; peste atât, rezumatul.
   - **Opțiuni cu valori implicite**, ca la celelalte componente: un apelant care nu cere
     nimic primește aspectul obișnuit.

3. **Modulul `charts`** (`crates/modules/viewers/charts`):
   - **Panoul de serii** (ca în captură), construit din componente:
     - felul graficului: segmente cu iconițe;
     - coloanele: dropdown-ul comun cu picker, doar coloanele potrivite (Y numerice; X
       orice; grupuri: text);
     - opțiunile: checkbox-uri, cu doar cele care au sens pentru felul ales;
     - „Adaugă o serie”, „Elimină”.
   - **Graficul** e un `Chart` alături de tabel, într-un splitter, deasupra sau la dreapta.
   - **Contractul** prin care apare lângă tabel (nou, în `contracts`, după precedentul
     registrelor):
     - `ResultPane`: „pot arăta ceva despre un rezultat tabelar”;
     - modulul bazelor de date îl întreabă dacă există cineva și abia atunci arată butonul
       „Grafic” în bara rezultatului (`presence`);
     - datele trec ca `Tabular`, deci niciun modul nu-l cunoaște pe celălalt;
     - fără modulul de grafice, butonul nu apare.
   - **Datele:**
     - implicit rândurile aduse deja; bara graficului spune „din 500 de rânduri aduse” și
       oferă „Adu toate” (cu o limită, de exemplu 200 000, și o bară de progres);
     - pregătirea rulează pe un worker (`kernel::work`) și e ținută după epoca
       rezultatului, deci nimic nu se refă la hover sau la desenare.
   - **Ce se păstrează:** specificația graficului, pe interogare sau pe tabelă, în folderul
     proiectului. Redeschis, rezultatul își regăsește graficul.

## Cuplajul: cine pe cine cunoaște (regula modulelor, `docs/plan/69-comunicare-module.md`)

```
 charts-model (domeniu)      ui_components::charts (componentă)
        ▲   ▲                       ▲        ▲        ▲        ▲
        │   │                       │        │        │        │
        │   └──── contracts::result_pane ◄───┼────────┼── modulele bazelor de date
        │              ▲                     │        │        (consolă, explorer)
        │              │ contribuție         │        │
   modulul charts ─────┘                     │        └── Task Manager, relaxare,
        └────────────────────────────────────┘            profilerul, bara de stare
```

- **Piesele comune nu sunt module:**
  - `charts-model` e un crate de domeniu;
  - `ui_components::charts` e o componentă, ca `DataGrid` sau `ds::button`.

  Oricine le folosește direct: Task Manager, modul de relaxare, profilerul, bara de stare,
  Mermaid. Așa se desenează un grafic, la fel peste tot, fără niciun modul la mijloc.
- **Modulul `charts` nu e cunoscut de nimeni.** Se leagă de rezultate pe două dintre cele
  patru căi permise:
  - **contribuție** (`ctx.contribute`): un `ResultPaneProvider` (id, iconiță, titlu, cum
    se face un panou). Aplicația îl pune în registrul din `contracts::result_pane`, ca la
    `profiling` și `extractors`;
  - **cerere înapoi, prin ce întoarce panoul:** de exemplu „vreau toate rândurile, până la
    N”, la care răspunde gazda.
- **Modulele bazelor de date nu știu de grafice.**
  - Întreabă registrul ce panouri există și arată câte un buton pentru fiecare (cu iconița
    și titlul lui). Fără niciunul, nu arată nimic (`presence`).
  - Dau datele ca `&dyn Tabular` (trăsătura din `charts-model`, neutră) și țin ele textul
    salvat al panoului, sub cheia interogării sau a tabelei, în folderul lor. Modulul de
    grafice nu citește fișierele altui modul.
- **Contractul panoului** (`contracts::result_pane::ResultPane`), în stilul `Control`:
  - `paint`, `hover`, `press` / `drag` / `release`, `scroll`, `tooltip`, `cursor`, `menu`;
  - `data_changed(epoch)`;
  - `saved() -> Option<String>` / `restore(&str)`;
  - `take_request() -> Option<PaneRequest>`.

  Gazda îl pune într-un splitter sub tabel și nu-i cunoaște conținutul.
- **Verificat de `cargo xtask check-deps`:**
  - modulul `charts` depinde doar de `charts-model`, `ui-components`, `contracts`,
    `extensions`, `i18n`;
  - modulele bazelor de date nu-l numesc;
  - testul care scoate pe rând fiecare modul trece și fără el.

## Pași

- **94a. Modelul:**
  - specificația, `Tabular`, pregătirea, scalele, diviziunile, geometria și `hit`, cu
    teste pe fiecare fel de grafic;
  - `ticks` și arcele din Mermaid mutate aici: Mermaid le folosește pe acestea, deci avem
    o singură implementare.
- **94b. Componenta `Chart`:**
  - desenul (dreptunghiuri, plus contururi prin `vector-tiles` pe worker), axele, legenda;
  - hover, tooltip cu bulină, legenda care ascunde, zoom și deplasare;
  - un test că bara de derulare și hover-ul merg; snapshot-uri pentru fiecare fel, pe
    ambele teme.
- **94c. Contractul `ResultPane` și `Tabular`:** rezultatul consolei și tabelul din
  explorer le implementează; butonul „Grafic” în bara rezultatului, doar când există un
  furnizor.
- **94d. Modulul `charts`:** panoul de serii, splitter-ul, „Adu toate”, păstrarea
  specificației.
- **94e. Task Manager și profilerul** trec pe `Chart`:
  - aspectul de azi rămâne implicit, fără niciun pixel mutat unde nu se cere;
  - câștigă axa de timp, legenda și tooltip-ul cu toate seriile;
  - `time_chart` și `sparkline` devin cazuri ale componentei, iar `chart.rs` vechi dispare.
- **94f. Restul graficelor:** widget-urile modului de relaxare (screensaver-ul, `modules/*/relax`: toate sunt grafice), popover-ul barei de stare (cu hover), Mermaid `xychart` și
  plăcinta (desenate cu aceeași geometrie, cu hover în vizualizarea markdown).
- **94g. Export și copiere:** PNG, SVG, valorile ca CSV; meniul la clic dreapta.

## Bugetele (măsurate, scrise aici la fiecare pas)

- 100 000 de puncte: pregătirea pe worker sub 50 ms, desenul unui cadru sub 1 ms (puncte
  rărite pe coloane de pixeli).
- Hover-ul nu refă nimic: o căutare în geometria ținută și un strat mic deasupra.
- Memorie: geometria e proporțională cu lățimea în pixeli, nu cu numărul de rânduri.
  Imaginea contururilor e eliberată când graficul nu se mai vede.
- Nimic pe firul UI în afară de desen și de căutarea sub mouse.

## Hotărât cu utilizatorul (2026-11-23)

- **Locul:** graficul stă sub tabel, cu panoul de serii la dreapta lui; splitter-ul ține
  mărimea aleasă.
- **„Adu toate”:** limita implicită e de 200 000 de rânduri, ca setare, cu o bară de
  progres cât se aduc. Setarea stă pe pagina „Grafice” (Instrumente, după Baze de date), a
  modulului de grafice: o pagină dispare doar cu modulul care o ține, deci o setare a
  graficelor pe pagina bazelor de date ar fi ținut-o vie fără modulele ei.
- **Păstrarea:** în proiect, pe interogare (după textul ei) și pe tabelă; redeschis,
  rezultatul își regăsește graficul.

## Lăsat de la început (și de ce)

- **O bibliotecă de grafice adusă din afară:** ar desena în felul ei, nu cu tema,
  fonturile, tooltip-urile și accesibilitatea noastre, și n-ar ști de rendererul nostru.
  Folosim piesele pe care le avem deja (`vector-tiles`, text, componente).
- **Grafice 3D, hărți:** nu sunt cerute.

## Făcut (2026-11-23): 94a–94d, prima trecere

- **94a, modelul** (`crates/domain/viewers/charts-model`, pur):
  - `spec` (felul, seriile, agregarea, opțiunile, serializabile);
  - `data` (`Tabular`, `Rows`, `moment` pentru datele scrise de baze de date);
  - `prepare` (categorii, grupuri, puncte, felii cu „restul”, histograma după
    Freedman–Diaconis, limite: 2 000 de categorii, 24 de grupuri, 12 felii);
  - `scale` (pași 1-2-5, logaritmic, timp cu luni și ani din calendar, etichete scurte);
  - `geometry` (barele, stivuirea, liniile cu golurile lor, ariile, plăcinta, legenda,
    rărirea la două puncte pe coloană de pixeli, ce e sub mouse).
  - 17 teste.
- **94b, componenta** (`ui_components::charts`):
  - `Chart` și `ChartLook`: culorile, grila, etichetele și fonturile din temă; `ChartParts`
    pentru ce se arată (`bare()` pentru un widget);
  - barele și punctele din dreptunghiuri; liniile, ariile și feliile rasterizate netede pe
    un worker (`chart-raster`), refăcute doar la un layout nou sau la altă temă, cu masca
    veche desenată cât se face cea nouă;
  - la hover: elementul luminat, linia de ghidaj și punctele seriilor, tooltip-ul cu
    categoria și valorile; felia de sub mouse iese din plăcintă;
  - legenda ascunde o serie la clic; zoom cu rotița, deplasare trăgând, dublu-clic pentru
    tot;
  - pentru cititorul de ecran: o imagine numită după titlu, cu rezumatul.
  - 7 teste (hover, legendă, zoom, rasterizare, accesibilitate).
- **94c, contractul** (`ui_components::result_pane`): `ResultPaneProvider` (contribuție,
  registru umplut de aplicație, ascuns cu modulul lui), `ResultPane` (un `Control` care
  desenează), `Lent` (rândurile împrumutate), `Held`, `PaneRequest::AllRows`.
  - **Gazda, rezultatul consolei** (`database-console`):
    - `lent_rows` (bucățile împărțite, fără copii);
    - `result_panes`: butoanele din bară, panoul sub tabel cu despărțitor tras, intrările,
      „Adu toate” până la setarea `console_pane_rows` (200 000 implicit), starea păstrată
      în `<proiect>/<dir>/database/panes/<cheie>-<panou>.txt`, citită și scrisă pe
      worker-ul consolei.
- **94d, modulul** (`crates/modules/viewers/charts`):
  - panoul de serii din componente: felurile ca butoane cu iconițe proprii, câmpurile ca
    dropdown cu picker-ul comun (Y alege mai multe), opțiunile care au sens pentru fel ca
    checkbox-uri;
  - bara: „din câte rânduri”, „Adu toate”, ⚙;
  - pregătirea pe worker (`chart-prepare`), cea mai nouă cerere câștigă;
  - mesaje pentru ce nu se poate desena.
  - 5 teste.
- `check.sh`: 4 ok.

### Lăsat (următorii pași)

- **Mai multe serii cu coloane X diferite** („Adaugă o serie”): acum o serie cu mai multe
  coloane Y și grupuri acoperă cazurile obișnuite.
- **Tastele în picker-ul panoului** (căutarea, Esc): gazda dă doar mouse-ul panoului. Se
  închide la un clic alături.
- **Tabelul din explorer** ca a doua gazdă.
- **94a, partea cu Mermaid:** Mermaid să folosească `scale` și arcele de aici.
- **94e–94g:** Task Manager, profilerul, modul de relaxare, bara de stare, Mermaid, exportul.

## 94h — Grafice din orice set de date (cerut 2026-11-23)

**Cererea:** graficul să se poată face din orice date. Un JSON, un XML, un YAML, un HTML
(tabelele lui), un CSV sau TSV dau un grafic, la fel ca rezultatul unei interogări. Pentru
asta trebuie un strat care recunoaște sursa.

**Stratul de recunoaștere** (`crates/domain/viewers/data-tables`, pur, fără I/O):

- **Formatul:** după extensie, apoi după conținut (primul caracter care nu e spațiu, un
  `<!doctype html>` / `<html`, un `<?xml`, un `---` YAML sau `cheie:` la început de rând, o
  linie cu separatori constanți pentru CSV / TSV). Un fișier fără extensie cunoscută e
  recunoscut tot după conținut.
- **Tabelele găsite într-un document**, toate, fiecare cu drumul ei (`$.orders`,
  `/catalog/book`, „tabelul 2”), ca utilizatorul să aleagă:
  - **JSON / YAML:** orice listă de obiecte (coloanele: unirea cheilor, cele imbricate
    aplatizate ca `client.oras`); o listă de liste (primul rând ca antet dacă e text); un
    obiect de obiecte (cheia devine o coloană);
  - **XML:** elementele repetate sub același părinte (coloanele: atributele și copiii cu
    text);
  - **HTML:** fiecare `<table>` (antetul din `<th>` sau primul rând), `colspan` respectat;
  - **CSV / TSV:** citit după RFC 4180 (ghilimele, rânduri pe mai multe linii), separatorul
    ghicit.
- **Tipul coloanelor**, din valori: numere (și cu zecimale cu virgulă `1,5` când așa sunt
  toate), momente (`moment`), restul text. Un câmp gol nu decide.
- **Limite:** un document ostil nu oprește nimic. Limite pe adâncime, pe numărul de tabele,
  rânduri și coloane; totul rulează pe un worker, niciodată pe firul UI.
- Rezultatul e un `Tabular` (`charts_model`), deci graficul, panoul de serii și păstrarea
  setărilor sunt aceleași.

**Unde apare:**

- **„Deschide ca grafic”** pe un fișier de date (meniul tabului, arborele proiectului,
  paleta de comenzi): un tab cu graficul și panoul de serii. Deasupra stă alegerea tabelului
  când documentul are mai multe.
- **Din selecție:** un text selectat în editor (un JSON lipit, un tabel HTML) deschis ca
  grafic.
- Contribuit de modulul `charts`, ca o comandă și un viewer. Nicio altă extensie nu-l
  cunoaște.

**Pași:** recunoașterea și tabelele (cu teste pe fiecare format și pe documente ostile),
apoi tab-ul de grafic pentru un fișier, apoi selecția din editor.

## Făcut (2026-11-23): a doua trecere

- **Tastele în lista de coloane:** cât e deschisă, lista panoului ia tastele și textul
  (↓ ↑ ↵ Esc, căutarea), indiferent cine are focusul (`captures_keys`), la toate gazdele.
- **Mai multe serii, fiecare cu coloana ei X:** categoriile sunt ale tuturor, fiecare serie
  își așază rândurile după X-ul ei. În panou: „1 2 … +” și ștergerea seriei curente (cel
  mult 8).
- **Gazda comună** (`ui_components::result_pane::PaneHost`): butoanele din bară, împărțirea
  cu despărțitor, intrările, tastele, tooltip-urile, cursorul, împrumutul rândurilor doar la
  o versiune nouă, starea păstrată (citită și scrisă pe worker, scrisă întreagă). Consola și
  explorerul o folosesc; fiecare își scrie doar cum împrumută și cum aduce rândurile.
- **Rândurile, într-un singur loc** (`database::lent::Lent<R>`): consola și explorerul le
  împrumută la fel, generic peste bucățile lor. Coloanele grilei devin coloane de grafic
  prin `GridColumn::chart_column`.
- **Explorerul ca gazdă** (`pane_rows`): rândurile tabelei, cu filtrul și ordinea arătate,
  citite pe worker în pagini de 2 000. La început prima pagină, apoi toate până la limita
  cerută. Se citesc din nou când se schimbă filtrul sau ordinea, se lasă la închidere.
- **Limita** stă în modulul de grafice (`charts.fetch_rows`, pagina lui de setări) și
  vine în cerere (`AllRows { limit }`); gazdele doar o respectă.
- **Optimizare:** cât se aduc rândurile, panoul doar își actualizează numărătoarea și
  pregătește graficul o singură dată, la final (înainte: la fiecare pagină, de sute de ori
  pentru 200 000 de rânduri). Gazda consolei împrumută vederea rezultatului fără s-o
  cloneze la fiecare poll.
- **94h, grafice din orice date** (`crates/domain/viewers/data-tables`): recunoașterea
  formatului (extensie, apoi conținut) și toate tabelele dintr-un JSON, JSON Lines, YAML, XML,
  HTML, CSV sau TSV, cu drumul fiecăruia și tipul coloanelor dedus (și numere cu virgulă
  zecimală). Limite pe adâncime, tabele, rânduri și coloane; 9 teste, cu documente ostile.
  - **„Deschide ca grafic”** (meniul editorului și al tabului, paleta): pe un fișier de
    date sau pe o selecție, un tab cu graficul. Textul vine așa cum e în editor, nimic citit
    de pe disc. Când documentul are mai multe tabele, se alege din dropdown.
  - **Prima propunere:** un grafic nou pornește cu prima coloană de text sau de timp pe X
    și primul număr ca valoare (o linie pe timp, bare altfel), la toate gazdele.
- **Exportul:** butonul din bara graficului cere „Salvează ca” gazdei (`PaneRequest::SaveAs`,
  drumul primit înapoi ca `picked-save:`). SVG-ul îl scrie modelul (`charts_model::svg`, din
  aceeași geometrie, cu culorile temei), PNG-ul e rasterizat din el (`image_decode`), totul
  pe worker și scris întreg; rezultatul e anunțat (`PaneRequest::Tell`).
- `check.sh`: 4 ok.

## Făcut (2026-11-23): 94e, Task Manager și familia graficelor

- **Detaliile din Task Manager** desenează cu `charts::Chart`: valorile pe axă în unitatea
  lor (`set_format`), scala fixă de până acum (`set_y_range`), tooltip-ul cu toate seriile la
  momentul de sub mouse („acum 12 s · CPU: 3,4%”), legenda la rețea și disc, linii netede.
  Datele (`Prepared::over_time`) se fac doar pentru o istorie nouă; testul „o redesenare
  identică nu alocă nimic” trece.
- **Fără alocări pe cadru în grafic:** layout-ul e recunoscut după o amprentă (nu o
  structură clonată), etichetele axelor sunt formatate și tăiate o singură dată, la layout,
  iar culorile aspectului stau într-un tablou fix.
- **O singură familie:** `ui_components::chart` a devenit `charts::spark` (graficele mici,
  fără stare, din dreptunghiuri: celule, carduri, widget-urile modului de relaxare, bara de
  stare). Vechiul `time_chart` a dispărut, `Chart` îl înlocuiește.
- **Lăsat:**
  - **Profilerul:** benzile lui pe aceeași axă de timp, cu un tooltip comun, cer o opțiune
    nouă a componentei („benzi”).
  - **Popover-ul barei de stare:** citirea la hover.
  - **Mermaid:** aceeași geometrie și hover.

## Făcut (2026-11-23): profilerul, bara de stare, rotița

- **Benzi** (`View::lanes`, `Chart::set_lanes`): fiecare serie în banda ei, una sub alta,
  fiecare pe scala ei, pe aceeași axă X, cu numele ei în stânga și un tooltip comun la
  momentul de sub mouse. Axa X se poate scrie în felul ei (`set_x_format`: durate `1:20`).
- **Profilerul:** timeline-ul (CPU, cadre, rețea, disc) desenează cu `Chart` în benzi, în
  trepte, fiecare valoare în unitatea ei. Datele se fac o dată, când înregistrarea e citită;
  graficele mici din linia timeline-ului nu se mai fac deloc. Se redesenează doar când se
  schimbă coloana de sub mouse (înainte: la fiecare mișcare). Rotița derulează pagina, ca
  peste tot pe ea (zoom-ul e oprit acolo).
- **Popover-ul barei de stare:** sub mouse, o linie de ghidaj și tooltip-ul „acum 12 s: 3,4%”
  (`Plot::reading`, formatorul rândului ca pointer de funcție: nicio alocare). Se
  redesenează doar la o coloană nouă.
- **Rotița peste panoul unui grafic** (și peste lista lui deschisă) nu mai ajunge la tabelul
  de dedesubt (`PaneHost::scroll`), cu un test.
- **„Adaugă o serie nouă”** în josul panoului de serii, ca un link (butonul comun, accent).
- **Lăsat:** Mermaid pe aceeași geometrie, cu hover.

## Făcut (2026-11-23): Mermaid

- Diviziunile axelor din `xychart` vin din modelul comun (`charts_model::scale::ticks_within`,
  același algoritm: în intervalul dat, la 1, 2, 2,5, 5 × 10ⁿ). Graficele din documente arată
  ca înainte; cele 96 de teste ale markdown-ului trec.
- **Lăsat cu motiv: hover în Mermaid.** O diagramă dintr-un document e o imagine statică,
  rasterizată pe worker și exportată identic (SVG, PNG). Hover-ul ar face din fiecare
  diagramă o componentă vie, cu stare și desen la fiecare cadru, pentru ceva care se
  citește, nu se explorează. Cine vrea să exploreze datele le deschide ca grafic.

## Făcut (2026-11-23): mișcarea și tooltip-urile graficelor

- **`ChartMotion`** în aspectul graficului, opțiune a componentei:
  - `enter` (`None`, `Fade`, `Grow`): barele cresc din bază, punctele din nimic, liniile,
    ariile și feliile apar treptat;
  - `update` (`None`, `Fade`, `Morph`): barele și punctele trec la noile valori, imaginea
    veche a liniilor se topește în cea nouă;
  - `hover`: elementul de sub mouse se aprinde lin (120 ms), linia de ghidaj la fel, felia
    iese treptat;
  - `millis` (implicit 280); `ChartMotion::none()`. Setarea aplicației „mai puțină
    mișcare” le oprește pe toate.
- **Ieftin:** nimic nu se rasterizează din nou pentru animație (imaginile existente se
  desenează cu opacitate, `Layer::image_faded`). Cadre noi se cer doar cât se mișcă ceva
  (`Chart::animating`, prin `PaneHost::animating` și `is_animating` al fiecărei gazde).
  Ce se ține pentru tranziție se lasă imediat după ea.
- **Task Manager:** istoria live alunecă în fiecare secundă, deci are `update: None`.
  Rămâne doar apariția și hover-ul lin.
- **Tooltip-uri:**
  - pe legendă: numele seriei și „Clic: o ascunde / o arată”;
  - pe o etichetă de pe axa X tăiată cu „…”: textul întreg.
- Teste pentru creștere, trecerea la valori noi, fără mișcare și tooltip-ul legendei.
