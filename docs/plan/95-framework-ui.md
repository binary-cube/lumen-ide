# 95 — Framework-ul interfeței: o singură rețetă, scrisă, pentru tot ce se vede și se atinge

**Cererea:** un fel de framework al felului în care facem lucrurile, scris și documentat,
aplicat și componentelor făcute deja. Ținta e consistența pe trei planuri: vizuală (cum
arată), de interacțiune (cum răspunde) și de cod (cum e scris).

## Unde suntem azi

- **Regulile există, dar sunt răspândite:**
  - `.claude/rules/ui-components.md`: ce componentă pentru ce nevoie, contractele
    `Control` / `OverlaySlot`, butoanele, câmpurile, scrollbar-urile, tabelele, meniurile,
    accesibilitatea;
  - `modals.md`, `performance.md`, `code.md`;
  - planurile 63–65, 92, 93, 94.

  Spun ce să folosești, dar nu și cum se construiește o componentă de la zero, nici cum
  arată documentația uneia.
- **Componentele urmează în mare același tipar, dar nescris:**
  - o specificație cu valori implicite și opțiuni (`ButtonSpec`, `DropdownSpec`,
    `CheckRowSpec`);
  - un aspect luat din temă prin `Ctx`;
  - o funcție care desenează;
  - o parte cu stare care răspunde la `Control`;
  - o descriere pentru accesibilitate.

  Abaterile apar exact acolo unde tiparul nu e scris (numele opțiunilor, unde stă
  aspectul, ce primește `paint`, cum se cere un cadru nou).

## Ce construim

**Un ghid al interfeței**, în `docs/ui/`, în engleză ca restul codului și al regulilor
(planurile rămân în română). Fiecare pagină e scurtă și are exemple din cod:

1. **`README.md`, principiile și harta:** ce e o componentă, unde stă (`ui-kit`,
   `ui-components`, `ds`, modulele), cine pe cine cunoaște.
2. **`tokens.md`, limbajul vizual**, citit din cod, nu inventat:
   - spațiere și colțuri (`space`, `radius`);
   - mărimi numite (`button_size`, înălțimile rândurilor, ale barelor);
   - tipografie (`ui_font(Δ)`, greutăți);
   - culori pe roluri din `Palette` (text, text estompat, accent, pericol, suprafețe);
   - elevație și umbre, opacitate și animații (`anim`, durate).
3. **`anatomy.md`, cum e făcută o componentă:**
   - **Specificația:** valorile implicite sunt aspectul de azi; fiecare opțiune e o metodă
     care întoarce `Self`; nicio opțiune nouă nu mută un pixel unde nu e cerută.
   - **Aspectul (`…Look`):** construit din `Ctx` și temă, niciodată culori scrise în cod.
   - **Desenul:** fără stare sau cu starea ei, ce primește (`Layer` / `Scene`, `TextSystem`,
     `Icons`, `Ctx`, dreptunghiul) și ce întoarce (zonele de lovit).
   - **Interacțiunea:** contractul `Control` (apăsare, tragere, eliberare, hover, rotiță,
     taste, text, tooltip, cursor, meniu), stările (hover, apăsat, focus, dezactivat,
     ocupat) și cum se cere un cadru (`frame_deadline`, `poll`, waker-ul).
   - **Accesibilitatea** (`ui_kit::access`, din planul 93).
   - **Performanța:** cache după cheie (generație, mărime, temă), munca grea pe un worker,
     nimic per cadru.
   - **Testele obligatorii:** apăsarea, hover-ul, tastele, accesibilitatea, mărimile mici, o
     temă deschisă și una închisă.
4. **`interaction.md`, comportamentul comun:**
   - focusul și Tab-ul, Esc, ↵;
   - clic dreapta și meniurile, tooltip-urile (întârziere, cele late urmează mouse-ul);
   - derularea, tragerea și eliberarea, dublu-clicul;
   - un modal nu se închide la un clic alături; ce se poate copia.
5. **`recipes.md`, rețete pas cu pas:** o componentă nouă, o opțiune nouă pentru una
   existentă, un view dintr-un modul, un panou peste date (`result_pane`), un grafic
   (`charts`).
6. **`components/<nume>.md`, câte o pagină pentru fiecare componentă**, după același
   șablon:
   - la ce folosește și unde e folosită;
   - anatomia (părțile ei);
   - opțiunile cu valorile implicite;
   - stările, interacțiunea, tastele;
   - accesibilitatea;
   - un exemplu minim de folosire;
   - ce să nu faci.

## Pași

- **95a. Ghidul de bază:** `README`, `tokens`, `anatomy`, `interaction`, `recipes`, din ce
  e deja în cod și în reguli. Regulile din `.claude/rules/` trimit spre ghid, nu-l repetă.
- **95b. Inventarul componentelor**, pe rând:
  - pentru fiecare: pagina ei, apoi verificarea codului față de anatomie (numele
    opțiunilor, aspectul din temă, `Control`, accesibilitatea, testele obligatorii);
  - abaterile sunt reparate în componentă, cu aspectul neschimbat, sau trecute în plan cu
    motivul;
  - ordinea, după cât de des apar: butonul, câmpurile, controalele (switch, checkbox,
    radio, segmente), dropdown-ul și picker-ul, listele și arborii, tabelele și grila de
    date, tab-urile, meniurile, modalele, notificările, graficele, restul.
