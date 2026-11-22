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

Pași:

1. **Puntea cu sistemul** în `platform-winit` (adaptorul AccessKit pentru winit), în
   spatele unui contract din `platform-api`: platforma primește un arbore și trimite
   înapoi acțiuni (focus, apăsare, setare de valoare). Arborele se cere **doar când un
   cititor de ecran e activ** (sistemul spune asta). Altfel nu costă nimic.
2. **Contractul:** o metodă implicită pe `Control` (și pe `Viewer`, `OverlaySlot`) care își
   descrie nodul: rol, nume, valoare, stare (bifat, dezactivat, extins), dreptunghi, copii.
   Implicitul e „nimic” (un grup), deci nu se strică nimic.
3. **Componentele comune întâi**, ca să se propage peste tot:
   - butoanele: `ds::button` / `ButtonSpec` (rolul, eticheta sau tooltip-ul ca nume,
     `on` → apăsat, `enabled`);
   - câmpurile: `SearchField`, `TextInput`, `MultilineField` (valoarea, selecția, caret-ul);
   - controalele de stare: `Checkbox`, `Radio`, switch, segmented, dropdown + `Picker`;
   - listele și tabelele: `Picker` (o listă), `DataGrid` / `table` (un tabel), `TreeList`
     (un arbore);
   - restul: tab-urile, meniurile aplicației, modalele (dialog, cu titlul lor), toast-urile
     (anunțate).
4. **Editorul de cod:** un câmp de text pe mai multe linii, cu linia curentă, caret-ul și
   selecția. Nu tot fișierul o dată: o fereastră în jurul caret-ului, cum fac editoarele
   mari.
5. **Focusul:** focusul nostru (`Controls`) e focusul cititorului de ecran, și invers: o
   acțiune venită de la cititor trece prin aceleași căi ca tastatura și mouse-ul.
6. **Nimic generic nu numește un modul:** fiecare modul își descrie view-urile prin
   contract. Regula din `ui-components.md` se aplică la fel.

**De verificat:**
- **Teste:** arborele unui view dat (rolurile, numele, focusul), plus o acțiune de apăsare
  venită din arbore ajunge la buton.
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
