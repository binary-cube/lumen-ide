# 92. Interacțiunea cu elementele, generică (ca într-un form)

Prioritate mare. Regula e în `.claude/rules/ui-components.md` („Generic and agnostic”).

## Problema

Codul care distribuie input-ul numește pe rând componentele către care îl trimite:

```rust
if self.completion.scrollbar_held() { … }
if self.code_hover.scrollbar_held() { … }
if self.dialogs.dialog.scrollbar_held() { … }
```

Același tipar apare pentru click, hover, scroll, taste, text, cursor, termene de cadru,
„se încarcă”. O componentă nouă cere atunci o ramură nouă în aplicație, iar o componentă
uitată acolo nu primește evenimentul (bug-ul thumb-ului din dropdown-ul setărilor). Analiza
din 2026-11-20 a găsit problema:

- în rutarea aplicației: `input.rs`, `clicks.rs`, `hovering.rs`, `editing.rs`,
  `app/mod.rs`;
- în view-urile compuse: setările își numeau barele și lista;
- în câteva locuri unde codul generic numește un modul sau un limbaj: `"php"` în
  `snippets.rs`, `"database"` în `paint.rs`, `"terminal"` în `shortcuts_editor.rs`.

## Ce vrem

Interacțiunea funcționează ca într-un form de fereastră cu controale. Fiecare element
răspunde unui contract comun, iar cine distribuie input-ul parcurge o listă de elemente și
nu numește niciunul:

- **apăsare → tragere → eliberare**: cine a luat apăsarea ține mouse-ul până la eliberare;
- **hover și cursorul**: elementul de sub mouse;
- **scroll**: elementul de sub mouse, apoi părintele lui;
- **focus**: tastele și textul merg la elementul cu focus; Tab trece la următorul;
- **selecție și copiere**, **tooltip**, **meniul de click dreapta**;
- **termenul următorului cadru, animația, „se încarcă”**.

Fiecare metodă are o implementare implicită, așa că un element o implementează doar pe cea
pe care o folosește.

## Pași

- **92a: stiva ferestrelor peste editor, completă.** `OverlaySlot` are deja `drag`,
  `release` și `held`. Mai trebuie:
  - lista de completare și cardul de hover devin sloturi plutitoare, care lasă tastele
    editorului;
  - `frame_deadline` / `loading` pe fiecare slot, cu un nume pentru profiler;
  - câmpul de text cu focus întrebat prin slot (`input` / `input_mut`), în locul enum-ului
    `InputOwner`.

  Aplicația rămâne doar cu propriile ecrane: home, find, splittere, barele.
- **92b: contractul elementelor din `ui-components`.** `Control` (apăsare, tragere,
  eliberare, hover, cursor, scroll, taste, text, selecție, tooltip, meniu, toate implicite)
  și `Controls`, containerul unui view. Containerul face lovirea după zonele ultimei
  desenări, ținerea mouse-ului, focusul și ordinea de Tab, și aduce hover-ul și cursorul.
  `grab::Grab` intră în el. Componentele comune îl implementează: scrollbar, picker, câmp de
  text, buton, dropdown, slider, tabel, arbore, `read_text`.
- **92c: ecranele aplicației pe `Controls`**, în ordine: setările, dialogul, sertarul de
  meniuri, lista de proiecte.
- **92d: view-urile modulelor**, treptat. Fiecare view care compune componente își
  păstrează elementele într-un `Controls`, iar `Viewer` doar îl distribuie.
- **92e: numele de module din codul generic.**
  - prefixul fragmentelor de cod îl declară limbajul (`<?php`);
  - iconița barei o întoarce `None` chiar bara bazei de date;
  - „ia textul tastat” devine o proprietate a locului tastelor (nu `"terminal"`);
  - `LEVEL_PICKER_EXTENSION`, `PICKER_EXTENSION` și cererile unui singur modul trec din
    `ui-components` în `contracts`.
- **92f: un test care păzește regula.** Codul aplicației și al componentelor nu conține
  id-uri de module sau limbaje (o listă permisă: înregistrarea modulelor, migrările de id-uri
  vechi).

## Făcut (2026-11-20)

- `OverlaySlot::drag` / `release` / `held` (cu textul, pentru acțiunea unei ferestre
  modale). Lista unei bare de editor, meniul de alegeri, dialogul, sertarul de meniuri,
  fereastra modală a unui modul și setările răspund prin ele. `mouse_drag`, `mouse_up` și
  `drag_pending` parcurg stiva și nu mai numesc pe nimeni.
- `ui_components::grab` (`Grab`, `Thumb`). `Picker` și bara de navigare a unui modal
  (`Modal::nav_grab`) îl implementează. Setările își țin bucățile într-o listă, fără să le
  numească.
