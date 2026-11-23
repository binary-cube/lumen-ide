# 93 — Desenarea, textul și accesibilitatea: mai bine și mai ieftin, cu codul nostru

**Cerut 2026-11-22**, pe ramura `render-text-accessibility`. Pornit de la o discuție despre
un framework de UI nativ în Rust, desenat pe GPU, care are aceeași arhitectură ca noi
(scena descrisă de UI, desenată de un driver al nostru). Nu-l aducem în proiect. Ne
inspirăm din trei idei ale lui și le facem în codul nostru, pe nevoile noastre:

1. **Accesibilitatea**: aplicația citită de cititoarele de ecran ale sistemului.
2. **Mai puține draw call-uri**: în ideal, un singur draw call pentru tot ce nu e blur.
3. **Shaping-ul textului**: stratul de text pe piesele de bază, fără un intermediar care
   face mai mult decât ne trebuie.

**Regula de aur rămâne:** ce vede utilizatorul nu se schimbă. Același design, aceleași
culori, același text, pixel cu pixel unde se poate (verificat cu snapshot-uri înainte și
după). Câștigul e dedesubt: CPU, GPU, memorie, cadre.

**Scalabil, nimic scris de două ori** (cerut 2026-11-22, la fel ca la controale):
- **un singur loc pentru fiecare lucru:**
  - un singur drawer, folosit și de ferestre, și de măsurători, și de teste;
  - un singur tip de instanță pe GPU;
  - un singur `shape_line`, deasupra căruia stau toate paragrafele;
- **accesibilitatea vine din componente:** fiecare componentă comună își descrie singură
  nodul (o metodă implicită pe contract), iar un view compus primește arborele din părțile
  lui. Niciun view nu scrie de mână rolul unui buton;
- **ce lipsește se adaugă în componentă**, ca opțiune, pentru toți;
- **o singură unealtă de comparare:** `scripts/ui-snapshots.sh`, aceeași pentru pixelii
  CPU, pixelii GPU și cifre.

**Ordinea:** întâi măsurăm (pasul 93a). Fiecare parte pornește doar dacă cifrele arată că
merită, și cifrele se scriu aici, înainte și după.

## Unde suntem azi (citit din cod, 2026-11-22)

### Desenarea (`crates/render/render-wgpu`)

- **Scena** (`render-api::Scene`) e o listă de straturi (`Layer`). Fiecare strat are un
  clip și patru liste: blur-uri de fundal, dreptunghiuri (`Quad`), imagini, glife. Ordinea
  de desenare e: strat cu strat, iar în strat blur → dreptunghiuri → imagini → glife.
- **Draw call-uri** (`drawer.rs`, `draw_batches`): pentru fiecare strat și fiecare zonă
  redesenată (damage tracking), până la patru pipeline-uri schimbate:
  - un draw pentru blur;
  - un draw pentru dreptunghiuri;
  - **un draw pentru fiecare imagine**, fiecare cu bind group-ul ei (textura ei);
  - un draw pentru glife (atlasul).

  Plus fundalul (`fill`) și colțurile ferestrei (`corner`) pe fiecare zonă. Numărul crește
  cu straturi × zone × tipuri. `FrameStats::draw_calls` le numără deja.
- **Desenarea parțială** (`render-api/src/damage.rs`) redesenează doar zonele schimbate.
  Rămâne. Orice schimbare de mai jos trebuie să păstreze zonele și scissor-ul lor.
- **Rendererul software** (`render-software`, tiny-skia) e rezerva fără GPU. Trebuie să
  deseneze la fel și după schimbări (contractul `Scene` nu se schimbă pentru el).

### Textul (`crates/render/text`)

- Contractul e al nostru: `TextSystem`, `Font`, `ShapedLine`, `ShapedGlyph`, plus
  `GlyphRasterizer` din `render-api`. Restul aplicației (350 de fișiere care folosesc
  `TextSystem`) nu vede ce e dedesubt.
