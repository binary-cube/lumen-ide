# 44 — Fereastra de setări reorganizată (planul 44)

Cerut (2026-11-02): „La Setări → Editor s-au adunat multe blocuri care nu sunt setări ale
editorului; Calitatea codului amestecă serverele de limbaj cu formatoarele. Regândește-le pe
amândouă, separă-le, fă-le ușor de înțeles și de folosit — cum am refăcut Theme Studio
(planul 42).” Aprobat: „go la toate să fie bine”.

## Ce nu merge acum

- **Editor** e o pungă: salvarea automată și rândul nou de la final (două cartonașe desenate
  de mână în `settings_view/appearance.rs`), apoi, în ordinea în care se înregistrează
  modulele (`modules.rs::builtin()`), datele, extractoarele, consolele de interogări,
  profilarea, formatarea SQL, istoricul local, cursorul, informațiile în cod.
- **Aspect** ține și notificările și Task Manager-ul.
- **Calitatea codului** amestecă serverele de limbaj (motorul fiecărui limbaj, limita de
  memorie, „Adaugă un server”, serverele descărcate) cu formatorul și verificatorul
  proiectului — cu **două** rânduri separate de pastile pentru limbaj (al serverelor și al
  calității), care nu știu unul de altul. Blocurile serverelor ajung acolo doar pentru că
  modulul lsp se înregistrează înaintea celui de calitate (`after: "terminal"` + ordinea
  modulelor).
- Rândurile sunt o etichetă deasupra unor pastile, fără descriere, fără „înapoi la
  implicit”, fără carduri; fereastra nu folosește deloc `ui_components::form` (pe care
  Theme Studio îl folosește de la 42b).

## Structura nouă

Navigarea din stânga are **grupuri** (o legendă mică deasupra), paginile fiecărui grup ca
în Theme Studio (`form::nav_list`: iconiță, nume, cea aleasă pe o pastilă colorată).
Îngust (fereastra sub ~760 px sau prea joasă pentru listă): două rânduri de taburi
(`form::nav_tabs`) — grupurile, apoi paginile grupului ales.

```
┌─ ⚙ Setări ─────────────────────────────── [🔍 Caută o setare…      ] [esc] ┐
├──────────────────┬──────────────────────────────────────────────────────────┤
│ INTERFAȚĂ        │  Limbaj  [PHP] [JavaScript] [TypeScript] [CSS] [SQL] …   │
│ ◐ Aspect         │                                                          │
│ ▣ Iconițe        │  Motor                                                   │
│ 🔔 Notificări    │  Ce răspunde pentru limbaj: un server, motorul inclus…   │
│ EDITARE          │  ┌────────────────────────────────────────────────────┐  │
│ ✎ Editor         │  │ Motorul           [intelephense][Inclus][Ambele] ↺ │  │
│ 🗎 Fișiere        │  │ Ce răspunde la completare, verificări, navigare     │  │
│ {} Limbaje   ◀── │  ├────────────────────────────────────────────────────┤  │
│ ⇅ Servere de lb. │  │ intelephense 1.18.5 instalat          [Dezinstal.] │  │
│ INSTRUMENTE      │  └────────────────────────────────────────────────────┘  │
│ >_ Terminal      │  Formatare și verificare                                 │
│ ⛁ Baze de date   │  ┌────────────────────────────────────────────────────┐  │
│ ▤ Task Manager   │  │ Formator          [Niciunul][php-cs-fixer][Binar]  │  │
│ ☁ Remote         │  │ …                                                  │  │
│ 🔍 Căutare       │  └────────────────────────────────────────────────────┘  │
│ APLICAȚIE        │  Server                                                  │
│ ⌨ Scurtături     │  ┌────────────────────────────────────────────────────┐  │
│ ▦ Module         │  │ phpactor 2025.x rulează · 3 documente  [Repornește]│  │
│ 🌐 Limbă         │  └────────────────────────────────────────────────────┘  │
└──────────────────┴──────────────────────────────────────────────────────────┘
```

