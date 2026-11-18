# 03 — Sistemul de module (plug-and-play)

## 1. Ce este un modul

Un modul este o unitate care **contribuie capabilități** la editor printr-un set de contracte. Un modul poate contribui una sau mai multe:

| Capabilitate (`Capability`) | Contract | Exemple |
|-----------------------------|----------|---------|
| `Language` | `LanguageModule` | lang-php, lang-css |
| `Intelligence` | `CompletionProvider`, `DefinitionProvider`, `ReferencesProvider`, `FormattingProvider`, `DiagnosticsProvider` | indexatorul PHP, adaptorul LSP |
| `Theme` | `ThemeProvider` | themes-builtin |
| `Panel` | `PanelProvider` | project-tree, terminal-panel |
| `Commands` | `CommandProvider` | acțiunile specifice fiecărui modul |
| `Settings` | `SettingsSchemaProvider` | schema de setări a modulului |

## 2. Contractul de bază

```rust
// extensions
pub struct ModuleManifest {
    pub id: ModuleId,                 // "lang.php"
    pub name: &'static str,           // "PHP"
    pub version: semver::Version,
    pub capabilities: &'static [Capability],
    pub depends_on: &'static [ModuleDependency], // lang.php depinde de lang.html (opțional: lang.javascript, lang.css)
    pub activation: ActivationPolicy,
}

pub enum ActivationPolicy {
    Eager,                            // la pornire (teme, project-tree)
    OnLanguage(&'static [&'static str]), // la primul fișier cu extensia respectivă
    OnCommand(&'static [CommandId]),
    OnWorkspaceContains(&'static [&'static str]), // ex: "composer.json"
}

pub trait Module: Send + Sync {
    fn manifest(&self) -> &ModuleManifest;
    fn activate(&self, ctx: &mut ModuleContext) -> Result<(), ModuleError>;
    fn deactivate(&self, _ctx: &mut ModuleContext) {}
}
```

`ModuleContext` este **singura poartă** prin care un modul interacționează cu editorul: își înregistrează contribuțiile (`ctx.languages().register(...)`, `ctx.commands().register(...)`), primește setările lui și se abonează la evenimente. Un modul nu primește niciodată referințe directe la servicii interne.

## 3. Registry și încărcare

```
ModuleRegistry
 ├── available: HashMap<ModuleId, Arc<dyn Module>>   // tot ce e compilat (sau, mai târziu, găsit pe disc)
 ├── state:     HashMap<ModuleId, ModuleState>       // Registered → Resolved → Active / Failed / Disabled
 └── contributions: index pe capabilitate
```

### Selecția modulelor (din setări)

```toml
# ~/.config/lumen/settings.toml   sau   <proiect>/.lumen/settings.toml
[modules]
mode = "auto"            # "auto" = toate cele disponibile | "explicit" = doar lista `enabled`
enabled  = ["lang.php", "lang.html", "lang.css", "lang.javascript", "terminal"]
disabled = ["lang.scss"] # în modul auto, excluderi
```

### Algoritm de activare

1. Se colectează modulele disponibile (statice, prin `inventory` sau o funcție `register_all` generată).
2. Se aplică selecția din setări (`auto` minus `disabled`, sau `explicit`).
3. Se rezolvă dependențele (sortare topologică). Dacă o dependență obligatorie lipsește, modulul trece în `Failed` cu mesaj clar în UI, fără panică.
4. Modulele `Eager` se activează imediat, iar celelalte rămân `Resolved` până la declanșare (activare leneșă, pentru pornire rapidă).
5. Activarea rulează sub `catch_unwind`: un modul care dă panic e marcat `Failed`, iar editorul merge mai departe.

