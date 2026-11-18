# 29 — Review: securitate, corectitudine, performanță, curățenie

Al doilea review după planul 28 (DDD, decuplare, organizare, monitorizare, plus securitate
și performanță). Fiecare constatare are fișierul și linia; cele marcate ✔ sunt verificate în
cod.

## Rezolvat deja

- **Esc din Settings făcea scroll în loc să închidă**: rândurile de scurtături ajunse prin
  scroll sub antet își păstrau zona de click, iar click-ul pe „esc” deschidea rândul ascuns.
  Acum cadrul modalului (esc, secțiunile) e verificat primul, iar listele conținutului
  răspund doar în zona de conținut vizibilă (click, hover, cursorul de mână). Am adăugat un
  test.

## 29a — Securitate (prioritate maximă)

1. ✔ **Comenzi rulate din `<project dir>/project.toml` fără încredere**:
   `quality/src/settings.rs:25-32` → `quality/src/plan.rs:73-104` → `runner.rs:211`
   (`sh -c` / `cmd /C`). Un repo clonat care aduce acest fișier rulează comenzi la Save.
   Propunere: comenzile (`command`, `binary`) se citesc doar din setările locale ale
   utilizatorului pentru proiect (nu din fișierul versionat). Altfel, se cere confirmare
   o dată per proiect și per comandă; amprenta comenzii aprobate e păstrată local.