- **Interfață**: Aspect (temele, opțiunile temei, decorațiunile) · Iconițe · **Notificări**.
- **Editare**:
  - **Editor**: Cursor · **Informații în cod** (numele parametrilor, code lens, becul
    acțiunilor, legăturile din document — ultimele două setări noi).
  - **Fișiere** (nouă): Salvare automată, Rând nou la final (blocuri obișnuite acum), Istoric
    local.
  - **Limbaje** (nouă): **un singur** selector de limbaj sus (toate limbajele cu o alegere
    de motor și/sau formator / verificator, plus SQL), apoi cardurile limbajului ales:
    **Motor** (serverul / motorul inclus / amândouă, serverele care se instalează: instalare,
    dezinstalare, cheia de licență), **Formatare și verificare** (formatorul și verificatorul
    proiectului; la SQL și **Formatarea SQL**), **Server** (ce face serverul în proiect, cu
    Repornește).
  - **Servere de limbaj** (nouă): serverele descărcate (Descarcă tot, spațiul), apoi un
    **Avansat** pliabil: limita de memorie, oprirea celor nefolosite, jurnalul traficului,
    „Adaugă un server”.
- **Instrumente**: Terminal · **Baze de date** (nouă): Date, Console de interogări,
  Extractoare, **Avansat** pliabil cu Profilarea · **Task Manager** (pagina lui) · Remote ·
  Căutare.
- **Aplicație**: Scurtături · Module · Limbă.

## Unde ajunge fiecare bloc / rând (vechi → nou)

| Bloc (id) | Rânduri | Acum | Nou |
|---|---|---|---|
| teme (aplicația) | grila temelor, Toate / Întunecate / Luminoase, Urmează sistemul, Creează o temă | Aspect | Interfață › Aspect |
| `theme-options` | opțiunile temei folosite | Aspect | Interfață › Aspect (ordinea 10) |
| `decorations` | mod, ale mele, cu, fps, pauză, urmează sistemul | Aspect | Interfață › Aspect (20) |
| iconițe (aplicația) | stilul iconițelor, iconițele fișierelor | Iconițe | Interfață › Iconițe |
| `notifications` | poziția | Aspect | Interfață › **Notificări** |
| `caret` | sursa, forma, grosimea, clipirea, mișcarea, haloul, urma | Editor | Editare › Editor (10) |
| `code_insight` | inlay hints, code lens (+ **becul**, **legăturile**: noi) | Editor | Editare › Editor (20) |
| `autosave` (era cartonaș fix) | Salvează automat + note | Editor | Editare › **Fișiere** (10) |
| `final_newline` (era cartonaș fix) | Adaugă rândul nou + notă | Editor | Editare › Fișiere (20) |
| `history` | zile, versiuni, spațiul + Șterge | Editor | Editare › Fișiere (30) |
| `language-servers` | ~~limbajul~~ (selectorul paginii), motorul, instalare / licență | Calitatea codului | Editare › **Limbaje** › Motor (10) |
| `quality` | ~~limbajul~~, formator, verificator, căi, „citește ca”, la salvare, stare | Calitatea codului | Editare › Limbaje › Formatare și verificare (20) |
| `sql_format` | majuscule, indentare, lățime, virgule, liste | Editor | Editare › Limbaje (30), doar cu SQL ales |
| `language-server-status` (nou, din `language-servers`) | starea serverului + Repornește | Calitatea codului | Editare › Limbaje › Server (40) |
| `language-servers-parts` | componentele descărcate, Descarcă tot, spațiul | Calitatea codului | Editare › **Servere de limbaj** (10) |
| `language-servers-limits` (nou, din `language-servers`) | memoria, inactiv, trafic, notele despre pachet | Calitatea codului | Servere de limbaj › Avansat (20) |
| `language-servers-add` | nume, comandă, limbaje, argumente, Adaugă | Calitatea codului | Servere de limbaj › Avansat (30) |
| `terminal-shell`, `terminal-display` | neschimbate | Terminal (după Editor) | Instrumente › Terminal |
| `database` | pagina, închiderea, eșantionul CSV, confirmarea | Editor | Instrumente › **Baze de date** (10) |
| `database-console` | limita, oprire la eroare, memorie, parametri, istoric, tabele, index, indicii | Editor | Baze de date (20) |
| `extractors` | coloanele calculate / generate | Editor | Baze de date (30) |
| `database-console-profile` | buffere, timp, verbose, statistici, timeout | Editor | Baze de date › Avansat (40) |
| `tasks` | istoricul, intervalul | Aspect | Instrumente › **Task Manager** |
| `remote` | urcări, descărcări, reîncercări | Remote (după Editor) | Instrumente › Remote |
| `semantic` | căutarea semantică | Căutare | Instrumente › Căutare |
| scurtături, module + cadre, limbă (aplicația) | neschimbate | — | Aplicație › Scurtături / Module / Limbă |