> **Cum e de fapt (planul 36e).** Toate modulele selectate se activează la pornire: în
> manifest nu există o politică de activare. Activarea doar înregistrează contribuțiile,
> deci e ieftină. Lenea e mutată mai jos:
> - datele unui modul (stub-uri, gramatici, iconițe) se citesc la prima folosire și se
>   eliberează când nu mai sunt folosite (`OnDemand`, `ModuleManager`);
> - serviciile de proiect nu țin fire: istoricul local și quality pornesc un worker la
>   primul lucru (o salvare), iar firele lor se opresc când n-au de lucru.

## 4. Etapele „pluginurilor”

| Etapă | Mecanism | Când |
|-------|----------|------|
| **E1: module statice** | Crate-uri Rust compilate în binar, activate sau dezactivate din config și prin feature flags Cargo (`--no-default-features --features lang-php`) | Fazele 1–7 |
| **E2: module dinamice WASM** | Runtime `wasmtime` + WIT (component model). Contractele de mai sus se exportă ca interfețe WIT. Modulele terțe rulează izolat (sandbox), cu acces doar prin host functions | Faza 8 |
| **E3: procese externe** | LSP / DAP ca procese separate, gestionate de module; driverele de baze de date externe, prin protocolul driverelor (§6) | Paralel, de la faza 6 (driverele: 37i) |

**De ce nu `.dll`/`.so` cu `libloading`:** Rust nu are ABI stabil, deci orice schimbare de compilator ar strica pluginurile, iar un crash al pluginului dărâmă procesul. WASM oferă izolare și ABI stabil (vezi ADR-0006).

Proiectarea contractelor din E1 trebuie să fie deja **„WASM-friendly”**: tipuri simple (fără lifetimes complicate și fără callback-uri care țin referințe), comunicare prin mesaje și DTO-uri serializabile. Astfel, trecerea la E2 înseamnă un strat de adaptare, nu o rescriere.

## 5. Exemplu de modul (schiță)

```rust
// crates/modules/languages/css/src/lib.rs
pub struct CssModule;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("lang.css"),
    name: "CSS",
    version: semver::Version::new(0, 1, 0),
    capabilities: &[Capability::Language, Capability::Intelligence],
    depends_on: &[],
    activation: ActivationPolicy::OnLanguage(&["css"]),
};

impl Module for CssModule {
    fn manifest(&self) -> &ModuleManifest { &MANIFEST }

    fn activate(&self, ctx: &mut ModuleContext) -> Result<(), ModuleError> {
        ctx.languages().register(Arc::new(CssLanguage::new()))?;
        ctx.intelligence().register_completion(LanguageId::CSS, Arc::new(CssCompletion::new()))?;
        Ok(())
    }
}
```

## 6. Drivere de baze de date externe: protocolul driverelor (planul 37i)

Un driver de baze de date poate veni și din afara aplicației: un **program separat**, scris
în orice limbaj (sau o punte JDBC, dacă e instalat Java), care vorbește **protocolul
driverelor** pe stdin / stdout. Aplicația îl vede ca pe oricare driver al ei
(`db_protocol::ProtocolDriver` implementează porturile `Driver` / `Session` ale domeniului
`database`): exploratorul, grila, modalurile și tranzacțiile nu știu că e extern, doar meniul
`+` îl marchează („extern”). Gazda WASM (E2) va folosi aceleași mesaje.

Cod:

| Crate | Ce face |
|-------|---------|
| `db-protocol` (`crates/infra/database/protocol`) | mesajele versionate (`dto`, cu `serde`), cadrele și erorile (`wire`), conversiile cu modelul domeniului (`convert`), capătul editorului (`client`), adaptorul (`driver`), manifestele (`manifest`), capătul driverului pentru drivere scrise în Rust (`server`), un driver de test în memorie (`toy`, feature `toy`) |
| `database-external` (`crates/modules/database/external`, modulul `database.external`) | citește manifestele și înregistrează driverele găsite (fabricile, fișierele, editorii de valori) |

Domeniul `database` rămâne fără `serde`: mesajele sunt ale protocolului, nu ale modelului
(modelul se poate schimba liber; mesajele doar cu o versiune nouă a protocolului), iar
conversia stă în adaptor.