- Dedesubt e `cosmic-text` 0.19 (ADR-0003: „începem cu el, îl înlocuim dacă apar
  limitări, fără să afectăm restul”). Pentru fiecare linie modelată: un `Buffer` de lucru,
  `set_text` + `shape_until_scroll`, adică layout de paragraf (wrap, linii) pentru o
  singură linie. Rezultatul e ținut într-un cache pe generații (`LINE_CACHE_LIMIT` 2048).
- Rasterizarea: `SwashCache` din cosmic-text, apoi atlasul nostru (`etagere`) pe GPU.
- În `Cargo.lock` avem câte **două versiuni** de `harfrust`, `skrifa` și `fontdb` (una
  adusă de cosmic-text, una de altceva). Asta înseamnă cod și memorie în plus.

### Accesibilitatea

- **Nu există.** Singurul lucru apropiat: „mai puțină mișcare” citit din sistem
  (`platform-api`). Cititoarele de ecran (VoiceOver, Narrator, Orca) văd o fereastră goală.
- Avem în schimb exact ce trebuie pentru a construi arborele: contractul `Control`
  (`ui_components::control`) cu focus, Tab, hover, apăsare și text selectat, plus `Viewer`,
  `OverlaySlot`, meniurile aplicației și componentele comune (`ds::button`, `SearchField`,
  `Checkbox`, `Picker`, `DataGrid`…). O componentă care știe ce e poate spune și
  cititorului de ecran ce e.

## 93a — Măsurăm întâi (fără nicio schimbare de comportament)

- **Ce adăugăm la sonde** (`kernel::probe`, planul 91), toate cu nume statice:
  - pe cadru: draw call-uri, schimbări de pipeline, schimbări de bind group, straturi,
    zone redesenate, instanțe de fiecare tip, octeți urcați pe GPU;
  - în text: linii modelate (din cache / noi), timpul de modelare, glife rasterizate,
    mărimea atlasului.
- **Scenariile de măsurat**, fiecare cu `EDITOR_NEW_INSTANCE=1` și un `HOME` izolat:
  1. editorul cu un fișier PHP mare, derulat;
  2. fereastra Settings deschisă, căutare tastată;
  3. Task Manager deschis (rânduri care se schimbă în fiecare secundă);
  4. terminalul cu output continuu;
  5. o diagramă mare (multe imagini și forme);
  6. aplicația stând degeaba (să fie aproape zero).
- **Unde se scriu:** un tabel aici, cu macOS (Apple silicon), Windows și Linux (colegul cu
  Ubuntu), pentru fiecare scenariu: draw call-uri pe cadru, timp CPU pe cadru, timp GPU
  (unde se poate citi), memorie GPU.
- **Când trecem mai departe:** 93b doar dacă draw call-urile sau schimbările de stare apar
  ca un cost real (de exemplu sute pe cadru, sau un timp CPU de encodare care contează).
  93c doar dacă modelarea textului apare în profil sau dacă dublurile de biblioteci cântăresc.

## 93b — Mai puține draw call-uri (desenarea „într-o singură trecere”)

**Ideea:** un singur tip de instanță pentru tot ce e dreptunghi, imagine sau glifă. Un
shader citește tipul și știe ce să deseneze. Ordinea scenei devine ordinea instanțelor,
deci straturile nu mai cer draw-uri separate.

Pași, fiecare măsurat și verificat separat:

1. **Imaginile într-un atlas** (sau o `texture_array` pe mărimi), nu fiecare cu textura și
   bind group-ul ei. Azi fiecare imagine e un draw call. E primul câștig, ușor de izolat.
   - Imaginile mari (o captură, un PDF) rămân pe texturi proprii: atlasul e pentru cele
     mici (iconițe raster, avatare, miniaturi).
   - Regulile de memorie rămân: `gpu_only`, `released`, eliberarea după neutilizare.
2. **O instanță unică** pentru dreptunghi, imagine și glifă: poziție, mărime, culoare,
   colțuri, chenar, coordonate în atlas, tipul. Un singur buffer, un singur pipeline,
   shaderul ramifică după tip. Dreptunghiurile au deja SDF pentru colțuri: rămâne.