- **95c. Verificări automate** (`cargo xtask`), ca rețeta să nu se piardă:
  - niciun modul nu desenează de mână ce are o componentă (un dreptunghi rotunjit cu un
    text și o lovitură, un scrollbar);
  - nicio culoare scrisă direct într-un view;
  - fiecare componentă din `ui-components` are pagina ei în `docs/ui/components/`;
  - testele obligatorii există pentru fiecare componentă cu `Control`.
- **95d. Graficele (plan 94) scrise direct după ghid**, ca prima componentă documentată
  întreagă: pagina lor e modelul pentru celelalte.

## Lăsat de la început

- **Un catalog vizual interactiv** (o pagină în aplicație cu toate componentele în toate
  stările): util, dar după 95b. Până atunci, snapshot-urile acoperă vizualul.

## Făcut (2026-11-23): 95a, prima parte

- `docs/ui/README.md` (harta și cine pe cine cunoaște), `tokens.md` (spațierea, colțurile,
  mărimile, tipografia, culorile pe roluri, elevația, mișcarea, citite din `ds`),
  `anatomy.md` (specificația, aspectul, desenul, `Control`, accesibilitatea, costul, testele
  obligatorii), `interaction.md` (mouse, taste și focus, tooltip-uri, meniuri, derulare,
  modale, graficul), `recipes.md` (o componentă nouă, o opțiune, un view de modul, un panou
  peste datele altui modul, un grafic).
- Primele pagini de componente: `components/charts.md`, `components/result-pane.md`.
- Regulile din `.claude/rules/ui-components.md` trimit spre ghid.
- **Lăsat:** paginile celorlalte componente și verificarea lor (95b), verificările automate
  (95c).

## Făcut (2026-11-23): 95b, paginile

- 50 de pagini în `docs/ui/components/` (cu indexul lor, `README.md`), după șablonul
  graficelor, cu exemple luate din locuri reale din cod. Le-au scris trei agenți separați
  pe grupe, fără să schimbe codul; fiecare a verificat componentele față de anatomie.

## De lucru: abaterile găsite (verificate în cod), pe priorități

1. **Accesibilitatea.**
   - Funcțiile vechi de butoane (`primary_button`, `danger_button`, `secondary_button`,
     `toolbar_button`, `primary_button_with_key`, `ds/controls.rs`) nu spun nimic
     cititorului de ecran. Le folosesc și butoanele din subsolul modalelor
     (`footer_buttons`).
   - Fără descriere: `multiline`, `read_text`, rândurile din `log_view`, antetele tabelelor
     (`table::header_cell`, grila), `splitter`, scrollbar-ul, `metric_card`, `slider`,
     `icicle`, `color_picker`, `date_range`, `level_picker`, rândurile din `form`, pastilele
     `choice`, stepper-ul, câmpul numeric, swatch-urile, `ProgressBar` / `Meter`
     (`Role::ProgressBar`), titlurile de secțiune (`Role::Heading`), loader-ul (ocupat).
   - Wordmark-ul își citește literele de două ori (umbra și litera).
   - `on` la buton e spus ca „ales”, nu ca „apăsat”.
2. **Alocări la fiecare cadru:**
   - **Vectori întorși la fiecare desen:** `segmented`, `footer_buttons`, `swatch_row`,
     `around`, `shown` din picker, `edges` din grilă, rândurile din `tree_table`, oprirea din
     `slider`.
   - **Texte făcute la fiecare desen:** tăierile cu „…” din `dropdown`, `radio_card`,
     `progress` și `file_row` (`draw_ellipsized` nu alocă), `i18n::t` / `format!` în
     `picker`, `date_range` (84 de zile), `level_picker`, `color_picker`, `multiline`.
   - **Copii întregi la fiecare cadru:**
     - `icicle` copiază toate celulele (`to_vec`);
     - `diagram` copiază punctele muchiilor;
     - modul de relaxare clonează paleta întreagă (`screen_palette`).
3. **Structuri comune nefolosite:**
   - `TreeTable` își desenează singur antetul în loc de `table::*` și nu are `Control`;
     consola profilului îi rutează intrările de mână.
   - Antetul grilei de date e desenat de mână (alt hover, alt semn de sortare).
   - `level_picker` își desenează singur pastilele.
4. **Logică dublată:**
   - Părțile de scroll din `control.rs`: `Thumb`, `ScrollThumb`, `Scrolled` și `BarThumb`
     au aceleași corpuri.
   - Trei rutine de împachetare a cuvintelor: `toast`, `read_text`, `multiline`.
   - `overflow::fit` și `columns::fit` fac același lucru.
   - Lipirea pe un rând e scrisă în trei feluri.
   - `Appear` e refăcut de mână din `Tween` de trei ori.
   - Formula luminanței apare de trei ori, FNV de trei ori.
   - Așezarea unui popover sub sau deasupra e scrisă de trei ori (`date_range` folosește
     4 px în loc de `POPOVER_GAP`).
   - `clear_button` dublează × al `ButtonSpec`, lupa e desenată de două ori, cele trei
     separatoare estompate sunt aproape copii.
   - Constante scrise ca numere în loc de numele lor (`FILLED_HOVER_MIX`, `SPINNER_FROM`,
     0,022 în subsoluri).
5. **`ctx.fade` uitat** (nu se estompează cu un modal): `dropdown`, `dot_badge`,
   `file_row`, `modal_footer_bar`, antetele tabelelor; `Scrollbar`, `Splitter`,
   `Tooltips`, `GridPaint`, `TablePaint`, `tree::*` primesc o paletă brută în loc de `Ctx`.