### 6.1 Instalarea și descoperirea

Un driver stă într-un folder al lui, sub folderul de configurare:
`<config>/drivers/<nume>/driver.toml` (pe Windows `%APPDATA%\<id>\drivers\…`), cu programul
lângă el.

```toml
protocol = { min = 1, max = 2 }   # versiunile protocolului pe care le vorbește
command = "bin/duck-driver"        # programul: în folderul lui, sau în PATH
args = ["--stdio"]
timeout = 600                      # secunde pentru o cerere (opțional, implicit 600)
[env]                              # adăugate la mediul lui (niciodată o parolă: refuzat)
DUCK_HOME = "/opt/duck"

[driver]                           # ce este și ce poate (aceeași descriere ca la handshake)
id = "duckdb"                      # stabil, salvat cu sursele: a-z, 0-9, `.`, `-`, `_`
name = "DuckDB"
version = "1.1"
kind = "relational"                # relational | document | search | key_value
default_port = 4000
sql = "none"                       # none | generic | sqlite | postgres (vezi 6.4)

[driver.capabilities]              # ce nu e spus, nu poate
transactions = true
edit_rows = true
file_based = true
cancel = true
sorting = "any"                    # none | any | { up_to = <octeți> }
schema_changes = { tables = true, columns = true, renames = true }

[driver.capabilities.console]      # consola (protocolul 2; vezi 6.3.1)
cursors = true                     # `fetch` citește restul unui rezultat
explain = { plan = true, analyze = false }
isolation = ["serializable"]       # read_committed | repeatable_read | serializable | deferred | immediate | exclusive
read_only = false                  # `begin` primește `read_only`
switch_schema = "statement"        # none | statement | reconnect | names_only
databases = false
error_positions = true
notices = true

[driver.capabilities.console.profile]   # profilele (protocolul 3; vezi 6.3.2)
estimated = true
actual = true
node_timing = true
buffers = true
table_stats = true
undo_writes = true                 # driverul anulează singur o scriere profilată

[driver.files]                     # fișierele pe care le deschide singur
extensions = ["duckdb"]
header = "DUCK"                    # sau header_hex = "4455434b"

[driver.connection]                # câmpurile modalului (lipsă: cele ale unui server,
fields = [                         # sau fișierul, la unul pe fișier)
  { id = "file", label = "File", kind = { type = "file", extensions = ["duckdb"] }, binding = "file", required = true },
  { id = "mode", label = "Mode", kind = { type = "choice", options = [{ value = "ro", label = "Read only" }] }, binding = { option = "mode" }, tab = "options" },
]
url = { schemes = ["duckdb"], style = "file" }

[[driver.value_editors]]           # editorii grilei pentru tipurile lui
id = "duck-uuid"
types = ["uuid"]
format = "text"                    # text | integer | decimal | bool | date_time | date | time | json | raw | { choice = [...] }
priority = 3
```

- **Leneș:** manifestele se citesc abia prima dată când se cer driverele
  (`database::drivers::register_discovery`): meniul `+`, deschiderea unei surse sau a unui
  fișier. Nu pornește nimic: lista, câmpurile conexiunii, fișierele și editorii vin din
  manifest.
- Un manifest care nu se poate citi e spus în log și sărit; unul cu id-ul unui driver al
  aplicației e lăsat afară (nu îi ia niciodată locul).
- Programul e găsit în folderul manifestului (pe Windows și cu `.exe`, `.cmd`, `.bat`), apoi
  în `PATH`. Pornește direct, fără shell (argumentele trec neschimbate), în folderul lui, fără
  fereastră de consolă pe Windows (`CREATE_NO_WINDOW`), prin `process::start` (infra-ul care
  are voie să pornească programe).

### 6.2 Ciclul de viață

1. **Un proces pe sesiune.** Programul pornește când se deschide o sesiune (prima cerere a
   worker-ului sursei, sau „Testează conexiunea”), pe pool-ul `db`, niciodată pe firul UI.