3. **Clip-ul în instanță**, nu în scissor: fiecare instanță își poartă dreptunghiul de
   clip (și colțurile lui, unde e cazul), iar fragment shader-ul taie. Așa straturi
   diferite intră în același draw. Scissor-ul rămâne doar pentru zonele de damage.
4. **Ce rămâne separat:** blur-ul de fundal (are nevoie de ce e desenat dedesubt, deci
   împarte trecerea), fundalul și colțurile ferestrei. Rezultatul așteptat: un draw pentru
   tot conținutul, plus câte unul pentru fiecare blur, pe zonă.

**Ce nu se schimbă:**
- contractul `Scene` / `Layer` (UI-ul nu află nimic);
- desenarea parțială;
- rendererul software;
- culorile și anti-aliasing-ul (snapshot-uri identice).

**De verificat:**
- **Teste:** numărul de draw call-uri pe o scenă dată (în `render-wgpu`, cu `FrameStats`).
- **Snapshot-uri înainte și după:** editor, Settings, Task Manager, o diagramă, un modal
  cu blur.
- **Windows (DX12 / Vulkan) și Linux:** ramificarea în shader și `texture_array` merg peste
  tot.
- **Problema lăsată deschisă:** ultima literă dublată după un paste lung (suspectăm desenarea
  parțială). O rezolvăm aici, cu un test, dacă apare în zona atinsă.

## 93c — Textul: piesele de bază, codul nostru

**Ideea:** `TextSystem` păstrează exact același contract, iar dedesubt lucrăm direct cu
piesele pe care cosmic-text le folosește deja, fără stratul lui de paragraf:

- **shaping:** `harfrust` (portarea HarfBuzz în Rust, pe care o folosim deja indirect);
- **fonturi și contururi:** `skrifa` / `read-fonts`;
- **rasterizare:** `swash` (ca acum) sau `skrifa` + rasterizatorul nostru; se alege după
  măsurători;
- **găsirea fonturilor și fallback-ul:** `fontdb` (ca acum), cu lanțul nostru de fallback.

O singură versiune din fiecare în `Cargo.lock` (azi sunt câte două).

Pași:

1. **Modelarea unei linii fără `Buffer`:** o linie e un rând de text. Se fac doar
   segmentarea pe scripturi și fonturi (fallback), shaping pe fiecare segment și
   avansurile. Fără wrap, fără layout de paragraf. Cache-ul pe linii rămâne (al nostru).
2. **Fallback-ul nostru:** ordinea fonturilor pe script (latin, chirilic, CJK, arabă,
   emoji), ținută o dată per font cerut, nu căutată la fiecare linie.
3. **Bidi** (arabă, ebraică) cu `unicode-bidi`, doar unde textul le conține (testul e
   ieftin: un scan de octeți).
4. **Rasterizarea** prin `GlyphRasterizer` (contractul există): aceeași acoperire, același
   gamma, aceleași poziții sub-pixel, deci snapshot-uri identice.
5. **Paragrafele** (wrap pe mai multe linii: `ds::wrap`, `read_text`, `multiline`) rămân
   în codul nostru, deasupra lui `shape_line`, ca acum.

**Parley** (motorul de layout de text al framework-ului de la care pornim) îl citim ca
inspirație pentru segmentare, fallback și bidi. Nu-l aducem ca dependență decât dacă
măsurătorile arată că face ceva mult mai bine decât am face noi cu aceleași piese.

**De verificat:**
- **Teste pe contract** (există, plus noi):
  - lățimi, caret (`x_at`, `byte_at`), ligaturi (`->`, `=>` în fontul de cod);
  - emoji compuse, CJK, arabă (dreapta-stânga), diacritice românești;
  - fonturi lipsă (fallback).
- **Comparație în test:** pe un set de texte, vechiul și noul dau aceleași glife,
  aceleași avansuri (la 0,01 px).
- **Cifre:** timpul de modelare pe linie, memoria `TextSystem`, mărimea binarului (fără
  cosmic-text și dublurile lui).
- **ADR-0003 actualizat:** decizia luată, cu cifrele.

## 93d — Accesibilitatea