Nimic nu se pierde: toate setările rămân, cu aceleași secțiuni și chei salvate (nicio
migrare). Pagina „Calitatea codului” dispare (blocurile ei sunt în Limbaje și Servere de
limbaj); id-urile paginilor nu se salvează nicăieri.

## Decizii

- **Paginile le declară proprietarii**: `SettingsContribution::page()` (id-ul paginii),
  `page_info()` (`SettingsPage { id, label, icon, group, order }`: pagina pe care o descrie,
  a lui — Terminal, Remote, Task Manager — sau una împărțită, din constructorii comuni din
  `workbench-model`: `SettingsPage::languages()`, `language_servers()`, `database()`…),
  `order()` (locul blocului pe pagină), `advanced()` (sub „Avansat”). Aplicația sortează
  blocurile după (pagină, ordine, id), navigarea după (grup, ordine, id): **ordinea de
  înregistrare a modulelor nu mai contează** (un test le amestecă). Paginile fixe ale
  aplicației (Aspect, Iconițe, Scurtături, Module, Limbă) sunt ale ferestrei; restul există
  doar cât au blocuri — un modul oprit își ia pagina cu el.
- **Grupurile**: `SettingsGroup { Interface, Editing, Tools, Application }` în
  `workbench-model`, ordinea lor e a enumerării.
- **Limbajul ales e al paginii**: fiecare bloc al paginii Limbaje spune ce limbaje are
  (`languages(scope)`: lsp-ul ale catalogului, calitatea ale uneltelor proiectului) sau că e
  doar al unui limbaj (`language()`: Formatarea SQL); aplicația le reunește (fără dubluri, o
  ordine fixă: PHP, JavaScript, TypeScript, … apoi după nume), ține limbajul ales
  (`Workbench::settings_language`) și îl dă tuturor în `SettingsScope::language`. Selectorul
  e desenat de fereastră, o dată, sus. Modulele nu depind unul de altul. Căutarea găsește și
  blocurile altui limbaj (SQL): deschiderea rezultatului alege limbajul.
- **Rânduri ca în Theme Studio**: fiecare bloc e un titlu (cu o descriere) și un card
  (`form::group_quad`, `row_divider` între rânduri); un rând e `form::setting_row` (eticheta,
  o descriere scurtă, controlul la dreapta — dedesubt când nu încape —, „↺” cât diferă de
  implicit). `SettingsRow` primește `detail` (toate rândurile cu etichetă) și `default`
  (alegeri, segmente, glisoare, comutatoare — din `Setting<T>::default_value()`); „↺” e un
  `Choose` cu valoarea implicită. Glisoarele devin `form::value_slider` (valoarea în cuvinte
  la dreapta). Starea / acțiunile: textul pe până la două rânduri, butoanele la dreapta.
  Notele de la final: sub card, mărunt.
- **Avansat**: blocurile `advanced()` ale unei pagini stau sub un `form::fold_section`
  „Avansat” la sfârșitul paginii, închis implicit (starea ține cât e deschisă fereastra);
  căutarea care duce acolo îl deschide.
- **Căutarea** găsește fiecare rând (eticheta și descrierea lui), nu doar blocul; rezultatul
  duce la pagina lui, deschide „Avansat” dacă trebuie, alege limbajul dacă e al altuia și
  luminează rândul.
- **Căutarea în antet**: câmpul de căutare trece în antetul ferestrei (dreapta, lângă esc),
  ca să încapă navigarea cu grupuri; fereastra de setări e puțin mai mare (920 × 720, cât
  încape) și fără subsolul cu taste (esc e în antet, ↑ ↓ merg ca înainte).
- **Becul și legăturile**: `[editor] action_bulb` și `[editor] document_links` (pornite),
  în „Informații în cod”; oprit, becul nu se mai arată (⌥⏎ merge), iar ⌘-click nu mai urmează
  legăturile documentului (declarațiile da).
- Performanță: modelul paginii se face ca acum — când se deschide fereastra, după o alegere,
  când un bloc spune că s-a schimbat (`poll`) —, nu pe cadru; rândurile se desenează ca
  înainte.

