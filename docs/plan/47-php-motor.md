# 47 — Motorul nostru PHP, mult mai deștept (întâi deducerea tipurilor)

Cerut (2026-11-03, aprobat): motorul PHP inclus a făcut **60%** pe cele 146 de sonde ale
comparației din 43 (phpactor 83%, intelephense 88%). El răspunde în PHP-ul din Blade / Twig,
acolo unde niciun server nu e instalat și peste tot unde rutarea din 43 îl lasă (motorul
inclus rămâne pentru șabloane și ca rezervă); e și cel mai rapid (µs, ~70–100 MB, 0% în
repaus). Golul principal: tipul receptorului / al variabilei (`context.rs::infer_variable`,
trei expresii regulate: `new`, tipul parametrului, `@var`).

Ținta: **≥ 80% în total, niciodată mai rău pe vreo categorie, zgomotul neschimbat**; măsurat
după fiecare pas cu harness-ul din 43 (`lsp-compare`, runner-ul `ours`).

## Golurile, pe sonde (din `results/score.md`)

| Gol | Sonde | Ce lipsește |
|---|---|---|
| Deducere pe expresii, nu doar pe `$var` | C02, C03, C05, C12, C33, C34, D04, R06, N04 | tipul proprietății (și al celei promovate în constructor), tipul întors de metodă, apeluri înlănțuite, `?->`, `static` întors prin părinte |
| Generice PHPDoc | C04, C06, C11, C30, C32, H02 | `@template` / `@extends Collection<int, User>` / `@return static<…>`, `list<T>`, `array{…}`, `@psalm-` / `@phpstan-` |
| Magia din PHPDoc | C10, D12, C28, C29, D17, H11, S08 | `@property`, `@method` (și `@method static`: fațadele Laravel) |
| Membrii claselor PHP (stubs) | C13, C14 | `DateTimeImmutable`, `Uri\Rfc3986\Uri`… după `->` |
| Enum-uri | C07, C08, C09 | `cases()`, `from()`, `tryFrom()`, `->name`, `->value` |
| Hover pe variabile | H02, H03, H13 | tipul și de unde vine |
| `use function` | R04, N03 | linia `use function App\Support\format_name;` nu e găsită |
| Zgomot la referințe | R09 | textul din comentarii și șiruri numărat ca folosire |
| Ierarhia la Rename | N04 | metoda abstractă din părinte și apelul generic |
| Segment de namespace | C20 | `use App\Mo|` → `Model` |
| Diagnostice semantice | G02–G07, G10, H10 | clasă necunoscută, prea puține argumente (proiect / PHP), apel `@deprecated` / `#[\Deprecated]`, variabilă nefolosită, „deprecated” în hover |
| Eloquent static | C26, D16, H14 | `User::where()` prin `__callStatic` (nimeni nu reușește la C26 / D16) |

## Decizii

### Unde stă deducerea

- În modulul PHP (`lang-php`), nu în domeniu: domeniul (`code-intelligence`) primește patru
  cârlige noi pe `LanguageIntelligence`, toate cu implicit „nimic” (celelalte limbaje nu se
  schimbă):
  - `receiver_type(text, offset, &Types)` — FQN-ul obiectului din fața lui `->` la `offset`,
    știind proiectul (`Types { index, path }`); motorul îl încearcă întâi la completare și la
    „go to definition” (deci și la hover, semnătură, Rename, Find Usages), apoi `Receiver`-ul
    vechi.
  - `builtin_members(fqn)` — membrii pe care îi dă limbajul însuși (clasele PHP din stubs,
    `cases()` / `from()` / `tryFrom()` / `name` / `value` ale enum-urilor), ca simboluri fără
    fișier, adăugați după cei din index (pentru strămoșii pe care indexul nu-i are).
  - `typed_hover(text, offset, &Types)` — hover-ul ce cere proiectul: o variabilă (tipul și
    de unde vine), metoda unei clase PHP prin tipul dedus.
  - `check(text, &Types)` — diagnosticele semantice (cu indexul), rulate pe firul de simboluri
    după ce fișierul e pus în index, doar cu indexul complet (fără „clasă necunoscută” cât
    încă indexează).
- **Arborele documentului**, ținut de modul pentru ultima versiune văzută (`kernel::cache::Memo`,
  cheia: textul însuși, comparat cu `memcmp`): o tastă reparsează **incremental** (diferența
  prefix / sufix față de textul ținut → `Tree::edit` → `parse(…, Some(&vechi))`), nu de la zero.
  Eliberat cu stubs-urile (`release_idle`) și la „Clear and Rebuild Caches”.
- Pe interogare: un `Infer` (arborele, textul, indexul, fișierul) cu memo-uri proprii
  (clasele citite, variabilele deduse); nimic ținut între interogări în afară de arbore.

### Tipurile

- `types::Ty`: `Named(fqn, args)`, `Template(nume)`, `Static` / `This` / `SelfType`
  (rezolvate la folosire), `Array(cheie, valoare)` (`array<K, V>`, `list<T>`, `T[]`,
  `iterable<K, V>`), `Shape([(cheie, tip)])`, `ClassString(T)`, `Scalar`, `Union`, `Mixed`.
- Un parser PHPDoc propriu: uniuni, intersecții, `?T`, generice, `T[]`, `array{…}` (chei
  opționale `k?:`), `callable(A): R` / `Closure(A): R`, paranteze, condiționale
  `($x is T ? A : B)` (→ tipul nativ dacă există, altfel uniunea), literali, `int<0, max>`,
  `key-of` / `value-of`, `class-string<T>`. Necunoscutul → `Mixed` (niciodată o eroare).
- Numele se rezolvă în fișierul care le declară (namespace-ul și `use`-urile lui, din index);
  numele șabloanelor (ale clasei și ale metodei) rămân `Template`.

### Ce intră în index (o singură schimbare de format)