**Ideea:** arborele de accesibilitate îl construim noi, din contractele pe care le avem.
Pentru legătura cu sistemul de operare folosim **AccessKit**: e o bibliotecă separată,
standardul în Rust, care vorbește cu UI Automation (Windows), NSAccessibility (macOS) și
AT-SPI (Linux). A scrie noi cele trei punți ar însemna luni de cod specific fiecărui
sistem, fără niciun câștig vizibil. Ce construim e al nostru: nodurile, rolurile, numele,
focusul, acțiunile.

**Cum se construiește arborele: odată cu desenarea, din componente.** UI-ul nostru se
desenează pe loc: `ds::button`, `SearchField`, `Checkbox` și celelalte nu sunt obiecte
care să existe între cadre, ci funcții care pun dreptunghiuri și glife în `Scene` la
fiecare cadru. Arborele de accesibilitate se face exact la fel, în același loc:

- **`AccessFrame`**, pereche cu `Scene`: o listă de noduri pusă de componente cât
  desenează, fiecare cu rolul (buton, câmp, comutator, listă, rând, tab, meniu, dialog,
  text), numele, valoarea, starea (bifat, apăsat, dezactivat, extins, selectat),
  dreptunghiul și părintele (grupurile: un panou, un modal, o listă).
- **Componenta comună își pune singură nodul**, o singură dată, în codul ei:
  - `ds::button` pune un buton, numit după etichetă sau, dacă n-are, după tooltip;
  - `SearchField` și `TextInput` pun un câmp, cu valoarea și selecția;
  - `Checkbox`, comutatorul și `Radio` pun starea lor;
  - segmentele, dropdown-ul și `Picker` pun o listă cu alegerea;
  - `DataGrid`, `table` și `TreeList` pun un tabel sau un arbore, doar cu rândurile
    vizibile;
  - tab-urile, meniurile aplicației, modalele (un dialog cu titlul lui), toast-urile
    (anunțate) și bara de stare.

  Niciun view nu scrie de mână rolul unui buton. Un view care folosește componentele
  devine accesibil fără să se schimbe.
- **Textul de pe ecran și numele controalelor fără etichetă proprie:**
  - `ShapedLine` ține textul ei, ca `Arc<str>` comun cu cheia din cache (16 octeți pe
    linie, nicio copie);
  - `draw_text` pune un nod „text” cât se face arborele, deci cititorul de ecran citește
    tot ce scrie pe ecran;
  - un comutator, un checkbox sau un buton doar cu iconiță primește numele textului de pe
    rândul lui (ca „Auto-save” lângă comutatorul lui);
  - nicio pagină nu se schimbă.
- **Zonele cu tooltip ale unui view** (tabelele lui de „hit”: dreptunghi → acțiune) devin
  noduri numite după tooltip. Ce are deja un nume pentru mouse îl are și pentru cititorul
  de ecran, fără cod nou.
- **Gol cât nu e cerut:** `AccessFrame` e `None` dacă nu rulează niciun cititor de ecran
  (sistemul spune asta). Componentele verifică un `Option`, deci nicio alocare și niciun
  cost în plus.
- **Numai când s-a schimbat:** arborele unui cadru se compară cu cel de dinainte (ca
  desenarea parțială), iar sistemului i se trimit doar nodurile schimbate. Identitatea
  unui nod vine din locul lui (view, componentă, cheie), nu din poziția pe ecran.

Pași:

1. **Contractul** în `render-api` (sau un crate mic `access-api`, pus în
   `architecture.toml`): `AccessFrame`, `AccessNode`, `Role`, `State`. Nu depinde de
   AccessKit: e al nostru, ca `Scene`.
2. **Puntea cu sistemul** în `platform-winit`: adaptorul AccessKit pentru winit, în
   spatele unui contract din `platform-api`. Primește arborele și trimite înapoi acțiunile
   (focus, apăsare, setare de valoare, derulare).
3. **Acțiunile trec pe aceleași căi ca mouse-ul și tastatura:**
   - o apăsare venită de la cititor devine un click în centrul nodului;
   - un focus devine focusul din `Controls`;
   - o valoare setată devine textul scris în câmp.

   Comportamentul nu se scrie de două ori.
