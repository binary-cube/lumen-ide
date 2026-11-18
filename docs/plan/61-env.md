# Plan 61 — Fișierele `.env` (modulul dotenv)

## Context
Cerere: un modul pentru fișierele `.env` — azi n-au nicio colorare proprie și se citesc greu.
Vrem: limbajul (`.env`, `.env.*`, `*.env`), colorarea (chei, `=`, valori neîncadrate / cu
ghilimele simple / duble, pe mai multe linii, `${VAR}` / `$VAR` în valori, comentarii,
`export`, comentarii după valoare, booleeni / numere / URL-uri / căi deosebite discret),
valorile care arată a secrete mascate până la hover (setare, implicit oprită); secțiuni în
Structure, breadcrumb și plieri (generic din comentarii; Symfony, Laravel… prin puncte de
extensie); Find Usages / Go to Definition / hover / Rename pentru chei, cu tiparele de citire
aduse de modulele limbajelor și framework-urilor; avertismente (cheie folosită dar
nedefinită, cheie dublată, cheie nefolosită).

## Ce există azi
- **Iconița**: `file-icons` pune `key` / culoarea `env` pe `.env` și `.env.*`
  (`FileIconRule`); pachetele de iconițe au `.env`, `.env.example`, `.env.local`.
- **Nimic altceva**: niciun limbaj nu recunoaște `.env` (text simplu: fără culori, fără
  structură, fără plieri). `bash` recunoaște `.zshenv`, nu `.env`.
- **Ce putem refolosi**:
  - limbajul ca toate celelalte: `LanguageDescriptor` + `Grammar` (tree-sitter, cu
    `highlights.scm` / `folds.scm` ale noastre), compilat leneș de `syntax-ts`;
  - `LanguageIntelligence::extract` → `FileSymbols`: din el se fac panoul Structure,
    breadcrumb-ul și `scope_at`; `analyse` → problemele fișierului deschis (live);
  - `DocumentAssist` (definiție / hover / completare pentru limbajele pe care le spune,
    întrebat înaintea framework-urilor) — punctul prin care un modul ajută în fișierele
    altor limbaje fără să le cunoască;
  - `RenameContext` (refactor): ținta, `Sources`, `Resolver`, `ConflictCheck` — Find Usages
    și Rename folosesc același plan (`plan_rename`), cu previzualizare și undo;
  - `DiagnosticsFound` pe magistrala proiectului (`ProjectService`): avertismentele apar ca
    ale verificatorilor (squiggle, bara de derulare, Structure, bara de stare);
  - evenimentele proiectului (`FileSaved`, `FilesWritten`, `FileRenamed`, `EntriesDeleted`,
    `EntriesChanged`, `EntriesRescan`) și `kernel::queue` pentru lucrul incremental;
  - registrele globale umplute de app din contribuții (`servers::register_data`): așa
    ajung la un modul datele aduse de alte module, fără ca vreunul să-l numească pe altul.
- **Gramatica**: nu există pe crates.io o gramatică dotenv; `pnx/tree-sitter-dotenv`
  (GitHub, MIT, întreținută — ultima versiune 1.1.3, oct. 2026, ABI 15) se potrivește
  setup-ului (vendorizată ca `tree-sitter-sql` / `twig` / `blade`). Îi lipseau escape-urile
  în ghilimele duble și `${VAR:-implicit}`: patch-uri locale, documentate în README.

## Design

### Limbajul (`crates/modules/languages/dotenv`, crate `lang-dotenv`, id `lang.dotenv`)
- Fișiere: `.env`, `.env.*` (`.env.local`, `.env.prod`, `.env.example`, `.env.test`,
  `.env.dist`…), `*.env`; comentariul de linie `#`; iconița rămâne a `file-icons`.