- `Symbol.types: Option<Box<str>>` — ce spune PHPDoc-ul declarației și codul nu, într-o formă
  compactă citită doar de modulul PHP (linii `<etichetă> <conținut>`): `r` tipul întors,
  `v` tipul proprietății, `p $nume tip` (doar parametrii cu generice / șabloane / forme),
  `t T [of Bound]` șabloanele, `x` / `i` / `u` `@extends` / `@implements` / `@use` cu
  argumente, `m` `@mixin`, `d` depreciat (`@deprecated` sau `#[\Deprecated]`). Scris doar
  când adaugă ceva (un `@return string` peste `: string` nu se ține). Codec-ul indexerului
  trece la versiunea 9 (cache-ul vechi se reface).
- `@property` / `@property-read` / `@property-write` / `@method [static]` devin **simboluri**
  ale clasei (proprietăți / metode publice, cu `note` = eticheta), la locul lor din docblock:
  completarea, „go to definition” (D12, D17), hover-ul și semnătura (H11, S08) le găsesc fără
  nimic special.
- Enum-urile primesc părinții impliciți din PHP: `\UnitEnum`, plus `\BackedEnum` la cele cu
  tip; `name` / `value` vin din `builtin_members`.
- Stubs-urile (ale noastre, generate) își citesc acum și metodele (`m`), constantele claselor
  (`cc`) și părinții / interfețele claselor (`c`), indexate pe clasă — tot din textul inclus în
  binar, fără copii.

### Scopurile și legările

- Scopul: funcția / metoda / closure-ul / arrow function-ul cel mai interior; arrow
  function-ul moștenește scopul în care e scris, closure-ul doar `use (…)` (citite în scopul de
  afară, la locul closure-ului); fișierul fără funcție (șabloanele) e un scop.
- O variabilă la un loc: **ultima legare dinaintea locului** în scop (fără scopurile
  interioare): atribuire (inclusiv `??=`, `[$a, $b] =` / `list()` cu chei), `foreach` (valoarea
  și cheia), `catch (T $e)`, parametri (tipul scris; `@param` când e mai precis), parametri
  promovați, `@var T $x` (și `/** @var T */` înaintea atribuirii), `global` (necunoscut).
  Îngustarea: în ramura `if ($x instanceof T)` (și după `&&`), după `assert($x instanceof T)`.
- Expresiile: `new` (inclusiv `new static` / `self`), `clone`, apeluri de metode / statice /
  funcții (proiect, stubs, helper-ele din vendor), proprietăți (și statice), constante de
  enum (cazul → enum-ul), `X::class` (`class-string<X>`), `?->`, ternar / `?:` / `??` (uniunea,
  fără `null` la stânga lui `??`), `match` (uniunea brațelor), indexare (`$row['address']`
  din formă, `$list[0]` din valoare), paranteze, atribuiri ca expresii.
- Membrii unei clase cu substituție: strămoșii parcurși cu argumentele lor
  (`@extends Collection<int, User>` → `TValue = User`), `static` / `$this` → receptorul
  (cu argumentele lui), `self` → clasa care declară; șabloanele metodei deduse din
  `class-string<T>` (`X::class` dat) și din `T $x` (argumentul dat); un `callable(TValue)`
  dă tipul parametrului unui arrow function / closure netipat.
- Limite: adâncime 24, cel mult 32 de strămoși; o buclă (`$a = $a->next()`) se oprește la
  legarea de dinainte.

### Diagnostice (zgomot zero)

- Doar ce e cert: **clasă necunoscută** (nici în index, nici în stubs, nici unde o caută
  PSR-4, numele nu e un șablon / alias, indexul complet), **argumente prea puține / prea
  multe** (funcții și metode cu tip cunoscut ale proiectului și ale PHP-ului; fără variadice,
  fără `...$args` la apel, fără argumente numite), **apel depreciat** (`@deprecated`,
  `#[\Deprecated]`), **variabilă nefolosită** (atribuită, niciodată citită în scop; fără
  `compact` / `extract` / `$$` / `include` / `eval` / `get_defined_vars` / referințe / `global` /
  `static` / `use (&…)` în scop), **metodă / proprietate inexistentă** doar pe o clasă
  cunoscută fără magie (`__call`, `__callStatic`, `__get`, `@mixin`, `@method`, `@property`,
  `#[\AllowDynamicProperties]`, niciun strămoș necunoscut).
- `requires ext-x` nu se mai spune pentru extensiile care nu pot lipsi (core, standard, date,
  spl, pcre, reflection, json, random, hash, uri).

### Restul

- `use function A\f;` / `use const A\C;`: ținta e funcția / constanta (`target`), locul se
  potrivește felului (`fits_kind`) — Rename și Find Usages îl găsesc (R04, N03).
- Find Usages / Rename: un nume într-un șir sau într-un comentariu care nu e docblock nu mai
  e o folosire (R09); docblock-urile (`@param User`) rămân.
- Rename-ul unei metode cuprinde declarațiile aceleiași metode din strămoși și descendenți
  (abstracta din părinte, suprascrierile) — `Declared::by_definitions_in`.
- Segmentele de namespace la completare (`use App\Mo|`, `new App\Mo|`) din FQN-urile
  indexului; tipurile după un prefix calificat doar din namespace-ul lui.
- Hover: `Deprecated` între note când docblock-ul spune `@deprecated` sau declarația are
  `#[\Deprecated]`.

## Pașii

1. **47a** — nucleul deducerii (arborele incremental, `Ty`, parserul PHPDoc, scopurile,
   expresiile, membrii cu substituție) + tipurile PHPDoc în index (codec 9) + magia
   `@property` / `@method` / `@mixin`; cârligul `receiver_type`.
2. **47b** — membrii stubs-urilor și ai enum-urilor (`builtin_members`), fațade / helper-e /
   Eloquent unde e ieftin și sigur.
3. **47c** — hover pe variabile, `use function`, zgomotul referințelor, ierarhia la Rename,
   segmentele de namespace, diagnosticele semantice (`check`, mesaje ro/en).