6. **Culori și mărimi scrise direct:**
   - Culori: cerneala barelor din `icicle`, tabla de șah din `color_picker`, albul din
     `level_picker`.
   - Mărimi: `switch`, colțul checkbox-ului, `toolbar_button`, `choice`, stepper-ul,
     `radio_card`, `dropdown`, tooltip-ul, `scroll_hints`, `multiline`, `metric_card`,
     popover-urile.
7. **Specificații fără `new` și metode** (anatomia, 1): `DropdownSpec/Look`, `SegmentedSpec`,
   `ChoiceSpec`, `StepperSpec`, `RadioCardSpec`, `MetricCard`, `SliderSpec`, `HeaderCell`,
   `MultilineSpec`, `GridPaint` și altele; `Badge::icon_size` nu face nimic înainte de
   `icon`.
8. **Teste lipsă:**
   - Accesibilitatea pentru componentele de la 1.
   - Hover-ul pentru `read_text`, `tooltip`, `multiline`, `tree_table` și `data_grid`.
   - „O redesenare identică nu alocă nimic” lipsește peste tot în afară de Task Manager.
   - `anim` n-are niciun test.
   - Prea puține teste la scara 2.

**Ordinea propusă:** 1 (accesibilitatea: o regulă a proiectului, cu efect pentru utilizator),
2 (resursele), 3 și 4 (o singură cale pentru fiecare lucru), apoi 5–8. Fiecare grup e un
pas, cu `check.sh` și fără să se schimbe ce se vede.

## Făcut (2026-11-23): primele reparații din listă

- **1. Accesibilitatea:**
  - **Butoanele vechi** (`primary_button`, `danger_button`, `secondary_button`,
    `toolbar_button`, `primary_button_with_key`): buton numit după etichetă, dezactivat,
    tasta lui ca scurtătură. Ajunge și la butoanele din subsolul tuturor modalelor
    (`footer_buttons`). Un buton desenat în interiorul altuia nu se mai citește de două ori.
  - **Un control cu nume își are textele, apăsabil sau nu:** eticheta unui buton dezactivat
    nu se mai citește din nou. Containerele (grup, listă, tabel, meniu, rând) nu le au.
  - **Textul tăiat cu „…” sau derulat într-un câmp** (`draw_ellipsized`, `draw_text_within`,
    celulele grilei) se citește întreg, unde se vede (`access::text_shown`).
  - **Roluri noi:**
    - `ProgressBar` (cu procentul), spinner-ul (o activitate fără sfârșit știut);
    - titlurile de secțiune ca `Heading`;
    - pastilele de alegere ca `Radio`;
    - slider-ul, cu treapta ca valoare și locul ei;
    - câmpul pe mai multe rânduri ca `TextArea`, cu caret.
  - **Wordmark-ul** se citește o dată, nu literă cu literă de două ori
    (`access::silenced`, o gardă).
  - Teste pentru toate acestea. `read_text` și `log_view` desenau deja prin `draw_text`, deci
    erau citite.
- **2. Alocări la fiecare cadru:**
  - **Modul de relaxare:** paleta widget-urilor se face doar pentru altă temă, după o
    amprentă (`screen_widgets::palette_stamp`). Înainte se clona toată paleta, cu tabelul
    ei, la fiecare cadru al unei scene care se mișcă mereu.
  - **Flame graph-ul:** celulele sunt citite din cache, nu copiate la fiecare cadru;
    etichetele „firimiturilor” sunt împrumutate.
  - **Calendarul:** numerele zilelor vin dintr-un tabel fix (înainte: până la 84 de texte pe
    cadru).
  - **Grila de date:**
    - descrierea coloanelor e citită pe loc, nu copiată (cu numele ei) de două ori pe cadru;
    - marginile coloanelor stau într-un vector păstrat de la un cadru la altul.
  - **Picker-ul și tabelul-arbore:** rândurile vizibile sunt citite pe loc, nu copiate.
- `check.sh`: 4 ok.

## Făcut (2026-11-23): restul grupului 2, începutul grupului 4

- **Textele tăiate cu „…” fără alocare:** `ds::ellipsized_width` măsoară ce ar desena
  `draw_ellipsized`, fără să deseneze și fără să aloce, pentru textele așezate după marginea
  lor dreaptă. Tăierea e într-un singur loc (`cut`). Pe ele: nota și valoarea dropdown-ului,
  eticheta barei de progres, titlul unui `radio_card`, numele și eticheta unui rând de fișier
  (eticheta tăiată doar când nu încape). Dropdown-ul își estompează acum textele cu fereastra
  (`ctx.fade`).
- **Un singur hash stabil** (`kernel::hash::fnv64`, `fnv32`), testat pe valorile publicate,
  în locul a nouă copii scrise de mână:
  - în UI: cheia stării panourilor, `text_hash` al barei editorului, culorile după nume,
    schițele iconițelor;
  - în aplicație și infrastructură: numele fișierelor descărcate, dosarul de date al unui
    proiect, încrederea git, indexul, instanța aplicației.

  Aceleași valori bit cu bit, deci ce e păstrat sub ele se găsește în continuare.
- **O singură luminanță** (`Rgba::luminance`, în `kernel`) în locul a două copii.
- **`Appear`** în locul copiilor de mână din `color_picker` și `level_picker` (același
  comportament).
- **Lăsat pentru o verificare pe ecran:** grupul 3 (tabelul-arbore și antetul grilei pe
  părțile comune de tabel). Ar schimba puțin ce se vede (hover-ul, semnul de sortare): de
  făcut cu un snapshot cerut sau cu o privire a utilizatorului.
- `check.sh`: 4 ok.

## Făcut (2026-11-23): grupul 3 și restul grupului 4, verificate pe ecran