- **Gramatica** vendorizată în `third_party/tree-sitter-dotenv`; `queries/` ale noastre:
  cheia `@property`, `=` `@operator`, `export` `@keyword`, comentariile, valorile
  `@string`, escape-urile, `${` `}` `@punctuation.special` cu numele interpolat colorat ca
  o cheie (`@property`: se vede legătura), booleeni `@boolean`, numere `@number`, URL-uri
  (`scheme://…`) `@function`, căi (`/…`, `./…`, `../…`, `~/…`) `@type` — capturi alese
  dintre cele pe care le colorează toate temele incluse (deosebirea e discretă).
  `own_errors: false`: gramatica e permisivă cu dialectele (Node permite `KEY=a b`), nu
  vrem alarme false.
- **Lexer-ul nostru** (`syntax.rs`, pur): un fișier → intrări (cheie, `export`, valoarea cu
  felul ei de ghilimele, interpolările din ea, comentariul de după), comentarii, linii goale.
  Pe el stau secțiunile, cheile din Structure, utilizările din `.env`, redenumirea și
  verificările — gramatica rămâne doar pentru culori (ca la celelalte limbaje).
- **Valorile** (`values.rs`, pur): felul unei valori (boolean, număr, URL, cale, text) și
  „arată a secret” (cheie cu `SECRET` / `TOKEN` / `PASSWORD` / `PASS` / `KEY` / `PRIVATE` /
  `CREDENTIAL` / `AUTH`, sau un DSN / URL cu `user:parolă@`), cu partea de mascat (pentru un
  DSN, doar parola).
- **Mascarea** (setarea `[dotenv] mask_secrets`, implicit oprită): punct de extensie nou
  `workbench_model::Concealment` — „intervalele unui document desenate ca `•`/`*` până când
  mouse-ul e peste ele sau caret-ul e pe linia lor”. App-ul le cere doar când se schimbă
  versiunea documentului sau generația contribuției, editorul înlocuiește la desenare
  caracterele cu un semn de aceeași lungime în octeți (coloanele, caret-ul și culorile rămân
  la locul lor). Textul rămâne neatins (copierea, salvarea, căutarea îl văd întreg).

### Secțiuni, Structure, plieri
- Regula generică (în modul, fără nimic de framework): un comentariu urmat de blocul lui de
  chei e o secțiune cu numele comentariului; liniile goale despart grupurile; un grup fără
  comentariu e o secțiune fără nume doar dacă o regulă contribuită nu-i dă unul.
- **Punct de extensie** `code_intelligence::env::EnvSections` (contribuție): reguli de
  secțiuni — `Fenced { open, close, trail }` (blocuri marcate: `###> pachet ###` …
  `###< pachet ###`, eticheta = ce e între marcaje) și `KeyPrefix` (grupurile fără
  marcaje / fără comentariu, etichetate cu prefixul comun al cheilor: `APP_`, `DB_`…; grupuri
  vecine cu același prefix unite). Regulile se aplică după conținut (marcajele se
  recunosc singure; `KeyPrefix` doar când fișierul n-are marcaje) — `extract` și plierile
  primesc doar textul.
  - **Symfony** (`fw-symfony`): `Fenced { "###> ", "###< ", " ###" }` (rețetele Flex);
  - **Laravel** (`fw-laravel`): `KeyPrefix`.
- `SymbolKind::Section` (nou): „un grup cu titlu al declarațiilor unui fișier”; cheile sunt
  `Constant`, cu secțiunea drept container (intervalele imbricate dau adâncimea în
  Structure și lanțul din breadcrumb).
- **Plierile**: punct de extensie nou în `syntax-ts`: `FoldProvider` (contribuție) —
  „regiuni pe care un limbaj le calculează singur din text, peste cele ale arborelui”;
  `parse_document` le adaugă pentru limbajul rădăcină (pe firul de parsare, nu pe UI).
  Secțiunile se pliază cu „N keys” (`FoldCount::Keys`), deci Fold / Unfold / Collapse All
  merg ca peste tot; la fel valorile pe mai multe linii.