4. **47d** — măsurători (latență, memorie), imaginile noi (`php-inference-completion`,
   `php-variable-hover`), lustruire.

Fiecare pas: teste mici pe fragmente PHP în `tests/unit/` ale modulului, harness-ul rulat din
nou, `./scripts/check.sh` verde, un commit.

## Stare

### 47a — Nucleul deducerii + tipurile PHPDoc (2026-11-03, branch `lsp`)

- **Domeniul** (`code-intelligence`): `Types { index, path }`; pe `LanguageIntelligence`
  `receiver_type`, `builtin_members`, `typed_hover`, `check` (folosit din 47c), `is_deprecated`,
  toate implicit „nimic”. Motorul: completarea după `->` și „go to definition” pe un membru
  întreabă întâi `receiver_type`; listele de membri (`->` și `::`) adaugă `builtin_members`
  (fiecare nume o dată); hover-ul încearcă `typed_hover` înainte de `hover`; cardul unei
  declarații are nota `Deprecated` când docblock-ul spune `@deprecated` (`DocBlock.deprecated`)
  sau limbajul o știe (`#[\Deprecated]`). `SymbolIndex::declared_members` (doar ai clasei).
  `Symbol.types: Option<Box<str>>` (codec-ul indexerului: formatul 9; extractorul PHP:
  versiunea 2).
- **Modulul PHP**: `types.rs` (`Ty` + parserul PHPDoc), `phpdoc.rs` (etichetele, variantele
  `@phpstan-` > `@psalm-` > simplu; `Facts`, forma compactă din index), `doctree.rs` (4 texte
  cu arborii lor, reparsare incrementală din cel mai apropiat; declarațiile documentului, o
  dată), `infer/` (`mod.rs` expresiile, `scope.rs` variabilele / legările / îngustarea,
  `members.rs` clasele, membrii cu substituție, șabloanele metodei prin unificare —
  `class-string<T>`, `T`, `array<T>`, `Arrayable<K, V>`, `callable(): T` —, `api.rs` ce cere
  editorul). Stubs-urile își citesc metodele, constantele claselor și părinții.
- **Extragerea**: `types` pe clase (șabloane, `@extends` / `@implements` / `@use` cu argumente,
  `@mixin`, depreciere, tipul enum-ului), metode și funcții (`@return`, șabloanele, parametrii
  cu generice / forme / callable), proprietăți (`@var`), parametri promovați (`@param`-ul
  constructorului); un tip PHPDoc fără nicio clasă (`void`, `int|string`, `array`) nu se ține.
  `@property*` / `@method [static]` → simboluri ale clasei la eticheta lor (nota = eticheta),
  intervalul clasei începe atunci la docblock.
- **Cod neterminat**: unde receptorul cade într-o eroare de sintaxă (`$x->` fără nume, o linie
  fără `;`, fișierul neînchis) se întreabă din nou textul reparat (un nume după `->`, `;` la
  capătul liniei, acoladele închise).
- **Abatere de la pași**: `builtin_members` (stubs, enum-uri) și hover-ul pe variabile /
  metodele claselor PHP au intrat deja aici — nucleul nu se putea testa fără ele, iar costul
  era mic; 47b rămâne pentru magia framework-urilor (Eloquent) și ce mai lipsește la stubs.
- Teste: `types` (7), `phpdoc` (5), `doctree` (3), `infer` (12, pe fragmente: proprietăți
  promovate, lanțuri, `?->`, `@template` / `@extends`, `static` păstrându-și argumentele,
  șabloanele metodei, `class-string<T>`, parametrul netipat al unui arrow function din
  `callable(TValue)`, `foreach` pe `list<T>` și pe un `IteratorAggregate`, forme, destructurare,
  closure / `use` / arrow function, `instanceof` în `if` / după `&&` / `assert` / ieșirea
  timpurie, ultima legare, `@var`, `catch`, `??` / ternar / `match`, enum-uri, completarea cu
  membrii PHP și ai enum-urilor, navigarea și `@property`, hover-ul variabilei, fațade,
  funcții cu șabloane, `@mixin`, cod neterminat, adâncimea mărginită), `stubs`, extragerea
  (`types`, magia), domeniul (un limbaj fals cu `receiver_type` / `builtin_members`, `@deprecated`,
  `declared_members`), codec-ul cu `types`.

Harness-ul (doar motorul nostru, aceleași sonde; serverele din 43):

| Categoria | înainte | 47a |
|---|---|---|
| completion (37) | 15 / 1 / 21 | 34 / 1 / 2 |
| definition (25) | 21 / 1 / 3 | 24 / 0 / 1 |
| references (10) | 7 / 2 / 1 | 8 / 2 / 0 |
| rename (7) | 5 / 1 / 1 | 5 / 2 / 0 |
| hover (14) | 8 / 0 / 6 | 13 / 0 / 1 |
| signature (8) | 7 / 0 / 1 | 8 / 0 / 0 |
| diagnostics (10) | 2 / 0 / 8 | 2 / 0 / 8 |
| symbols (5) / stubs (25) | 5 / 25 | 5 / 25 |
| noise (5) | 4 / 0 / 1 | 4 / 0 / 1 |
| **total (fără stubs / noise)** | **70 (60%)** | **99 (85%)** |

Ce lipsește încă: C20 (segmentul de namespace), C26 / C27 / D16 / H14 (Eloquent), R04 / N03
(`use function`), R09 (șiruri / comentarii), N04 (abstracta din părinte), diagnosticele.
Latența (p50 / p95 cald, ms): completion 0.07 / 1.27 (înainte 0.01 / 1.40), definition
0.02 / 0.06, hover 0.01 / 0.06, signature 0.05 / 0.08. Memoria indexului: laravel 22.1 →
25.8 MB (+7 000 de simboluri magice, mai ales fațadele), symfony 22.0 → 23.0 MB; RSS în
repaus laravel 103 → 109 MB, symfony 69 → 70 MB.

### 47b — Magia framework-urilor: Eloquent, mixin-uri, trăsături generice (2026-11-03, branch `lsp`)