Verificare: snapshot-uri înainte și după, pe ecranele atinse (profilul consolei în trei
moduri și tab-ul lui, grila de date pe SQLite, CSV, structură și interogare, selectorul de
niveluri, selectorul de culori; tema întunecată și cea luminoasă), comparate bit cu bit.

- **3. Structurile comune:**
  - **Antetul tabelului-arbore** e acum pe părțile comune (`table::header_band`,
    `header_hover`, `header_edge`, `header_rule`). Rămâne pe un rând, la 28 px, fiind un
    tabel compact. Singura diferență pe ecran: liniile dintre antete sunt cu 2 px mai scurte
    la fiecare capăt, ca în grila de date și în Task Manager. Pozele diferă doar în
    coloanele de 2 px ale acelor linii, pe rândurile antetului.
  - **`TreeTable` e un `Control`**, cu `take_event` pentru ce a făcut o apăsare (testat ca
    parte). Profilul consolei își păstrează rutarea după mod: arată un tabel din mai multe,
    iar comutatorul acela e sensul vederii, nu rutare.
  - **Antetul grilei de date** ia din `table` hover-ul (`header_hover`, acum același în
    toate tabelele), fonturile și liniile de bază (`name_baseline`, `subtitle_baseline`).
    Păstrează ce e doar al lui: cheia și locul în sortarea pe mai multe coloane. Pozele sunt
    identice; hover-ul nu apare în ele.
  - **Pastilele selectorului de niveluri** sunt `ds::choice_cell`, o alegere care umple o
    celulă dintr-o grilă. Desenul e același (pozele identice); în plus se citesc ca radio și
    nu mai fac texte la fiecare cadru.
- **4. Logica dublată:**
  - **Părțile de derulare** (`Thumb`, `ScrollThumb`, `Scrolled`, `BarThumb`) au o singură
    apăsare pe bară (`bar_press`) și o singură mutare a offset-ului (`move_to`).
  - **Împachetarea:** `toast` folosește `ds::wrap` (era o copie). `read_text` și
    `multiline` rămân separate cu intenție. Câmpul de editare păstrează spațiul la capătul
    rândului, ca să aibă unde sta caret-ul; textul de citit îl scoate.
  - **Lipirea pe un rând:** o singură regulă, `kernel::text::pasted_line` (rândurile unite
    printr-un spațiu, un `\r\n` o singură trecere, rândurile goale scoase, caracterele de
    control scoase). Se aplică în câmpurile de text, în picker și în câmpurile WHERE, ORDER
    și Find ale grilei; înainte, grila păstra doar primul rând. Celula editată în grilă ia
    doar primul rând, cu intenție: rândurile unite ar fi o valoare pe care n-a scris-o
    nimeni.
  - **Popover-ele** sunt așezate de `ds::popover_rect`: sub sau deasupra, de la o margine a
    ancorei, în fereastră. Selectorul de niveluri nu mai poate da panică într-o fereastră mai
    îngustă decât el (`clamp` cu capetele inversate). Calendarul stă acum la `POPOVER_GAP`
    de câmpul lui, ca restul (înainte 4 px) — singura schimbare vizibilă, fără snapshot
    (nu are un caz).
  - **Separatoarele estompate** au un singur desen (`faded_line`), cu aceleași calcule.
  - **Lupa** e desenată într-un singur loc.
  - **Constantele:** `ds::BAND_TINT` (0,022) în subsoluri, bare și cardurile formularelor;
    butoanele vechi folosesc `FILLED_HOVER_MIX` și `SPINNER_FROM`.
  - **Cântărite și lăsate:**
    - `overflow::fit` și `columns::fit` seamănă doar ca idee. Regulile diferă: coloanele au
      alegerile utilizatorului, coloana sortată și coloanele fixate; bara are pași și readuce
      ce încape din nou. Un singur algoritm ar schimba ce se ascunde.
    - `clear_button` rămâne × al câmpurilor; un `ButtonSpec` rotund ar schimba dimensiunile.
- **Ghidul UI** e actualizat: `table` (`header_hover`, liniile de bază), `tree-table` (e un
  `Control`, antetul comun), `controls` (`choice_cell`), `interaction` (popover-ele),
  `tokens` (`BAND_TINT`).
- `check.sh`: 4 ok.

## Făcut (2026-11-23): grupurile 5–8

- **5. Estomparea uitată** (`ctx.fade`): punctul unui badge, rândul de fișier (semnul,
  dosarul), bara subsolului unui modal, toate părțile de tabel (bandă, antet, linii, rânduri
  alternate). Barele de derulare dintr-un modal, un dialog, un meniu de alegeri sau un picker
  se estompează acum cu fereastra (`EdgeScrollbar::paint_in` / `paint_on_line_in`, cu `Ctx`);
  `paint` cu paletă rămâne pentru vederi. Testate cu opacitatea 0,5. **Cântărit și lăsat:**
  `Splitter`, `Tooltips` (își are estomparea ei), `GridPaint`, `TablePaint`, `tree::*` se
  desenează doar în vederi, care nu se estompează; când unul ajunge într-o fereastră care se
  estompează, primește un `_in` ca bara.