2. **Handshake:** `initialize`, apoi `connect` cu parola.
3. **Cererile**, una câte una (worker-ul le trimite pe rând); fiecare are un id.
4. **Închiderea:** sesiunea închisă (de utilizator, la inactivitate — `[database]
   idle_close` —, odată cu proiectul) trimite `exit` și închide stdin-ul; programul are 1 s
   să se termine, apoi e oprit cu tot ce a pornit.
5. **Căderea:** dacă programul se oprește singur sau încalcă protocolul (un mesaj care nu e
   al lui), cererile în curs și cele următoare primesc `DbError::Driver(Stopped)`, cu codul de
   ieșire și ultima lui linie de eroare; sesiunea e pierdută, iar următoarea cerere pornește
   programul din nou. Mesajele sunt în `locales` (`database.driver_*`).
6. **Timpul:** handshake-ul are 15 s; o cerere are `timeout` (implicit 600 s), apoi
   programul e oprit (`TimedOut`). O cerere anulată primește `cancel`; dacă nu răspunde în
   5 s, programul e oprit.

### 6.3 Mesajele

Transportul: **JSON-RPC 2.0** în cadre ca la LSP: `Content-Length: <octeți>\r\n\r\n<JSON
UTF-8>` (alte antete sunt permise și ignorate; un cadru are cel mult 256 MiB). O cerere are
`id` și primește un singur răspuns (`result` sau `error`); o notificare nu are `id` și nu
primește răspuns.

| Metoda | Parametri | Rezultat |
|--------|-----------|----------|
| `initialize` | `{"protocol": {"min", "max"}, "editor": "<versiune>"}` | `{"protocol": <versiunea aleasă>, "driver": <descrierea, ca `[driver]`>}` |
| `connect` | `{"target": {host, port, user, database, options, tls}, "password": "…"}` | `{"server": "DuckDB 1.1", "version": <număr, opțional>}` |
| `children` | `{"path": [...]}` | noduri `{kind, name, detail, path, expandable}` |
| `tables` | `{"path": [...]}` | `[{"table": {schema?, name}, "kind": "table" \| "view", "rows"?}]` |
| `structure` | `{"table"}` | `{table, columns, indexes, foreign_keys, rowid, lines}` |
| `read` | `{"table", "page": {offset, limit, order, filter?}}` | `{columns, rows, keys, stopped}` — câte o cheie pe rând când tabelul are cheie |
| `count` | `{"table", "filter"?}` | numărul de rânduri |
| `execute` | `{"sql", "max_rows"?}` | `{columns, rows, affected?, stopped}` |
| `apply` | `{"changes": {table, changes: [insert \| update \| delete]}}` | `{affected, inserted}` |
| `alter` | `{"changes": [<schimbări de structură>]}` | `{affected}` |
| `plan_alter` | `{"changes": [...]}` | un `Script` sau `null` |
| `rebuild_source` | `{"table"}` | ce cere reconstruirea unui tabel (doar la `sql = "sqlite"`) |
| `cancel` (notificare) | `{"id"}` | — (cererea răspunde cu eroarea `cancelled`) |
| `exit` (notificare) | `{}` | — (sesiunea se închide, programul se termină; la fel când se închide stdin-ul) |

- **Valorile:** `"null"`, `{"bool": true}`, `{"int": 5}` (întregi pe 64 de biți, citiți
  exact), `{"float": 1.5}` (cu `"NaN"`, `"Infinity"`, `"-Infinity"` ca text), `{"text": "…"}`,
  `{"bytes": "<base64>"}`, `{"json": "…"}`, `{"other": "…"}` (o dată, un decimal, un UUID, cum
  le scrie motorul).
- **Cheile rândurilor:** `{"primary": [["id", {"int": 1}]]}`, `{"row_id": 5}`,
  `{"line": {"line": 3, "fingerprint": 123}}`.