- **Apelurile statice redirecționate**: `X::m()` negăsit pe `X`, dar `X` are `__callStatic` și un
  `query()` static → metoda celui dat de `query()` (modelele Laravel: `User::where()` e
  `User::query()->where()`, `Builder<static>` → `Builder<User>`). Fără nume de framework în cod:
  regula e a PHP-ului (`__callStatic`) plus convenția `query()`.
- **`@mixin` la completare**: `builtin_members` adaugă membrii mixin-urilor (indexul lor, cu
  ai strămoșilor), după ai clasei: `User::query()->` oferă `where` / `first` (ale builder-ului
  Eloquent) și `orderBy` (al builder-ului de interogări, prin `@mixin`).
- **`member_owner`** (cârlig nou al domeniului): clasa care declară membrul de la cursor
  (strămoș, mixin, redirecționarea statică), întrebată întâi de „go to definition” pe un membru
  sau un apel static — D16 (`User::where` → `Eloquent\Builder::where`), `orderBy` →
  `Query\Builder`.
- **`/** @use BuildsQueries<TModel> */ use BuildsQueries;`**: argumentele unei trăsături se
  citesc din docblock-ul instrucțiunii `use` din corpul clasei (`first()`-ul `BuildsQueries`
  dă `TValue` = `TModel` = `User`: H14).
- Completarea după `->` / `::` întoarce până la 400 de membri (restul: 80): un builder are
  sute; membrii daţi de limbaj (mixin-uri, stubs) au scorul sub ai bibliotecilor.
- Teste: un Eloquent în miniatură (model cu `__callStatic` / `query()`, builder cu `@template`,
  `@mixin`, `@use`): hover-ul lui `$user = User::where()->first()` (`?User`), tipul lui
  `User::query()->`, definiția prin mixin și prin redirecționare, completarea (ai clasei
  înaintea mixin-ului).

| Categoria | înainte | 47a | 47b |
|---|---|---|---|
| completion (37) | 15 / 1 / 21 | 34 / 1 / 2 | 36 / 0 / 1 |
| definition (25) | 21 / 1 / 3 | 24 / 0 / 1 | 25 / 0 / 0 |
| references (10) | 7 / 2 / 1 | 8 / 2 / 0 | 8 / 2 / 0 |
| rename (7) | 5 / 1 / 1 | 5 / 2 / 0 | 5 / 2 / 0 |
| hover (14) | 8 / 0 / 6 | 13 / 0 / 1 | 14 / 0 / 0 |
| signature (8) | 7 / 0 / 1 | 8 / 0 / 0 | 8 / 0 / 0 |
| diagnostics (10) | 2 / 0 / 8 | 2 / 0 / 8 | 2 / 0 / 8 |
| noise (5) | 4 / 0 / 1 | 4 / 0 / 1 | 4 / 0 / 1 |
| **total** | **70 (60%)** | **99 (85%)** | **103 (88%)** |

C26 / D16 (Eloquent static) — la care nici phpactor, nici intelephense nu răspund — merg acum.
Latența caldă p50 / p95: completion 0.06 / 1.32 ms, definition 0.02 / 0.05, hover 0.01 / 0.10.
Memoria: neschimbată față de 47a (laravel 25.8 MB, symfony 23.1 MB).

### 47c — Hover, Rename / Find Usages, diagnosticele semantice (2026-11-03, branch `lsp`)

- **`use function` / `use const`**: ținta e funcția (`\App\Support\format_name`) / constanta,
  locul se potrivește felului; Rename și Find Usages găsesc linia importului (R04, N03).
- **Șiruri și comentarii**: `fits_kind` nu mai ia drept folosire un nume dintr-un șir sau
  dintr-un comentariu care nu e docblock (arborele documentului, întrebat doar când linia
  arată a șir / comentariu); ce interpolează un șir (`{$user->name}`) rămâne cod (R09).
- **Familia unei metode**: `Declared::by_definitions_in` — declarațiile aceleiași metode în
  strămoșii și descendenții clasei ei (abstracta din părinte, suprascrierile) sunt ținta
  (N04); resolverul aplicației (`refactor_ports.rs`) îl folosește. O metodă la declarația ei
  (`function describe(` într-o clasă) e acum `Member { This }`: navigarea o găsește pe ea.
- **Segmentele de namespace**: după `App\` (`use App\Mo|`, `new \App\|`) completarea oferă
  namespace-urile din FQN-urile indexului (`CompletionKind::Namespace`, „N” în listă), inserate
  cu `\` (C20); cu nimic tastat după `\`, doar ele.
- **Diagnostice** (`checks.rs`, prin `LanguageIntelligence::check`, pe firul de simboluri
  după ce documentul e în index, doar cu indexul complet — `Job.index_ready`, cererea refăcută
  când indexarea se termină): clasă necunoscută (`new X`, `X::`, `instanceof X`; nu `X::class`,
  nu un nume pe care proiectul îl are în alt namespace), prea puține argumente (proiect și PHP),
  prea multe (doar funcțiile PHP-ului), metodă necunoscută (doar pe un receptor sigur — `$this`
  în afara closure-urilor, un parametru tipat, o variabilă îngustată sau atribuită în blocul
  funcției, `$this->prop` tipată —, o clasă a proiectului închisă: fără `__call`, `@mixin`,
  `@method` / `@property`, toți strămoșii cunoscuți, nicio trăsătură dintr-o bibliotecă,
  nicio subclasă care s-o aibă, numele nu apare într-un șir al funcției — `method_exists` —,
  nu într-o clasă abstractă pe `$this`), apel / `new` depreciat (`@deprecated`,
  `#[\Deprecated]`). Nimic într-un fișier al unei biblioteci (`vendor/`), în
  `.phpstorm.meta.php` sau într-un text care nu se parsează. Variabilă neutilizată
  (`unused.rs`): atribuită ca instrucțiune, niciodată citită în funcție; fără parametri, `use`,
  supraglobale; funcțiile cu `compact` / `extract` / `$$` / `include` / `eval` / referințe /
  `global` / `static` lăsate în pace. Aplicația le arată ca avertismente (`problem.*`,
  `unused.variable`, `deprecated.method|function|class`, ro/en).
