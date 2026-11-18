# 48 — Lucrurile mici

Cerut (2026-11-03): șase lucruri mici rămase, fiecare cu commit-ul lui, pe branch-ul `lsp`,
în ordinea de mai jos. Regulile obișnuite: nimic greu pe firul UI, modulele prin
`ui-components`, testele izolate (niciodată `~/.config`, keychain-ul sau folderul de date
real), Windows.

## 48a — „Golește istoricul” în consola SQL

- Istoricul unei surse (`console/src/history.rs`, `Histories::clear`) știa deja să se
  golească și să-și șteargă fișierul (`TomlQueryHistory::rewrite` cu nimic: fișierul șters),
  dar nimic din UI nu-l folosea.
- În fereastra istoricului (`history_view.rs`, modala `view-console-history`): un buton
  „Golește istoricul…” în subsol, lângă „Șterge”, și același rând în meniul de click dreapta.
  Golirea nu se poate anula, deci e întrebată **în aceeași modală**: subsolul devine
  „Se șterg toate cele N interogări ale sursei X.” cu „Renunță” și „Golește” (roșu); Esc
  renunță întâi la întrebare (a doua oară închide modala), ↵ nu golește niciodată din greșeală
  (butonul cu tastele e „Renunță”). O modală peste modală nu e nevoie.
- Golit: lista goală (starea ei goală obișnuită), fișierul sursei șters în fundal; alte
  console ale aceleiași surse văd lista goală (generația istoricului).

## 48b — Literalii enum-urilor în completarea SQL

- Unde se așteaptă valoarea unei coloane cunoscute: `col = |`, `col <> |`, `col != |`,
  `col IN (|`, `col IN ('a', |`, `SET col = |`, `VALUES (…, |)` (coloana după poziția ei din
  lista `INSERT INTO t (a, b)`, sau din ordinea tabelei fără listă) și în ghilimeaua deschisă
  (`col = 'a|`).
- **Domeniul** (`database::sql::scope`): `QueryScope.value_of` — coloana (cu calificatorul ei
  sau tabela din `INSERT` / `UPDATE`) a cărei valoare e la cursor; `in_string` când cursorul
  e într-un literal deschis (`'a|`), ca literalii să se ofere fără ghilimele duble.
- **Valorile**: MySQL le are în tip (`enum('S','M','L')`: citite din `type_name`, cu `''`
  dublate înțelese); PostgreSQL le are în `pg_enum`: interogarea coloanelor (`catalog_columns`)
  aduce și etichetele (`string_agg(quote_literal(enumlabel), ',' ORDER BY enumsortorder)`,
  9.2+) într-un câmp nou `ColumnBrief.values` (DTO-ul protocolului cu `serde(default)`, deci
  cache-urile vechi se citesc), păstrat în `IndexColumn.values`.
- **Completarea** (`database::completion`): literalii coloanei, în ordinea declarată, grupul
  cel mai de sus, inserați cu ghilimele (`'active'`; în ghilimeaua deschisă: fără cea de
  început și cu cea de sfârșit), detaliul `enum · coloana`.

## 48c — Cardul parametrilor pentru funcțiile SQL

- Cardul existent (`ParamHintCard`, cel din PHP): numele, parametrii, cel scris acum
  evidențiat; deschis la `(` / `,` și la acceptarea unei funcții, urmărește cursorul.
- **Semnătura**: din indexul sursei (funcțiile ei și ale motorului, cu argumentele lor:
  PostgreSQL, MySQL), altfel din lista proprie a funcțiilor cunoscute (`BUILTINS`, mutată
  într-o funcție a domeniului `database::completion::builtin_signature`, completată cu cele
  ale MySQL-ului).
- **Unde**: un cârlig nou `DocumentAssist::signature` (implicit nimic): consola și fișierele
  SQL atașate răspund din sursa lor; aplicația îl întreabă întâi în `param_hint_card`. Un
  fișier SQL neatașat are aceleași funcții după dialectul lui: un cârlig nou al limbajului,
  `LanguageIntelligence::call_signature(path, name)` (nu `hover`: acela ar aduce și carduri
  de hover și indicii de parametri peste literali, nedorite în SQL).