- **Schimbările de rânduri:** `{"insert": {"values": [["name", {"text": "ana"}]]}}`,
  `{"update": {"key", "cells": [{column, old, new}]}}`, `{"delete": {"key"}}`. `old` e valoarea
  citită: un rând schimbat de atunci e un conflict.
- **Toate sau niciuna:** `apply` și `alter` rulează într-o singură tranzacție; o actualizare
  sau o ștergere care nu schimbă exact un rând e `conflict` (cod 6), iar nimic nu rămâne
  schimbat.
- **Erorile:** codurile JSON-RPC (`-32700` … `-32603`; o metodă necunoscută e „nu se poate”)
  și ale domeniului: 1 conectare, 2 autentificare, 3 interogare, 4 anulat, 5 nu se poate,
  6 conflict, 7 doar citire, 8 fișier ocupat, 9 protocol (`data`: `{"min", "max"}` ale
  driverului), 10 neconectat.
- Toate numele sunt `snake_case`; un enum e un text (`"any"`) sau un obiect cu o singură
  cheie (`{"up_to": 1000}`).

#### 6.3.1 Consola (protocolul 2, planul 39c)

Metodele consolei SQL, doar într-o sesiune care a ales versiunea 2 la `initialize` (una de
versiunea 1 le refuză cu „metodă necunoscută”):

| Metoda | Parametri | Rezultat |
|--------|-----------|----------|
| `run` | `{"sql", "fetch_limit", "keep_cursor"}` — o instrucțiune | rândurile prin `$/rows`, apoi `{affected?, rows, truncated, more?, stopped, notices, in_transaction?}` |
| `fetch` | `{"cursor", "rows"}` | rândurile următoare prin `$/rows`, apoi același rezultat (`more` lipsește când nu mai sunt) |
| `close_cursor` | `{"cursor"}` | `null` (un cursor deja închis nu e o eroare) |
| `explain` | `{"sql", "analyze"}` | planul: `[{operation, object?, detail?, startup_cost?, total_cost?, plan_rows?, actual_rows?, actual_time_ms?, loops?, children}]` |
| `begin` | `{"isolation"?, "read_only"}` | `null` |
| `commit` / `rollback` | `{}` | `null` |
| `set_schema` | `{"schema"}` | `null` |
| `current` | `{}` | `{database?, schema?}` |
| `databases` | `{}` | `["…"]` |
| `catalog_columns` | `{"schema"?}` | `[{table, columns: [{name, type_name, nullable, default?, comment?, key_position?}]}]` — toate tabelele dintr-o dată |
| `routines` | `{"schema"?}` | `[{schema?, name, arguments, result?, kind, arity?, builtin}]` (`kind`: `function` \| `procedure` \| `aggregate` \| `window`) |
| `$/rows` (notificarea driverului) | `{"id", "columns"?, "rows"}` | — |

- **Rândurile în flux:** cât rulează un `run` sau un `fetch`, driverul trimite notificări
  `$/rows` cu `id`-ul cererii: `columns` când începe un set de rezultate (singur, pentru un
  set fără rânduri), apoi rândurile lui, câte cel mult 1 000 de rânduri sau ~1 MiB pe mesaj
  (`db_protocol::driver::CHUNK_ROWS` / `CHUNK_BYTES`). Răspunsul final vine după ultimele
  rânduri. `fetch_limit` e cel mult numărul de rânduri trimise din fiecare set; `truncated`
  spune că au rămas, iar cu `keep_cursor` (și `cursors` în capabilități) `more` e cursorul
  din care le citește `fetch`.
- **Anularea:** `cancel` din versiunea 1, și în mijlocul fluxului (rândurile care mai vin
  după ea sunt ignorate). Când editorul are destule rânduri, trimite el `cancel`; răspunsul
  `cancelled` nu mai e atunci o eroare.