- **Zgomotul, vânat pe biblioteci** (harness-ul `lint`, cu `vendor/` indexat și verificat ca
  al proiectului): pe tot `vendor/` din laravel și din symfony au rămas doar cazuri reale sau
  de nedecis (dependențe opționale neinstalate, cod dependent de versiune) — de aici regulile
  de mai sus; pe fișierele proiectelor: doar ce e greșit cu adevărat în `Legacy.php` și o
  variabilă neutilizată reală (`$moved`, Z01). Din drum: ordinea PHP la căutarea unui membru
  (trăsăturile înaintea părintelui, interfețele la urmă), mixin-urile căutate o singură dată
  (o buclă `@mixin` A ↔ B nu mai explodează), aliasurile din `use T { a as b; }` ca metode,
  comentariile dintre parametri scoase din semnătură, `use function count;` nu mai apare
  neutilizat, metodele private ale trăsăturilor / claselor cu trăsături sau `include`-uri nu
  mai apar neutilizate, `requires ext-x` nu mai apare pentru extensiile mereu prezente.
- **Stubs-urile**: `dump.php` și `cargo xtask php-stubs` scriu de acum și proprietățile
  claselor (`p`), funcțiile și metodele depreciate (`df` / `dm`, cu versiunea); modulul le
  citește (proprietățile la completare și în deducere, deprecierile la diagnostice). **Datele
  nu au fost regenerate** (cer Docker cu toate versiunile PHP: workflow-ul lunar o face) —
  până atunci G07 (`utf8_encode`, depreciată în 8.2) rămâne nespusă.
- Teste: `checks` (clase necunoscute, numărul argumentelor, metode doar pe clase închise,
  deprecieri, trăsătura înaintea părintelui, mixin-uri în buclă, înregistrările noi din
  stubs), `unused` (variabile), țintele și `fits_kind` (`use function`, șiruri / comentarii),
  segmentele de namespace, familia unei metode la Rename, cheile noi din xtask, aplicația
  (problemele marcate după indexare, cu mesajul lor).

| Categoria | înainte | 47a | 47b | 47c |
|---|---|---|---|---|
| completion (37) | 15 / 1 / 21 | 34 / 1 / 2 | 36 / 0 / 1 | 37 / 0 / 0 |
| definition (25) | 21 / 1 / 3 | 24 / 0 / 1 | 25 / 0 / 0 | 25 / 0 / 0 |
| references (10) | 7 / 2 / 1 | 8 / 2 / 0 | 8 / 2 / 0 | 10 / 0 / 0 |
| rename (7) | 5 / 1 / 1 | 5 / 2 / 0 | 5 / 2 / 0 | 7 / 0 / 0 |
| hover (14) | 8 / 0 / 6 | 13 / 0 / 1 | 14 / 0 / 0 | 14 / 0 / 0 |
| signature (8) | 7 / 0 / 1 | 8 / 0 / 0 | 8 / 0 / 0 | 8 / 0 / 0 |
| diagnostics (10) | 2 / 0 / 8 | 2 / 0 / 8 | 2 / 0 / 8 | 8 / 0 / 2 |
| noise (5) | 4 / 0 / 1 | 4 / 0 / 1 | 4 / 0 / 1 | 4 / 0 / 1 |
| **total** | **70 (60%)** | **99 (85%)** | **103 (88%)** | **114 (98%)** |

Rămân: G01 (metoda pe `User`, care are `__call`: refuzat cu bună știință — PHP o poate
primi), G07 (până la regenerarea stubs-urilor), Z01 (zgomot: `requires ext-mbstring` /
`ext-bcmath`, cerințe reale lipsă din `composer.json`, și `$moved`, chiar neutilizată — erau 5
diagnostice, acum 3). Latența caldă p50 / p95: completion 0.06 / 1.18 ms, definition
0.02 / 0.06, hover 0.01 / 0.07, references 0.24 / 3.98 (fișierele candidate parsate pentru
receptor și pentru șiruri), rename 0.17 / 3.57. Memoria: ca la 47b.

Notă: patru teste ale serverelor PHP din aplicație (`workbench::servers::php_*`) pică pe
mașina de lucru și la codul de dinainte de 47c: testele aplicației citesc
`~/.config/<app>/settings.toml` al utilizatorului (alegerea motorului pentru PHP de acolo); cu
un `HOME` izolat trec toate 43. `./scripts/check.sh` a fost rulat cu `HOME` izolat
(`CARGO_HOME` / `RUSTUP_HOME` cele reale). Izolarea setărilor în teste rămâne de făcut.

### 47d — Măsurători, imaginile noi, lustruire (2026-11-03, branch `lsp`)

- **Imaginile**: `php-inference-completion` (în `Shop.php`, `$d = new \DateTimeImmutable();`
  apoi `$d->modify('+1 day')->` tastate: lista metodelor lui `DateTimeImmutable`, din stubs, prin
  `static` întors de `modify`) și `php-variable-hover` (`$made = new Shop($this->cart);
  $made->total(1);` inserate, mouse-ul pe `$made`: cardul „Variabilă — `Shop $made` — Din
  `$made = new Shop($this->cart)`”). Pentru ele: `TYPE` acceptă `\n` și `{sc}` (ca `INSERT`),
  opțiunea nouă `HOVER_WORD=<text>` pune mouse-ul pe primul loc al textului.
- **Comparația completă** cu referința: diferă doar `usages` (cum trebuie: `Cart::items` și
  `$this->cart->items()` nu mai sunt oferite „nesigur” ca folosiri ale lui `Shop::items` — sunt
  ale lui `Cart`; rămâne declarația ei, „1 utilizare în 1 fișier”), `splash` (ilustrația nouă a
  splash-ului, de dinaintea planului 47) și `task-manager*` (memoria măsurată, cunoscut);
  celelalte imagini PHP (editorul, hover-ul, lentilele, `insight`, `context-actions`, `rename`,
  `signature`, `diagram`, navigarea) sunt identice. Copiate în `scratchpad/mine/new-47/`.