2. ✔ **Injecție prin numele fișierului** (`plan.rs:132` `quoted`): ghilimelele duble lasă
   `$(…)`, `` ` `` și `$VAR` să fie expandate. Pe Unix: ghilimele simple (`'` → `'\''`); pe
   Windows: escape pentru `cmd` (`^`, `%`). Mai bine: fișierul ca argument separat când
   linia se poate împărți.
3. ✔ **Path traversal la sincronizarea remote** (`fs-local/src/remote_copy.rs:26`): numele
   de pe server cu `..`, `\` sau litere de unitate ies din proiect. Respingem componentele
   `..`, căile absolute, `\` și `:` pe Windows. Aceeași regulă în listările SFTP/FTP.
4. ✔ **Folder temporar comun pentru promptul terminalului** (`pty/src/lib.rs:211`):
   `temp_dir()/terminal-prompt` e comun tuturor utilizatorilor pe Linux. Mutat în folderul
   de date al utilizatorului (sau un temp per utilizator, 0700, cu verificarea
   proprietarului).
5. **Credențiale**: `ProjectVault::lines` ignoră erorile de citire, iar un `set` ulterior
   rescrie seiful doar cu intrarea nouă, pierzând celelalte parole. Am mai găsit:
   - `zeroize` lipsește pentru parole;
   - Postgres pornește cu `Tls::Off` și ignoră `sslmode`;
   - `restrict()` pe Windows nu face nimic (fișierul e totuși criptat).

## 29b — Pierderi de date și crash-uri

1. ✔ **`.editorconfig` cu `indent_size = 0`** → împărțire la zero la Tab
   (`settings/src/editorconfig.rs:70`, `document/.../commands.rs:441`). Valoarea trebuie
   limitată la ≥ 1.
2. ✔ **Slice în mijlocul unui caracter** (`fw-react/src/lib.rs:175`): scrierea `usă` în
   JS → panic.
3. **Bucle de symlink-uri** (`fs-local/src/lib.rs:105-145`): `metadata` urmează link-ul,
   deci recursivitatea poate să nu se oprească. Folosim `symlink_metadata`.
4. ✔ **Descărcări paralele cu același fișier temporar** (`remote_copy.rs:70`): `a.php` și
   `a.js` ajung amândouă la `a.download-tmp`. Folosim `files::temporary`.
5. **Planul de sincronizare aplicat după modificări** (`remote/application.rs:482`): o
   salvare făcută după plan e suprascrisă. Verificăm `stat` față de planificare înainte de
   fiecare transfer.
6. **Folderul proiectului (`<project dir>`) poate fi sincronizat în Mirror** dacă
   utilizatorul îl scoate din excluderi. Trebuie exclus mereu, ca la On-demand.
7. **Upload SFTP**: dacă `rename` eșuează, fișierul de pe server e șters înainte de a doua
   încercare. Numele `.upload-tmp` e fix.
8. **Save fără verificarea discului** chiar înainte de scriere (doar flag-ul watcher-ului).
   Mai trebuie verificat `mtime`/dimensiunea, și temp-ul de salvare trebuie să fie unic.

## 29c — Performanță

1. **Schimbări multe pe disc** (checkout, `composer install`): fiecare cale re-listează
   folderul părinte pe firul UI, de trei ori (`app/workbench/tree.rs:28`,
   `project-tree/panel.rs:246`). Grupăm pe foldere distincte, listăm pe un fir de lucru, iar
   peste o limită facem rescan.
2. **Căutarea din arbore**: `q.paths()` pe firul UI la fiecare tastă. Înainte de indexare
   parcurge tot proiectul (`app/queries.rs:48`). Potrivirea trebuie mutată pe un worker,
   cu debounce.
3. **Meniul de click dreapta**: ~10 verificări `enabled` citesc fiecare fișierul de pe disc
   (`menus.rs:324`, `host.rs:20`). Păstrăm textul cât timp meniul e deschis.
4. **Fișier deschis, modificat din afară**: e recitit integral pe firul UI, chiar și după
   propriul Save (`tree.rs:60`). Comparăm întâi dimensiunea, `mtime` și hash-ul propriei
   salvări.
5. **Settings deschis**: orice wake reconstruiește tot `SettingsModel`, cu toate cele 22 de
   palete de teme (`app/events.rs:196`).
6. **Bara de stare se reconstruiește în fiecare cadru**, inclusiv `home_dir()` și
   numărarea selecției pe fișiere mari (`status.rs`). O memorăm pe o cheie.
7. **`Tab::text()`**: o copie a documentului per tab care nu se eliberează
   (`tab.rs:8`). Trebuie inclusă în `release_unseen_syntax` și `Purge`.
8. **Arborele proiectului**: nodurile șterse rămân în arenă (`panel.rs:256`).
9. **Sesiunea se scrie sincron din `paint()`** (`session.rs:46`). La fel, `reload()`-ul
   setărilor la focus.
10. **Citiri pe firul UI**: local-history `view.rs:61`, `vcs-git` `changes_view.rs:84`,
    `move_view.rs:118` (`exists()` în paint).
11. **Spinner-ele de încărcare redesenează la rata ecranului**; le limităm la ~20 fps.

## 29d — Monitorizare

1. Firele `process-input` / `process-output` sunt atribuite modulului quality, deși le
   folosesc și git și lang-php.
2. `embed` (modelul semantic) descarcă din rețea, dar traficul și RAM-ul nu sunt numărate.
3. Firul `instance` nu e atribuit niciunui rând.
4. `warm()` din lang-php (`php -r`) și `module-preload` sunt puse pe seama app-ului.
5. vcs-git nu are `ModuleCounters` (nici disc, nici fetch/push). local-history `view.rs`
   citește fără să numere.
6. Fără `Module::memory`: cache-urile vcs-git, scrollback-ul terminalului, gramaticile.

## 29e — DDD și decuplare (rămășițe)

1. **Lista de extensii căutate e fixă în indexer** (`infra/indexer/src/search.rs:16`); ar
   trebui să vină din limbajele modulelor.
2. **Cunoștințe PHP în motorul generic de completare** (`intelligence/src/engine.rs`):
   `NO_SPACE`, `CALL_LIKE`, `strip_promotion`, `new X(`, `signature_plainly`. Din
   `docs.rs` și `usages.rs`: `DECLARES_TYPE`. Toate devin hook-uri ale limbajului.
3. **CSS în domeniu și app**: `intelligence/src/css.rs`, `is_css_kind` în app.
4. **App-ul are cod specific unor limbaje sau module**:
   - `"/**"` + Enter (`editing.rs:392`);
   - `unclosed_tag` (`indenting.rs:68`);
   - `'$'` în `links.rs:299`;
   - Theme Studio (`viewers.rs:383`, `theme.rs:204`).
5. **`Overlay::LevelPicker`** e cod de app, deși ține de limbaj.
6. **Setările quality sunt citite cu chei text dinamice**, nu cu `Setting<T>`.
7. **Numerotarea terminalelor („Local (2)”) e globală** între proiecte.
8. **Texte UI scrise în cod**:
   - `"Untitled"` (`tab.rs:110`);
   - `"Local"` (terminal);
   - `"Auto"` (level picker);
   - `"Not formatted"` (lang-php);
   - `"⌘ click"` (`paint.rs:495`), în timp ce cheile `link.click*` există, nefolosite.

## 29f — Curățenie

- **Chei de traducere nefolosite**:
  - `status.words`, `error.more`;
  - `canvas.used_by|empty|open`;
  - `compare.title|result|saved|with|with_active`.
- **Cod public folosit doar de teste**: `BUNDLE_ID` (nefolosit deloc), `describe_layers`,
  `Keymap::from_specs` etc.
- **Helperi duplicați**:
  - granița de caracter (5 copii) → `kernel`;
  - numele fișierului dintr-o cale (4 copii);
  - `is_name` (CSS);
  - calea relativă cu `/`;
  - `hex` cu alfa;
  - `wrap_message`;
  - `short(fqn)`.
- **Teste**: `vcs-git/tests/unit/fake.rs` nu oglindește niciun fișier din `src`.
  `lsp-client` și `platform-winit` nu au teste.

## Ordinea propusă

29a → 29b → 29c (1–4 întâi) → 29d → 29e → 29f, câte un commit per etapă, fiecare după
`check.sh` și capturi identice.

## Stare (făcut)

**Ferestrele cu scroll**: în Settings și în celelalte 10 locuri cu liste derulante,
zonele de click ale rândurilor se taie la partea vizibilă a listei. Locurile:
- log-ul Git, ramurile și Changes;
- Compare, Task Manager, Local History, Theme Studio;
- marginea editorului;
- Remote (review, conexiune, „To upload”).

**29a — securitate**:
- Comenzile, binarele și uneltele alese în `project.toml` rulează doar după aprobarea
  utilizatorului:
  - ce alegi în Settings e aprobat automat;
  - ce vine cu proiectul cere „Permite”, cu confirmare;
  - lista aprobărilor stă în folderul de date al proiectului, în afara repo-ului.
- `{file}` ajunge la shell prin variabila `EDITOR_FILE`, deci numele fișierului nu e
  interpretat niciodată.
- Numele de la server care ar ieși din folder (`..`, `\`, `:`, caractere de control) sunt
  ignorate la listare și refuzate la scriere.
- Promptul terminalului stă în cache-ul utilizatorului, nu în `/tmp`.
- Seiful de parole care nu poate fi citit nu mai e suprascris, iar o citire nu creează o
  cheie nouă.

**29b — date și crash-uri**:
- `indent_size = 0` e ignorat;
- slice-ul din fw-react merge pe caractere;
- link-urile simbolice nu mai sunt urmate la copiere și măsurare și rămân link-uri la
  salvare;
- descărcările și salvările au nume temporare unice;
- planul de sincronizare verifică fișierul local înainte să-l suprascrie sau să-l șteargă,
  iar pe server verifică doar înainte de ștergere;
- `<project dir>` e exclus mereu de la sincronizare;
- upload-ul SFTP/FTP mută întâi fișierul vechi deoparte și îl pune la loc dacă
  înlocuirea eșuează;
- autosave-ul verifică dimensiunea și `mtime` chiar înainte de scriere.

**29c — performanță**:
- un lot de schimbări pe disc recitește fiecare folder o singură dată, iar copiii sunt
  găsiți prin hash map;
- căutarea din arbore rulează pe firul ei („tree-search”);
- meniul contextual citește fișierul o singură dată;
- un fișier cu aceeași stare pe disc nu e recitit, iar textul e comparat pe bucăți;
- Settings deschis actualizează doar starea modelului semantic;
- bara de stare: poziția cursorului e memorată, iar `home_dir` e citit o singură dată;
- textul taburilor nevăzute e eliberat;
- sesiunea se scrie pe firul „session-save”, așteptat la închidere;
- `exists()` din fereastra de mutare e memorat;
- loaderele (log git, Remote, canvas) redesenează la ~20 fps prin `Viewer::is_loading`.

**29d — monitorizare**:
- firele proceselor externe poartă numele celui care le pornește: `git-*`, `quality-*`,
  `php-*`;
- descărcarea modelului semantic e numărată la trafic;
- firele `instance` și `session-save` sunt atribuite interfeței;
- disk-ul citit de Local History și de Git e numărat;
- memoria terminalului (ecran și istoric) e raportată.

**29e — DDD**:
- tabelele de cuvinte cheie PHP/JS au devenit `LanguageIntelligence::keyword_insert`;
- `new` și modificatorii parametrilor au trecut în `Naming` (`construct`,
  `parameter_modifiers`);
- terminalele sunt numerotate per proiect;
- textele „Untitled”, „Local”, „Auto”, „Not formatted” și „⌘ click” vin din traduceri.

**29f — curățenie**:
- cheile de traducere nefolosite au fost șterse;
- cele patru copii ale helperului de graniță de caracter au fost înlocuite cu
  `str::floor_char_boundary` din std;
- submodulul `workbench/file_icons` a devenit `tab_icons`, pentru că avea același nume ca
  modulul `file-icons` și check-deps îl lua drept dependență interzisă.

## Rămas (cu motiv)

**Securitate:**
- Postgres e implicit `Tls::Off` și ignoră `sslmode`. Driverul nu e încă legat de nicio
  fereastră; se decide odată cu ea.
- Lipsește `zeroize` pentru parole.
- Pe Windows, `restrict()` nu face nimic, dar seiful e criptat.

**Performanță:**
- Nodurile șterse din arborele proiectului rămân în arenă. Refolosirea lor ar cere ca
  selecția și intrarea nouă să nu mai țină `NodeId` vechi.
- `reload()` al setărilor la focus rulează încă pe firul UI.

**Monitorizare:**
- `warm()` din lang-php și `module-preload` rulează pe firele app-ului.
- Memoria cache-urilor git și a gramaticilor nu e raportată.

**DDD:**
- `css.rs` e încă în domeniu, iar app-ul folosește încă `is_css_kind`.
- Stub-ul `/**` + Enter e încă în app. Mutat pe limbaj, ar dispărea din `<script>` în HTML.
- `unclosed_tag` și `'$'` din `links.rs` sunt încă în app.
- `Overlay::LevelPicker` e încă cod de app; ar trebui să devină view-ul unui modul.
- Setările quality sunt secțiuni dinamice per limbaj, deci nu se pot declara ca
  `Setting<T>`.
- Lista de extensii a căutării text rămâne fixă, decizie acceptată în planul 28.

**Curățenie:**
- Helperii mici duplicați: numele fișierului dintr-o cale, `is_name`, `hex`,
  `wrap_message`, `short`.
- `BUNDLE_ID` e păstrat ca parte din brand.