- Thumb-ul listelor `Picker` se apasă, se trage și se eliberează, deci și în dropdown-urile
  setărilor și ale barelor de editor (test).

- **92a, cea mai mare parte:**
  - **Cardurile editorului pe stivă:** cardul de hover (`CodeHover`) și lista de completare
    (`Completion`) sunt sloturi plutitoare. Fiecare primește click-ul, scroll-ul, hover-ul,
    tragerea thumb-ului, animația și termenul lui. Pleacă singure la o apăsare sau la un
    scroll în altă parte (`press_elsewhere`, `scroll_elsewhere`), iar editorul își păstrează
    tastele.
  - **Termenele de cadru:** sunt ale sloturilor (`frame_deadline`): notificările, fereastra
    modală, cardul din setări. Aplicația le parcurge cu numele lor (`overlays_due`, pentru
    profiler) și nu mai numește `toast`, `modal-view` sau `settings-card`.
  - **Câmpul de text cu focus:** îl dă fereastra de sus (`OverlaySlot::input` /
    `input_mut`). `InputOwner` mai are doar ecranele aplicației (home, find).
  - **Clipboard și editare:** copierea, tăierea, lipirea, undo/redo și ⌘A ale ferestrei de
    sus trec prin `selected_text`, `edit_command` și `paste`. „Se încarcă” trece prin
    `loading`.
- **92e, o parte:**
  - **Fragmentele de cod:** ce le trebuie în față o spune limbajul
    (`LanguageDescriptor::fragment_opening`: PHP-ul își declară `<?php`). Aplicația nu mai
    scrie `"php"`.
  - **Iconița generică de bază de date:** consola o lasă singură pe a fișierului. Aplicația
    nu mai filtrează `"database"`.
  - **Scurtăturile:** „aici se tastează” vine din locul tastelor (`BindingRow::typing`, din
    `When`), nu dintr-un `"terminal"` (cod mort: terminalul nu are un asemenea loc). Editorul
    unui limbaj (`editor-sql`) e acum socotit loc unde se tastează (test).
  - **Nivelul limbajului din bara de stare:** își rulează comanda ca orice element, fără
    ramura dublă din click.

## Făcut (2026-11-20, a doua parte)

- **92a, încheiat:**
  - indiciul parametrilor e slot (`ParamHint`);
  - momentul în care mouse-ul se oprește vine din sloturi (`rest_deadline`: cardul de hover,
    sertarul de meniuri);
  - meniul proiectelor își ține singur apăsarea, tragerea și eliberarea (`held` / `drag` /
    `release`), iar aplicația păstrează doar lista din home, ecranul ei.

  `mouse_down`, `mouse_drag`, `mouse_up`, `scroll`, `hover`, `animating`, `loading`,
  termenele, câmpul cu focus și clipboard-ul nu mai numesc nicio fereastră sau componentă
  de peste editor.
- **92b, nucleul:** `ui_components::control`.
  - `Control` e contractul unui element: zona, lovirea, apăsarea (`Press::Ignored` / `Took` /
    `Held`), tragerea, eliberarea, hover-ul, cursorul, scroll-ul, focusul, tastele, textul,
    selecția, comenzile de editare, tooltip-ul, meniul, animația și termenul. Toate au
    implementări implicite.
  - `Controls` e starea de folosire a elementelor unui view, după locul lor în lista dată
    de view:
    - cine ține mouse-ul până la eliberare;
    - focusul, cu Tab și ⇧Tab între elementele care îl iau;
    - hover-ul, cu elementul părăsit anunțat;
    - cursorul, scroll-ul, tooltip-ul și meniul de sub mouse;
    - textul și comenzile, către elementul cu focus.

    Întoarce locul elementului care a răspuns, așa că doar view-ul știe ce înseamnă.
  - Îl implementează câmpul de text (`TextInput`), lista `Picker` (lovită oriunde cât e
    deschisă; spune ce a făcut prin `take_event`), bara cu offset-ul ei (`Thumb`), textul de
    citit (`ReadText`) și bara de navigare a unui modal (`Modal::nav_grab`). `Grab` a intrat
    în el.
- **92c, setările:** apăsarea, tragerea, eliberarea și hover-ul trec prin `Controls`
  (`with_controls`: lista, bara navigării, bara conținutului). Un test pe tot drumul
  deschide un dropdown și îi trage thumb-ul.
- **92e / 92f:**
  - „ascuns implicit” e un câmp al manifestului (`ModuleInfo::hidden_by_default`), citit din
    modulele înregistrate; `HIDDEN_BY_DEFAULT` a dispărut.
  - Un test păzește regula: codul aplicației și al componentelor nu conține ca literal
    id-ul niciunui modul sau limbaj. Locurile unde același text e altceva (o tastă, o
    extensie, un format de export al grid-ului, exemplele paginii de iconițe) sunt listate
    cu motivul lor. Am verificat că pică pe un id pus de probă.