### Utilizări, navigare, redenumire, avertismente
- **Punct de extensie** `code_intelligence::env::EnvReads` (contribuție): „cum citește codul
  unor limbaje o variabilă de mediu”: limbajele, opțional fișierele (glob-uri: doar
  `compose*.yaml`), formele de acces (`EnvAccess`) — date, nu cod —, dacă o citire nedefinită
  se semnalează (nu pentru `$X`-ul unui shell: cele mai multe sunt ale scriptului) și numele
  pe care runtime-ul le dă singur (`NODE_ENV`, `MODE` ale lui Vite). Formele:
  `EnvAccess::call("getenv")` (`getenv('X')`), `index("$_ENV")` (`$_ENV['X']`),
  `member("process.env")` (`process.env.X`), `named("env")` (`env: 'X'`),
  `placeholder("%env(", &[")%"])` cu procesoare (`%env(int:X)%`), `INTERPOLATION`
  (`${X}`, `${X:-implicit}`), `VARIABLE` (`$X`; `$$X` e un dolar). Potrivirea lor e logică
  pură în modulul dotenv (`reads.rs`).
  - PHP generic (`getenv`, `$_ENV`, `$_SERVER`): contribuit de modulul dotenv în numele
    limbajului `php` (fără a atinge `lang-php`), cheie pe id-ul limbajului;
  - Laravel: `env('X')`, `env('X', implicit)`, `Env::get('X')` (PHP, Blade);
  - Symfony: `%env(X)%` / `%env(int:X)%` / `%env(resolve:X)%` în YAML și PHP,
    `#[Autowire(env: 'X')]`, `env('X')` din configurările PHP (configurările XML nu au un
    limbaj al lor: rămân pe dinafară);
  - JS / TS (`lang-javascript`): `process.env.X`, `process.env['X']`, `import.meta.env.X`;
  - Docker Compose (`lang-dockerfile`): `${X}` / `${X:-implicit}` / `$X` în
    `compose*.yaml` / `docker-compose*.yml` (nesemnalate);
  - shell (`lang-bash`): `$X`, `${X}` (nesemnalate).
- **Ordinea încărcării** — `code_intelligence::env::EnvFileOrder` (contribuție): pentru un
  proiect recunoscut după fișierele din rădăcină (`markers`), numele fișierelor în ordinea
  în care framework-ul le încarcă (`{env}` = mediul): Symfony `.env` → `.env.local` →
  `.env.{env}` → `.env.{env}.local`; Laravel `.env`, apoi `.env.{env}` când există. Fără
  convenție — ordinea comună (a lui Vite, dotenv-flow, Symfony): `.env`, `.env.local`,
  `.env.{env}`, `.env.{env}.local`, apoi celelalte după nume; documentația (`.env.example`,
  `.env.dist`, `.env.sample`, `.env.template`, `.env.defaults`) la urmă, marcată. Vite n-are
  nevoie de o contribuție (e ordinea comună); Rails / Django n-au module aici.
- **Catalogul proiectului** (`catalog.rs`): per proiect (după `ProjectId`, dispare cu el),
  fișierele `.env` ale unui folder (după limbajul lor), citite în fundal — rădăcina la
  deschidere, un folder când e cerut — în ordinea încărcării; codul e servit de folderul lui
  sau de cel mai apropiat de deasupra care are (un monorepo are `.env`-uri pe subfoldere).
  Ținut la zi din evenimentele proiectului (salvări, scrieri, redenumiri, ștergeri,
  schimbări pe disc, rescan). Cât un folder se citește, cine întreabă primește `None` și e
  chemat înapoi (nimic nu așteaptă discul pe firul UI).
