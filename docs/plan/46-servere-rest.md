# 46 — Ce a rămas de la serverele de limbaj (planul 46)

Cerut (2026-11-02, aprobat): restul planului 43 („Rămas pentru mai târziu”), în patru pași,
pe branch-ul `lsp`:

1. **Find Usages din referințele serverului** (46a), separat de Rename: intelephense fără
   cheie de licență nu redenumește, dar răspunde la `textDocument/references` (10/10 sonde,
   față de 7 / 2 / 1 ale noastre); referințele phpactor găsesc ce ne scapă nouă (R06 / N04).
2. **Actualizarea lunară a versiunilor serverelor** (46b): `cargo xtask lsp-update` și un
   workflow ca `language-data.yml`, care deschide un PR.
3. **ESLint și Prettier din `node_modules`-ul proiectului** (46c): niciodată incluse sau
   descărcate de noi; oferite ca verificator / formatator când proiectul le are.
4. **Structura din `documentSymbol`-ul serverului** (46d), pentru limbajele unde structura
   noastră (tree-sitter) e goală sau lipsește.

Regulile de la 43 rămân: nimic din motorul inclus nu se pierde, nimic greu pe firul UI,
totul pe proiect, testele fără rețea, fără `~/.config`, fără keychain, fără folderul de date
real.

## Decizii

### 46a — Find Usages

- **Rutarea**: o comandă nouă a gazdei, `RefactoringHost::usages_at_cursor` (implicit: cea
  a redenumirii), folosită de Find Usages (și de lentilele „N folosiri”, care rulează Find
  Usages). În aplicație `usages_context`: dacă serverul fișierului răspunde la referințe
  (capabilitatea anunțată, serverul gata), locurile lui sunt cerute **oricum ar fi
  Rename-ul** (intelephense fără cheie: Rename-ul nostru, Find Usages al lui).
- **Ce se arată**: în fișierele limbajelor serverului, locurile lui; în ale celorlalte
  limbaje (Blade, Twig, o funcție a unui script folosită într-o pagină), ale noastre — ca la
  Rename (43e). **Unite cu ale noastre și în fișierele lui** acolo unde motorul inclus își
  păstrează Rename-ul (`built-in = ["rename"]`: phpactor, CSS) sau unde utilizatorul a ales
  „Amândouă”: fiecare loc o dată (aparițiile sunt ale textului, deci nicio dublură).
  Serverul fără răspuns (eroare, gol, timeout): ale noastre peste tot.
- **De ce nu uniunea la intelephense**: măsurat pe sonde (mai jos), uniunea îi strică o sondă
  (R09: ale noastre adaugă 2 locuri greșite); singur e 10/10. La phpactor uniunea câștigă
  (R06 îl știe el, R04 / R05 noi).
- Nicio regulă nouă în descrieri: `built-in = ["rename"]` spune deja că motorul inclus
  găsește bine acolo.

### 46b — `cargo xtask lsp-update` și workflow-ul lunar

- Pe componentă, ultima versiune: Node — ultima **LTS** din `nodejs.org/dist/index.json`
  (niciodată „Current”); pachetele npm — cea mai nouă stabilă până la eticheta `latest` a
  registry-ului, fără cele `deprecated`; clangd / LuaLS / zls / taplo / phpactor — ultimul
  release GitHub (fără pre-release / draft) care are **toate** fișierele pe care le folosește
  fixarea (numele cu versiunea nouă); PHP static — cea mai nouă versiune din listele
  dl.static-php.dev (`?format=json`) care există pentru fiecare platformă; intelephense (și
  orice `[language-servers.install]` npm din descrierile limbajelor) — din registry, cu
  integritatea SHA-512 și mărimea tarball-ului.
- **Politica semver**: patch / minor aplicate direct; o versiune majoră (primul număr; sub
  1.0 al doilea; la PHP și linia minoră, 8.4 → 8.5: pe ea rulează phpactor; o linie nouă de
  Node LTS) doar cu `--major`; versiunile-calendar (phpactor, `2026.10.07.0`) nu sunt
  niciodată „majore”. Workflow-ul face două PR-uri separate (minor / major), fiecare deschis
  doar dacă testele cu serverele reale trec.