- `enclosing_call_in(…, dashes)`: în SQL (limbajul are comentarii `--` în `code_style`)
  căutarea apelului sare peste `--` (`-- the customers' names` nu mai deschide un șir);
  `enclosing_call` rămâne fără (în JavaScript `i--` e cod).

## 48d — Confirmarea din Task Manager, o modală adevărată

- Azi dialogul e desenat de vederea Task Manager-ului peste ea însăși (`paint_confirm`): nu e
  pe stiva modalelor, deci nici „nimic din spate nu primește tastele” al aplicației.
- Acum: o modală a modulului (`view-task-confirm`, `host.open_modal`): titlul („Oprești X?”),
  ce se pierde, „Renunță” și acțiunea (roșu); se închide doar prin ×, Esc sau butoanele ei;
  ↵ apasă butonul cu tastele („Renunță” implicit), Tab / ← / → îl mută.
- Fluxul: vederea pune întrebarea (cu o identitate UUID v4) și cere comanda internă
  `tasks.ask`, care deschide modala; răspunsul „da” rămâne la modul sub identitatea ei, iar
  comanda `tasks.answered` trimite vederii `confirmed`; vederea care a întrebat (și numai ea)
  face acțiunea. Închisă altfel, nimic nu se face.
- Dispar din vedere: `Confirm`, `paint_confirm`, `Hit::Dialog / ConfirmYes / ConfirmNo`.

## 48e — Contrastul textului estompat în 9 teme

- Măsurat (WCAG) `text.muted` pe un card (fundalul editorului împins 3% spre text, cum își
  desenează vederile cardurile): sub 4,5:1 erau `arctic` 3,72, `fjord` 4,29, `latte` 3,35,
  `mint` 3,86, `neon` 4,31, `paper` 4,29, `sakura` 3,96, `solar-dark` 4,41, `solar-light`
  2,82.
- Corectat doar `muted`-ul paletei (aceeași nuanță și saturație, luminozitatea mutată până la
  ~4,6:1): aspectul temei rămâne.
- Testul: fiecare temă inclusă are textul estompat ≥ 4,5:1 pe card și ≥ 3:1 pe fiecare
  suprafață (fereastra, panourile, popup-urile, editorul).

## 48f — Testele aplicației nu mai citesc setările reale

- `config::settings()` sub `cfg!(test)`: un serviciu pe fir, cu un fișier al lui (ca
  `recent.rs`); `config_dir()` / `data_dir()`: un folder temporar al procesului de test —
  și pentru module: `storage::isolate(root)` (setat o dată) face ca `storage::config_dir` /
  `data_dir` să fie `root/config` / `root/data`, chemat de aplicație sub `cfg!(test)` din
  `config`, `modules::load` și `Workbench::new`. Niciun test nu mai citește sau scrie
  `~/.config/<app>/` sau folderul de date.
- Verificat: cele 43 de teste `workbench::servers` trec fără `HOME` izolat.

## Stare

- [x] 48a — „Golește istoricul” (2026-11-03): butonul din subsolul ferestrei istoricului și
  rândul din meniul ei; întrebarea în subsol („Renunță” are tastele, Esc renunță la ea, Tab /
  ← / → mută), golit = lista goală, fișierul sursei șters. Test: întrebat, păstrat (↵, Esc),
  golit cu fișierul. Capturi: `console-history` (butonul nou), `console-history-clear` (nouă).