- **6. Culori și mărimi scrise direct:** cernelurile flame graph-ului (`INKS`) și pătratele
  tablei de șah (`CHECKER`) au nume și motivul (aceleași în orice temă). Albul pe accent e
  convenția tuturor butoanelor pline (scrisă în ghid). Mărimile au nume: comutatorul
  (`SWITCH`, `SMALL_SWITCH_H`, `SWITCH_KNOB_INSET`), colțul checkbox-ului, butonul de bară,
  pastila de alegere (`CHOICE_H`, `CHOICE_PAD`), pașii, cardul radio, × câmpului (`CLEAR`),
  butonul cu tastă (`BUTTON_H`), nota dropdown-ului, săgeata tooltip-ului, câmpul pe mai
  multe rânduri (`PAD`, `LINE_H`, `LANE`, aceleași la măsurat și la desenat), cardul de
  metrică. Aceleași valori; unde o sumă de rotunjiri ar fi putut diferi la scări ca 1,25
  (`px(4)` față de `2 × px(2)`), calculul a rămas cel de dinainte.
- **7. Specificațiile au `new` și metode** (`ChoiceSpec`, `SegmentedSpec`, `StepperSpec`,
  `RadioCardSpec`, `DropdownSpec` / `DropdownLook`, `SliderSpec`, `MultilineSpec`,
  `MetricCard`, `HeaderCell`), testate; apelurile cu câmpuri rămân valabile. `Badge::icon_size`
  / `icon_color` țin oricare ar veni întâi. `GridPaint` și celelalte structuri de desen (doar
  argumentele unui `paint`) rămân cum sunt.
- **8. Teste:**
  - **„O redesenare identică nu alocă nimic”** pentru grila de date, tabelul-arbore și flame
    graph, la scara 1 și 2, cu un numărător comun (`ui_components::allocations`, alocatorul
    testelor). Au găsit ce mai aloca și s-a reparat:
    - **grila (93 → 0):** coloanele arătate și marginile în buffere păstrate
      (`visible_into`, `edges_into`); numărul unui rând scris fără alocare
      (`write_grouped`, separatorul limbii ținut sub `i18n::generation`); textele barelor
      (intervalul, totalul, mărimea paginii, „Trimite”), rândul de jos și indiciile
      câmpurilor păstrate pentru ce spun; așezarea barei de unelte și numele extractorului
      împărțite (`Arc`), nu copiate; lățimea coloanei de numere păstrată; un text tăiat cu „…”
      doar când chiar nu încape; sursa e întrebată `can_edit()` (nou în contract: fără motiv
      scris; consola și explorer-ul îl dau din starea lor, testat că spune ce spune
      `editing()`), nu `editing()` cu motivul tradus la fiecare cadru;
    - **tabelul-arbore (27 → 0):** textul unei celule măsurat, apoi desenat, fără listă de
      bucăți; numerele scrise în texte păstrate (`NumberText`); titlurile tăiate la desen;
    - **flame graph (5 → 0):** calea firimiturilor într-un buffer păstrat; etichetele tăiate
      la desen.
  - `i18n::generation()`: un număr care se schimbă când mesajele se pot schimba (un catalog
    adăugat, limba schimbată), ca textele făcute din ele să fie păstrate sub el; testat.
  - `anim` are teste (tween-ul ajunge și se oprește, se întoarce din locul în care e,
    apariția).
  - Tabelul-arbore ca `Control` (hover-ul, tastele, tragerea) și estomparea au teste.
  - **Rămas:** hover-ul pentru `read_text`, `tooltip`, `multiline`; „nu alocă nimic” pentru
    picker și calendar (scoaterea alocărilor lor e făcută, testul nu).
- Verificare: aceleași 20 de poze (profil, grilă, selectoare; temele întunecată și luminoasă)
  identice bit cu bit cu cele de după grupul 4; `check.sh`: 4 ok.

## Făcut (2026-11-23): restul grupului 8; lista de dosare din căutare

- **„Nu alocă nimic” pentru picker (46 → 0) și calendar (25 → 0):** textele tastelor de jos
  ale picker-ului scrise o dată pe limbă (pentru toate picker-ele); numele și detaliul
  rândurilor tăiate la desen; inițialele avatarului pe stivă (`ds::Initials`, testate:
  litere care cresc la majusculă, `?`). Calendarul își păstrează textele: formatul datei
  (sub amprenta lui), titlul lunii (un text păstrat), zilele săptămânii împrumutate,
  subsolul („alege prima zi”, intervalul, Aplică / Anulează) sub generația mesajelor și
  interval.
- **Hover testat** pentru `tooltip` (așteaptă, apoi trece imediat la alt element, plecat:
  ascuns), `read_text` și `multiline` (bara se lărgește sub mouse și revine; cursorul
  textului).
- **Lista de dosare din câmpul „In” al căutării** (cerut, cu captură): numele dosarului
  întâi, literele tastate în accent, dosarul părinte după el, estompat și tăiat la mijloc
  când nu încape, calea întreagă în tooltip. Componente noi, comune: `picker::PathRows` (orice
  listă de căi), `PickerItem::marks`, `PickerRows::tooltip` / `Picker::tooltip_at`,
  `Picker::open_suggestions_with`; în `ds`: `draw_ellipsized_marked` (`Marks`) și
  `draw_ellipsized_middle`, fără alocare; în domeniu: `workspace::search::folder_marks` (ce
  a potrivit completarea: începutul numelui, altfel potrivirea liberă), testat. Snapshot nou:
  `search-folders` (dosare imbricate puse doar pentru el, `FOLDERS_SAMPLE=1`).

## Făcut (2026-11-23): inelul „Folder Sizes” pe graficul comun

- **Abaterea** (observată de utilizator, cu captură): inelul era desenat de modul
  (`folder-sizes/src/donut.rs`, rasterizat de el, cu culori proprii), nu de `charts::Chart`.