- **Ce scrie**: `packaging/lsp.toml` (versiunile, URL-urile, SHA-256-urile — calculate din
  ce descarcă; Node din `SHASUMS256.txt`), `package.json` + `package-lock.json` din
  `packaging/lsp/<server>/` (refăcut cu `npm install --package-lock-only --ignore-scripts`:
  integritatea SHA-512 a registry-ului fixată în lockfile), apoi `cargo xtask lsp-manifest`
  (mărimile, `downloads.toml`). `--check`: doar spune ce s-ar schimba, nu scrie nimic.
- **Workflow-ul** `.github/workflows/language-servers.yml` (lunar, 1 ale lunii, și manual):
  `lsp-update`, `lsp-manifest`, apoi testele reale (`EDITOR_TEST_LSP_DOWNLOAD` într-un folder
  temporar: instalează tot din manifest și pornește fiecare server), PR cu
  `peter-evans/create-pull-request` (ramura `language-servers`), ca workflow-ul datelor.
- Logica de alegere (versiuni, LTS, semver, listele) e pură și testată cu listări false;
  rețeaua doar în comanda reală.

### 46c — ESLint și Prettier din proiect

- Găsite în proiect: `node_modules/.bin/eslint` / `prettier` (și `.cmd` pe Windows), sau
  pachetul (`node_modules/eslint/bin/eslint.js`, `node_modules/prettier/bin/prettier.cjs`
  / `bin-prettier.js`), rulat cu Node: cel din PATH, altfel Node-ul nostru (descărcat /
  inclus), dacă e. Nimic instalat de noi.
- Uneltele modulului de calitate (cum sunt php-cs-fixer, phpstan…): verificatorul ESLint
  (JS, TS, TSX, JSX, Vue: `--format json` pe stdin, `--stdin-filename`), formatatorul Prettier
  (JS, TS, CSS, SCSS, LESS, JSON, YAML, HTML, Vue, Markdown: stdin → stdout,
  `--stdin-filepath`), alese pe limbaj în Setări → Limbaje → Formatare și verificare,
  „Permite” — consimțământul pentru uneltele proiectului — ca la celelalte unelte ale
  proiectului (codul proiectului rulează doar după acord).
- La salvare: formatarea / verificarea, ca la celelalte unelte; nimic pe firul UI.

### 46d — Structura de la server

- Panoul Structura întreabă serverul activ (`textDocument/documentSymbol`, portul de la 43a)
  doar când limbajul n-are structură a noastră (nicio regulă de structură) sau structura
  noastră e goală: C / C++, Lua, Zig, Bash, TOML, Dockerfile, YAML…; altfel tree-sitter, ca
  acum (instant, fără server).
- Ținută pe (document, versiune); cererea pe un fir al modulului / în fundal, rezultatul
  citit la cadru (nimic așteptat pe firul UI); o versiune nouă cere din nou, după o pauză.

## Pașii

| Pasul | Ce |
|---|---|
| 46a | Find Usages din referințe (uniunea unde Rename-ul e al nostru), teste, imaginea `lsp-usages`, măsurătorile |
| 46b | `cargo xtask lsp-update` (+ `--check`, `--major`), testele alegerii versiunilor, workflow-ul |
| 46c | ESLint / Prettier din `node_modules`, setările, testele cu unelte false, imaginea setărilor |
| 46d | Structura de la server, cache pe versiune, testele, imaginea C++ cu serverul fals |

## Stare

### 46a — Find Usages din referințele serverului (2026-11-02, branch `lsp`)

**Făcut.**

- `workbench_model::RefactoringHost::usages_at_cursor` (implicit `rename_at_cursor`); Find
  Usages (modulul `refactoring`) îl folosește. Aplicația: `usages_context` — serverul care
  răspunde la referințe e întrebat (`served_references`, portul `references` de la 43a),
  oricum ar fi Rename-ul; altfel contextul redenumirii, ca înainte.
- `refactor_ports::ServedPlaces` (`Rename` / `References`): același `ServedRename` (locurile
  serverului găsite în fișiere după rând și locul în rând, ale noastre în fișierele altor
  limbaje), cu `joined`: unde motorul inclus își păstrează Rename-ul (phpactor, CSS) sau
  „Amândouă”, ale noastre se adaugă și în fișierele serverului (rezolvatorul: al serverului
  întâi, al nostru pentru celelalte apariții; candidații: ai serverului + toți ai noștri;
  un simbol local rămâne în fișierul lui).
- Comentariile descrierilor PHP actualizate (Find Usages nu mai e „al nostru”).