- **Timpul** unei cereri cu rânduri se numără de la ultimul mesaj primit, nu de la început.
- **Erorile unei instrucțiuni** (cod 3) au în `data` ce a spus motorul: `{"sqlstate",
  "position", "offset", "detail", "hint"}` (toate opționale; `position` în caractere de la 1,
  ca PostgreSQL, sau `offset` în octeți de la 0, ca SQLite), locul fiind în textul trimis.
  La versiunea 1 nu se trimite `data`: mesajul are detaliul între paranteze.
- **Tranzacțiile:** `in_transaction` din rezultatul unui `run` spune dacă e deschisă una
  după el (un `BEGIN` scris de utilizator); `begin` / `commit` / `rollback` o schimbă direct.
- **Un driver de versiunea 1** merge mai departe, cu o consolă limitată
  (`ConsoleCaps::limited`, spus în bara consolei): instrucțiunile trec prin `execute` (cel
  mult 100 000 de rânduri, întregi, fără flux și fără locul erorii), tranzacțiile prin
  cuvintele dialectului, catalogul din `tables` + `structure`; fără cursoare, planuri sau
  schimbarea schemei.
- Un driver de versiunea 2 care nu spune `console`, dar are `sql` diferit de `none`, are
  consola de bază: `run` fără cursoare, planuri sau izolare.

#### 6.3.2 Profilele (protocolul 3, planul 39h2)

Doar într-o sesiune care a ales versiunea 3 (una de versiunea 2 le refuză cu „metodă
necunoscută”; editorul face atunci profilul din `explain`, cu operațiile, costurile și
rândurile):

| Metoda | Parametri | Rezultat |
|--------|-----------|----------|
| `profile` | `{"sql", "options": {depth, buffers, verbose, settings, wal, serialize, statement_history, timeout_ms?, max_raw_bytes?}}` — `depth`: `estimated` \| `actual` \| `actual_no_timing` | profilul normalizat (mai jos) |
| `table_stats` | `{"tables": [{schema?, name}]}` | `[{table, live_rows?, dead_rows?, table_bytes?, index_bytes?, total_bytes?, seq_scans?, idx_scans?, last_analyze?, last_autoanalyze?, last_vacuum?, modified_since_analyze?, stats_present?, indexes: [{name, columns, unique, bytes?, scans?}], extra}]` |

- **Profilul** are aceleași nume de câmpuri ca exportul JSON al editorului
  (`database::profile::export::to_json`), deci un profil scris de unul se citește de celălalt:
  `{format, id?, engine: {driver, name, version?, version_num?}, depth, statement: {…}, nodes:
  [{parent, children, role?, subplan_name?, op, name, join?, object?, index?, conditions?,
  estimates?, actuals?, io?, memory?, spill?, parallel?, wal?, flags?, extra?}], roots,
  collected: {…}, notes?, raw?}`. Arborele e plat: `parent` și `children` sunt poziții în
  `nodes`. `op` e felul normalizat (`seq_scan`, `hash_join`, …, `other`), `name` cuvintele
  motorului.
- **`format`** e al profilului, separat de versiunea protocolului (acum 1): un câmp nou e
  opțional; un format mai mare se citește cât se înțelege, cu nota `newer_format`. Un fel, un rol
  sau o unitate necunoscute devin `other` / lipsă, nu o eroare.
- **`extra`** ține tot ce nu are un câmp: `{key, label?, group?, value: {type, value, unit?}}`, cu
  `type` `bool` \| `int` \| `float` \| `text` \| `quantity` (cu `unit`: `rows`, `ms`, `bytes`,
  `blocks`, `pages`, `count`, `percent`, `cost`, `ratio`, `none`) \| `list` (o listă de valori) \|
  `group` (o listă de extra). Nimic nu se aruncă.
- **O profilare rulată** execută instrucțiunea: driverul anulează o scriere (`undo_writes`) și
  o oprește după `timeout_ms`; rândurile ei nu se trimit (`$/rows` nu se folosește).
- **Capabilitățile** profilelor stau în `console.profile` (lipsă: ce dau planurile driverului).
- Exemplu:

```json
{"jsonrpc": "2.0", "id": 9, "method": "profile",
 "params": {"sql": "SELECT * FROM users", "options": {"depth": "actual", "buffers": true}}}

{"jsonrpc": "2.0", "id": 9, "result": {
  "format": 1, "engine": {"driver": "duckdb", "name": "DuckDB", "version": "1.1"},
  "depth": "actual",
  "statement": {"planning_ms": 0.01, "execution_ms": 0.06, "rows_returned": 3},
  "nodes": [{"parent": null, "children": [], "op": "seq_scan", "name": "SEQ_SCAN",
             "object": {"kind": "table", "name": "users"},
             "estimates": {"rows": 3}, "actuals": {"rows": 3, "loops": 1, "total_ms": 0.05},
             "extra": [{"key": "Projections", "value": {"type": "list",
                        "value": [{"type": "text", "value": "id"}]}}]}],
  "roots": [0], "collected": {"estimated": true, "actual": true, "node_timing": true}}}
```

### 6.4 Cine scrie SQL-ul (`sql`)

- **`generic` / `sqlite` / `postgres`:** editorul scrie instrucțiunile în dialectul lui
  (la `postgres`, pentru versiunea pe care o dă `connect`), le arată în previzualizare și le
  rulează prin `execute`, pas cu pas, cu regulile lui (ordinea, „exact un rând”, rollback-ul).
  Ce se previzualizează e exact ce rulează. Paginile, structura și tabelele rămân cereri
  structurate. La `sqlite`, reconstruirea unui tabel își citește sursa prin `rebuild_source`.
- **`none`:** driverul primește schimbările ca atare (`apply`, `alter`) și numără singur
  (`count`); previzualizarea le arată în cuvinte — exact ce se trimite. Pentru schimbările de
  structură driverul își poate scrie propriile instrucțiuni (`plan_alter` întoarce un
  `Script`: `begin`, pașii cu ce trebuie să facă fiecare — `any`, `one_row`, `no_rows`… —,
  `commit`, `rollback`; o instrucțiune goală nu se arată și nu rulează): editorul le arată, iar
  la „Aplică” le replanifică, refuză dacă nu mai sunt exact cele arătate și le rulează el, pas
  cu pas, prin `execute`. `null` înseamnă că driverul le face singur prin `alter`.
- `sql` din handshake trebuie să fie cel din manifest (previzualizările se fac după manifest);
  altfel conexiunea e refuzată. Capabilitățile din handshake sunt cele după care e ținută
  sesiunea (ce nu a spus nu i se cere).

### 6.5 Versiunile

- Versiunea protocolului e un număr întreg; editorul vorbește un interval (acum 1–3,
  `db_protocol::VERSIONS`; 2 a adus consola, 6.3.1; 3 profilele, 6.3.2), driverul tot unul (în manifest și la `initialize`). Se alege cea
  mai nouă versiune comună.
- **Fără versiune comună, driverul e refuzat cu un mesaj clar, în ambele sensuri:** „driverul
  e prea vechi, actualizează-l” sau „editorul e prea vechi”. Intervalul din manifest se
  verifică înainte de pornire (nu pornește nimic); cel din handshake, la `initialize` (eroarea
  9 cu intervalul driverului, sau o versiune aleasă din afara intervalului editorului).
- **Politica:** un câmp nou, opțional, ignorat de cine nu-l știe, nu cere o versiune nouă.
  Orice altă schimbare (un câmp obligatoriu, un sens schimbat, o metodă scoasă) e o versiune
  nouă; editorul păstrează versiunile vechi cât de mult se poate.

### 6.6 Securitatea

- **Parola trece doar prin pipe**, în mesajul `connect`: niciodată în argumentele
  programului, în mediul lui (un manifest cu o variabilă cu nume de parolă e refuzat) sau
  într-un log. Octeții cadrului care o conțin sunt șterși din memorie după scriere (și, în
  `server`, după citire); `Debug`-ul mesajului o ascunde.