4. **Componentele comune, una câte una**, fiecare cu testul ei. Ordinea, după cât de des
   apar: butoane, câmpuri, comutatoare și checkbox-uri, liste și picker, tab-uri, meniuri,
   modale, tabele și arbori, bara de stare, toast-uri.
5. **Editorul de cod:** un câmp de text pe mai multe linii, cu linia curentă, caret-ul și
   selecția. Nu tot fișierul o dată, ci o fereastră în jurul caret-ului, ca alte editoare
   mari.
6. **Nimic generic nu numește un modul:** fiecare modul primește accesibilitatea prin
   componentele pe care le folosește. Ce are în plus (un desen al lui, cum e o diagramă)
   descrie prin aceeași listă.

**De verificat:**
- **Teste:** arborele unui view dat (rolurile, numele, focusul), plus o acțiune de apăsare
  venită din arbore ajunge la buton.
- **Snapshot:** `EDITOR_SNAPSHOT_ACCESS=1` scrie arborele unui ecran ca text, iar
  `scripts/ui-snapshots.sh` îl compară, ca pe pixeli. Așa un ecran care își pierde numele
  sau rolurile se vede la compare.
- **Manual, de utilizator:** VoiceOver pe macOS, Narrator pe Windows, Orca pe Linux. Le
  verifică utilizatorul sau colegul, nu le pornesc eu.
- **Cost:** zero cât nu e activ niciun cititor de ecran (măsurat în 93a, scenariul 6).
- **Privacy:** arborele rămâne pe calculator, la sistemul de operare, ca orice aplicație.
  Nu se scrie nicăieri și nu ajunge în jurnal sau în profiluri.

## Ordinea și ramura

- **Ramura `render-text-accessibility`.** Fiecare pas e un commit separat, cu
  `./scripts/check.sh` la 4 ok. Se unește în `main` doar când utilizatorul spune.
- **Ordinea:**
  1. 93a, măsurarea;
  2. 93b1, imaginile în atlas;
  3. 93b2–b4, instanța unică;
  4. 93c, textul;
  5. 93d, accesibilitatea.

  93d e independent de 93b și 93c. Poate merge în paralel sau primul, dacă utilizatorul
  vrea asta înainte.
- **După fiecare bucată mare:** review făcut de un agent separat, apoi constatările
  verificate și reparate.

## Lăsat de la început (și de ce)

- **Backend Metal direct** (fără wgpu): ar câștiga puțin pe macOS și ar dubla codul de
  desenare. Mai întâi vedem ce dau 93a și 93b.
- **Un layout generic de tip flexbox:** view-urile noastre își calculează singure
  dreptunghiurile și merg bine. Ar fi o rescriere a tuturor view-urilor, fără câștig vizibil.
- **Rasterizatorul de fonturi propriu:** doar dacă `swash` sau `skrifa` arată o problemă
  reală (calitate, viteză) la măsurători.

## Făcut (2026-11-22): 93a, sondele

- **Ce raportează un cadru desenat** (`record_frame` în `crates/app/src/app/mod.rs`, sub
  `render`):
  - numărate: draw call-uri, dreptunghiuri, glife, straturi, pixeli redesenați, blur-uri
    refăcute;
  - niveluri: draw call-uri pe cadru (cel mai mare și ultimul), glife în atlas.

  Datele vin din `FrameStats`, pe care rendererul îl dădea deja, deci nimic nou în driver.
- **Ce raportează textul** (`text-layout`, sub `text`): linii din cache și linii modelate
  din nou, timpul de modelare (`shape`), glife rasterizate și timpul lor (`rasterize`).
- **Cât costă cât nu înregistrează nimic:** o citire atomică pe apel. Test:
  `a_frames_cost_is_recorded_only_while_recording`.
- **Cum se măsoară în aplicație:** Help → „Record a Profile…”, câte un profil pentru
  fiecare scenariu, pe sistemele utilizatorilor (macOS, Windows, Ubuntu).