**Măsurători** (sondele `references` ale harness-ului `scratchpad/lsp-compare`, 10 sonde pe
features / laravel / symfony-demo; răspunsurile de la 43g / 43h, combinate cum le combină
acum aplicația — `scratchpad/mine/46a/score_usages.py`):

| Find Usages | ok / parțial / nimic+greșit |
|---|---|
| înainte (al nostru, oricare server PHP) | 7 / 2 / 1 |
| phpactor singur | 7 / 1 / 2 |
| **phpactor + al nostru (46a, uniunea)** | **8 / 2 / 0** |
| **intelephense fără cheie (46a, ale lui în PHP)** | **10 / 0 / 0** |
| (intelephense + al nostru, uniunea — neales) | 9 / 1 / 0 |

R04 (phpactor nimic) și R05 (phpactor 1 din 3) iau ale noastre, R06 (al nostru nimic) îl
ia phpactor; R09 rămâne parțial cu phpactor (8 / 8 găsite, 3 în plus), corect cu intelephense.

**Teste** (aplicația, serverul fals): intelephense fără `renameProvider` — Find Usages îi
cere referințele, Rename-ul nu e cerut, în PHP doar locurile lui (index.php, pe care nu-l
știe, lipsește), șablonul Twig al nostru; phpactor — referințele lui unite cu ale noastre,
fiecare loc o dată (index.php găsit de noi); serverul care greșește / nu răspunde / tace —
ale noastre peste tot. Testele de redenumire de la 43e / 43h trec neschimbate. Testul ierarhiei de
tipuri de la 43j (lanțul de răspunsuri ale serverului în harul de 1 s) pica sub încărcarea
rulării complete după ce s-au adăugat testele noi (răspunsul venea după ~1,8 s): harul
navigării are acum o valoare doar a testelor (`navigation::TEST_GRACE`, 4 s în acel test),
aplicația rămâne la 1 s.

**Imagini**: nouă `lsp-usages` (un fișier TypeScript răspuns de serverul fals și o pagină
care îi cheamă funcția: Find Usages cu cele două locuri ale serverului în `cart.ts` și al
nostru, nesigur, în `cart.html`).

### 46b — `cargo xtask lsp-update` și workflow-ul lunar (2026-11-02, branch `lsp`)

**Făcut.**

- `xtask/src/lsp_update.rs`: `cargo xtask lsp-update [--check] [--major] [--root <dir>]`.
  Blocurile lui `packaging/lsp.toml` (`[node]`, `[php]`, `[[servers]]`, comentariile dinainte
  de următorul lăsate lui); sursa fiecăruia după ce fixează (Node, PHP, `packages` → npm, un
  URL de release GitHub → API-ul de release-uri, cu `GITHUB_TOKEN` dacă e); versiunea
  aleasă cu `pick` / `is_major` / `is_major_line`; blocul „reorientat” (versiunea veche
  înlocuită pe toate rândurile care nu sunt comentarii: URL-uri, `strip = "clangd_<v>"`,
  tag-urile LICENSE-urilor), apoi fiecare URL schimbat cu SHA-256-ul lui: digest-ul GitHub
  al fișierului, `SHASUMS256.txt` la Node, altfel descărcat și calculat. Serverele Node:
  fiecare dependență din `package.json` după politica ei (vtsls 0.3 → 0.3.x, TypeScript
  5.9 → 5.x fără `--major`), versiunea serverului = a pachetului principal (numele lui sau
  scope-ul), lockfile-ul refăcut cu `npm install --package-lock-only --ignore-scripts`.
  Descrierile cu `[language-servers.install]` (intelephense): `version`, `integrity`, `size`.
  La final `lsp-manifest` (mărimile din HEAD, `downloads.toml`). `--check`: raportul, nimic
  scris, nimic descărcat pentru sume. O componentă care nu se poate afla („not updated: …”)
  nu le oprește pe celelalte. Rețeaua prin trăsătura `Net` (curl în comanda reală).
- `.github/workflows/language-servers.yml` (lunar, 1 ale lunii la 07:00, și manual): o
  matrice minor / `--major`; `lsp-update` (Node din `actions/setup-node` doar pentru npm),
  `lsp-manifest --check`, descărcarea reală a tuturor componentelor din manifestul nou
  (`EDITOR_TEST_LSP_DOWNLOAD`, testul de la 43i), testele serverelor reale pe ce s-a
  descărcat (`EDITOR_LSP_DIR`, `real_servers`), testele modulului `language-servers` și ale
  xtask-ului, apoi PR-ul (`peter-evans/create-pull-request`, ramurile `language-servers` /
  `language-servers-major`, corpul cu raportul).