- **Graficul comun are acum inel:** `Options::hole` (procentul razei lăsat gol, cel mult
  `MOST_HOLE`; o configurare salvată fără el se citește ca plăcintă, testat). Gaura e folosită
  în geometrie (`MarkShape::Slice { inner }`), la lovire (gaura nu e a nimănui), în raster
  (arcul interior parcurs înapoi: un inel întreg își păstrează gaura) și în SVG. `Chart`
  primește `set_lit` (o felie aprinsă din afară: rândul din listă de sub mouse) și `slice_at`
  (felia de sub un clic); `Prepared::slices` face o plăcintă din valori deja numărate.
- **„Folder Sizes”** folosește graficul: culorile seriilor din `ChartLook` (aceleași pe
  pătratele listei), tooltip-ul graficului cu mărimea formatată, felia ieșind în afară sub
  mouse sau sub rândul ei. Textul din mijloc rămâne al modulului. `donut.rs`, testul lui și
  `colors()` s-au șters.
- **Reparat pe drum:** „Analyze Folder Sizes” din paleta de comenzi, cu un fișier în față,
  încerca să măsoare fișierul („Folderul nu poate fi citit”). Acum măsoară ținta doar când
  e dosar (`ActionContext::folder`, din ce știe arborele, fără disc), altfel proiectul.
- **Snapshot-urile** așteaptă acum ce vine după încărcare: bucla finală cere vederilor ce au
  primit între timp (`poll_viewers`, ca aplicația la fiecare cadru), iar secunda pentru
  animații se numără de la sfârșitul încărcării. Fără asta, graficele desenate pe un worker
  lipseau din poze. Caz nou: `folder-sizes`.

## Făcut (2026-11-23): ce mai era deschis

- **Inelul cu o singură felie** (cerut, cu captură): sub mouse cobora tot inelul (felia
  „scoasă” spre mijlocul ei, jos), iar sus se vedea o linie. O felie cât tot cercul nu se mai
  mută și nu-și mai desenează marginea (`charts::whole_turn`); testat. Tooltip-ul unei felii
  spune valoarea în unitatea proprietarului (`17 B`, nu `17`) și procentul cu separatorul
  limbii.
- **Etichetele axei valorilor** (cerut, cu captură din Task Manager): geometria le măsura după
  textul modelului (`50`), graficul le scria formatate (`50.0%`) și ieșeau spre stânga, peste
  marginea cardului. Acum geometria le măsoară cum sunt scrise (`View::y_label`,
  `Written`); un format nou refăcută așezarea. Testat la scara 2.
- **Task Manager, detaliul:** conținutul care derulează lasă un culoar barei de derulare
  (`SCROLL_LANE`, mereu, ca să nu sară); un fir fără nume de la sistem se scrie „fir fără
  nume”, estompat, la sfârșitul listei (nu un rând gol).
- **Windows, „Open with”:** drumul executabilului adus la o formă unică înainte de hash
  (`instance::one_form`), testat; jurnalul spune canalul ascultat și cel căutat. De confirmat
  pe Windows.
- **Windows, decupajele:** codul recitit; jurnal `debug` cu zona paginii, găurile și caseta
  regiunii puse, de citit pe Windows.
- **Accesibilitatea, ce rămăsese:** roluri noi `SpinButton`, `ScrollBar`, `Splitter`,
  `ColumnHeader` (și în puntea spre sistem). Bara de derulare își spune procentul;
  separatorul se spune; antetele de coloană (cele comune, ale grilei, ale tabelului-arbore)
  sunt antete numite; câmpul numeric e un spin button cu numărul; câmpul de culoare un buton
  cu valoarea scrisă (mostra din el nu se mai spune o dată); o mostră singură, un buton cu
  culoarea; pașii unui wizard o listă (cel în curs ales, cei gata bifați; numerele dintr-un
  tabel, fără text pe cadru). `ButtonSpec::toggle` spune un buton-comutator pornit / oprit:
  pus pe opțiunile căutării, ale găsirii, filtrele git, pinul, pauza Task Manager-ului,
  uneltele diagramei, ale grilei și ale jurnalului, opțiunile comparării. Testate.

## Făcut (2026-11-23): 95c, verificările automate

`cargo xtask check-deps` citește acum și regulile ghidului (`xtask/src/ui_conventions.rs`,
testat):

- **Fiecare componentă are pagina ei:** orice `pub mod` din `ui-components` are
  `docs/ui/components/<nume>.md`, în afară de ajutoarele și contractele din `NO_PAGE`, fiecare
  cu motivul ei (`ds` stă în `tokens.md` și paginile controalelor, `painter`, `scroll_state`,
  `decorations`, `services`, `shortcuts`, `snippets`, `code_tokens`). Pagini noi: `severity`,
  `tag-colors`, `tool-panel`.
- **Nicio culoare scrisă direct în codul de desen:** o culoare fixă (`Rgba::rgb(0x…)`) din
  module, `ui-components` și `ui-workbench` e o constantă cu nume, al cărei comentariu spune
  de ce e fixă. Excepția sunt temele implicite (`palette.rs`), care sunt chiar culorile.
  Mutate în constante: culorile scenelor de relaxare, ale efectelor de decor (soarele,
  balonul, ecranul vechi), ale diagramelor C4, cerneala pe o umplere deschisă (diagrame,
  semne, Mermaid), fundalul imaginilor, culorile severității (Okabe–Ito), strălucirea din
  splash, tabla de șah din gutter, badge-ul JS și bara întunecată din pânză. Albul și negrul
  sunt `Rgba::WHITE` / `Rgba::BLACK`. Valorile sunt aceleași.