- **Cum se compară înainte / după, reproductibil, fără fereastră:**
  - **`render_wgpu::Offscreen`:** desenează pe GPU fără fereastră, cu același drawer
    (pipeline-uri, atlas, desenare parțială). Dă cifrele unui cadru și pixelii citiți
    înapoi. Testele GPU existente folosesc aceeași funcție `headless`.
  - **`EDITOR_SNAPSHOT_GPU=<cadre>`:** ecranul unui snapshot pictat și desenat de atâtea
    ori. Tipărește straturile, dreptunghiurile, glifele, draw call-urile, timpul de pictare
    și de encodare (medie, mediană, maxim) și liniile modelate sau luate din cache.
  - **`EDITOR_SNAPSHOT_GPU_SAVE` / `_REFERENCE` / `_TOLERANCE`:** poza GPU păstrată sau
    comparată pixel cu pixel.
  - **`SNAPSHOT_GPU=10 scripts/ui-snapshots.sh record|compare <dir>`:** toate ecranele, în
    ambele teme. Compară pixelii CPU, pixelii GPU și cifrele (înainte / după, unul sub
    altul). Cifrele de referință rămân în `<dir>/gpu.txt`.

| Scenariu | Sistem | Draw call-uri / cadru | Timp CPU / cadru | Linii modelate / s | Glife rasterizate |
|---|---|---|---|---|---|
| 1. editor, fișier PHP mare, derulat | | | | | |
| 2. Settings, căutare tastată | | | | | |
| 3. Task Manager deschis | | | | | |
| 4. terminal cu output continuu | | | | | |
| 5. diagramă mare | | | | | |
| 6. aplicația stând degeaba | | | | | |

## Făcut (2026-11-22): 93b, o singură trecere

- **O singură instanță pentru dreptunghiuri și glife** (`PrimGpu`, 76 de octeți;
  `vs_prim` / `fs_prim`):
  - poartă clip-ul stratului ei, pe `u16`, tăiat direct în geometrie: aceiași pixeli ca
    un scissor;
  - poartă și marcajul de estompare sub modal (un bit), deci straturile nu mai cer
    globals diferite;
  - o singură formulă SDF în shader, folosită și de fundal.
- **Batch-urile, zonă cu zonă, în ordinea scenei** (`Drawer::prepare` + `batch_area`):
  - straturile consecutive formează o rulare (un draw);
  - se întrerupe doar la imaginile unui strat, la blur-urile lui și la stratul unui blur
    refăcut, unde se termină o trecere de desenare;
  - fiecare zonă își are lista ei (`area_batches`), iar pipeline-ul și legăturile se pun
    o singură dată, nu la fiecare rulare.
- **Aceeași imagine de mai multe ori la rând** (particulele decorațiunilor) se desenează
  într-un singur draw (`same_image_runs`).
- **Atlasul plin:** o glifă fără loc se sare, iar restul cadrului se desenează (test
  `a_glyph_without_room_leaves_the_rest_drawn`).
- **Teste:**
  - `clipped_layers_are_drawn_in_one_draw_in_their_order`: nouă straturi cu clip-uri
    diferite, un draw, marginile exacte;
  - `the_same_image_in_a_row_is_one_run`;
  - toate testele GPU de dinainte trec (desenarea parțială egală cu cea completă,
    estomparea ca în referință, blur-urile, imaginile).
- **Review făcut de un agent separat**, 12 constatări, toate verificate:
  - **reparate:**
    - atlasul plin;
    - cifrele `Offscreen` aceleași ca ale ferestrei;
    - rulările se întrerup doar la un blur refăcut;
    - lista de batch-uri pe zonă;
    - legăturile puse o dată;
    - o singură formulă SDF;
    - instanța de la 84 la 76 de octeți;
    - lungimea fișierului comparat;
    - poza GPU luată din scena snapshot-ului (reproductibilă și pe ecranele animate);
    - `Offscreen` doar cu `feature = "offscreen"`;
  - **lăsate:**
    - culorile pe 8 biți: ar schimba pixelii;
    - contorul de linii din cache: o citire atomică pe apel cât nu înregistrează nimic.

### Cifrele 93b (2026-11-23, toate ecranele `scripts/ui-snapshots.sh`, ambele teme)