**Încercat** (rețeaua reală, 2026-11-02): `--check` — totul la zi în afară de PHP 8.5.7 și
TypeScript 7.0.2 (majore: `--major`); `--major --root` pe o copie a ambalajului din
scratchpad: PHP 8.4.22 → 8.5.7 (cele patru arhive descărcate și calculate, LICENSE-ul la tag-ul
nou — același conținut), TypeScript 7.0.2 cu lockfile-ul refăcut de npm (pachetele native ale
lui `@typescript/typescript-*` cu integritatea lor), `downloads.toml` refăcut (26 de mărimi
noi). Nimic din repo schimbat: versiunile fixate rămân cele de acum.

**Teste** (xtask, fără rețea, listări inventate): versiunile (`v`, pre-release, build,
calendar), ce e major (primul număr, sub 1.0, calendarul, linia PHP), alegerea cu majorele
reținute, doar LTS-urile Node (o linie nouă doar cu `--major`), pachetele până la `latest`
fără cele `deprecated`, release-urile GitHub fără draft / pre-release cu digest-urile lor,
PHP doar cu fișierul fiecărei platforme, blocurile și comentariile lor, un server nativ
(URL-uri, digest GitHub sau suma calculată), un release căruia îi lipsește un fișier fixat,
Node cu `SHASUMS256.txt`, phpactor (calendar, LICENSE-ul recalculat), intelephense (versiune,
integritate, mărime), pachetele unui server Node după politică, `--check` care nu scrie și nu
descarcă, o componentă de negăsit spusă, celelalte mergând mai departe.

**Teste instabile reparate pe drum** (modulul `language-servers`, picau ~1 din 2 rulări și
sub `check.sh`): „clearing the caches…” număra lansările înainte ca a doua să fie numărată
(lansată pe firul driverului ei, puțin după ce e cunoscută) — acum le așteaptă pe amândouă
întâi; „a server is chosen…” citea folderul de descărcări global lăsat de alt test — acum ține
starea modulului (`Harness`) și n-are descărcări. Sub încărcarea `check.sh` au picat o dată și
două teste `editor_toolbars` ale aplicației (tastele consolei / profilului), neatinse aici;
trec singure și la rularea următoare.

**Nefăcut**: workflow-ul n-a rulat pe GitHub (se va vedea la prima rulare); testele reale pe
Linux în CI pornesc serverele descărcate pentru `linux-x64`.

### 46c — ESLint și Prettier din proiect (2026-11-02, branch `lsp`)

Ce exista deja (de la planul uneltelor de calitate): Prettier și ESLint ca unelte ale modulului
JavaScript, găsite în `node_modules/.bin`, apoi în `PATH`, rulate pe fișierul de pe disc
(Prettier cu `--write`). Ce lipsea: Node (shim-ul `.bin` cere `node` în `PATH`; pe Windows
`.cmd`), intrarea pe stdin, oferta.

**Făcut.**

- `code_quality`: `QualityTool::scripts()` (programul ca script Node al proiectului, în
  ordine) și `stdin()` (fișierul pe stdin); `Invocation::stdin`; `Lookup::node`; `plan`: un
  script al uneltei găsit în proiect + un Node ⇒ `node <script> <args>`, altfel shim-ul /
  `PATH`-ul ca înainte. `code_quality::runtime`: Node-ul aplicației, spus de aplicație
  (`use_node`; aplicația dă `language_servers::node_program` — bundle-ul, altfel cel
  descărcat), fără ca modulele să se cunoască.
- Modulul JavaScript: ESLint — `node_modules/eslint/bin/eslint.js`, pe un fișier `--format=json
  --stdin --stdin-filename <fișier>`, pe proiect `--format=json .`; Prettier —
  `node_modules/prettier/bin/prettier.cjs` (3.x), `bin-prettier.js` (2.x), pe un fișier
  `--stdin-filepath <fișier>` (textul formatat pe stdout), pe proiect `--write .`.
- Rularea (`runner`): Node-ul sistemului (`PATH`), altfel al aplicației; fișierul citit și dat
  pe stdin; un formatator care reușește și tipărește altceva decât a primit e scris înapoi
  atomic (`files::write_atomic`; nimic scris dacă n-a tipărit nimic sau a atins limita de
  16 MB a ieșirii); editorul îl reîncarcă ca la `--write`. La salvare: ca la celelalte unelte
  (formatarea, apoi verificarea).