- [x] 48b — literalii enum (2026-11-03): `database::sql::value` (`ValueOf`: coloana comparată,
  setată, din `IN (…)` sau din valorile unui `INSERT` — după listă sau după ordinea tabelei;
  literalul deschis; `enum_values` pentru tipul MySQL), `QueryScope.value_of` și
  `Expect::Literal` (în literal: domeniul de la începutul lui, literalul înlocuit întreg),
  `SuggestionKind::Value` (grupul 0, inserat cu ghilimele, detaliul `tabela.coloana`);
  `ColumnBrief.values` / `IndexColumn.values` (PostgreSQL: etichetele din `pg_enum` în
  `catalog_columns`, separate prin U+001F, 9.2+); `'` deschide completarea în SQL.
  - **Abatere:** etichetele PostgreSQL vin doar din citirea în bloc a catalogului; coloanele
    unei scheme uriașe citite pe rând (al doilea nivel, din structura tabelei) au doar tipul
    (MySQL-ul le are oricum, în tip). Fără captură: baza de exemplu a capturilor e SQLite
    (fără enum-uri).
  - **Teste:** `sql::value` (comparat / setat / listă / `INSERT` / literal / tipul MySQL),
    completarea (MySQL, PostgreSQL, în listă, în `INSERT`, în literal, nimic pe o coloană
    obișnuită), matricea pe serverele noastre `editor-test-*` (pg92, pg17, mysql57,
    mariadb118): `o.status = ‸` dă `'new'`, `'paid'`, `'shipped'`, `'cancelled'`.
- [x] 48c — cardul parametrilor SQL (2026-11-03): `DocumentAssist::signature` (implicit
  nimic), cerut întâi de `param_hint_card`; consola (și fișierele atașate) răspunde prin
  `database::completion::signature` — rutina schemei cu argumentele ei (PostgreSQL, MySQL),
  altfel `builtin_signature` (lista `BUILTINS`, acum și cu funcțiile MySQL-ului:
  `date_format`, `ifnull`, `if`, `datediff`…, plus câteva PostgreSQL), altfel aritatea;
  `SqlIntelligence::call_signature` dă semnătura unei funcții cunoscute după dialectul
  fișierului, deci și cardul într-un fișier SQL neatașat. Teste: semnăturile (a schemei,
  cunoscute, după dialect), `call_signature` (fără hover), aplicația (consola: `substr(` → cardul, al doilea parametru după
  `,`, ascuns la ieșirea din apel; un fișier SQL simplu: `coalesce(`). Captură nouă:
  `console-signature` (cu un comentariu cu apostrof deasupra: `enclosing_call_in`).
- [x] 48d — confirmarea Task Manager-ului ca modală (2026-11-03): `task-manager/src/confirm.rs`
  (`ConfirmView` în `view-task-confirm`, `Question` cu identitate UUID v4, `ask` / `take_yes`,
  comenzile interne `tasks.ask` și `tasks.answered`); vederea pune întrebarea și cere
  `tasks.ask`; „da” e făcut doar de vederea care a întrebat (prin `tasks-confirmed` trimis
  vederii active, sau la următorul ei `poll`). Scoase din vedere: `Confirm`, `paint_confirm`,
  `Hit::Dialog / ConfirmYes / ConfirmNo` și gărzile lor (modala aplicației blochează deja tot
  ce e în spate). Teste: modala (↵ pe „Renunță”, Esc, Tab / ← / →, „da” păstrat doar pentru
  întrebarea lui, luat o dată), acțiunile vederii prin modală (detaliul, meniul unui rând,
  „Stop” din paletă; un „da” făcut o dată și doar de vederea care a întrebat), contribuțiile
  modulului. Captura `task-manager-stop` arată acum modala (cifra de memorie vie rămâne
  diferența cunoscută a `task-manager*`).
- [x] 48e — contrastul în 9 teme (2026-11-03): `muted` din paletă (aceeași nuanță și
  saturație, luminozitatea mutată până la ~4,6:1 pe card): `arctic` #8a93a6 → #9da5b5,
  `fjord` #929cad → #98a2b2, `latte` #7c7f93 → #65687b, `mint` #6b7f76 → #60726a, `neon`
  #7a83a8 → #8088ac, `paper` #7c7468 → #776f64, `sakura` #8a7378 → #7e696d, `solar-dark`
  #839496 → #87989a, `solar-light` #839496 → #617072 (aproape de base01 al Solarized). Pe
  suprafețe cel mai slab rămâne acum 4,2:1 (`solar-light` / `paper` pe fereastră). Testul
  `every_builtin_theme_s_muted_text_reads_on_its_surfaces` (în locul celui doar pentru temele
  implicite): ≥ 4,5:1 pe card, ≥ 3:1 pe fereastră, panouri, popup-uri, editor, pentru toate
  temele incluse. Nicio captură existentă nu folosește cele 9 teme (toate identice); capturi
  noi: `theme-solar-light`, `theme-latte`.