**Cum s-a măsurat:** referința e făcută cu vechiul renderer (un worktree cu `target`
propriu), cu aceeași unealtă. Comparația:
`SNAPSHOT_GPU=5 scripts/ui-snapshots.sh compare`, build de debug, macOS, Apple silicon.

| | Înainte | După |
|---|---|---|
| Draw call-uri pe cadru, media celor 297 de ecrane (tema închisă) | 47,1 | **5,7** |
| Cel mai mare număr de draw call-uri pe un ecran | 107 | 61 (Theme Studio, multe imagini diferite) |
| Encodarea CPU a unui cadru, mediana medie | 0,61 ms | 0,58 ms |

- **Pixelii:** din 594 de ecrane, 580 au pixelii GPU identici (100) sau cu cel mult 1/255
  pe margini (480).
- **Celelalte 14** sunt exact ecranele la care diferă și poza CPU, unde rendererul software
  e neatins:
  - `settings-5`: iese diferit chiar și de la o rulare la alta cu același binar, verificat
    și cu binarul vechi;
  - `rename`: iese identic când e rulat separat;
  - „running”, „running-wait”, notificările: au animații, numărători și inele după ceas;
    nu au fost rulate separat.
- **Pe GPU** câștigul vine din mai puține schimbări de stare (pipeline, legături, scissor),
  pe care un build de debug pe CPU nu le arată. Cifrele de GPU și de release vin din
  profilurile din aplicație (93a).

## Făcut (2026-11-23): 93d, accesibilitatea (prima parte)

- **Contractul** (`platform_api::access`):
  - `Frame` cu noduri sub grupuri: rol, nume, valoare, stare (bifat, parțial, apăsat,
    deschis, ales, focus), acțiuni, dreptunghi;
  - identități stabile, după părinte, rol, nume și ordinea între frați, nu după loc, deci
    o listă derulată își păstrează nodurile;
  - `changes`: doar ce s-a schimbat;
  - `Request`: apăsare, focus, valoare.
- **Înregistratorul** (`ui_kit::access`): pe firul UI, oprit cât nu e cerut (o verificare
  de `Cell<bool>`, nodul nici măcar descris).
  - **Textul de pe ecran:** `ShapedLine` ține textul ei, ca `Arc<str>` comun cu cheia din
    cache, iar `draw_text` pune un nod de text. Semnele fără litere (✓, săgeți) nu se pun.
  - **Fără dubluri:** un text desenat într-un control cu nume (eticheta unui buton, un rând
    de meniu, o notificare) nu se mai citește încă o dată.
  - **Nume automate:** un control fără nume (comutator, checkbox, dropdown, rândul unui
    arbore) ia textul de pe rândul lui, cel mai apropiat.
- **Componentele care se descriu singure:**
  - butonul (`ds::button`): numele din etichetă sau iconiță, starea activ / dezactivat /
    deschide un meniu;
  - câmpurile (`TextInput`, deci și `SearchField`): valoarea, iar la parolă doar puncte
    (un test verifică că textul nu ajunge în arbore);
  - comutatorul, checkbox-ul și radio-ul;
  - segmentele (radio-uri), dropdown-ul (combo, valoarea, deschis), rândurile cu ✓;
  - rândurile picker-ului;
  - tab-urile (pastilă și subliniate);
  - meniurile aplicației (rând, bifat, submeniu deschis);
  - modalele (dialog cu titlul lor);
  - notificările (alertă cu titlul și detaliul lor);
  - rândurile de arbore care se pliază (`tree::chevron`).
- **Legătura cu sistemul** (`platform-winit/src/access_bridge.rs`, AccessKit 0.34: UI
  Automation, NSAccessibility, AT-SPI):
  - fereastra se creează invizibilă, i se atașează adaptorul, apoi se arată;
  - cererea arborelui pornește înregistrarea, plecarea cititorului de ecran o oprește;
  - arborele se trimite întreg, doar când un cadru l-a schimbat (AccessKit îl compară și
    anunță sistemul doar de diferențe);
  - cererile cititorului ajung la aplicație ca `PlatformEvent::Access`: o apăsare sau un
    focus devin un click în mijlocul nodului, iar o valoare înseamnă tot textul ales și
    înlocuit.