- **Latența** (harness, caldă, p50 / p95, ms): completion 0.06 / 1.18 (înainte 0.01 / 1.40),
  definition 0.02 / 0.06, hover 0.01 / 0.07, signature 0.05 / 0.07, references 0.24 / 3.98
  (înainte 0.16 / 1.28: fișierele candidate se parsează pentru receptorul fiecărui loc),
  rename 0.17 / 3.57. Tastare simulată 90 s în `LaravelProbe.php` (soak-ul din 43): completarea
  0.43 / 8.6 ms (înainte 0.24 / 18.7), hover + definiție 0.17 / 1.8, analiza documentului
  4.1 / 6.6 ms (înainte 1.2 / 3.8: acum și verificarea cu indexul — în aplicație pe firul de
  simboluri, nu pe UI), RSS maxim 66 MB.
- **Memoria indexului**: laravel 22.1 → 25.8 MB (+7 000 de membri magici, mai ales ai
  fațadelor, și tipurile PHPDoc), symfony 22.0 → 23.0 MB; RSS în repaus laravel 103 → 109 MB,
  symfony 69 → 69 MB; arborii documentelor (4 texte) se dau drumul cu stubs-urile.

**Total**: 70 → 114 din 116 (60% → 98%; intelephense 88%, phpactor 83%), nicio categorie mai
rea, zgomotul neschimbat (4 / 5).

Rămas: G07 (deprecierile funcțiilor PHP — vin la următoarea regenerare a stubs-urilor), G01
(refuzat: clasa are `__call`), proprietățile claselor PHP (aceeași regenerare), izolarea
setărilor în testele aplicației, diagnosticele care se refac când se schimbă *alt* fișier
(acum: la versiunea documentului și la terminarea indexării).

### 47e — Comparația refăcută și măsurată din nou (2026-11-04)

Harness-ul din 43 (`lsp-compare`) se pierduse cu scratchpad-ul vechi; e acum **în repo**,
refolosibil: `tools/lsp-compare/run.sh <folder de lucru>` (tot ce face — proiectele, serverele,
cache-urile lor, un `HOME` propriu — stă în folderul dat; niciodată folderul de date al
aplicației sau `~/.config`).

- **Sondele** (`probes.py`): tot 116, pe aceleași categorii și cu aceleași teme — completion 37,
  definition 25, references 10, rename 7, hover 14, signature 8, diagnostics 10, noise 5 —, dar
  **rescrise** (cele vechi nu mai existau): proiectul `features` (`tools/lsp-compare/projects/
  features`: modele cu proprietăți promovate, `@property` / `@method`, trăsături, enum-uri,
  generice `@template` / `@extends`, `class-string<T>`, forme, `use function` / `use const`,
  `Legacy.php` cu deprecieri, `Problems.php` cu câte o problemă pe linie), un schelet Laravel și
  unul Symfony făcute cu Composer, cu fișierele de sonde copiate în ele (`LaravelProbe.php`,
  `ProbeController.php`). Scorurile nu se compară deci sondă cu sondă cu cele din 43 / 47a–d.
- **Motorul nostru** răspunde prin aplicația însăși: un test ignorat al aplicației
  (`the_comparisons_probes_answered_by_the_built_in_engine`, în
  `tests/unit/workbench/intelligence.rs`) deschide proiectul în `Workbench`, PHP fixat pe motorul
  inclus, și întreabă ca editorul (lista de completare, `definition_at`, cardul de hover,
  ajutorul la parametri, Find Usages / Rename prin planul refactorizării, marcajele tab-ului).
- **Serverele** (`client.py`, stdio): intelephense 1.18.5 (npm, în folderul de lucru) și
  phpactor 2026.10.07.0 (PHAR-ul release-ului, SHA-256 verificat ca în `packaging/lsp.toml`; PHP-ul
  sistemului cu `-n`), configurate ca în `data/language-servers.toml`. Notarea: `score.py`
  (`score.md`, `details.md`).

| Categoria | al nostru | intelephense | phpactor |
|---|---|---|---|
| completion (37) | 36 / 1 / 0 | 35 / 1 / 1 | 32 / 2 / 3 |
| definition (25) | 22 / 0 / 3 | 24 / 0 / 1 | 21 / 0 / 4 |
| references (10) | 8 / 1 / 1 | 10 / 0 / 0 | 6 / 2 / 2 |
| rename (7) | 7 / 0 / 0 | 0 / 0 / 7 (fără cheie) | 5 / 1 / 1 |
| hover (14) | 12 / 2 / 0 | 13 / 0 / 1 | 13 / 0 / 1 |
| signature (8) | 8 / 0 / 0 | 7 / 0 / 1 | 6 / 0 / 2 |
| diagnostics (10) | 10 / 0 / 0 | 10 / 0 / 0 | 2 / 0 / 8 |
| noise (5) | 5 / 0 / 0 | 5 / 0 / 0 | 5 / 0 / 0 |
| **total (116)** | **108 (93%)** | **104 (90%)** | **90 (78%)** |

- Față de 47d (98% pe setul vechi): 93% pe setul nou; ordinea rămâne (noi > intelephense >
  phpactor). **G07** (`utf8_encode`, depreciată în 8.2) **trece** acum, după regenerarea
  stubs-urilor din 55o; și diagnosticele merg toate (10 / 10), zgomotul 5 / 5.