- **Fiecare componentă cu `Control` are teste de apăsare și hover:** în fișierul ei de teste
  (sau în cel al dosarului ei). Teste noi: × câmpului de căutare, bara flame graph-ului,
  rândurile listei de fișiere schimbate, zilele calendarului (hover și apăsare), părțile
  selectorului de culori, caret-ul câmpului pe mai multe rânduri.
- **Niciun modul nu-și desenează singur un control** (`fn draw_button`,
  `fn paint_scrollbar`, `fn draw_checkbox`…): ca regresiile să nu se întoarcă (azi niciunul).
- Verificat pe dos: o culoare strecurată într-un modul e semnalată, cu fișierul și rândul.

## Făcut (2026-11-23): Task Manager-ul care încărca procesorul

- **Văzut** (profilul utilizatorului, 38 s): firul principal la ~27% cu Task Manager-ul deschis;
  la fiecare secundă, o rafală de ~300 ms cu 1 100–1 400 de cadre la 100 ms (~11 000 pe
  secundă), toate cerute de Task Manager ca „animație”.
- **Cauza:** graficele din detaliu pentru figurile pe care partea aleasă nu le are nu erau
  desenate, dar primeau date în fiecare secundă. Nedesenate, n-aveau aspectul Task
  Manager-ului („fără tranziție la actualizare”) și porneau tranziția implicită de 280 ms,
  nevăzută. Cadrele ieșeau identice, nu ajungeau pe ecran, deci nimic nu le încetinea.
- **Reparat:**
  - **în grafic** (pentru toți cei care îl folosesc): unul nedesenat, sau uitat cu `forget`,
    nu pornește tranziții și nu spune că se animă;
  - **în Task Manager:** graficele detaliului care nu se arată sunt uitate la fiecare desen;
  - **în aplicație**, ca plasă de siguranță: o animație al cărei cadru n-a arătat nimic nou
    cere următorul cadru abia după un cadru de 120 Hz (`ANIMATION_FLOOR`), deci nimic nu mai
    poate învârti procesorul fără frână.
- **Teste:** un grafic nedesenat nu se animă; detaliul care primește date în fiecare secundă
  nu cere cadre de animație. Testul pică pe comportamentul vechi și trece pe cel nou.
- Regula e scrisă în `.claude/rules/performance.md` („nothing moves unseen”).

### Nicio parte nu spune că se mișcă dacă nu se mișcă (verificat peste tot)

- **O singură trecere, agnostică** (`Workbench::each_frame_asker`): tot ce poate cere cadre
  e întrebat într-un singur loc, după nume (`Asker { name, moving, due }`). Din ea citesc
  `animating()`, sondele profilului și cele două verificări de mai jos. O parte nouă se
  adaugă doar acolo, iar restul aplicației n-o numește.
  - Se întreabă **doar ce e pe ecran**: tab-ul activ al fiecărui grup (`shown_tabs`), nu
    tab-urile din spate.
  - Sub o vedere care acoperă toată fereastra (Relax, ecranul ținut treaz) se întreabă doar
    ferestrele de deasupra editorului.
  - Ferestrele peste editor sunt întrebate deschise sau nu, pentru că una care se închide
    încă alunecă (meniul lateral). Cele care apar se animă doar cât sunt deschise (About,
    dialogul, modalul, alegerea nivelului, alegerea culorii).
- **Alarma din aplicație** (`UnseenMotion`): dacă timp de 2 s (`UNSEEN_FOR`, peste orice
  tranziție) toate cadrele cerute ca animație ies identice, jurnalul spune o singură dată,
  pe nume, ce părți zic că se mișcă (`warn`, câmpul `parts`).
- **Verificarea din snapshot-uri:** după ce un ecran s-a liniștit și încă 1,5 s
  (`SETTLE_MOVING`), ce se mai „mișcă” e scris (`moving: …`).
  - `ui-snapshots.sh` listează ecranul; la `compare` el contează ca diferență.
  - O scenă care se mișcă intenționat se marchează cu `MOVES=1`.
- **Auditul celor 162 de implementări** (`is_animating`, `frame_deadline`, `next_frame`…):
  - Majoritatea doar deleagă sau se termină singure în timp (`Tween`, `Appear`, tranzițiile
    graficelor, estomparea barelor).
  - Reparate cele care cereau cadre la viteza ecranului fără să se schimbe ceva:
    - **căutarea din setări:** cât se făceau vectorii pe un worker, cerea cadre. Acum
      worker-ul trezește fereastra când a terminat. Un worker căzut nu mai lasă cererea
      deschisă pentru totdeauna;
    - **Folder Sizes:** cât măsura (minute pe un arbore mare), era animație. Acum e
      încărcare, în ritmul loader-ului. Un worker căzut se închide curat;
    - **terminalul:** prima ieșire a unui program pornit (până la 20 s) e acum încărcare,
      cu un singur cadru la sfârșit (`frame_deadline`);
    - **dialogurile git** (schimbarea ramurii, stash-ul): operația care rulează e acum
      încărcare;
    - **filtrul text din istoricul git:** pauza de tastare (300 ms) era animație. Acum e un
      singur cadru, la termen;
    - **lista unui dropdown:** termenul barei de derulare era luat drept „mișcare”, inclusiv
      în cele 600 ms în care bara stă pe loc, și chiar cu lista închisă. Acum e un termen,
      doar cât lista e deschisă.