- **Go to Definition** dintr-o citire (un `DocumentAssist` pentru limbajele cu tipare și
  pentru `dotenv` — `${X}` dintr-un `.env` duce la cheie): un singur fișier → saltul; mai
  multe → o listă de ales, în ordinea încărcării. Pentru asta: `AssistTarget::Choose`
  (nou) — „mai multe locuri, în ordinea dată, utilizatorul alege”; app-ul îl arată cu lista
  de navigare existentă.
- **Hover**: valoarea cheii în fiecare fișier (mascată dacă arată a secret), în ordinea
  încărcării; documentația (`.env.example`) marcată.
- **Find Usages / Rename**: punct de extensie nou `workbench_model::CursorSymbols` — „un
  simbol al unui modul la cursor (o cheie `.env`): ce redenumește / caută Rename / Find
  Usages”, întrebat de app înaintea serverelor și a motorului încorporat. Modulul dă un
  `RenameContext`: ținta (numele, `NameStyle` simplu), `Sources` (fișierele `.env` ale
  proiectului + fișierele de cod care citesc cheia, din index; până e gata indexul, toate
  fișierele limbajelor cu tipare), `Resolver` (o apariție e ținta dacă e definiția unei
  chei, o interpolare dintr-un `.env` sau o citire după tipare; altfel nu), `ConflictCheck`
  (noul nume e deja o cheie). Previzualizarea, aplicarea și undo-ul sunt ale modulului
  de refactorizare, ca la orice redenumire.
- **Avertismente** (`service.rs`, `ProjectService`):
  - cheie dublată în același fișier — din `check` (live, nesalvat);
  - variabilă citită de un document deschis dar definită în niciun `.env` (documentația
    contează; nici una din mediul procesului, nici una dată de runtime) — `DiagnosticsFound`
    pe fișierul de cod, cât se scrie (`DocumentOpened` / `DocumentChanged`, ultima versiune
    pe firul modulului, `kernel::Latest`), din nou când fișierele `.env` se recitesc; un
    proiect fără `.env` nu primește nimic;
  - cheie pe care n-o citește niciun cod — notă, oprită implicit (`[dotenv] unused_keys`):
    fișierele proiectului citite în fundal (`used.rs`, fiecare din nou doar dacă s-a
    schimbat), la pornirea setării, la recitirea `.env`-urilor și la salvări.

### Setări (`crates/modules/languages/dotenv/src/settings.rs`)
`[dotenv] mask_secrets` (oprit), `[dotenv] unused_keys` (oprit); blocul „.env” din
pagina Languages (`SettingsContribution`); cine depinde de ele e anunțat la schimbare
(`settings::follow`).

## Pași
- **61a** — gramatica vendorizată (+ patch-uri), modulul `lang-dotenv`: limbajul, culorile,
  lexer-ul, valorile, mascarea (`Concealment` + editorul), setările, locale ro/en, teste.
- **61b** — secțiunile: `SymbolKind::Section`, `EnvSections`, `FoldProvider`, Structure /
  breadcrumb / plieri, contribuțiile Symfony și Laravel, verificarea cheilor dublate, teste.
- **61c** — `EnvReads`, `EnvFileOrder`, `AssistTarget::Choose`, `CursorSymbols`;
  catalogul proiectului, definiția cu alegere, hover, Find Usages, Rename, avertismentele;
  contribuțiile PHP / Laravel / Symfony / JS / Compose / shell / Vite; teste; capturi
  Symfony și Laravel.