- **Ce n-a mers la noi** (goluri arătate de sondele noi, nu regresii văzute — setul vechi nu
  le avea): constantele din namespace — definiția (D06) și referințele (R04) lui `DEFAULT_CITY`;
  definiția unei variabile locale (D14) și a unui parametru (D15); referințele lui `Shop::cart()`
  iau și proprietatea `$cart` (R06: două locuri în plus); uniunea unui ternar la completare
  (C29: doar `Cart`, nu și `Shop`); hover-ul pe `new Shop(` arată constructorul, nu clasa (H01);
  hover-ul constantei de clasă fără valoare (H12). Trecute în `TODO.md`.
- Doar noi: Eloquent static (C31, D16, H14), Rename fără cheie / mai bun ca phpactor.
- **Latența caldă** (p50 / p95 ms; a 2-a – a 4-a întrebare a fiecărei sonde; serverele măsurate
  din client, cu drumul prin pipe și JSON):

| | al nostru | intelephense | phpactor |
|---|---|---|---|
| completion | 0.03 / 0.53 | 0.40 / 2.20 | 8.48 / 17.02 |
| definition | 0.00 / 0.05 | 0.15 / 0.40 | 0.86 / 4.00 |
| references | 0.21 / 0.36 | 0.36 / 0.82 | 0.99 / 2.54 |
| rename | 0.16 / 0.21 | (eroare: fără cheie) | 0.82 / 1.87 |
| hover | 0.03 / 0.06 | 0.23 / 3.30 | 0.43 / 1.01 |
| signature | 0.02 / 0.06 | 0.17 / 2.17 | 0.90 / 2.59 |

  Neschimbate față de 47d (completion 0.06 / 1.18, references 0.24 / 3.98: acum chiar mai
  mici). Prima indexare (laravel, cu `vendor/`): noi 4.7 s, intelephense 2.7 s, phpactor 25.7 s.

### 47f — Golurile găsite de comparație, închise (2026-11-04)

- **Constantele din namespace** (`const X` într-un namespace, `define('App\LIMIT', …)`,
  `use const`): un nume cu majuscule care nu stă unde se scrie un tip (`new X`, `instanceof`,
  `extends`, `X $param`, `X::`, `): X`) e o constantă — „go to definition” o găsește (D06), iar
  Find Usages / Rename prin ea (R04, cu linia `use const`). `define()` cu un nume literal devine
  simbol (`Constant`, cu FQN-ul lui).
- **Variabilele locale** (`locals.rs`): definiția unei variabile e prima legare
  din scopul ei — atribuire, `[$a, $b] =` / `list()`, `foreach` (cheia, valoarea), `catch`,
  `global` / `static` —; un parametru e cel din semnătură (D15), un closure își are `use ($x)`,
  un arrow function vede scopul în care e scris; `$this` și `self::$x` nu. Motorul întreabă acum
  `local_definition` înaintea țintei (o variabilă PHP nu are țintă proprie) — D14.
- **Metodă vs proprietate cu același nume** (`Shop::cart()` / `$cart`, R06): `fits_kind` cere
  apelul pentru o metodă (`->cart(`, `::cart(`, `->cart(...)`, `function cart(`) și lipsa lui
  pentru o proprietate; `member()` din motor alege după același semn (apelat: metoda; altfel
  proprietatea / constanta / cazul), deci și Rename și Find Usages pornesc de la membrul corect.
- **Uniunea la completare** (C29): cârlig nou `receiver_types` (toate clasele receptorului,
  implicit cea a lui `receiver_type`); completarea după `->` unește membrii tuturor (fiecare o
  dată) — `$flag ? new Cart() : new Shop(…)` oferă și `total`, și `checkout`.
- **Hover pe `new Shop(`** (H01): cardul clasei (`class Shop`), cu semnătura constructorului pe
  rândul de sub ea și parametrii lui; ajutorul la parametri ia doar semnătura constructorului.
- **Valoarea constantelor** (H12): constantele claselor, cele din namespace / `define()` și
  cazurile enum-urilor își țin valoarea scurtă în `detail` (` = 50`, ` = 'big'`); cardul arată
  `const MAX_LINES = 50`, `case Active = 'active'`. Extractorul PHP trece la versiunea 3 (indexul
  vechi se reface).
- Teste: `locals` (2: prima atribuire, parametrul, `foreach`, `[$a, $b]`, `catch`, closure /
  `use` / arrow function, `$this`), `infer` (+4: uniunea, metoda vs proprietatea, constantele cu
  valoarea și `define()`, `new` cu constructorul și ajutorul la parametri), `fits_kind`
  (apelul, callable-ul de primă clasă).

| Categoria | 47e | 47f | intelephense | phpactor |
|---|---|---|---|---|
| completion (37) | 36 / 1 / 0 | 37 / 0 / 0 | 35 / 1 / 1 | 32 / 2 / 3 |
| definition (25) | 22 / 0 / 3 | 25 / 0 / 0 | 24 / 0 / 1 | 21 / 0 / 4 |
| references (10) | 8 / 1 / 1 | 10 / 0 / 0 | 10 / 0 / 0 | 6 / 2 / 2 |
| rename (7) | 7 / 0 / 0 | 7 / 0 / 0 | 0 / 0 / 7 | 5 / 1 / 1 |
| hover (14) | 12 / 2 / 0 | 14 / 0 / 0 | 13 / 0 / 1 | 13 / 0 / 1 |
| signature (8) | 8 / 0 / 0 | 8 / 0 / 0 | 7 / 0 / 1 | 6 / 0 / 2 |
| diagnostics (10) | 10 / 0 / 0 | 10 / 0 / 0 | 10 / 0 / 0 | 2 / 0 / 8 |
| noise (5) | 5 / 0 / 0 | 5 / 0 / 0 | 5 / 0 / 0 | 5 / 0 / 0 |
| **total (116)** | **108 (93%)** | **116 (100%)** | **104 (90%)** | **90 (78%)** |

Latența caldă p50 / p95 (ms): completion 0.04 / 0.60, definition 0.01 / 0.08, references
0.30 / 0.45, rename 0.20 / 0.34, hover 0.03 / 0.06, signature 0.02 / 0.07 — practic ca la 47e
(references +0.1 ms: definițiile cerute pentru fiecare loc găsit). Zgomotul rămâne 5 / 5.