- **Snapshot:** `EDITOR_SNAPSHOT_ACCESS=1` scrie arborele ecranului ca text lângă poză
  (`<out>.access.txt`).

### Lăsat, pentru partea a doua

- Tabelele (`DataGrid`, `table`) ca tabel cu rânduri și celule; editorul de cod ca câmp de
  text pe mai multe linii (o fereastră în jurul caret-ului).
- Arborele trimis parțial (doar nodurile schimbate și părinții lor), dacă măsurătorile
  arată că trimiterea întreagă costă.
- `scripts/ui-snapshots.sh` să compare și arborii ca text, ca pe pixeli.
- **Verificarea manuală, de utilizator:** VoiceOver pe macOS, Narrator / NVDA pe Windows,
  Orca pe Linux.

### Făcut, după review (2026-11-23)

Un review făcut de un agent separat, cu 16 constatări de corectitudine, plus performanță și
duplicare; toate verificate.

- **Reparate:**
  - **Cererile cititorului de ecran:**
    - un „focus” nu mai apasă nimic: dă focus doar unui câmp de text;
    - o valoare se scrie doar într-un câmp care are tastatura, după ce i se alege textul
      (calea lui ⌘A);
    - click-ul trece pe calea reală a mouse-ului (mutare, apăsare, eliberare) și nu
      intră în numărătoarea unui dublu-click.
  - **Înregistrarea:**
    - e o gardă (`Recording`): o ieșire timpurie din cadru sau o panică o opresc;
    - un cadru care desenează doar decorațiunile nu face arbore, deci rămâne cel trimis
      ultima dată.
  - **Arborele trimis sistemului:** din nou întreg de fiecare dată când sistemul îl cere
    din nou (`access_asked`: un număr al cererii) sau când se schimbă titlul ferestrei.
  - **Identitățile:**
    - un control fără nume ia identitatea după numele găsit pe rândul lui, nu după
      ordinea între cele fără nume, iar copiii lui o urmează;
    - rândurile picker-ului și ale tabelelor au o cheie (indexul).
  - **Ce se citește:**
    - un text dintr-un control cu nume se ignoră doar dacă e numele sau valoarea lui;
      celelalte (detaliul unui rând) rămân, sub el;
    - controlul ultim e ținut pe părinte și se uită la deschiderea sau închiderea unui
      grup;
    - numele de pe rând se caută doar printre textele aceluiași părinte, sortate pe
      înălțime, la cel mult 12 înălțimi de control (nu mai e O(n²));
    - textele au înălțimea fontului (`ShapedLine.size`), nu zero.
  - **Ce nu ajunge în arbore:**
    - un buton doar cu iconiță nu mai e numit după id-ul intern al iconiței;
    - un câmp secret nu dă nimic din text, nici lungimea, nici afișat;
    - inițialele avatarului (`access::silent`).
  - **Editorul de cod:** codul și numerele de linie nu mai sunt citite ca texte risipite
    (`access::silent`), până la partea a doua.
  - **Fereastra:** se arată după ce bara, caption-ul și ce a restaurat aplicația sunt puse.
  - **Fără alocări pe nod:** numele sunt `Arc<str>`, textele împart linia din cache;
    compararea cu ultimul arbore e o egalitate de liste; `Frame::children()` e locul unic
    al grupării pe părinte (puntea și snapshot-ul îl folosesc).
  - **Regula pentru componente noi:** în `.claude/rules/ui-components.md`.
- **Lăsate (partea a doua):**
  - rândurile de arbore descrise de rândul întreg (azi: chevron-ul celor care se pliază);
    rândurile sub `List` / `Table` / `Menu` („element n din m”);
  - pe Windows, orice client UI Automation (nu doar un cititor de ecran) pornește
    înregistrarea și nu o mai oprește: de măsurat, eventual o limită de cost;
  - arborele trimis la cel mult ~10 cadre pe secundă în animații, dacă măsurătorile o cer.