- Setări → Limbaje → Formatare și verificare: cât timp nu e ales nimic, o unealtă pe care
  proiectul o are (pachetul în `node_modules` — versiunea din `package.json` — sau un program în
  `vendor/bin`) e oferită: „Proiectul are ESLint 9.12.0 (node_modules/eslint), rulat cu
  Node-ul sistemului sau al aplicației. [Folosește-l]”; clicul întreabă (rulează codul
  proiectului, butonul roșu), „da” o alege și o permite (consimțământul „Permite” de până
  acum). Aleasă, rândul de stare arată comanda (`node …/eslint.js --format=json …`).

**Teste**: planul — scriptul pe Node (al sistemului / al aplicației), stdin doar pe fișier,
fără Node shim-ul, fără pachet `PATH`-ul; modulul JavaScript — argumentele, scripturile; rularea
— un `node_modules` fals (un „Node” care e un shell, scripturi shell în locul pachetelor):
verificatorul primește fișierul pe stdin și îi citim problemele, formatatorul îl scrie înapoi;
pagina — oferta (doar când pachetul e acolo, cu versiunea), întrebarea, alegerea salvată și
permisă. Fără rețea, fără Node real.

**Imagini**: nouă `settings-page-languages-javascript` (un `node_modules` cu ESLint 9.12.0 și
Prettier 3.3.3 pus doar pentru cazul acesta: cele două oferte).

**Nefăcut**: ESLint nu verifică CSS / JSON / YAML / HTML (fără pluginuri nu le știe; Prettier
le formatează); `vscode-eslint` (serverul) nu e folosit, cum s-a decis.

### 46d — Structura din declarațiile serverului (2026-11-02, branch `lsp`)

**Făcut.**

- Aplicația (`servers::served_outline`): pentru documentul activ, doar unde motorul inclus n-are
  nimic — limbajul n-are inteligență a noastră, sau a noastră a fost extrasă pentru versiunea
  aceasta și e goală —, serverul activ (cel care răspunde fișierului, cu `documentSymbol`
  anunțat) e întrebat `textDocument/documentSymbol` (portul de la 43a, `convert::symbols`:
  ierarhia `DocumentSymbol` cu containerele ei, sau `SymbolInformation`). O singură întrebare
  pe rând, ținută pe (document, versiune); răspunsul citit de `poll_servers` (nimic așteptat pe
  firul UI), ca `FileSymbols` al limbajului; ultimul răspuns al documentului e arătat până vine
  cel al versiunii noi (o tastare nu golește panoul). Un răspuns gol / o eroare: nimic pentru
  versiunea aceea, întrebat din nou la următoarea.
- Panoul Structura (`update_side_panel`): structura noastră când are ceva, altfel a serverului;
  cheia panoului primește generația răspunsurilor (refăcut doar când vine unul).
  Declarația de la cursor, problemele ei și rândurile se fac din ea ca din a noastră.

**Teste** (aplicația, serverul fals): C (doar sintaxa noastră) — declarațiile serverului,
întrebat o dată pe versiune (10 cadre: o cerere), o versiune nouă — cele vechi păstrate, întrebat
din nou, cele noi; TypeScript (structura noastră) — serverul niciodată întrebat.

**Imagini**: nouă `structure-cpp-server` (un fișier C++ răspuns de serverul fals — descris în
imagini și pentru `c` / `cpp` —: Structura cu clasele lui, numele lui în bara de jos).

**Nefăcut**: pentru limbajele cu structura noastră (PHP, JS / TS, CSS…) rămâne a noastră
(instant, fără server), cum s-a decis; serverul nu e unit cu ea.

### Imaginile planului 46 (comparate cu referința, 2026-11-03)

Noi (copiate în `scratchpad/mine/new-46/`, ambele teme): `lsp-usages`,
`settings-page-languages-javascript`, `structure-cpp-server`. Diferențe față de referință, toate
de dinainte de planul 46: `console`, `console-completion`, `console-join`, `console-hover`,
`console-unknown-table` (doar tema închisă: bara instrucțiunii curente de la 4acdb264, mutată
la marginea gutter-ului) și `task-manager*` (cifra de memorie măsurată). Restul identice.