### 47g — Metodele numite în șiruri (callable-uri) (2026-11-05)

- **Formele** (`callables.rs`): `[$obj, 'cart']`, `[$this, 'cart']`,
  `[self::class, 'cart']` / `[static::class, …]` / `[Shop::class, …]`, `['App\Shop', 'cart']` —
  un literal de tablou cu două elemente, fără chei, oriunde (deci și în
  `Closure::fromCallable(…)`, `is_callable(…)`, `array_map(…)`) —, forma de șir
  `'App\Shop::cart'` și `method_exists($x, 'cart')` (numărat și el ca folosire). Doar șiruri
  simple (nimic interpolat), un nume valid.
- **Clasa**: cea scrisă (un șir e un nume complet; altfel cum l-ar rezolva fișierul), a lui
  `X::class` (`self` / `static` / `parent` ale clasei de la loc), iar pentru un obiect tipul lui
  dedus — doar când e o singură clasă sigură; un obiect de tip necunoscut nu se numără (locul
  apare la Rename nesigur, nebifat). Metoda trebuie să existe pe clasă (strămoși, mixin-uri):
  un nume luat de `__call` sau un șir oarecare nu e o folosire.
- **Ce merge pe șir**: „go to definition” (⌘-clic pe `'cart'`), hover, Find Usages și Rename —
  ținta e `Static` cu clasa scrisă (goală pentru un obiect: răspunde `member_owner`), `fits_kind`
  primește numele dintr-un astfel de șir pentru o metodă, numele în ghilimele contează drept
  apelat (alege metoda, nu proprietatea cu același nume); Rename rescrie doar numele (ghilimelele
  și partea clasei rămân), pornit și din șir. **Completarea** în șir (`[$this, 'c|']`): context
  nou `ContextKind::Methods` — doar metodele clasei (și statice), inserate ca nume goale.
- Teste: `callables` (formele și ce nu e callable), `infer` (definiția din fiecare formă, obiect
  necunoscut / metodă inexistentă / șir oarecare: nimic, hover, completarea în șir).
- **Sondele** noi (`tools/lsp-compare`): D26–D28 (definiția din `[$shop, '…']`,
  `[Shop::class, '…']`, `'App\Services\Shop::…'`), R11 (folosirile lui `Shop::customer`: cele
  cinci forme + apelul, fără `[$mystery, 'customer']`), N08 (Rename pornit din șir) — 121 de sonde.

| Categoria | 47f (116) | 47g (121) | intelephense | phpactor |
|---|---|---|---|---|
| definition | 25 / 0 / 0 | 28 / 0 / 0 | 26 / 0 / 2 | 21 / 0 / 7 |
| references | 10 / 0 / 0 | 11 / 0 / 0 | 10 / 1 / 0 | 6 / 3 / 2 |
| rename | 7 / 0 / 0 | 8 / 0 / 0 | 0 / 0 / 8 | 5 / 1 / 2 |
| restul (completion, hover, signature, diagnostics, noise) | 74 / 0 / 0 | 74 / 0 / 0 | 70 / 1 / 3 | 58 / 2 / 14 |
| **total** | **116 (100%)** | **121 (100%)** | **106 (88%)** | **90 (74%)** |

Latența caldă p50 / p95 (ms): completion 0.04 / 0.56, definition 0.01 / 0.04, references
0.29 / 0.42, rename 0.19 / 0.43, hover 0.03 / 0.06, signature 0.02 / 0.06 — neschimbată (un șir
se parsează doar când linia arată că locul poate fi într-un șir). Zgomotul rămâne 5 / 5.

### 47h — Un șir e o clasă doar unde PHP îl ia drept clasă (2026-11-05)

- Înainte, „go to definition” pe orice cuvânt dintr-un șir căuta o clasă după numele scurt, fără
  să țină seama de majuscule (`strlen('cart')` → `Cart`). Acum (`callables::class_in_string`) un
  șir numește o clasă doar: în partea de clasă a unui callable (`['Shop', 'cart']`,
  `'App\Shop::cart'`), ca argument al funcțiilor care cer un nume de clasă (`class_exists`,
  `interface_exists`, `trait_exists`, `enum_exists`, `get_parent_class`, `method_exists`,
  `property_exists` — primul; `is_a` — al doilea; `is_subclass_of` — oricare din primele două) și
  oriunde ca nume complet cu namespace (`'App\Service\Mailer'`: un container, o configurare, un
  atribut). Altfel (un cuvânt, o propoziție, o cheie de tablou) nimic.
- Un șir dă un nume complet: ținta e `\App\Cart`; motorul caută un nume scris complet exact
  (fără căutarea după numele scurt), insensibil la majuscule cum face PHP, iar un nume global
  (`\Cart`) cu majusculele lui. Din drum: `use Trait;` în corpul unei clase se rezolvă ca în
  fișier (nu ca nume complet) — se baza pe căutarea după numele scurt (D10).
- Teste: `callables` (pozitive și negative: `strlen('cart')`, `echo 'User'`, cheile, argumentul
  greșit), `infer` (definiția din șiruri: nimic pentru `'cart'` / cheie / `'Cart'` global, clasa
  pentru `class_exists('App\Cart')`, un FQN în configurare, `'app\cart'`), ținta `use Trait;`.
- Comparația: 121 / 121 neschimbat (definition 28 / 0 / 0, zgomotul 5 / 5).

**Testul instabil** `module_manager::tests::nothing_is_in_memory_before_it_is_needed`: starea
comună era `lang_php::stubs::STUBS` (stubs-urile PHP, un static al procesului) — orice alt test
din același binar care întreabă PHP-ul le încarcă în paralel. Testul rulează acum singur, într-un
proces al lui (se pornește pe sine cu `EDITOR_TEST_ALONE`, `--exact`, un fir), unde nimeni nu le
atinge. Rulat de 60 de ori tot binarul testelor aplicației: testul a trecut de fiecare dată.