## Pașii

1. **44a — modelul, grupurile, paginile**: `SettingsGroup`, `SettingsPage` cu id / grup /
   ordine, `page()` / `page_info()` / `order()` / `advanced()` în contribuții, blocurile
   sortate, navigarea cu grupuri (`Modal`: legende, `nav_list`, taburi când e îngust),
   fiecare bloc pe pagina lui nouă (Notificări, Fișiere, Limbaje, Servere de limbaj, Baze de
   date, Task Manager). Test: ordinea nu depinde de înregistrare; un modul oprit își ia
   pagina.
2. **44b — rândurile pe `form`**: `detail` și `default` în `SettingsRow`, descrieri pentru
   toate rândurile (ro / en), cardurile, `setting_row`, `value_slider`, „↺”.
3. **44c — paginile noi**: salvarea automată și rândul nou ca blocuri, selectorul comun de
   limbaj, `language-servers` împărțit (Motor / Server / limite), „Avansat”, becul și
   legăturile.
4. **44d — căutare, pliere, implicit, snapshot-uri**: căutarea pe rânduri (și în „Avansat”,
   și a altui limbaj), testele, snapshot-urile fiecărei pagini noi și unul îngust.

## Teste

- Paginile și ordinea blocurilor nu depind de ordinea înregistrării (amestecate de mai multe
  ori: aceeași navigare, aceleași blocuri în aceeași ordine).
- Limbajul ales e același pentru toate blocurile paginii; schimbat, toate îl văd.
- „↺” trimite valoarea implicită; fără `default` nu apare.
- Căutarea găsește un rând din „Avansat” și îl deschide; un rând al altui limbaj alege
  limbajul.
- Un modul oprit: pagina lui dispare (Baze de date fără modulele bazei de date).
- Toate setările declarate înainte sunt încă pe o pagină (un test enumeră blocurile și
  rândurile).

## Stare

### 44a–44c — făcute (un singur commit)

Pașii 44a, 44b și 44c s-au făcut împreună (un commit: modelul, desenarea și paginile noi
depind unul de altul — `SettingsRow` cu `detail` / `default` atinge aceleași ~100 de locuri ca
mutarea blocurilor, iar fără git stash nu se puteau despărți curat). 44d (căutarea pe rânduri,
„Avansat”, „↺”) e și el în cod deja; commit-ul lui are snapshot-urile și lustruirea.

- **Model** (`workbench-model::settings_page`): `SettingsGroup` (Interfață, Editare,
  Instrumente, Aplicație: icon, cheia numelui), `SettingsPage { id, label, icon, group,
  order }` cu paginile împărțite ca constante (`EDITOR`, `FILES`, `LANGUAGES`,
  `LANGUAGE_SERVERS`, `DATABASE`); contribuțiile au `page()` (fostul `section()`),
  `page_info()`, `order()`, `advanced()`, `languages(scope)`, `language()`;
  `SettingsScope::language`; `SettingsLanguage` + `merge_languages` (o dată fiecare, ordinea
  fixă PHP, JavaScript, TypeScript, TSX, Vue, HTML, CSS, SCSS, Less, JSON, YAML, SQL, Markdown,
  apoi după nume). `SettingsRow` are `detail` pe toate rândurile cu etichetă și `default` pe
  alegeri / segmente / glisoare / comutatoare (din `Setting::default_value()`);
  `SettingsRow::searchable()`.
- **Ordinea**: `ui_workbench::order_blocks` (pagină, avansat, ordine, id) — aplicația nu mai
  depinde de `builtin()`; `theme-options` nu mai e `insert(0, …)`, are ordinea 10.
- **Navigarea** (`Modal`): legende de grup (`form::nav_caption`), rândurile cu
  `form::nav_list_sized` (28 px, ca să încapă 15 pagini la 700 px), taburi pe două rânduri
  (grupurile, paginile grupului) sub 760 px sau când lista nu încape; căutarea în antet
  (`ModalSpec::header_field`, titlul tăiat înaintea lui), fără subsol, `ModalSize::XLarge`
  (920 × 720). Doar setările foloseau secțiunile modalului: celelalte ferestre neschimbate.