- **Metadatele modulelor:**
  - fiecare manifest are `ModuleInfo`: iconița (din setul aplicației, deci se vede și cu
    modulul oprit), autorul, versiunea, data actualizării și ascuns implicit;
  - un modul care vine cu aplicația ia autorul (`brand.toml`: `publisher`), versiunea
    aplicației și data release-ului (`[workspace.metadata.release] date`, scrisă lângă
    versiune la fiecare release);
  - pachetele de iconițe au ca autor creditul lor;
  - pagina Modules arată iconița, iar lângă nume autorul, versiunea și data;
  - lista nivelurilor de log arată iconița fiecărui modul (`RowSpec::icon`, pe rândul comun
    de setări: orice rând o poate avea).

## Făcut (2026-11-20, a treia parte)

- **92c, încheiat:**
  - dialogul (bara listei), sertarul de meniuri (trei bare, `ScrollThumb`) și separatoarele
    docurilor din aplicație (`workbench/dividers.rs`, `Divider`) trec prin `Controls`;
  - redimensionarea fiecărei margini și dublu-click-ul care o readuce la mărimea implicită
    sunt spuse o singură dată, pe margine;
  - lista de proiecte nu are bare ale ei, doar sensul ei (rândul tras într-un grup), deci
    rămâne așa;
  - teste: thumb-ul sertarului, dialogul.
- **Contractul, completat** (după ce au cerut view-urile modulelor):
  - `NOWHERE`;
  - `Shown` (o parte arătată doar uneori);
  - `press_with` (⇧ / ⌘);
  - `keeps_tab` (o listă deschisă își ține Tab-ul);
  - `scroll_hovered` (rotița fără punct);
  - răspunsurile doar-citire (`cursor_at`, `tooltip_at` din ultimul hover; `is_animating`,
    `next_frame` din `settle` la desenare);
  - controale noi: `BarThumb` (o bară cu metricile ei), `ScrollThumb`, `Embedded` (un view
    în alt view), `Divider`, `DataGrid`, `MultilineField`, `TreePart` (arborele de
    fișiere), `RangePart` (alegătorul de date), `ColorPart`, `Icicle`.

  Toate au teste.
- **92d:** patru agenți au mutat view-urile modulelor în două runde, fiecare pe grupul lui
  de crate-uri, cu un test de tragere a thumb-ului la fiecare view cu bară.
  - Câmpurile cu comportament al lor devin controale ale modulului, cu exact comportamentul
    de azi, nu boolene care le rutează de mână.
  - S-au reparat pe drum bare care se vedeau dar nu se trăgeau: dialogurile git, confirm,
    preview, schema, consent, editorul de cereri.

## Făcut (2026-11-20, a patra parte: rundele finale și recenzia)

- **Contractul, încheiat** (cerut de view-uri):
  - `paste`, `char_key`;
  - `captures_keys`: o parte peste tot, cu tastele, textul și comenzile pe care nu le
    folosește trecute mai departe la partea cu focus;
  - `takes_wheel`, `in_tab_order`;
  - `Controls::{sticky, hold, wheel, scroll_focused, selected_text_with}`;
  - cursorul părții ținute în timpul tragerii;
  - Tab fără alt loc unde să meargă nu mai e consumat;
  - adaptoare noi: `Absent`, `Option<C>`, `OneClick`, `Scrolled`, `TabView`;
  - tabelul de date: Tab, caracterele, ⌘A.
- **Alegătorul de culori, un singur comportament** (`ColorPart`), în Theme Studio și în
  editor:
  - ține tastele cât e deschis, iar focusul rămâne pe partea de sub el;
  - ⇧ ← → selectează, ↵ e preluat și când culoarea e scrisă doar pe jumătate, ⌘A
    selectează câmpul;
  - spune când tastele i-au schimbat culoarea.

  `PickerPart`-ul din Theme Studio a dispărut.
- **Bug-uri reparate pe drum:**
  - lista de agenți din panoul de sesiuni își arăta căutarea, dar nu primea text; acum îl
    primește și preia rotița cât e deschisă;
  - în editorul de diagrame, un `gap` care umbrea spațiul dintre butoanele barei rapide
    împingea ultimele butoane în afara barei, unde nu mai puteau fi apăsate (test);
  - testul cozii `the_latest_job_per_key_runs` din `kernel` avea o cursă: acum așteaptă ca
    primul job să fi pornit.