- **Ieșirea de erori** a programului (stderr) e citită pe un fir al ei, câte o linie, cu
  parola sesiunii înlocuită cu `••••`; se păstrează ultimele 64 de linii (cea mai nouă intră
  în mesajul unei căderi) și fiecare apare în logul aplicației la nivel `debug`.
- Driverul rulează cu drepturile utilizatorului: se instalează doar drivere în care ai
  încredere (ca orice program).

### 6.7 Cum scrii un driver

- **În Rust:** implementezi `database::Driver` / `Session` (ca driverele aplicației) și îl
  servești: `db_protocol::server::serve(driver, descriere, db_protocol::VERSIONS, stdin,
  stdout)`. `serve` face handshake-ul, ține sesiunea, răspunde pe rând și duce `cancel` la
  cererea care rulează; metodele consolei cheamă `Session::run` / `fetch` / `explain` /
  `profile` / `table_stats`… ale
  driverului (cu implicitele domeniului pentru ce nu are), iar rândurile lor pleacă în
  `$/rows` pe măsură ce vin.
- **În alt limbaj:** citești cadrele de pe stdin și răspunzi pe stdout (nimic altceva pe
  stdout; logurile merg pe stderr). Stdin-ul se citește tot timpul, și cât rulează o cerere
  (acolo vine `cancel`; un pipe necitit, odată plin, l-ar bloca pe editor la scriere). Un schimb minim:

  ```text
  → {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":{"min":1,"max":2},"editor":"0.8.7"}}
  ← {"jsonrpc":"2.0","id":1,"result":{"protocol":1,"driver":{"id":"duckdb","name":"DuckDB","sql":"none","capabilities":{"edit_rows":true}}}}
  → {"jsonrpc":"2.0","id":2,"method":"connect","params":{"target":{"database":"/data/a.duckdb"},"password":""}}
  ← {"jsonrpc":"2.0","id":2,"result":{"server":"DuckDB 1.1"}}
  → {"jsonrpc":"2.0","id":3,"method":"read","params":{"table":{"name":"users"},"page":{"offset":0,"limit":500}}}
  ← {"jsonrpc":"2.0","id":3,"result":{"columns":[{"name":"id","type_name":"INTEGER"}],"rows":[[{"int":1}]],"keys":[{"primary":[["id",{"int":1}]]}],"stopped":false}}
  → {"jsonrpc":"2.0","method":"exit","params":{}}
  ```

  Iar o rulare din consolă (protocolul 2):

  ```text
  → {"jsonrpc":"2.0","id":4,"method":"run","params":{"sql":"SELECT * FROM users","fetch_limit":500,"keep_cursor":true}}
  ← {"jsonrpc":"2.0","method":"$/rows","params":{"id":4,"columns":[{"name":"id","type_name":"INTEGER"}],"rows":[[{"int":1}],[{"int":2}]]}}
  ← {"jsonrpc":"2.0","id":4,"result":{"rows":2,"truncated":false,"stopped":false,"in_transaction":false}}
  → {"jsonrpc":"2.0","id":5,"method":"run","params":{"sql":"SELECT nope FROM users","fetch_limit":500}}
  ← {"jsonrpc":"2.0","id":5,"error":{"code":3,"message":"column \"nope\" does not exist","data":{"sqlstate":"42703","position":8}}}
  ```

- Testat: driverul de test (`toy`) și SQLite servit prin protocol, pornite ca programe de
  testul `crates/modules/database/explorer/tests/protocol.rs` (structura, paginile, schimbările
  cu conflict, instrucțiunile driverului rulate cum au fost arătate, anularea, căderea și
  repornirea, versiunea refuzată, parola doar prin pipe); SQLite dă prin protocol exact ce dă
  direct, inclusiv instrucțiunile planificate. Consola: rândurile în flux identice cu cele
  date direct, cursorul citit până la capăt, anularea în mijlocul fluxului, eroarea cu locul
  ei, consola SQLite prin protocol ca direct, și un driver de versiunea 1 cu consola
  limitată.