- **Rândurile** (`settings_view/rows.rs`, rescris): titlul blocului + card
  (`form::group_quad`, `row_divider`), `form::setting_row` pentru fiecare rând, pastilele la
  dreapta sau sub etichetă pe câte rânduri trebuie, segmentele (pastile dacă nu încap),
  `form::value_slider` cu valoarea la dreapta, comutatorul (tot rândul se apasă), câmpurile
  text / secrete, starea pe până la trei rânduri cu butoanele la dreapta, notele de la final
  sub card. „↺” e `SettingsAction::Reset` (aplicația îl tratează ca `Choose`).
- **Pagini noi / mutări**: Notificări (Interfață), Fișiere (blocurile aplicației `autosave` /
  `final_newline` în `file_settings.rs` — cartonașele fixe și `ToggleAutosave` /
  `ToggleFinalNewline` au dispărut, aceleași chei `[editor]`; plus istoricul local), Limbaje
  (selectorul comun desenat de fereastră, `Workbench::settings_language`,
  `select_settings_language`), Servere de limbaj, Baze de date (Profilarea sub „Avansat”), Task
  Manager (pagina lui, Instrumente). Blocul `language-servers` s-a împărțit: „Motor” (alegerea
  + instalarea / licența), `language-server-status` („Server”, cu Repornește) și
  `language-servers-limits` („Limite și jurnal”, sub „Avansat”, cu notele despre pachet).
  Calitatea și serverele nu mai au rândul lor de limbaj (nici starea internă
  `show_language` / `SHOWN`); Formatarea SQL e blocul limbajului `sql`.
- **Becul și legăturile**: `[editor] action_bulb`, `[editor] document_links` (pornite) în
  „Informații în cod”; `Workbench::bulb` și `document_link_at` le ascultă.
- **Descrieri**: fiecare rând cu etichetă are una (ro / en); unele etichete lungi au fost
  scurtate, explicația trecând în descriere (consola, profilarea, datele, terminalul, lsp).
  Opțiunile temelor nu au descriere (numele vin din fișierul temei).
- **Teste**: amestecarea blocurilor (navigare și ordine identice), grupurile (late și
  înguste: fiecare pagină atinsă), „↺”, selectorul de limbaj și blocul SQL, căutarea în
  „Avansat” și a altui limbaj, `merge_languages`, ordinea paginilor; în aplicație: toate
  setările de dinainte încă pe pagina lor (o listă bloc → rânduri), limbajul comun (PHP, SQL,
  unul necunoscut), un modul oprit își ia pagina (Baze de date); testele modulelor trecute pe
  `scope.language`.

### 44d — făcut

- Căutarea, „Avansat”, „↺” și limbajul cerut de un rezultat erau deja în commit-ul 44a–44c;
  pasul acesta le-a verificat pe snapshot-uri și a reparat ce s-a văzut: selectorul de limbaj
  își măsura rândurile de pastile mai îngust decât le desena (un rând gol în card); textul
  unui câmp (valoarea și indiciul) e tăiat la marginea câmpului; detaliile comutatoarelor
  de la Fișiere nu mai repetă notele de sub card; nota despre servere spune „mai sus”;
  `SETTINGS_ADVANCED` derulează la sfârșit printr-un semn (`scroll_to_end`), nu printr-un
  scroll uriaș (randorul software dădea overflow).
- Snapshot-uri noi (ambele teme): `settings-page-files`, `-languages-php`, `-languages-sql`,
  `-language-servers` (Avansat deschis), `-database` (Avansat), `-tasks`, `-notifications`,
  `settings-narrow` (760 × 620, taburile), `settings-10` … `settings-14`. Toate `settings-*`
  și `settings-query` se schimbă (așteptat). În afara ferestrei de setări, comparația cu
  referința arată doar: `console*`, `sql-file-attached`, `services-output` / `-result` —
  din commit-ul 4acdb264 (bara instrucțiunii curente a consolei, făcut între timp pe
  ramură, nu de pasul acesta) — și `task-manager*` (cifra memoriei măsurate pe viu, ca
  la 43j3).
- Ce n-a intrat: paginile fixe ale aplicației (temele, iconițele, scurtăturile, modulele,
  limba) își păstrează desenul (galerii, liste), doar navigarea și antetul s-au schimbat;
  opțiunile temelor nu au descrieri (n-au de unde: fișierul temei dă doar numele).