- **Focusul ferestrei, un contract nou** (`Viewer::set_window_focused`, implicit nu face
  nimic): aplicația îl spune fiecărei vederi din tab-uri la primul cadru, apoi doar când se
  schimbă (în `sync_shown`, lângă `set_shown`). Theme Studio își oprește previzualizarea
  efectelor cât fereastra e în fundal, cum se opresc și decorațiunile ferestrei, și o
  pornește la loc când fereastra revine în față. Teste: vederea află focusul doar la
  schimbare (`[true, false, true]`); previzualizarea stă pe loc în fundal.
- **Lăsat:**
  - Diagrama web verifică fișierul o dată la 2 s cât se vede: e un cadru rar, nu o
    animație.
  - Notificările cu progres și scena Relax se mișcă vizibil, intenționat.


### Recenzia (agent separat), verificată și reparată

Fiecare problemă a fost verificată în cod înainte de reparare și are un test care ar fi picat
pe codul vechi:

- **„Save As” anulat la grafic** (consola, tabelul din Explorer): un dialog anulat nu trimite
  nimic înapoi, deci steagul rămânea pus, iar următorul export al tabelului scria un PNG.
  Exportul tabelului șterge acum steagul când cere calea.
- **Rânduri tăiate în ultima pagină:** cu limita atinsă într-o pagină scurtă, graficul zicea
  „toate rândurile” și ascundea Fetch All. Acum sunt „mai multe” (`came > room`).
- **YAML ostil:** alias-urile copiau ce numesc, așa că un fișier de ~1 KB creștea la sute de
  milioane de noduri. Nodurile se numără întâi din evenimente, fără expandare
  (`MOST_YAML_NODES` = 4 000 000), iar documentul se refuză peste buget.
- **Tranziția graficelor mari:** fiecare marcaj își căuta forma anterioară liniar, deci
  O(n²) pe cadru. Acum se caută binar, pe o ordine sortată o dată
  (50 000 de puncte: 0,06 s).
- **Gradații la capetele float-ului:** un interval de ±1,7e308 dădea infinit și panică.
  Intervalul e ținut în `f64::MAX / 4`, numărul de gradații e plafonat, iar rotunjirea nu
  mai depășește.
- **Starea panoului scrisă în paralel:** un singur scriitor golește coada, cea mai nouă
  stare a fiecărui fișier ia locul celei care aștepta, iar numele temporare sunt unice.
- **Selecția cititorului de ecran:** se pune doar în câmpul al cărui text e cel al nodului
  (altfel nimic: altă bară de căutare, un editor de cod care spune doar liniile văzute).
- **HTML ostil:** sfârșitul unui script se caută pe loc, fără copie. Rândurile și celulele
  au limite din timpul citirii, iar ce trece de ele se numără. Copiii XML se grupează
  într-o singură trecere.
- **Mii cu virgulă:** „1,204” se citea 1,204. O coloană în care fiecare virgulă e urmată de
  trei cifre se citește acum cu virgula între mii (`Marks::Thousands`). O valoare cu altă
  virgulă („1,5”) dovedește virgula zecimală.
- **Nume generate** („column 1”, „table 2”, cheie, valoare, „count”): vin acum din
  traduceri (ro și en).
  - **Lăsat:** un grafic salvat pe o coloană generată (un fișier fără antet) n-o mai
    găsește după o schimbare de limbă; își alege din nou coloanele, ca la una ștearsă.

### Grafic din selecția unui tabel

- **Clic dreapta pe rândurile unui tabel** (rezultatul unei console, datele unui tabel din
  Explorer): „Grafic: din selecție”, „Grafic: din toate rândurile, din nou” (cât e desenat
  dintr-o selecție), „Închide panoul Grafic”.
- **Agnostic, fără nimic hardcodat** (regulă nouă în `.claude/rules/ui-components.md`):
  - grila dă selecția ca tabel neutru (`DataGrid::picked`: rândurile atinse, coloanele
    selectate sau toate cele afișate pentru rânduri întregi; cel mult 20 000 de rânduri,
    doar cele deja citite);
  - gazda panourilor face rândurile meniului pentru fiecare panou înregistrat
    (`PaneHost::menu` / `menu_command`);
  - vederea doar le leagă. Un panou nou sau un tabel nou merge fără nicio linie schimbată
    în celelalte.
- **Panoul de grafic:**
  - spune „Din N rânduri selectate” și are butonul „Toate rândurile”;
  - graficul configurat se desenează pe selecție când coloanele lui sunt acolo; altfel
    se propune unul pentru coloanele selectate;
  - înapoi la toate rândurile, revine graficul de dinainte, sau cel schimbat între timp;
  - starea salvată rămâne cea pentru toate rândurile.
- **Teste:**
  - selecția ca tabel (celule, rânduri întregi, sub două celule nimic);
  - meniul gazdei (din selecție, înapoi, închis, un id străin lăsat);
  - panoul pe o selecție și înapoi.

- **Graficul din selecție, fără așteptare:**
  - un panou abia deschis nu avea încă trezitorul ferestrei, deci graficul pregătit pe
    worker aștepta următorul eveniment. Lucrul îl citește acum dintr-un loc comun, pus la
    desen;
  - o pregătire cerută din nou înainte să înceapă e lăsată (nu se mai pregătesc toate
    rândurile înaintea selecției);
  - bara arată loader-ul și „Se pregătește graficul…” cât lucrează worker-ul.
  - Pe firul interfeței rămâne doar copierea selecției: 20 000 de rânduri × 3 coloane
    (maximul) iau 2 ms, iar 3 rânduri iau microsecunde. Pregătirea și desenul rulează pe
    worker.
- **Lista de foldere din căutare** are lățimea câmpului (`PickerRows::widens`): căile se
  scurtează, deci lista nu mai crește și nu se mai micșorează cât scrii.