- [x] 48f — setările izolate în teste (2026-11-03): ca mai sus; testele `config`
  (folderele temporare, și prin `storage`; un serviciu de setări pe fir). Verificat cu
  `HOME`-ul real: cele 43 de teste `workbench::servers` trec (înainte cele patru `php_*`
  picau cu setările reale), `check.sh` întreg verde fără `HOME` izolat, iar `~/.config/<app>`
  neatins (mtime-urile la fel înainte și după).
  - **Rămas:** câteva module își fac singure calea cu `dirs::config_dir()` (`remote`:
    conexiunile, `known_hosts`, aprobările; `git`: depozitele de încredere — pe macOS în
    `~/Library/Application Support/<app>`); acestea nu trec
    prin `storage::isolate`. De mutat pe `storage::config_dir` într-un plan care le
    migrează fișierele.

### Urmări (2026-11-03, după raportul lui 48)

- [x] 48g — modala Task Manager-ului pe măsura textului: `Viewer::modal_width` (cel mai
  îngust la care se citește conținutul) și `ModalSpec.fit_width`: fereastra cât cere
  titlul (cu marginile, iconița și `esc`), între acel minim și lățimea mărimii ei; întrebarea
  cere 440 (ca dialogurile aplicației), iar înălțimea urmează rândurile în care s-a împărțit
  corpul. Teste: cadrul (scurt → 440, titlu lung → mai lat, niciodată peste mărime), modala.
  Captura `task-manager-stop` refăcută.
- [x] 48h — etichetele enum PostgreSQL și pentru schemele citite tabelă cu tabelă:
  `ColumnInfo.values` (structura tabelei; DTO-ul protocolului cu `serde(default)`), citite de
  interogarea coloanelor unei tabele (`catalog::columns`, aceeași subinterogare `pg_enum` ca
  la citirea în bloc: `ENUM_LABELS`), păstrate de `ColumnBrief::from(&ColumnInfo)`, deci și
  în al doilea nivel al indexului consolei. Teste: domeniul (brief-ul le păstrează), matricea
  pe `editor-test-pg92` și `editor-test-pg17` (`sales.orders.status` citit cu tabela lui: cele
  patru etichete, în ordine; celelalte coloane fără); containerele oprite după.
- [x] 48i — toate căile aplicației printr-un singur loc: `files::places` (crate comun al
  adaptoarelor, InfraShared): `config_dir`, `data_dir`, `cache_dir`, `isolate`, `config_file`.
  `storage::config_dir` / `data_dir` / `isolate` trec prin el; `remote` (conexiunile,
  `known_hosts`, aprobările), `git` (depozitele de încredere) și terminalul (fișierele
  prompt-ului, în cache) nu mai fac singure `dirs::config_dir()` / `dirs::cache_dir()`.
  Fișierele ținute înainte în folderul de config al sistemului (pe macOS
  `~/Library/Application Support/<app>`) se folosesc mai departe acolo cât timp folderul de
  config al aplicației n-are unul al lui (`config_file`: nimic mutat, nimic scris la cerere).
  Testele crate-ului `remote` își țin conexiunile și `known_hosts` în fișiere temporare pe fir
  (ca aprobările), `git` le avea deja.
  - **Incident:** o primă variantă muta fișierul vechi la prima cerere; rulând `check.sh`,
    un test al crate-ului `remote` (neizolat de aplicație) a mutat astfel `known_hosts`-ul
    real al utilizatorului din `~/Library/Application Support/lumen/` în
    `~/.config/lumen/` (conținutul neatins). Mutarea a fost scoasă; fișierul a rămas în
    noul loc, unde aplicația îl găsește oricum (mutarea înapoi n-a fost permisă agentului:
    o decide utilizatorul).
  Folderele `app-test-home-<pid>`: al procesului șters la pornire, ale altor procese mai
  vechi de 10 minute șterse. Rămân directe doar `~/.ssh/known_hosts` (al utilizatorului,
  citit de SSH) și `~` pentru afișarea căilor. Teste: fișierul vechi folosit pe loc, nimic
  mutat, căile modulelor în folderul testelor, curățarea.