## Stare
- [x] 61a — `third_party/tree-sitter-dotenv` (pnx 1.1.3, regenerat cu CLI 0.27.1, ABI 15;
  patch-uri: escape-uri și `$` singur în ghilimele duble, `${X:-implicit}`, `0`; scanner-ul
  fără avertismente `-Wunused-parameter`). `lang-dotenv`: `.env` / `.env.*` / `*.env`
  (regula nouă din `languages::detect`: un glob deschis la capăt cedează extensiei altui
  limbaj — `.env.local.php` rămâne PHP), `highlights.scm` (chei, `=`, `export`, valori,
  escape-uri, interpolări cu numele colorat ca o cheie, booleeni, numere, URL-uri
  `@function`, căi `@type`), `folds.scm` (valorile pe mai multe linii); `syntax.rs` (lexer-ul:
  intrări, comentarii, ghilimele pe mai multe linii, interpolări, comentarii după valoare,
  CRLF), `values.rs` (felul valorii, cheile secrete după cuvintele lor, parola dintr-un URL),
  `conceal.rs` + `workbench_model::Concealment` (`hidden`, `mask` — semne de aceeași lungime
  în octeți), în app `workbench/concealed.rs` (cerute din nou doar la altă versiune /
  generație; arătate sub mouse și pe liniile caret-urilor), `Decorations::concealed` în
  editor; `settings.rs` (`[dotenv] mask_secrets`, blocul din Languages), locale ro/en.
- [x] 61b — `SymbolKind::Section` (în completare `Namespace`, în Structure `§`, în codec-ul
  indexului la coadă: fișierele vechi se citesc la fel); `code_intelligence::env`
  (`EnvSections::{Fenced, KeyPrefix}`, `register_sections` / `sections`, umplut de app);
  `syntax_ts::FoldProvider` (`SyntaxConfig::add_fold_provider`, adăugat la regiunile
  stratului rădăcină în `parse_document`); în modul `sections.rs` (regula generică,
  blocurile marcate, prefixele — rulări de chei consecutive, unite peste linii goale, de la
  două chei), `outline.rs` (secțiunile și cheile ca simboluri — fără valori: pot fi secrete;
  plierile „N keys”), `checks.rs` (cheile dublate), `intelligence.rs` (`DotenvIntelligence`:
  `extract`, `check` → `problem.env_duplicate_key`; `SectionFolds`). Symfony contribuie
  `Fenced { "###> ", "###< ", " ###" }`, Laravel `KeyPrefix`. Capturi: Structure + breadcrumb
  pe un `.env` Symfony (`.env › symfony/mailer › MAILER_DSN`), Collapse All pe unul Laravel
  (`APP_NAME=Laravel 7 keys`…).
- [x] 61c — `code_intelligence::env`: `EnvAccess` / `Quoting` (constructorii formelor
  comune), `EnvReads` (`new`, `in_files`, `unflagged`, `providing`, `covers`),
  `EnvFileOrder` (`rank`, `{env}`), registrele lor; `AssistTarget::Choose` + `AssistPlace`
  (app: lista de navigare existentă; indiciul ⌘ duce la primul); `workbench_model::
  CursorSymbols` + `CursorContext` (app: întrebate în `rename_context` / `usages_context`
  înaintea serverelor; textele deschise într-un singur loc, `open_texts`). În modul:
  `reads.rs` (formele potrivite, `$$`, procesoare, linia cursorului), `keys.rs` (cheia la
  cursor oriunde e scrisă), `order.rs` (ordinea, documentația), `catalog.rs`,
  `navigation.rs` (`DocumentAssist`: definiția — una sau o listă —, hover-ul cu valorile pe
  fișiere, secretele ascunse), `rename.rs` (`RenameContext`: sursele — `.env`-urile și
  fișierele limbajelor care citesc, nu bibliotecile —, rezolvitorul, conflictele),
  `used.rs`, `service.rs`, `checks.rs` (`undefined`, `unused`, `position`); contribuțiile
  PHP (în modul), Laravel, Symfony, JS, Compose, Bash. Snapshot: `RELATED=definition` și
  serviciile așteptate și după primele cadre (diagnosticele lor arătate). Capturi: hover
  (Symfony, întunecat; Laravel, luminos), lista definițiilor (`.env` → `.env.local` →
  `.env.example`), avertismentul pe `SENTRY_DSN`, Find Usages pe `APP_NAME` (5 în 4
  fișiere).