- **Recenzia independentă, reparată:**
  - tooltip-urile view-urilor care răspund prin `Controls` nu mai apăreau, pentru că
    aplicația cerea tooltip-ul înaintea hover-ului care memorează punctul; acum hover-ul e
    primul;
  - un card de hover în așteptare nu mai era anulat de o apăsare sau de rotiță
    (`elsewhere` le spune și sloturilor închise; test);
  - focusul rămas pe o listă închisă nu mai înghite tastele (test);
  - cele două zone de scroll din explorer fără limită reală își țin acum limita măsurată la
    desenare;
  - ⌘C cu un meniu contextual deschis peste lista de proiecte copiază din câmpul ei, nu din
    ce e în spate.

## Făcut (2026-11-20, a cincea parte: ce rămăsese de mână)

- **Bara de filtre din log-ul git:** un `BarField` care ține tastele peste arborele de
  fișiere cât scrii în el. Când arborele caută, textul îi trece lui, iar Esc lasă focusul
  pe arbore. Teste: scrisul cu arborele focusat, meniul unui fișier în timp ce scrii,
  `filter:<tip>`, Esc.
- **Câmpurile ferestrei de surse de date:**
  - un singur `FieldPart`, în ordinea stabilă a câmpurilor driverului;
  - focusul e în `Controls`, iar Tab / ↑ ↓ / rândul de filtru îl mută prin ordinea
    ferestrei;
  - un click doar pune caret-ul; la tastare, un rând nou nu devine spațiu;
  - parola nu dă nimic la copiere sau tăiere (teste).
- **Filtrul din Task Manager:** o parte cu focus, în afara ordinii de Tab; inelul de focus
  al view-ului i-l dă și i-l ia (`focus_on`). Tastarea, lipirea, tastele de editare și
  ⌘A / ⌘C / ⌘X / ⌘Z trec prin `Controls` (teste).
- **Barele editorului de text:** `EditorBar` (bara verticală și bara orizontală) prin
  `Controls`:
  - offset-ul rămâne în pixeli întregi;
  - după eliberare, bara rămâne o clipă, apoi se estompează;
  - apăsarea pe un marcaj al barei verticale rămâne sensul editorului, decis înaintea
    rutării;
  - test: tragerea thumb-ului.
- **Contractul:** lipirea refuzată de o parte de deasupra trece mai departe la partea cu
  focus, ca textul.
- **Dialogul de parametri fără alocări la evenimente:**
  - valorile sunt o singură parte (`Values`), care știe rândul cu tastele, iar lista de
    părți e un tablou fix de două;
  - înainte, la fiecare hover, tastă sau desenare se făceau trei `Vec`-uri;
  - Tab / ↑ ↓ (în jurul capetelor), rândurile `NULL` și focusul păstrat la o apăsare alături
    rămân ca înainte (testele lor trec neschimbate).

## Făcut (2026-11-21: ultimele id-uri din codul generic)

- **Exportul din data grid:** fiecare format își declară extensiile care îl cer
  (`Extractor::names_files`):
  - JSON `json`, Markdown `md` / `markdown`, HTML `html` / `htm`, XML `xml`, tabelul text
    `txt`;
  - CSV-ul cu antet `csv`, TSV-ul cu antet `tsv` / `tab`, inserările SQL simple `sql`;
  - registrul Excel `xlsx`.

  `format_of` caută printre formatele oferite, fără listă scrisă de mână.
- **Previzualizarea iconițelor din setări:** fiecare limbaj își declară fișierul de exemplu
  și locul lui (`LanguageDescriptor::icon_sample`: PHP `index.php`, JavaScript `app.js`,
  CSS `style.css`, JSON `package.json`, Markdown `README.md`). Setările iau lista din
  registru (`SettingsModel::icon_samples`); arată aceleași cinci fișiere, în aceeași
  ordine (test).
- **Lista permisă a testului care păzește regula** are acum doar texte scrise la fel ca un
  id, dar care nu sunt unul: tasta `c`, numele iconiței terminalului, faza de desenare
  `decorations`, editorul de valori JSON al grid-ului, meniul de extractoare, dosarul și
  extensia temelor.

## Lăsat

- **Testul de alocări al Task Manager-ului** (`an_identical_repaint_allocates_nothing`) a
  picat o dată, sub încărcarea verificării complete („split at 600: 22 allocations”). Nu
  s-a mai reprodus: nici singur, nici sub încărcare, nici în două verificări complete
  după aceea. Probabil o bară care apare sau nu după cât trece între încălzire și
  măsurare. De urmărit dacă revine.

## Lăsat (înainte de partea a doua; rezolvat în ea, păstrat pentru istoric)

- **Încă în codul generic, după căutarea id-urilor de module și limbaje:**
  - `settings::HIDDEN_BY_DEFAULT` numește `tool.diagram-editor`. Devine un câmp al
    manifestului, împreună cu metadatele cerute ale modulelor (iconiță, autor, versiune,
    data actualizării).
  - Previzualizarea iconițelor din setări are fișiere de exemplu scrise de mână pe
    limbaje.
  - Exportul din data grid alege limbajul după extensie.