- [x] 48j — testele de taste ale consolei, stabile: listele globale pe care le scrie
  înregistrarea modulelor (`keymaps::set_places`, `set_module_presets`,
  `commands::set_contributed`, `set_listed`) nu mai sunt înlocuite, ci completate după cheie
  (`commands::merge_by`): ce descrie o comandă (locul tastelor, specificația, scurtăturile
  presetului) e al modulului ei oricum s-ar încărca, deci o încărcare cu mai puține module
  (testul cu `lang.html` dezactivat, alt test, altă fereastră) nu mai ia nimic celorlalte.
  Cauza: `a_disabled_module_takes_its_dependents_along` încărca modulele fără HTML / PHP în
  paralel cu testele consolei și le lua locul `EditorLanguage("sql")`. Teste: încărcarea din
  nou fără consolă păstrează locul și specificația ei, `merge_by`. Verificat: testele
  aplicației rulate de 30 de ori la rând: testele consolei (`editor_toolbars`) au trecut de
  fiecare dată. Altul, fără legătură, a picat de 2 ori din 30:
  `a_silent_servers_card_signature_and_declaration_dont_wait_for_its_timeout` (un termen
  măsurat pe ceas, depășit sub încărcare) — rămas pentru altă dată.

### Urmări 2 (2026-11-03)

- [x] 48k — „Anulează” din studioul de teme închide: cu modificări, butonul doar le anula
  (ciorna adusă înapoi, studioul rămas deschis, eticheta devenită „Închide”): era nevoie de
  a doua apăsare, pe orice sistem — nimic specific Windows-ului în drumul clicului (lovitura
  butonului, scara, cererea `CancelTheme` trec la fel; ciorna pornește egală cu tema, deci
  „Închide” apărea abia după prima apăsare). Acum o apăsare: modificările lăsate, tema
  ferestrei adusă înapoi (`CancelTheme`: `apply_theme`) și studioul închis; „Închide” la fel.
  Test: anularea cu modificări cere închiderea dintr-o apăsare, ciorna egală cu tema.
- [x] 48l — setările într-o fereastră scundă: același aspect (lista din stânga lată,
  filele pe două rânduri când e îngustă), dar lista din stânga nu mai trece pe file când nu
  încape pe înălțime: derulează (rotița deasupra ei, bara ei), iar conținutul își arată bara
  (`EdgeScrollbar` din `ui-components`, în marginea din dreapta a conținutului / pe o bandă a
  ei în listă: niciodată peste rânduri). Derularea ca la lista proiectelor: starea ei mutată
  din `projects_picker` în `ui_components::ScrollState` (niciodată dincolo de capăt, pixeli
  întregi, rândul ales ținut în vedere) și folosită de amândouă; lista proiectelor neschimbată
  la pixel (capturile ei identice). Doar rândurile care se văd sunt clicabile. Teste:
  `ScrollState`, lista care derulează (rotița, capătul, rândurile ascunse neatinse).
  Capturi: `settings-*` (bara conținutului, acum vizibilă unde conținutul e mai lung),
  noi `settings-short` și `settings-short-narrow`.
- [x] 48m — setările au subsolul cu taste al modalelor-listă (aceeași bandă a cadrului
  `Modal`, `paint_key_hints`, ca lista proiectelor), cu tastele care merg acum: ↑ ↓ paginile,
  A–Z caută, esc închide; în căutare ↑ ↓ rezultatele, ↵ deschide, esc golește căutarea; cât
  se scrie un câmp ↵ salvează, esc renunță (ro / en). Subsolul stă jos, lista și conținutul
  derulează deasupra lui. Test: tastele arătate în fiecare stare, conținutul deasupra
  subsolului. Capturi `settings-*` și `settings-query` refăcute (la
  înălțimea obișnuită subsolul face loc, așa că lista din stânga își arată acum bara).

