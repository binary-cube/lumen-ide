# Consola SQL și fereastra „Services” (planul 39)

Cerut: „În partea de DB vreau, ca într-un IDE complet, să scriu query-uri și să le rulez: când sunt
pe un tabel sau pe o bază, se deschide un tab nou în editor unde scriu query-uri, le rulez și
le văd rezultatul. Mai e și zona Services, jos în stânga. Fă un plan de bătaie profesionist
pentru toate acestea. Trebuie să știe pe ce bază rulează, să aibă autocomplete, colorare
SQL, tot ce face un IDE complet.”

Din capturile de referință reținem:

1. **Fereastra „Database”** are în bara ei „Jump to Query Console… ⇧⌘F10”, lângă DDL și
   deschiderea tabelului.
2. **Tab-ul consolei** se numește `console [acme:serv…es@localhost]` și are iconița
   driverului. Bara lui are, în ordine:
   - Run (▶ verde), cu opțiunile de rulare și Explain;
   - istoricul (ceasul), parametrii (`p`), setările, comutatorul panoului;
   - „Tx: Auto”, Stop (pătratul), „Playground” / „Script”;
   - în dreapta, alegerea bazei și a schemei (`acme.<schema>`).

   Completarea listează tabelele împreună cu sursa lor
   (`doctrine_migration_versions   acme:server:prod:postgres@localhost`), iar sus, în
   dreapta editorului, apare numărul de erori.
3. **Fereastra „Services”, jos**, are două părți:
   - în stânga, un arbore: Docker, Database › sursă › sesiunea `console 543 ms` și tabelele
     deschise;
   - în dreapta, tab-ul „Output” și câte un tab pentru fiecare rezultat
     (`acme.public.doctr…ne_migration_versions`). Fiecare tab de rezultat are o grilă cu
     „89 rows”, controalele Tx, exportul CSV și altele.
4. **„Output” e un jurnal:**
   - `[2026-10-31 10:00:23:698] Connected to acme`;
   - `[…] acme.public> select * from doctrine_migration_versions`;
   - `[…] 89 rows retrieved starting from 1 in 411 ms (execution: 43 ms, fetching: 368 ms)`.

   Pe lateral are butoane: soft wrap, derulare la capăt, tipărire, golire. Breadcrumb-ul de
   jos arată `Database Consoles › sursă › console`.

Planul 13 (`13-baze-de-date.md`) e viziunea: consola SQL, `lang-sql`, SQL-ul din PHP.
Planul 37 (`37-surse-de-date.md`) a construit sursele, driverele, grila și fereastra
„Database”. Planul 39 e drumul concret de acolo până la consolă, fără să piardă nimic din ce
există.

## Ce avem deja (pe ce construim)

- **Domeniul `crates/domain/database/database`** (pur, fără I/O și fără ceas):
  - **`Session`** are două lucruri de care avem nevoie:
    - `execute(query, sink, cancel)`: o instrucțiune, cu rândurile în flux într-un
      `RowSink` (`columns` apelat la fiecare set nou, `row` → `Flow::Stop`);
    - `Outcome { affected, rows, stopped }`, `Cancel` și `dialect()`, pe versiunea
      serverului.

    Mai are `tables`, `structure`, `read`, `count`, `apply`, `alter` și `plan_alter`.
  - **`Driver`** dă `kind()`, `language()` (`Language::Sql` / `Json` / `Commands`),
    `capabilities()` și `dialect()`.
  - **`Capabilities`** spune ce poate driverul: `transactions`, `edit_rows`,
    `schema_changes`, `schemas`, `file_based`, `cancel`, `returning`, `sorting`.
  - **`Dialect`** știe citarea identificatorilor și a literalelor (`quote_identifier`,
    `literal`), `pagination`, `select`, `count`, `changes`, `begin` / `commit` /
    `rollback`. Implementări: `GenericSql`, SQLite și `Postgres { version }`.
  - **`transaction`** are `Script`, `Step`, `Expect`, `run` și `apply_script`.
  - **`danger`** are `Danger`, `schema_dangers`, `change_dangers` și `script_dangers`.
    Atenție: `script_dangers` găsește doar obiectele pierdute la o reconstruire SQLite.
    **Nu există încă o analiză a textului SQL** (un `DELETE` fără `WHERE`, un `DROP`):
    o facem aici.
  - **`relations`** (37j) are `RelationNotes`: cheile virtuale acceptate și cele ignorate.
    **`diagram`** are `SchemaGraph` și portul `SchemaAccess`, adică precedentul pentru o
    graniță între module.
  - **`folders`** are `SourceColor`, `effective_color` și `same_connection`.
  - **`events`** are `DataSourcesChanged` și `DataChanged { source, table, structure }`.
  - **`DbError`** are `Query(String)`: fără poziție, fără SQLSTATE.
- **Infra** (`crates/infra/database/*`):
  - **`db-sqlite`**: un `execute` doar pentru o instrucțiune (`prepare` + pași), anulare
    prin `progress_handler`, `busy_timeout` 5 s.
  - **`db-postgres`**: `execute` prin `simple_query_raw`, cu tipurile coloanelor dintr-un
    `prepare` când e o singură instrucțiune, mai multe seturi de rezultate, anulare prin
    `cancel_query` (și la `Flow::Stop`), versiunea din `server_version_num`. Mesajul
    erorii păstrează `detail`, dar pierde `position`, `hint` și codul.
  - **`db-protocol`**: protocolul v1, JSON-RPC cu cadre `Content-Length`. Metoda `execute`
    e limitată la 100 000 de rânduri (`EXECUTE_ROWS`) și nu trimite rândurile în flux.
    Starea 37i a notat deja: „consola SQL va avea nevoie de rânduri în flux”.
- **Modulul `database-explorer`** (`crates/modules/database/explorer`):
  - `SessionWorker`, cu cererile `Tables`, `Structure`, `Count`, `ReadPage`, `Apply`,
    `Alter`, `PlanAlter`, `AlterAsShown` și `Close`, rulate pe pool-ul `db`;
  - `Sessions`, pe proiect: `source_worker`, `open_with` (parola citită în fundal din
    `credentials`, scriptul de pornire rulat), `IDLE_CLOSE`;
  - `Catalog` (sursele, folderele, `color_of`, `generation`), `credentials` (seiful
    proiectului), `FileIdentities`;
  - grila (`view`, `grid_source::TableSource`), exportul (`export`), previzualizarea
    (`preview`), modalele de structură (`schema`) și de surse (`sources`);
  - fereastra „Database” (`window`, `ToolWindowProvider` pe dreapta, butoanele din antet
    `add` / `refresh` / `collapse`, meniurile rândurilor) și setările
    (`settings.rs`: `page_size`, `idle_close`, `csv_sample`, `confirm_delete`).
- **`ui-components`:**
  - `data_grid` (`GridSource`, `DataGrid`, bara de unelte în `data_grid::toolbar`, copierea,
    `CopyStream`, editorii de valori);
  - `ToolWindowProvider`, cu `Dock::Bottom` și `ToolContent::Views` (terminalul, Git,
    Remote);
  - `Viewer`, cu `accent`, `poll`, `command`, `context_menu`, `tooltip_at` și
    `take_notifications`;
  - `ToolPanel`, `tree::TreeList`, `toast`, `tooltip`, `code_tokens` (colorarea
    cuvintelor cheie în afara editorului) și `diagram`.
- **`workbench-model`:**
  - `WorkbenchHost`, cu `open_view`, `show_in_panel`, `open_modal`, `open_overlay`,
    `confirm`, `choose`, `notify` (toast-urile din planul 38), `request_command` și
    `events()`;
  - `ProjectService` și `ProjectContext` (`events`, `storage`, `notifier`, `wake`);
  - `DiagnosticsFound` pe bus, afișate de aplicație ca erorile ei;
  - `LineDecorator` (marcaje în gutter), `CommandContribution`, `MenuContribution`,
    `SettingsContribution` și `ResourceReporter`.
- **Limbajele:**
  - `syntax-ts::Grammar` (highlights, injections, folds, indents);
  - `LanguageDescriptor` (extensii, comentarii, iconiță), `LanguageIntelligence`
    (`context`, `keywords`, `completion_triggers`…), `InjectionProvider`, `ContentSniffer`;
  - gramaticile vendorizate în `third_party/` (twig, blade), cu `LICENSE` și `Cargo.toml`
    proprii.

  Injecția PHP din heredoc-uri e scrisă deja (`<<<SQL`, în
  `languages/php/queries/injections.scm`): se aprinde singură când există limbajul `sql`.
  Fișierele `.sql` au deja iconița `database` (`file-icons`).
- **Completarea în aplicație** (`app/src/workbench/intelligence.rs::update_completion`)
  e sincronă, pe firul UI. Întreabă întâi framework-urile (`FrameworkSupport::complete`,
  per proiect), apoi limbajul. **Nu există un punct prin care un modul să completeze un
  anumit document după date pe care doar el le știe** (sursa atașată unei console).
  Îl adăugăm (39j).
- **Comenzile:** `command_model::When` are `Everywhere`, `Editor` și `View(kind)`.
  Tastele ⌘↵ sunt ale grilei (`database.submit`), cu `When::View`.

## De ce un plan separat, și ce înseamnă „ca într-un IDE complet”

Consola nu e „încă o vedere a grilei”. E un fișier de text cu un limbaj, cu o conexiune
proprie (deci o tranzacție proprie), un jurnal, rezultate multiple și o inteligență care
citește schema reală.

Într-un IDE complet pentru baze de date, cineva care lucrează pe o bază de producție se bazează pe:

- consola știe mereu pe ce sursă, bază și schemă rulează, și o arată (culoarea sursei);
- se rulează instrucțiunea de la cursor (⌘↵) sau selecția; scriptul întreg, în modul
  „Script”;
- tranzacția e automată sau manuală (Commit / Rollback), iar o tranzacție deschisă se vede;
- nimic distructiv nu pleacă fără confirmare: `DELETE` / `UPDATE` fără `WHERE`, `DROP`,
  `TRUNCATE`, mai ales pe producție;
- o interogare lentă se poate opri;
- rezultatele vin pe pagini; timpul e despărțit în execuție și aducere;
- erorile arată locul exact în text;
- completarea știe tabelele, coloanele, aliasurile, CTE-urile și `JOIN … ON` după cheile
  externe;
- ⌘-click pe un tabel îl deschide; hover-ul îi arată coloanele;
- istoricul rămâne pe sursă;
- consolele sunt fișiere care se redeschid.

## Principii

1. **O consolă e un fișier `.sql` deschis în editor**, nu o vedere desenată de mână.
   - Primește gratuit tot editorul: multi-cursor, undo, find / replace, salvare automată,
     istoricul local, mutarea liniilor, plierea.
   - Peste el, un modul adaugă o bară (39e: un punct de extensie nou, generic) și
     inteligența legată de sursă (39j).
   - Consolele sunt deci fișiere (de ex. `consoles/db/<uuid>/console.sql`).
2. **Totul e un modul, leneș.**
   - **`lang-sql`** (`crates/modules/languages/sql`): limbajul. Nu știe de baze de date.
   - **`database-console`** (`crates/modules/database/console`): consolele, rularea,
     rezultatele, contribuția la „Services”, inteligența legată de schemă.
   - **`services-panel`** (`crates/modules/tools/services`): fereastra „Services”,
     generică.
   - Nimic nu pornește până nu se deschide o consolă. O consolă închisă își închide
     conexiunea și își eliberează rezultatele, iar indexul schemei se eliberează după un
     timp fără console pe acea sursă.
3. **Modulele nu depind unele de altele.**
   - Consola ajunge la surse, parole și sesiuni printr-un port din domeniu,
     `database::console::ConsoleAccess`, implementat de explorer. Precedentul e
     `SchemaAccess`, implementat pentru diagramă în 37j.
   - Limbajul nu știe de consolă; consola nu știe de limbaj decât după id-ul `sql`.
   - „Services” nu știe de baze de date: le primește prin contribuții.
4. **Regulile SQL sunt pure și în domeniu.**
   - Ce intră acolo: lexerul, împărțirea în instrucțiuni pe dialecte, felul fiecărei
     instrucțiuni, parametrii, pericolele, domeniul de vizibilitate pentru completare
     (aliasuri, CTE), maparea pozițiilor de eroare și formatarea (mai târziu).
   - Toate se testează fără bază și fără UI.
   - Tree-sitter servește doar colorarea și structura din editor. Rularea nu depinde de
     gramatică.
5. **Nimic greu pe firul UI.**
   - Pe pool-uri numite (`kernel::work`) rulează conectarea, rularea, aducerea rândurilor,
     introspecția, construirea indexului schemei, deducerea relațiilor și scrierea
     istoricului.
   - Pe firul UI rămân doar căutări în structuri deja construite, cu buget: completarea sub
     2 ms la 5 000 de tabele.
   - Nimic nu se lucrează pe cadru dacă nu s-a schimbat nimic: `kernel::Memo` pe versiunea
     documentului și pe generația indexului.
6. **Fiecare proiect e al lui.**
   - Consolele, sesiunile, rezultatele, istoricul și indexul schemei stau în datele
     proiectului.
   - Evenimentele (`ConsoleRan`, `TransactionChanged`, `SchemaInvalidated`) merg pe
     bus-ul proiectului.
   - Nimic nu trece între proiecte; o sursă globală deschisă în două proiecte are în
     fiecare consolele ei.
7. **Siguranța e înaintea comodității.**
   - O sursă doar citire refuză scrierile înainte să plece, iar driverul o deschide oricum
     doar citire.
   - Pericolele cer confirmare; pe producție, orice scriere în mod automat cere confirmare.
   - O tranzacție deschisă nu se pierde niciodată pe tăcute: închiderea pe inactivitate o
     ocolește, iar închiderea consolei întreabă.
   - Parolele nu apar în jurnal, în istoric sau în fișiere.
8. **Identitățile sunt UUID v4** (consola, sesiunea consolei, tab-ul de rezultat, nodurile
   din „Services”). Numele și pozițiile nu sunt identități.
9. **Fără numele aplicației în identificatori.** Crate-urile se numesc `lang-sql`,
   `database-console`, `services-panel`; pool-urile `db-console`, `db-schema-index`;
   extensiile virtuale `view-console-*`, `view-services-*`.

## Deciziile mari (cu motivul)

- **Consola e un fișier, nu un `Viewer`.**
  - **De ce:** un editor de text al modulului ar fi dublat tot editorul aplicației, sau ar
    fi rămas mult în urmă.
  - **Ce trebuie în aplicație:** o bară deasupra editorului, contribuită de un modul pentru
    fișierele pe care le recunoaște, ca un `Viewer` mic (`EditorToolbarProvider`, 39e).
    Prin ea vin și culoarea tab-ului (ca `Viewer::accent`), titlul tab-ului și începutul
    breadcrumb-ului.
  - **Ce câștigăm:** același mecanism servește și un `.sql` obișnuit din proiect, căruia
    i se atașează o sursă (39n).
- **Rezultatele merg în „Services”, jos.**
  - Consola e editorul; rezultatele sunt ale sesiunii.
  - O variantă „rezultatele sub editor, în tab” rămâne pentru mai târziu, ca setare
    (vezi întrebările).
- **„Services” e generic, iar baza de date e primul ei contributor.**
  - Punctul de extensie e `ServiceProvider`, în `ui-components`. Contractul întoarce
    `Viewer`-e, deci stă lângă `ToolWindowProvider`.
  - Contributorii următori: sesiunile terminalului, configurațiile de rulare și Docker,
    când vor exista.
  - O fereastră doar pentru baza de date ar fi trebuit refăcută la al doilea contributor;
    o fereastră comună pentru toate e soluția obișnuită.
  - Fereastra stă jos (`Dock::Bottom`). E un singur panou cu arbore și tab-uri proprii,
    deci aplicația învață să așeze un `ToolContent::Panel` și jos, nu doar pe laterale
    (39d, schimbare generică mică).
- **O conexiune pe consolă.**
  - Tranzacția manuală a unei console nu are voie să se amestece cu commit-urile grilei
    sau ale altei console: fiecare consolă are sesiunea ei, implicit.
  - Worker-ul explorer-ului rămâne neschimbat. Consola își are `ConsoleRunner`-ul ei, pe
    pool-ul `db-console`, cu conexiunea făcută prin `ConsoleAccess::connector`: aceeași
    parolă, același script de pornire, același „doar citire”.
  - La SQLite, o tranzacție manuală ținută deschisă blochează scrierile grilei pe același
    fișier: `busy_timeout`, apoi `DbError::InUse`, cu mesajul care spune că o consolă are o
    tranzacție deschisă.
- **CSV-ul nu are consolă (acum).**
  - Driverul CSV nu scrie SQL (`dialect() == None`).
  - Variantele ar fi fost un motor SQL propriu peste CSV (imens) sau importul într-un
    SQLite în memorie. Importul ar însemna o a doua copie a datelor în RAM la fișiere de
    sute de MB, alte tipuri decât cele inferate, iar o editare nu s-ar mai întoarce în
    fișier.
  - Decizia: consola se oferă doar pentru surse cu dialect (`Driver::dialect().is_some()`,
    sau `Session::dialect()` după conectare). Pentru CSV, „Jump to Query Console” e
    dezactivat, cu indiciul „sursa nu are SQL”.
  - „Interoghează CSV-ul prin SQLite” (import într-un `:memory:` atașat, doar citire) e în
    „Ce nu facem acum”.
- **Parametrii se pun în text, prin dialect.**
  - `?`, `:name`, `$1` și `@name` (configurabile) se înlocuiesc cu literalele scrise de
    `Dialect::literal` (citare sigură), sau cu text brut când utilizatorul alege „expresie”.
  - Așa merg la fel pe SQLite, pe Postgres și prin protocolul driverelor externe, iar
    previzualizarea și istoricul arată exact ce rulează.
  - Lexerul nu atinge parametrii din șiruri, comentarii sau corpuri `$$ … $$`.
- **Paginile de rezultate.**
  - Prima pagină (setarea `console_fetch_limit`, implicit 500) vine în
    flux. Driverul care poate ține un cursor deschis (`Capabilities::console.cursors`:
    Postgres, printr-un portal, într-o tranzacție) aduce pagina următoare din același
    cursor.
  - Fără cursor (SQLite, driverele v1):
    - o interogare pură (`SELECT`, `WITH … SELECT`, `VALUES`) se rulează din nou de la
      rândul următor, prin paginarea dialectului peste interogare;
    - o instrucțiune care scrie (`… RETURNING`) nu se rulează niciodată din nou: arată ce a
      încăput, cu mesajul „rândurile rămase n-au fost aduse”.
  - Motivul: un fir din pool ținut blocat în sink până cere cineva pagina următoare ar fi
    ocupat pool-ul pe termen nelimitat.

## Organizarea pe foldere și crate-uri

```
crates/domain/database/database/src/
    sql/                    nou, pur: lexer, împărțire, fel, parametri, scope, format
        mod.rs  lex.rs  split.rs  classify.rs  params.rs  scope.rs  errors.rs  format.rs
    console.rs              nou: ConsoleId, ConsoleSpec, TxMode, RunMode, TxState,
                            RunPlan, ConsoleAccess (portul), registrul per proiect
    history.rs              nou: QueryHistory (modelul și regulile), HistoryStore (portul)
    schema_index.rs         nou: SchemaIndex (numele, coloanele, cheile, rutinele)
    plan.rs                 nou: PlanNode (EXPLAIN citit ca arbore)
    danger.rs               + statement_dangers
crates/infra/database/{sqlite,postgres,protocol}   + rularea consolei (39c)
crates/infra/files/storage/src/
    consoles.rs             nou: TomlConsoles (consoles.toml)
    query_history.rs        nou: TomlQueryHistory (adăugare la capăt, compactare)
crates/modules/languages/sql/        nou: `lang-sql` (modulul `lang.sql`)
crates/modules/database/console/     nou: `database-console` (modulul `database.console`)
crates/modules/tools/services/       nou: `services-panel` (modulul `services`)
third_party/tree-sitter-sql/         nou: gramatica vendorizată (39b)
crates/ui/ui-components/src/
    editor_toolbar.rs       nou: EditorToolbarProvider / EditorToolbar
    services.rs             nou: ServiceProvider / ProjectServices / ServiceNode, registrul
    log_view.rs             nou: jurnalul (Output), generic
crates/domain/code/intelligence/src/assist.rs   nou: DocumentAssist (completare, definiție,
                                           hover pentru documentele știute de un modul)
```

- **Dependențe:**
  - `database-console` depinde de `database`, `kernel`, `extensions`, `workbench-model`,
    `settings-model`, `command-model`, `code-intelligence`, `i18n`, `ui-components`,
    `ui-kit`, `render-api`, `text-layout`, `icons` și `storage` (permis în `MODULE_INFRA`).
  - `lang-sql` depinde de `languages`, `syntax-ts`, `code-intelligence` și
    `tree-sitter-sql` (din `third_party`).
  - `services-panel` depinde doar de `ui-components`, `workbench-model`, `i18n`, `icons`
    și `kernel`.
  - Niciun crate infra nou de modul: `MODULE_OWNED_INFRA` nu se schimbă.
  - `cargo xtask check-deps` trece neschimbat.
- **Explorer-ul** implementează `ConsoleAccess`. Câștigă comanda „Jump to Query Console”
  (cu butonul din antet și rândurile din meniuri) și evenimentul `SchemaInvalidated` la
  „Refresh”. Nu depinde de consolă.

## Domeniul (`crates/domain/database/database`)

### `sql`: lexerul și împărțirea (39a)

- **`Flavor`:** `Generic`, `Sqlite`, `Postgres { version }`, `MySql` (pregătit pentru
  driverele viitoare, nefolosit încă).
  - `Dialect` primește `fn flavor(&self) -> Flavor`, implicit `Generic`.
  - SQLite și Postgres îl dau pe al lor.
- **`lex(text, flavor) -> Vec<Token>`:** un singur pas, fără alocări pe token (intervale
  de octeți). `TokenKind` poate fi:
  - cuvânt (cuvânt cheie sau nume);
  - identificator citat: `"x"`; `` `x` `` la MySql; `[x]` la SQLite, unde merg și celelalte
    două;
  - șir: `'…'` cu `''` dublat; `E'…'` cu backslash la Postgres; `X'…'` binar;
  - corp `$tag$ … $tag$` (Postgres);
  - număr, operator, `::` (Postgres), punctuație, `;`;
  - comentariu de linie: `--`; `#` la MySql;
  - comentariu de bloc: `/* */`, imbricat la Postgres;
  - parametru: `?`, `?1`, `:name`, `@name`, `$1`;
  - spațiu.

  Un șir sau un comentariu neînchis ține până la capăt și e marcat (`unterminated`), fără
  panică: textul din editor e des neterminat.
- **`split(tokens) -> Vec<StatementSpan>`:**
  - instrucțiunile, despărțite prin `;` doar la nivelul de sus (nu în șiruri, în
    comentarii, în `$$` sau între paranteze);
  - `BEGIN … END` dintr-un `CREATE TRIGGER` SQLite și blocurile `BEGIN ATOMIC … END`
    Postgres ≥ 14 rămân întregi;
  - o linie cu doar `GO` sau `/`, ca separator, e opțională (setare, pentru scripturi
    copiate);
  - instrucțiunile goale sau doar comentarii sunt sărite;
  - fiecare `StatementSpan` are intervalul fără spațiile de margine, intervalul cu
    comentariul de dinainte (pentru „rulează instrucțiunea”) și primul cuvânt.
- **`statement_at(spans, offset)`**, după regula obișnuită: instrucțiunea care conține
  cursorul. Pe un rând gol între două, cea de deasupra, dacă cursorul e chiar după ea;
  altfel niciuna (bara spune „pune cursorul pe o instrucțiune”).
- **`classify(span) -> StatementKind`:**
  - `Query` (`SELECT`, `WITH … SELECT`, `VALUES`, `TABLE x`, `SHOW`, `PRAGMA` care
    citește);
  - `Write(WriteKind::{Insert, Update, Delete, Merge, Upsert})`, inclusiv un `WITH` care
    scrie;
  - `Ddl(DdlKind::{Create, Alter, Drop, Truncate, Rename, Comment, Grant})`;
  - `Transaction(TxControl::{Begin, Commit, Rollback, Savepoint, Release, RollbackTo})`;
  - `Explain { analyze }`, `Session` (`SET`, `USE`, `PRAGMA` care scrie, `ATTACH`),
    `Other`.

  `writes()` spune dacă poate schimba ceva: `Write`, `Ddl`, `Explain { analyze: true }`
  peste o scriere, `Other` (necunoscutul e tratat ca scriere). `touches_schema()` spune
  dacă e DDL (indexul schemei se invalidează după el).
- **`params(tokens) -> Vec<Parameter>`:** nume (`Positional(n)`, `Numbered(n)`,
  `Named(s)`) și locurile lui. Același `:id` de două ori e un singur parametru.
  `bind(text, params, values, dialect)` dă textul cu literalele puse. Un parametru fără
  valoare e o eroare a cererii, nu un `NULL` tăcut.
- **`mask_secrets(statement)`:** literalul de după `PASSWORD`, `IDENTIFIED BY` sau
  `ENCRYPTED PASSWORD` devine `'••••'`, pentru jurnal și istoric (39f / 39i).
- **`errors::locate(statement_range, engine_position)`:** poziția motorului devine un loc
  în document (linie și coloană).
  - Postgres dă o poziție de caracter, numărată de la 1, în instrucțiunea trimisă.
  - SQLite dă un offset în octeți (`sqlite3_error_offset`, 3.38+), unde rusqlite îl expune.
  - Parametrii puși în text mută pozițiile; harta lor (`BindMap`) le întoarce.

### Pericolele din text (39a, în `danger`)

`statement_dangers(span, flavor) -> Vec<Danger>`, cu variante noi:

- `UnfilteredWrite { kind: Update | Delete, table }`: un `UPDATE` sau `DELETE` fără `WHERE`
  la nivelul lui. Un `WHERE` dintr-o subinterogare nu contează. `WHERE true` sau
  `WHERE 1=1` contează ca lipsă.
- `DropObject { kind, name }`: `DROP TABLE` / `VIEW` / `INDEX` / `SCHEMA` / `DATABASE` /
  `FUNCTION`…, cu `CASCADE` spus.
- `Truncate { table }`.
- `AlterDrop { table, column }`: `ALTER TABLE … DROP COLUMN`.

Pericolele vechi (`schema_dangers`, `change_dangers`, `script_dangers`) rămân neschimbate.

### Consola (`console`, 39c–39g)

- **`ConsoleId`:** UUID v4, prin `kernel`, ca `DataSourceId`.
- **`ConsoleSpec`:** ce se ține despre o consolă:
  - `id`, `source: DataSourceId`, `name` (`console`, `console_1`…), `file` (relativ la
    dosarul proiectului);
  - `database` și `schema` alese (`None`: ale conexiunii);
  - `tx: TxMode`, `isolation: Option<Isolation>`, `mode: RunMode`;
  - `last_run` (durata, pentru arbore), `params` (numele ținute minte, fără valori).
- **`TxMode`:** `Auto` (fiecare instrucțiune se confirmă singură) sau `Manual` (primul
  lucru care scrie deschide tranzacția, iar ea rămâne deschisă până la Commit / Rollback).
- **`Isolation`:**
  - Postgres: `ReadCommitted`, `RepeatableRead`, `Serializable`, plus `read_only`;
  - SQLite: felul `BEGIN`-ului (`Deferred`, `Immediate`, `Exclusive`).

  Ce oferă un driver vine din `ConsoleCaps::isolation`.
- **`RunMode`:**
  - `Playground`: ⌘↵ rulează instrucțiunea de la cursor sau selecția; cu mai multe
    instrucțiuni selectate, pe rând.
  - `Script`: ⌘↵ rulează tot fișierul (sau selecția) ca script, fără întrebarea „care
    instrucțiune?”.
- **`TxState`** e o mașină de stări pură:
  - stări: `Idle`, `Open { since_statement, writes }`, `Failed` (Postgres: tranzacția e
    abandonată după o eroare și cere `ROLLBACK`);
  - tranziții: `on_statement(kind, ok)` și `on_control(TxControl)`;
  - în `Manual`, un `BEGIN` scris de utilizator e acceptat, iar un `COMMIT` scris închide
    starea;
  - în `Auto`, un `BEGIN` scris trece consola în tranzacție până la `COMMIT`.
- **`RunPlan`:** din document, selecție, cursor și mod se fac `PlannedStatement`-urile
  (textul, intervalul în document, felul, parametrii, pericolele). Se decide ce se cere
  înainte de rulare:
  - parametrii (39h);
  - confirmarea pericolelor și a producției (39g);
  - refuzul la o sursă doar citire: o scriere nu pleacă, cu motivul.

  `stop_on_error` vine din setare (implicit da în `Script`).
- **`ConsoleAccess`** e portul, implementat de explorer. Nu se cheamă niciodată pe firul
  UI pentru ce poate aștepta.
  - `sources() -> Vec<SourceBrief>`: id, nume, driver, culoare, producție, doar citire,
    are dialect.
  - `source(id)`.
  - `connector(id, overrides) -> Result<Connector, DbError>`. `Connector` e
    `Arc<dyn Fn() -> Result<Box<dyn Session>, DbError> + Send + Sync>`, adică
    `worker::Connect`. Parola e citită la conectare, iar baza aleasă în consolă intră prin
    `overrides`.
  - `color(id)` și `production(id)`: ieftine, cerute la desen.
  - `open_table(id, table) -> Option<Opening>`, ca la diagramă.
  - `notes(id)`: cheile virtuale, pentru `JOIN`.
- **Registrul portului, per proiect:**
  - `console::provide(project, access)` întoarce o gardă care îl scoate. Îl cheamă
    `SessionsService::attach` din explorer.
  - `console::access(project)`.

  E același model ca `database::drivers`: consola nu primește portul printr-o comandă,
  pentru că o consolă redeschisă la pornire (din sesiunea proiectului) n-a fost deschisă
  de explorer.
- **Evenimentele noi**, pe bus-ul proiectului:
  - `ConsoleRan { console, source, statements, writes, ddl }`;
  - `TransactionChanged { console, open }`;
  - `SchemaInvalidated { source }`: după un DDL rulat în consolă sau după „Refresh” în
    fereastra „Database”.

  O scriere reușită din consolă pe un tabel cunoscut dă și `DataChanged`, ca grilele
  deschise pe acel tabel să se reîmprospăteze.

### Istoricul (`history`, 39i)

- **`HistoryEntry`:**
  - `source`, `database`, `schema`;
  - textul cu secretele mascate (`mask_secrets`) și cu parametrii nepuși (șablonul,
    numele lor fără valori);
  - când (dat de modul: domeniul n-are ceas), durata, rândurile, ok sau eroare.
- **Regulile:**
  - aceeași instrucțiune rulată din nou urcă sus, nu se dublează;
  - cel mult `console_history_size` intrări pe sursă (implicit 1 000), cele vechi ies;
  - căutarea după text.
- **`HistoryStore` (portul):**
  - implementat de `storage::TomlQueryHistory`, în
    `<dosarul proiectului>/database/history/<source-uuid>.toml`;
  - adăugarea scrie un bloc `[[entry]]` la capăt (TOML rămâne valid la adăugare, fără să
    rescrie fișierul);
  - compactarea, la depășirea limitei, rescrie atomic;
  - un fișier stricat e păstrat ca `.broken`, nu scris peste.

### Indexul schemei (`schema_index`, 39j)

- **`SchemaIndex`** (pur, construit în fundal, `Arc`, imuabil):
  - pe `(sursă, bază)`: schemele, tabelele și view-urile (nume, fel, comentariu,
    estimarea rândurilor);
  - coloanele (nume, tip, nul, implicit, comentariu, poziția în cheie);
  - cheile externe, plus cheile virtuale acceptate (`RelationNotes`);
  - rutinele (nume, argumente, ce întorc, felul), cuvintele cheie și funcțiile încorporate
    ale dialectului.
- **Căutările:**
  - numele fără majuscule, în vectori sortați: căutare binară după prefix, apoi o
    potrivire fuzzy (`kernel::fuzzy`) pe cel mult câteva sute de candidați;
  - `generation` se schimbă la fiecare reconstruire.
- **Pe două trepte, pentru scheme uriașe:**
  1. numele tabelelor tuturor schemelor alese (o interogare);
  2. coloanele: dintr-odată sub `console_eager_tables` tabele (implicit 1 000); peste,
     doar pentru tabelele numite în instrucțiunea curentă, cerute în fundal când apar.

     Lista de completare se reface când vin (`poll`), iar până atunci rândul „Se încarcă
     coloanele…”.
- **Bugetul:**
  - 5 000 de tabele × 30 de coloane încap în mai puțin de 20 MB;
  - indexul are o limită de memorie (setare);
  - indexul unei surse fără console deschise se eliberează după `IDLE_CLOSE`
    (`kernel::OnDemand::release_if_idle`);
  - „Clear and Rebuild Caches” îl golește (`Purge`).

### Planul de execuție (`plan`, 39h)

- **`PlanNode`:** operația, obiectul, costul estimat / real, rândurile, timpul, copiii.
- **De unde se citește:**
  - Postgres: `EXPLAIN (FORMAT JSON[, ANALYZE, BUFFERS])`;
  - SQLite: `EXPLAIN QUERY PLAN` (`id`, `parent`, `detail`).
- **Regula:** `EXPLAIN ANALYZE` peste o scriere chiar rulează instrucțiunea. Consola o
  pune într-o tranzacție terminată cu `ROLLBACK` și cere confirmare, ca la o scriere.

## Contractul driverului pentru consolă (39c)

Toate metodele noi au implementări implicite, deci driverele de azi compilează neschimbate,
iar driverele externe v1 merg mai departe, cu o consolă mai săracă.

- **`ConsoleCaps`**, în `Capabilities` (`console: Option<ConsoleCaps>`; `None`: fără
  consolă):
  - `cursors`: o pagină următoare din același rezultat;
  - `explain: ExplainKinds` (plan, analiză);
  - `isolation: &'static [Isolation]`;
  - `switch_schema: SchemaSwitch::{Statement, Reconnect, None}`: Postgres schimbă schema
    prin `search_path` și baza prin reconectare; SQLite are schemele atașate;
  - `databases`: are mai multe baze pe același server;
  - `error_positions`.
- **`Session::run(&mut self, statement, options: &RunOptions, sink, cancel) ->
  Result<RunOutcome, DbError>`:**
  - `RunOptions { fetch_limit, keep_cursor }`;
  - `RunOutcome { affected, rows, more: Option<CursorToken>, notices }`. `notices` sunt
    mesajele `RAISE NOTICE` / `WARNING` ale serverului, care merg în Output.

  Implicit e `execute` cu `Flow::Stop` după `fetch_limit` + 1 rânduri, ca să se știe că
  mai sunt.
- **`Session::fetch(token, n, sink, cancel)` și `close_cursor(token)`:**
  - implicit `Unsupported`;
  - Postgres: un portal (`bind` + `query_portal(max_rows)`) într-o tranzacție. În modul
    automat e o tranzacție `READ ONLY` doar pentru cursor, închisă la instrucțiunea
    următoare sau la închiderea tab-ului.
- **`Session::explain(statement, analyze) -> Result<Vec<PlanNode>, DbError>`:** implicit
  `Unsupported`.
- **Tranzacțiile:**
  - `begin(isolation)`, `commit()`, `rollback()` și `in_transaction()`;
  - implicit trec prin `execute` cu textele dialectului (`Dialect::begin` / `commit` /
    `rollback`, care există).
- **Schema:**
  - `set_schema(schema)` și `current() -> (database, schema)`;
  - Postgres: `SET search_path TO "x", public` și `current_database()` /
    `current_schema()`;
  - SQLite: `current()` dă `main`, iar `set_schema` schimbă doar ce vede completarea
    (SQLite nu are o schemă implicită de schimbat), spus în indiciul alegerii.
- **`databases()`:** bazele de pe server (`pg_database` fără șabloane); SQLite: fișierul
  principal și cele atașate.
- **Introspecția în bloc:**
  - `catalog_columns(schema) -> Vec<(TableRef, Vec<ColumnBrief>)>` dintr-o singură
    interogare, nu una pe tabel: Postgres din `pg_attribute` ⋈ `pg_class` ⋈
    `pg_namespace`, valabil de la 9.2; SQLite din `sqlite_schema` ⋈ `pragma_table_info`;
  - `routines(schema)`: Postgres din `pg_proc`, cu `pg_get_function_arguments` și
    `pg_get_function_result`, după `prokind` de la 11 și `proisagg` sub 11; SQLite din
    `pragma_function_list` (3.30+), altfel din lista încorporată.

  Implicit, ce dau `tables` + `structure`.
- **Erorile cu loc:**
  - `DbError::Sql(SqlError { message, sqlstate, position, detail, hint })`, lângă
    `Query(String)`, care rămâne pentru ce n-are detalii;
  - Postgres păstrează `position` (`ErrorPosition::Original`), codul, `detail` și `hint`;
  - SQLite păstrează offset-ul, unde e disponibil;
  - `grid_source::message` le arată pe amândouă la fel.
- **Doar citire, apărare în adâncime:** o sursă doar citire deschisă de consolă primește
  la Postgres `SET SESSION CHARACTERISTICS AS TRANSACTION READ ONLY`. SQLite e deschis
  oricum doar citire.
- **Protocolul driverelor (`db-protocol`), v2:**
  - metode noi: `run`, `fetch`, `close_cursor`, `explain`, `begin`, `commit`, `rollback`,
    `set_schema`, `current`, `databases`, `catalog_columns` și `routines`;
  - rândurile vin în flux prin notificarea `$/rows` (bucăți de cel mult 1 000 de rânduri
    sau 1 MB, cu id-ul cererii), iar răspunsul final aduce `RunOutcome`;
  - `cancel` rămâne cel din v1;
  - erorile au `data: { sqlstate, position, detail, hint }`;
  - `VERSIONS` devine 1–2: un driver v1 merge mai departe, consola folosește `execute`
    (100 000 de rânduri, fără flux, fără poziții), iar bara spune „driver vechi: rezultate
    limitate”;
  - DTO-urile noi se adaugă în `dto`, conversiile în `convert`, driverul de test (`toy`)
    le implementează;
  - contractul se documentează în `03-sistem-module.md` §6.

## Limbajul (`lang-sql`, 39b)

- **Gramatica.** Candidatul e `DerekStride/tree-sitter-sql`, publicat pe crates.io ca
  `tree-sitter-sequel`:
  - licența MIT, întreținut activ;
  - permisiv, acoperă SQL-ul comun plus mult din PostgreSQL, SQLite și MySQL: `RETURNING`,
    `ON CONFLICT`, `::`, corpurile `$$`, CTE-uri, ferestre;
  - cuvintele cheie nu țin cont de majuscule;
  - are `highlights.scm`.

  Alternativele, respinse:
  - `m-novikov/tree-sitter-sql`: neîntreținut din 2021;
  - gramaticile doar Postgres: incomplete, și ar fi trebuit câte una pe dialect.
- **Vendorizarea**, ca twig și blade:
  - în `third_party/tree-sitter-sql/`: `LICENSE`, `grammar.js`, `src/parser.c` generat,
    `queries/`, `Cargo.toml` propriu (`publish = false`), cu versiunea și commit-ul din
    README;
  - `parser.c` se regenerează pentru ABI-ul `tree-sitter` 0.27 din workspace;
  - **criteriul de acceptare** în 39b: cel mult +4 MB la binarul release și cel mult +20 s
    la un build curat. Se măsoară și se scrie în Stare. Peste prag, gramatica se
    regenerează fără regulile dialectelor nefolosite (MySQL, BigQuery…), iar abia apoi se
    caută altă cale;
  - `[profile.dev.package.tree-sitter-sql]` optimizat, ca celelalte gramatici.
- **`LanguageDescriptor`:**
  - id `sql`, numele „SQL”;
  - extensiile `.sql`, `.ddl`, `.pgsql`, `.psql`, `.sqlite.sql`;
  - comentariu de linie `--`, de bloc `/* */`;
  - iconița `database`, care există.
- **`Grammar`:**
  - `highlights` (cuvinte cheie, tipuri, funcții, șiruri, numere, comentarii, parametri,
    operatori, identificatori citați), `folds` (instrucțiuni lungi, paranteze, CTE-uri,
    `CASE`, corpuri `$$`), `indents` și perechile de paranteze;
  - `injections`: corpul `$$ … $$` al unei funcții `LANGUAGE sql` / `plpgsql`, colorat tot
    ca SQL;
  - `embedded_errors: false`: erorile gramaticii nu se arată ca erori (vezi
    „Diagnosticele”);
  - `injected_errors: false`: în PHP, șirurile sunt des incomplete.
- **`LanguageIntelligence`** (fără bază):
  - cuvintele cheie pe `Flavor`, din liste ale noastre (din documentația motoarelor, nu
    copiate din alte unelte), cu `KeywordInsert` potrivit (`COUNT(` cu cursorul între
    paranteze);
  - `completion_triggers` `.`, `"` și `` ` ``;
  - `word_chars` cu `$` (Postgres), `naming`;
  - comentarea liniilor cu ⌘/ vine din descriptor, ca la celelalte limbaje.
- **Funcțiile încorporate pe versiuni** sunt datele noastre, ca stub-urile PHP:
  - `cargo xtask sql-data` citește `pragma_function_list` din SQLite-ul inclus în
    `rusqlite` (aceeași versiune care rulează) și scrie
    `crates/modules/languages/sql/data/sqlite.functions`;
  - Postgres se citește de la serverul conectat (`routines`), deci e mereu exact versiunea
    lui;
  - o listă de bază pentru Postgres fără conexiune se generează mai târziu din imaginile
    Docker, ca la PHP (vezi „Ce nu facem acum”).
- **PHP:** `<<<SQL` din heredoc se colorează singur. Injecția în șirurile obișnuite (39o)
  vine mai târziu.

## Inteligența legată de schemă (39j–39k)

- **Punctul de extensie nou, `code_intelligence::DocumentAssist`:** ajutorul unui modul
  pentru documentele pe care le știe (o consolă și sursa ei).
  - **Ce primește:** `AssistContext { path, language, text, offset, version }`.
  - **Ce poate da:**
    - `complete` → `FrameworkCompletion` (se adaugă la sugestiile limbajului sau le
      înlocuiește);
    - `definition` → `AssistTarget::Location(Location)` sau `Command(String)` (deschide
      structura sau datele unui tabel);
    - `hover` → `HoverInfo`.
  - **Unde se cheamă:** aplicația îl întreabă în `update_completion`, `goto_definition` și
    la hover, înaintea framework-urilor, doar pentru limbajele cerute (`languages()`).
  - **De ce nu `FrameworkSupport`:** acela e per proiect și per framework detectat; aici
    contează documentul.
  - **Regula:** e sincron și ieftin. Răspunde doar din ce are deja construit
    (`SchemaIndex` + `sql::scope` pe instrucțiunea curentă); ce lipsește cere în fundal și
    trezește aplicația.
- **`sql::scope(statement, offset) -> QueryScope`** (pur, în domeniu, pe tokenii
  instrucțiunii):
  - **ce se știe:** tabelele din `FROM`, `JOIN`, `UPDATE`, `INSERT INTO`, `DELETE FROM`,
    `MERGE`, cu aliasurile lor (`AS` opțional); CTE-urile (`WITH x(a, b) AS (…)`, cu
    coloanele din listă sau din `SELECT`-ul lor), inclusiv cele recursive; tabelele
    derivate `(SELECT …) t`, cu coloanele din lista `SELECT`; coloanele `USING (…)`;
  - **subselecturile pe niveluri:** un subselect din `WHERE`, `IN (…)`, `EXISTS (…)` sau din
    lista `SELECT` își are scope-ul lui și vede și aliasurile din afară (subselect corelat);
    aliasurile dinăuntru nu ies în afară;
  - **locul din instrucțiune:** coloanele se oferă în lista `SELECT`, `WHERE`, `ON`,
    `GROUP BY`, `HAVING`, `ORDER BY`, `SET` (la `UPDATE`) și `RETURNING`; în `ORDER BY` și,
    unde dialectul permite, în `GROUP BY` / `HAVING`, și aliasurile coloanelor din lista
    `SELECT` (`count(*) AS total` → `total`);
  - **`Expect` la cursor:** `Keyword`, `Schema`, `Table`, `Column { qualifier }`,
    `Function`, `JoinCondition { left, right }`, `InsertColumns { table }`, `Value`,
    `Nothing` (în șir sau comentariu);
  - **toleranța:** merge pe text neterminat (`SELECT u. FROM users u`, `SELECT FROM`),
    pentru că acesta e cazul obișnuit la completare;
  - **testele:** pe zeci de forme din fiecare dialect.
- **Completarea**:
  - **cuvintele cheie** potrivite locului (după `FROM`: `WHERE`, `JOIN`, `GROUP BY`…);
  - **schemele, tabelele și view-urile:** cele din schema curentă întâi, apoi celelalte
    calificate (`public.users`). Detaliul din dreapta e schema și numele sursei
    (`acme:server:prod`), ca în captură, cu iconița felului;
  - **coloanele:**
    - după `alias.`, coloanele acelui tabel;
    - fără calificare, coloanele tuturor tabelelor din scope, cu tabelul în detaliu;
    - cele ambigue se inserează calificate;
    - în `INSERT INTO t (`, lista coloanelor, cu „toate coloanele” ca primă sugestie;
  - **`SELECT *` desfăcut:** pe `*` se poate cere lista coloanelor (o acțiune, ⌥↵);
  - **`JOIN … ON`:**
    - după `JOIN orders o ON`, condiția întreagă din cheile externe reale și din cheile
      virtuale acceptate (37j): `o.user_id = u.id`;
    - după `JOIN`, tabelele legate de cele din scope vin primele, cu condiția inclusă
      (`orders o ON o.user_id = u.id`);
    - relațiile deduse, neacceptate, apar ultimele, marcate „dedusă”;
  - **funcțiile** dialectului și ale serverului, cu semnătura în detaliu și ajutorul
    pentru semnătură (`SignatureSyntax`, dacă se potrivește, altfel doar detaliul);
  - **literalele de tip enum** (Postgres `pg_enum`): după `= '` pe o coloană de tip enum,
    valorile. Mai târziu, dacă e scump.
- **Rezolvarea numelor (39k):**
  - **⌘-click** sau „Go to Definition” pe un tabel îi deschide structura sau datele (prin
    `ConsoleAccess::open_table` → `Opening`). Pe o coloană, structura tabelului, cu
    coloana arătată. Pe un alias sau pe un CTE, locul lui din text;
  - **hover:** tabelul (schema, felul, rândurile estimate, comentariul, coloanele cu tipul
    și cheile) sau coloana (tipul, nul, implicit, comentariul, „→ tabel.coloană” la o cheie
    externă);
  - **Find Usages** al unui alias, în instrucțiune: mai târziu.
- **Diagnosticele (39k)**, prin `DiagnosticsFound` pe bus (sursa `sql-schema`), deci
  arătate de aplicație ca toate celelalte (sublinieri, marcaje pe bara de derulare, bara de
  stare):
  - **tabel necunoscut:** doar când indexul acelei scheme e complet. Nu se raportează nimic
    pentru numele create mai sus în același script (`CREATE TABLE`, `CREATE TEMP`, CTE),
    pentru tabelele temporare, pentru o schemă necitită sau pentru SQL dinamic;
  - **coloană necunoscută:** doar pe un tabel cu coloanele citite, cu o calificare
    rezolvată sigur;
  - **ambiguitate:** o coloană fără calificare în două tabele, ca avertisment;
  - **erorile de sintaxă ale gramaticii:** nu le arătăm ca erori. O gramatică permisivă
    pentru mai multe dialecte ar da alarme false pe SQL valid, ceea ce strică încrederea.
    Se arată doar ca informație estompată, cu setarea `[database] console_grammar_hints`
    (implicit oprită);
  - **erorile reale vin de la server, după rulare:** sublinierea pe locul exact
    (`SqlError::position` → document), care dispare când se schimbă textul în acel loc;
  - **calculul:** în fundal (pool-ul `db-schema-index`), pe versiunea documentului, cu o
    pauză după tastare (`kernel::queue::Coalescing`), nu la fiecare tastă.
- **Invalidarea indexului:**
  - `SchemaInvalidated` (DDL rulat din consolă, „Refresh” din fereastra „Database”);
  - `DataChanged { structure: true }` (modalele de structură);
  - `DataSourcesChanged` cu o conexiune schimbată (`same_connection` fals);
  - `Purge`;
  - din fereastra „Database” se mai poate cere explicit „Refresh”.

  Reconstruirea e în fundal; vechiul index servește până vine cel nou.

## UI: consola în editor (39e–39h)

- **Bara consolei** e un `EditorToolbar`, contribuit de `database-console` prin
  `EditorToolbarProvider`, pentru fișierele din `<dosarul proiectului>/consoles/`. Are
  înălțimea barei grilei și componentele `ds`. De la stânga la dreapta:
  - **▶ Run** (⌘↵), cu meniul ▾: „Execute”, „Execute Script” (⇧⌘↵), „Explain Plan”,
    „Explain Analyze”.
  - **Istoricul** (iconița ceasului, ⌥⌘E): fereastra cu căutare, 39i.
  - **Parametrii** (`p`): valorile ultimei rulări, 39h.
  - **Tx:** „Auto” / „Manual” ▾, cu nivelul de izolare. În `Manual`, cu o tranzacție
    deschisă, apar **Commit** (⌥⌘↵) și **Rollback**, iar eticheta arată câte instrucțiuni
    așteaptă.
  - **■ Stop** (⌘F2), activ cât rulează ceva.
  - **„Playground” / „Script”** ▾.
  - **Setările consolei** (⚙): limita de rânduri, oprirea la eroare.
  - **Comutatorul „Services”** (arată sau ascunde fereastra).
  - **În dreapta, `bază.schemă` ▾:** popover-ul cu bazele, apoi schemele, cu filtru, după
    `ConsoleCaps::switch_schema`. Schimbarea bazei cu o tranzacție deschisă întreabă
    întâi.

  Ce nu încape intră într-un meniu „…”, după aceeași regulă ca `data_grid::toolbar`
  (calculată o dată pe lățime și stare, cu `kernel::Memo`). Toate butoanele au indicii cu
  tastele comenzii (`Tip`).
- **Tab-ul consolei:**
  - titlul e `console [sursă]`, prin `EditorToolbar::title`; fișierul se numește doar
    `console.sql`, deci fără `[` și `:` în nume (Windows);
  - iconița e a driverului;
  - culoarea sursei vine prin `EditorToolbar::accent`, desenată de aplicație ca la
    `Viewer::accent`: punctul pe iconița tab-ului și dunga de sus;
  - o tranzacție deschisă pune un semn pe tab;
  - breadcrumb-ul începe cu `Database Consoles › sursă › console`
    (`EditorToolbar::crumbs`).
- **Instrucțiunea de la cursor:**
  - în `Playground`, instrucțiunea care ar rula e marcată discret (fundalul, pe rândurile
    ei) cât stă mouse-ul pe ▶ sau după ⌘↵;
  - fără instrucțiune la cursor, se cere alegerea cu `host.choose` („care instrucțiune?”);
  - cât rulează, instrucțiunea are un semn în gutter (un `LineDecorator` al modulului);
    după rulare, ✓ sau ✗.
- **„Jump to Query Console” (⇧⌘F10)**, comanda `database.console.jump`. Se deschide din:
  - butonul nou din antetul ferestrei „Database” (iconița `console`), între `+` și
    reîmprospătare;
  - meniul unei surse, scheme sau tabel („Query Console”, cu „New Query Console” alături);
  - vederea de date (antetul ei și meniul listei de tabele);
  - paleta de comenzi, cu sursa selectată sau a tabului activ.

  Comportamentul:
  - cu console existente pe sursă, un popover le listează („New Query Console”, apoi
    `console`, `console_1`…, cu ultima rulare);
  - fără console, se face direct `console`;
  - deschisă de pe un tabel, consola primește schema lui. Dacă e goală, primește
    `SELECT * FROM <tabel>;`, cu tabelul selectat (ca „Open Query Console” pe tabel);
  - o sursă fără dialect: comanda e dezactivată, cu motivul în indiciu.
- **Tastele:**
  - ⌘↵ e deja `database.submit` în grilă (`When::View`). În consolă trebuie să fie Run,
    fără ciocniri. `When` primește varianta `EditorLanguage("sql")` (o parte din `Editor`,
    pentru `overlaps`); `database.console.run` e legat acolo;
  - presetările de taste (`PresetShortcuts`): JetBrains ⌘↵ / ⇧⌘F10 / ⌘F2; VS Code
    Ctrl+Enter (MSSQL / SQLTools), `Ctrl+Shift+E` pentru Explain;
  - pe Windows și Linux, `CmdOrCtrl` dă Ctrl, ca peste tot.
- **Confirmările (39g):**
  - pericolele (`statement_dangers`) și scrierile pe producție deschid modalul modulului
    `view-console-confirm` (`host.open_modal`, nu se închide la click afară);
  - modalul arată instrucțiunile exacte, colorate (`code_tokens` cu `sql`), sursa, culoarea
    ei și eticheta „producție”, plus lista pericolelor;
  - pe producție, butonul se activează abia după bifa „Am înțeles” (`danger_button`);
  - o sursă doar citire refuză scrierea fără modal, cu motivul în Output și un toast.
- **Închiderea unei console cu o tranzacție deschisă:** `Viewer::may_close` nu se aplică
  (consola e fișier), deci `EditorToolbar::may_close` întreabă: Commit, Rollback sau
  Anulează. La închiderea proiectului: Rollback, cu un toast care spune asta.
- **Parametrii (39h):**
  - înainte de rulare, un modal cu fiecare parametru: valoarea, felul (text, număr, `NULL`,
    expresie brută) și „ține minte”;
  - valorile se țin în memorie, pe consolă, cât e deschis proiectul. Nu se scriu niciodată
    pe disc: pot fi date personale;
  - ↵ rulează, Esc renunță.

## UI: fereastra „Services” (39d, 39f)

- **Modulul `services-panel`:**
  - `ToolWindowProvider` cu id `services`, iconița `services` (nouă în setul inclus),
    titlul „Services”, `Dock::Bottom`, `order` după terminal;
  - comanda `view.toggle_services` (⌘8 / Alt+8);
  - `ToolContent::Panel`: aplicația învață să așeze un panou și jos (39d);
  - meniul butonului e un `MenuContribution` cu același id;
  - panoul se face doar la prima afișare.
- **Punctul de extensie** (`ui_components::services`):
  - `ServiceProvider` (id, ordine, iconiță, titlu, `open(&ServicesContext) ->
    Option<Box<dyn ProjectServices>>`);
  - `ProjectServices`, per proiect, cum e `LineDecorator` → `ProjectDecorations`:
    - `nodes()` (arborele, cu `generation`) și `poll()`;
    - `tabs(node) -> Vec<ServiceTab>`, unde un tab are id UUID, titlu, iconiță, „fixat”;
    - `open_tab(tab) -> Box<dyn Viewer>`;
    - `menu(node)`, `command(node, id)` și `close_tab`.
  - `ServiceNode`: id UUID, părinte, iconiță, nume, detaliu (`543 ms`), stare (`Idle`,
    `Running`, `Error`, `TxOpen`), culoare.

  Aplicația adună contribuțiile și le înregistrează în registrul din `ui-components` (ca
  `database::drivers`), de unde le citește modulul. „Services” nu știe cine contribuie.
- **Panoul:**
  - **Stânga:** arborele (`TreeList`, filtru `ListFilter`, tastele ferestrei „Database”),
    cu grupurile contributorilor („Database”, mai târziu „Docker”…) și rândurile lor.
    Pentru baza de date:
    - sursa, cu iconița driverului și semnul în culoarea ei;
    - sesiunile consolelor, cu ultima durată, spinner cât rulează și semnul tranzacției;
    - tab-urile de rezultat ale fiecăreia.
  - **Dreapta:** tab-urile nodului selectat, iar tab-ul ales e un `Viewer`.
  - **Despărțitorul:** `splitter`, cu lățimea ținută în sesiunea proiectului.
- **Contribuția bazei de date** (`database-console`):
  - **„Output”**, prin `ui_components::log_view` (nou, generic):
    - rânduri cu marca de timp, felul (info, instrucțiune, rezultat, avertisment, eroare,
      notice) și textul;
    - instrucțiunile colorate ca SQL, cu prefixul `bază.schemă>`;
    - o eroare e o legătură: click duce cursorul în consolă, la locul erorii;
    - butoanele laterale: soft wrap, derulare la capăt (ține capătul), golire,
      copiere;
    - limita e de 10 000 de rânduri pe sesiune, cele vechi ies;
    - textele modelate se păstrează, iar pe cadru se desenează doar ce e în vedere.

    Mesajele vin din `locales`:
    - `console.connected` („Conectat la {source}”);
    - `console.rows_retrieved` („{n} rânduri aduse începând cu 1 în {total} (execuție:
      {exec}, aducere: {fetch})”);
    - `console.rows_affected`, `console.completed`, `console.cancelled`;
    - `console.tx_committed`, `console.tx_rolled_back`;
    - `console.error_at` (codul SQLSTATE, mesajul, `detail`, `hint`, „linia L, coloana
      C”).

    Timpul se arată cu milisecunde.
  - **Rezultatele:** un tab pe fiecare set, numit după tabel, când instrucțiunea e un
    `SELECT` dintr-un singur tabel (`bază.schemă.tabel`), altfel `Result 1`, `Result 2`.
    - Grila e `DataGrid` peste `ResultSource: GridSource` (nou, în consolă): rândurile în
      `ResultBuffer` (bucăți, octeți numărați, limită pe consolă), „N rânduri”, „mai
      sunt”, aducerea paginii următoare sau a tuturor (cu avertisment peste 100 000).
    - Sortarea și filtrul `WHERE` / `ORDER BY` ale grilei se fac:
      - local, prin `database::eval`, când toate rândurile sunt aduse;
      - altfel, pentru o interogare pură, rulând din nou interogarea învelită (subselect
        prin dialect), spus în bară.
    - Copierea și exportul refolosesc `CopyStream` și `export` (exportul cere toate
      rândurile prin aceeași cale).
    - Reîmprospătarea (F5) rulează din nou instrucțiunea (doar o interogare pură; o
      scriere, niciodată fără confirmare).
  - **Tab-urile:** un tab nefixat e înlocuit de rularea următoare, iar un
    tab fixat (📌, „Pin Tab”) rămâne. Închis, își eliberează rândurile pe loc.
  - **Bara tab-ului de rezultat:** Tx (Commit / Rollback, când consola are tranzacție),
    timpul rulării, exportul, transpunerea, căutarea; restul e al grilei.
  - **Planul (39h):** un tab „Plan”, cu `PlanNode` ca arbore pe coloane (operația,
    obiectul, cost, rânduri, timp). Cea mai scumpă ramură e evidențiată.
- **Ce nu face „Services” acum:** tipărirea jurnalului (nicio altă parte a aplicației nu
  tipărește încă), Docker și configurațiile de rulare (n-au modul încă).

## Fișierele consolelor

- **Unde:** `<dosarul proiectului>/consoles/<source-uuid>/<nume>.sql` (`console.sql`,
  `console_1.sql`…), plus `<dosarul proiectului>/consoles/consoles.toml` cu `ConsoleSpec`
  (scris de `storage::TomlConsoles`, atomic).
  - Un fișier de console stricat e o eroare și nu se scrie peste el, ca la
    `data-sources.toml`.
  - O sursă globală are consolele ei în fiecare proiect: e munca proiectului, iar
    cerința e „local, pe proiect”.
  - Fără proiect deschis: `config_dir()/consoles/`.
- **Redeschiderea:** tab-urile consolelor revin cu sesiunea proiectului, ca orice fișier.
  Conexiunea se face abia la prima rulare: nimic nu se conectează singur.
- **Un fișier `.sql` mutat sau șters de mână:** consola lui dispare din listă, fără eroare.
  Un `.sql` pus de mână în folder apare ca o consolă nouă a acelei surse.
- **„Database Consoles”:** în meniul sursei din fereastra „Database” și în „Services”.
  Consolele se pot redenumi și șterge de acolo; ștergerea cere confirmare.

## Performanță și resurse

- **Leneș:** modulul nu face nimic până la prima consolă. Indexul schemei se face doar pentru
  o sursă cu o consolă deschisă, la prima completare sau la primul diagnostic, nu la
  deschidere.
- **Rândurile:**
  - limita de pagină `console_fetch_limit` (implicit 500);
  - memoria rezultatelor unei console, `console_result_memory` (implicit 64 MB): peste ea,
    tab-urile nefixate cele mai vechi își lasă rândurile (refăcute la cerere pentru o
    interogare pură);
  - o celulă mare (text sau BLOB peste 64 KB) se ține tăiată, cu mărimea, și se aduce
    întreagă doar deschisă în editorul mare;
  - în Task Manager (`ResourceReporter`): sesiunile consolelor, rândurile și octeții ținuți,
    indexul schemei.
- **Firele:**
  - rularea pe `db-console` (`ModuleThreads`), indexul și diagnosticele pe
    `db-schema-index`;
  - o rulare ocupă un fir cât durează, iar anularea îl eliberează (Postgres
    `cancel_query`, SQLite `progress_handler`, protocolul `cancel` cu 5 s de grație, apoi
    procesul oprit);
  - închiderea tab-ului sau a consolei anulează ce rulează.
- **Pe cadru:**
  - lexarea documentului e ținută pe versiune (`Memo`); un fișier de peste 1 MB se lexează
    în fundal;
  - bara se calculează o dată pe lățime și stare; jurnalul și grila desenează doar ce e în
    vedere;
  - cât rulează ceva, se redesenează doar spinner-ul (30 fps, ca inelul toast-urilor).
- **Bugetele, verificate în teste:**
  - lexare + împărțire: 1 MB de SQL sub 30 ms (release);
  - `scope` pe o instrucțiune de 5 KB sub 1 ms;
  - completarea pe 5 000 de tabele sub 2 ms;
  - indexul pe 5 000 × 30 sub 20 MB.
- **Windows:** căile prin `config_dir()` și prin dosarul proiectului, numele fișierelor
  fără caractere interzise, `\r\n` tratat de lexer, anularea și seiful la fel, aceleași
  teste pe `windows-latest`.

## Siguranță

- **Parolele:**
  - doar prin `credentials` (seiful proiectului, keychain-ul), citite pe firul de conectare;
  - niciodată în jurnal, istoric, `consoles.toml`, eroare sau log: „Connected to {source}”
    arată numele sursei, nu URL-ul;
  - la protocol, ca în 37i: doar în `connect`.
- **Istoricul și jurnalul** maschează literalele de parole (`mask_secrets`) și nu țin
  valorile parametrilor.
- **Producția:**
  - un semn nou, `DataSource::production` sau `SourceFolder::production`, moștenit de la
    folder ca și culoarea (`effective_production`);
  - se arată în modalul surselor (Opțiuni) și în meniul folderului;
  - eticheta „PROD” pe bara consolei, iar orice scriere în `Tx: Auto` cere confirmare;
  - vezi întrebarea 2.
- **Doar citire:** refuzat înainte de trimitere (`StatementKind::writes`), plus driverul
  (SQLite deschis doar citire, Postgres `READ ONLY`). Un `Other` necunoscut e tratat ca
  scriere.
- **Inactivitatea:** `IDLE_CLOSE` nu închide niciodată o sesiune de consolă cu tranzacție
  deschisă sau cu un cursor deschis. Pentru o tranzacție veche de peste o oră, un toast o
  semnalează („tranzacția din console ține blocări”).
- **Fișierele consolelor** sunt textul utilizatorului. `<dosarul proiectului>` e deja în
  afara indexării și a VCS-ului, prin regulile existente.

## Pașii

Fiecare pas e un commit, după `./scripts/check.sh` și cu capturile vechi identice, în afară
de cele numite. Capturile noi sunt scenarii în spatele feature-ului `snapshot`, pe fișiere
SQLite făcute de captură într-un folder temporar, cu ceasul oprit (jurnalul arată aceleași
ore la fiecare rulare).

1. **39a — SQL-ul, pur.**
   - **Ce intră:** `database::sql` (`Flavor`, `lex`, `split`, `statement_at`, `classify`,
     `params`, `bind`, `mask_secrets`, `errors::locate`), `Dialect::flavor` și
     `danger::statement_dangers`.
   - **Teste (domeniu):** fiecare formă de șir, comentariu, identificator și parametru, pe
     fiecare dialect (`$$`, `$tag$`, `E'\''`, comentarii imbricate, `[x]`); `;` în toate
     locurile în care nu desparte; triggerele SQLite; `BEGIN ATOMIC`; text neterminat;
     `\r\n`; felurile; `WITH` care scrie; pericolele (și fals-pozitive: `WHERE` într-o
     subinterogare, `DELETE` cu `USING` / `WHERE`); parametrii (repetați, în șiruri, în
     `$$`); `bind` cu harta pozițiilor; mascarea.
   - **Teste de proprietăți:** împărțirea și lipirea dau textul înapoi; niciun token nu se
     suprapune.
   - **Buget:** 1 MB sub 30 ms.
2. **39b — `lang-sql`.**
   - **Ce intră:** gramatica vendorizată în `third_party/tree-sitter-sql` (licența, ABI-ul,
     mărimea măsurată și scrisă în Stare), `LanguageDescriptor`, interogările (highlights,
     folds, indents, injecția `$$`), cuvintele cheie pe dialecte, `cargo xtask sql-data` și
     `data/sqlite.functions`.
   - **Teste:** colorarea pe fișiere de exemplu (SQLite, Postgres), plierea, comentarea
     liniilor, heredoc-ul `<<<SQL` din PHP colorat.
   - **Capturi noi:** `sql-editor` (un fișier `.sql` în ambele teme) și `php-heredoc-sql`.
   - **Captura `about`** numără un modul în plus.
3. **39c — contractul driverului.**
   - **Domeniul:** `ConsoleCaps`, `Session::run` / `fetch` / `close_cursor` / `explain` /
     `begin` / `commit` / `rollback` / `set_schema` / `current` / `databases` /
     `catalog_columns` / `routines`, `DbError::Sql(SqlError)`, `PlanNode`, toate cu
     implicite.
   - **SQLite și Postgres** le implementează (portalul Postgres, `READ ONLY` la doar
     citire, notices).
   - **Protocolul v2:** `$/rows`, metodele noi, erorile cu `data`, `VERSIONS` 1–2;
     driverul `toy` le implementează.
   - **Teste:**
     - SQLite pe fișiere temporare: rulare, limită + „mai sunt”, anulare, poziția erorii,
       tranzacții, `catalog_columns` pe 2 000 de tabele într-o interogare, `EXPLAIN QUERY
       PLAN`;
     - protocolul: rândurile în flux identice cu cele directe, anulare la mijlocul fluxului,
       un driver v1 acceptat cu consola limitată, eroare cu poziție;
     - Postgres real, doar cu `EDITOR_TEST_POSTGRES_URL`: portalul, `search_path`,
       `pg_proc` pe versiuni, `position`, `RAISE NOTICE`, cancel;
     - niciun server pornit de teste.
4. **39d — „Services”, generic.**
   - **Ce intră:** `ui_components::services` (contractul și registrul),
     `ui_components::log_view`, modulul `services-panel`, `ToolContent::Panel` la
     `Dock::Bottom` în aplicație, iconița `services`, comanda și tastele.
   - **Teste:** cu un furnizor fals (arbore, tab-uri, fixare, închidere, taste, filtru), două
     proiecte care nu își văd nodurile, `log_view` (soft wrap, capătul ținut, limita,
     legăturile).
   - **Capturi noi:** `services-empty`, `services-fake`. Captura `about` numără un modul în
     plus.
5. **39e — consola în editor, rularea simplă.**
   - **Ce intră:**
     - `ui_components::editor_toolbar` și găzduirea în aplicație (bara, accentul, titlul,
       breadcrumb-ul, `may_close`), `When::EditorLanguage`;
     - modulul `database-console`: `ConsoleSpec`, `TomlConsoles`, portul `ConsoleAccess` și
       registrul lui, implementat de explorer;
     - `ConsoleRunner` pe `db-console`, cu ▶ / ■, Playground / Script și alegerea
       instrucțiunii;
     - „Jump to Query Console” din toate locurile (butonul din antetul ferestrei
       „Database”);
     - semnele din gutter.
   - **Teste:**
     - modulul, pe SQLite temporar și pe driverul fals: planul rulării pe cursor / selecție /
       script, anularea, conexiunea închisă cu consola, nicio conexiune la redeschidere,
       sursa fără dialect refuzată;
     - aplicația: bara peste editor nu mută textul, titlul și breadcrumb-ul, tastele ⌘↵ în
       consolă față de grilă.
   - **Capturi noi:** `console` și `database-jump-console` (popover-ul). Capturile
     `database-tree`, `database-menu`, `database-folders` și `database-folder-grid*` arată
     butonul nou din antet sau rândul de meniu.
6. **39f — rezultatele în „Services”.**
   - **Ce intră:** contribuția bazei de date (sursă › console › rezultate), Output cu
     mesajele din `locales`, `ResultSource` / `ResultBuffer`, mai multe seturi, paginile
     următoare (cursor sau rerulare), sortarea și filtrul, fixarea, exportul, memoria
     limitată, erorile cu legătură în text și sublinierea lor, `ConsoleRan`, `DataChanged`
     după o scriere, toast-ul la o rulare lungă terminată cu fereastra ascunsă.
   - **Teste:** timpii (execuție față de aducere), cu ceasul fals; limita de memorie; un tab
     nefixat înlocuit și unul fixat păstrat; o rulare care scrie nu e rerulată niciodată;
     click pe eroare.
   - **Capturi noi:** `services-output`, `services-result`, `services-results-many`,
     `console-error`.
7. **39g — tranzacțiile și siguranța.**
   - **Ce intră:** `TxState`, Tx Auto / Manual și izolarea, Commit / Rollback (bară, tab-ul
     de rezultat, `TransactionChanged`), semnul pe tab și în arbore, închiderea cu
     tranzacție, `IDLE_CLOSE` ocolit, doar citire, modalul de confirmare, semnul
     „producție” (domeniu, `TomlDataSources`, modalul surselor, folderul).
   - **Teste:** mașina de stări (inclusiv starea abandonată după o eroare la Postgres, un
     `BEGIN` / `COMMIT` scris de mână), SQLite blocat de o tranzacție a consolei (grila
     primește `InUse` cu mesajul potrivit), producția moștenită, refuzurile.
   - **Capturi noi:** `console-tx-manual`, `console-danger`, `console-prod-confirm`.
     Capturile `database-sources*` arată bifa „producție”.
8. **39h — parametrii și planul.**
   - Continuat de profilare în 39h2–39h6 (vezi „Profilarea interogărilor” mai jos).
   - **Ce intră:** modalul parametrilor, `bind` cu harta pozițiilor (erorile cad la locul
     lor), Explain / Explain Analyze (cu rollback și confirmare la scriere), tab-ul „Plan”.
   - **Teste:** parametrii pe fiecare formă, valorile păstrate doar în memorie, planul
     SQLite citit ca arbore, `EXPLAIN (FORMAT JSON)` dintr-un JSON salvat.
   - **Capturi noi:** `console-params`, `services-plan`.
9. **39i — istoricul și consolele păstrate.**
   - **Ce intră:** `history`, `TomlQueryHistory`, fereastra istoricului (căutare, inserare,
     rulare din nou, ștergere), consolele redenumite / șterse, „Database Consoles”.
   - **Căutarea în lista consolelor (cerut):** popover-ul „Query consoles of …” (⇧⌘F10) are
     un câmp de filtrare deasupra listei, cu focusul în el la deschidere. Filtrează după numele
     consolei și după textul ei (prima linie care se potrivește, arătată sub nume, cu
     potrivirea evidențiată), fuzzy ca la căutarea fișierelor. „New Query Console” rămâne
     mereu primul. Enter deschide prima potrivire; lista e virtualizată pentru multe console.
   - **Teste:** adăugarea la capăt și compactarea, un fișier stricat păstrat, secretele
     mascate, nicio valoare de parametru pe disc, dublurile urcate; filtrarea consolelor
     (nume, text, fuzzy, ordinea).
   - **Capturi noi:** `console-history`, `database-jump-console-filter`.
10. **39j — indexul schemei și completarea.**
    - **Ce intră:** `SchemaIndex` pe două trepte, construit pe `db-schema-index` și
      invalidat (`SchemaInvalidated` publicat și de „Refresh” din explorer),
      `DocumentAssist` și locul lui în aplicație, `sql::scope`, completarea întreagă
      (cuvinte cheie, scheme, tabele cu sursa în detaliu, coloane, aliasuri, CTE-uri,
      `INSERT`, `JOIN … ON` din cheile reale și virtuale, funcții).
    - **Teste:** `scope` pe zeci de forme neterminate, ordinea sugestiilor, bugetul pe 5 000
      de tabele, coloanele cerute leneș și lista refăcută, invalidarea, `Purge`.
    - **Capturi noi:** `console-completion` (tabelele cu sursa, ca în captura de referință) și
      `console-join`.
11. **39k — rezolvarea și diagnosticele.**
    - **Ce intră:** ⌘-click (structura sau datele tabelului), hover pe tabel și pe coloană,
      diagnosticele din schemă prin `DiagnosticsFound`, `console_grammar_hints`.
    - **Teste:** fără fals-pozitive (scripturi cu `CREATE TEMP`, CTE-uri, scheme necitite,
      SQL dinamic), ambiguitățile, diagnosticele care se șterg când textul se schimbă.
    - **Capturi noi:** `console-hover` și `console-unknown-table`.
12. **39l — rezultatele editabile.**
    - **Ce intră:** un rezultat dintr-un singur tabel, cu cheia primară printre coloane și
      fără agregate, `DISTINCT`, `GROUP BY` sau `JOIN`, devine editabil prin aceleași reguli
      ca grila (`Dialect::changes`, „exact un rând”, previzualizarea). Ce se trimite intră
      în tranzacția consolei în `Manual`. Rămân „Deschide tabelul” și „Mergi
      la rândul referit” (37j).
    - **Teste:** ce e editabil și ce nu, cu motivul; conflictul; tranzacția manuală.
    - **Captură nouă:** `services-result-edit`.
13. **39m — formatarea.**
    - **Ce intră:** `database::sql::format`, pur și determinist (cuvintele cheie cu
      majuscule sau nu, după setare; indentarea clauzelor; listele pe rânduri peste o
      lățime), prin acțiunea „Reformat Code” a limbajului `sql`. Comentariile și șirurile
      se păstrează exact.
    - **Teste:** idempotent; textul, fără spații, e același înainte și după.
14. **39n — un `.sql` din proiect atașat unei surse.**
    - **Ce intră:** bara apare și pe fișierele `.sql` obișnuite, cu „Fără sursă — Atașează…”.
      Alegerea se ține în `<dosarul proiectului>/sql-files.toml`. Fișierul primește
      rularea, rezultatele și inteligența, ca o consolă.
15. **39o — extractoarele de date („Data Extractors”: formatele de copiere / export).**
    - **De ce:** copierea și exportul au acum câteva formate fixe (`CopyFormat`). Cerut: o
      listă de formate aleasă din bara grilei, folosită la fel de ⌘C, „Copy As ›” și export.
    - **Ce intră:**
      - un model pur `Extractor` (id, nume, grup, opțiuni, ce produce din coloane, rânduri și
        dialect), un registru cu cele de bază și un punct de extensie, ca modulele să-și
        aducă propriile extractoare;
      - **Built-in:** SQL Inserts, SQL Inserts pe mai multe rânduri, SQL Updates (după cheia
        primară), Where Clause (`id IN (…)` / condiția pe rândurile selectate), cu bifele
        „Skip Computed Columns” și „Skip Generated Columns”; SQL-ul prin dialectul sursei;
      - **CSV:** CSV, TSV, separat prin `|`, separat prin `;` și „Configure CSV Formats…”
        (separator, ghilimele, antet, text pentru NULL, sfârșit de rând), păstrate prin
        `Setting<T>`;
      - **altele:** JSON, Markdown, HTML, XML, One-row, Pretty (tabel text aliniat);
      - **alegerea:** un buton „CSV ▾” în bara grilei (și în tab-urile de rezultat din
        „Services”), cu un popover în grupuri ca în captură; formatul ales e cel al lui ⌘C și
        al exportului, ținut per proiect;
      - **exportul** folosește aceleași extractoare (în fundal, cu anulare), plus formatele
        binare (XLSX) doar acolo, cu nota din captură.
    - **Extractoarele utilizatorului (cerut: „da, să le facem”):** în locul unor scripturi
      Groovy / JS, șabloane simple, fără un interpretor în aplicație:
      - fișiere `*.extractor` în `<dosarul proiectului>/extractors/` (ale proiectului) și în
        dosarul de configurare (ale tuturor proiectelor), apărute în grupul „Scripted” /
        „Ale mele”, cu „Go to Extractors Directory” și reîncărcate când se schimbă pe disc;
      - un limbaj de șablon pur, în domeniu, cu antet (nume, extensie pentru export), bucle
        peste rânduri și coloane (`{{#rows}}…{{/rows}}`, `{{#columns}}`, primul / ultimul
        element, separatori), câmpurile (`{{value}}`, `{{column.name}}`, `{{column.type}}`,
        `{{table}}`, `{{row.index}}`), condiții (`{{#null}}`, `{{^last}}`) și filtre de
        escapare (`|sql`, `|json`, `|csv`, `|html`, `|xml`, `|md`, `|upper`, `|lower`,
        `|trim`, `|pad:N`);
      - erorile din șablon arătate cu linia și coloana lor, fără să oprească restul
        extractoarelor; un șablon nu poate citi fișiere, rula programe sau accesa rețeaua;
      - câteva exemple puse la prima deschidere a dosarului (SQL-Insert-Multirow,
        Python-DataFrame, PHP array, Laravel seeder, o listă Markdown), ca punct de plecare;
      - fișierele `.extractor` au colorare și completare a câmpurilor și filtrelor.
    - **Teste:** fiecare extractor pe tipuri (NULL, text cu ghilimele și linii noi, binar,
      numere, date) și dialecte; selecția parțială; coloanele calculate sărite; opțiunile CSV;
      limbajul de șablon (bucle, condiții, filtre, erori cu poziția, nimic în afara datelor),
      dosarele reîncărcate, exemplele.
    - **Capturi noi:** `grid-extractors`, `services-result-extractors`, `extractor-template`.

16. **39p — driverul MySQL / MariaDB.**
    - **De ce:** consola, profilarea și explorer-ul sunt gata pe PostgreSQL și SQLite; MySQL
      și MariaDB sunt cele mai folosite servere în proiectele PHP. Un singur driver, construit
      exact ca cel PostgreSQL (crate-ul de infrastructură `db-mysql` și modulul
      `database-mysql`, înregistrat ca `database-postgres`), două intrări în lista surselor
      (`mysql`, `mariadb`).
    - **Ce intră:**
      - **clientul:** un crate pur Rust, cu licență MIT / Apache, care se compilează pe
        Windows fără bibliotecă C, cu `caching_sha2_password` (și peste o conexiune
        necriptată, prin cheia RSA a serverului) și `mysql_native_password`, cu TLS prin
        rustls; serverele de la MySQL 5.7 și MariaDB 10.2 până la cele mai noi;
      - **conexiunea:** câmpurile unui server, `sslmode` cu cele șase trepte ale surselor
        (`prefer` implicit), fișierele certificatelor, `connect_timeout`; politica parolei
        (`SecretPolicy`, seiful) ca la orice sursă; o sursă doar citire, `SET SESSION
        TRANSACTION READ ONLY`;
      - **dialectul pe versiuni** (`database::mysql`, pur): `ServerVersion` (motorul și
        versiunea din `VERSION()`), `Feature` (ce are fiecare motor și versiune), numele cu
        backtick, textul care se citește la fel în orice `sql_mode` (cu backslash: hex în
        `utf8mb4`), DDL-ul cum îl ia MySQL (`CHANGE COLUMN`, `DROP INDEX … ON`, `DROP FOREIGN
        KEY`, redenumirea pe 5.7 prin definiția coloanei), `RETURNING` doar pe MariaDB
        10.5+, implicitele-expresie doar de la MySQL 8.0.13 / MariaDB 10.2;
      - **catalogul:** bazele serverului sunt schemele lui (arborele: sursă › baze ca scheme
        › tabele, o singură sesiune pentru toate, oricare ar fi baza din sursă), din
        `information_schema` (tabele, view-uri, coloane, indecși — și cei pe expresii —,
        chei străine), în bloc pentru consolă (coloanele, cheile, rutinele cu parametrii,
        funcțiile serverului pentru completare, secvențele MariaDB);
      - **sesiunea:** rândurile în flux prin protocolul text, tipizate după metadatele
        coloanelor; paginile grilei după cheie; oprirea prin `KILL QUERY` de pe o a doua
        conexiune (și când grila are destule rânduri); starea tranzacției cum o spune
        serverul (`SERVER_STATUS_IN_TRANS`: un DDL o închide singur); `CLIENT_FOUND_ROWS`,
        ca „exact un rând” al grilei să nu fie conflict la o valoare neschimbată;
      - **consola (39c):** fără cursor pe server (pagina următoare: interogarea rulată din
        nou), avertismentele ca notices (`SHOW WARNINGS`), erorile cu SQLSTATE, cod și loc
        (`near '…' at line N` găsit în text), tranzacțiile cu izolarea (`SET TRANSACTION` apoi
        `START TRANSACTION [READ ONLY]`), schema prin `USE`;
      - **profilarea (39h2)**, după „Cum se potrivește fiecare motor”: `EXPLAIN FORMAT=JSON`
        pentru plan, `EXPLAIN ANALYZE` (MySQL 8.0.18+) și `ANALYZE FORMAT=JSON` (MariaDB)
        pentru ce a rulat, într-o tranzacție doar citire a ei sau sub un savepoint în a
        utilizatorului, anulată la final; refuzate cele ce nu se pot anula (DDL, tabelele
        fără tranzacții); contoarele sesiunii înainte / după, `performance_schema` când se
        poate citi; orice cheie necunoscută păstrată ca `Extra`; statisticile tabelelor
        (`information_schema.TABLES`, `STATISTICS`, `performance_schema`);
      - **completarea:** cuvintele cheie MySQL ale `lang-sql` (completate), funcțiile
        serverului din driver.
    - **Teste:**
      - fără server: dialectul pe versiuni, interogările catalogului, valorile după tip,
        erorile, locul erorii, planurile reale (fișiere capturate pe matricea locală) citite
        în model, contoarele, statisticile;
      - pe matricea locală (`scripts/test-databases.sh`, `EDITOR_TEST_MYSQL_URLS`, doar dacă
        e dată): `db-mysql/tests/matrix.rs` și `database-explorer/tests/mysql.rs`, pe MySQL
        5.7, 8.0, 8.4 și MariaDB 10.11, 11.4, 11.8.
    - **Captură nouă:** `database-tree-mysql` (un server MySQL inventat, în memorie: bazele
      ca scheme sub sursă, `shop` deschisă).

## Profilarea interogărilor (extinderea pasului 39h: 39h2–39h6)

Cerut: „Nu uita că avem nevoie de profilarea interogărilor, după MySQL / Postgres sau ce baze
există și ce informații de profilare oferă. Normalizează profilarea, ca să arate la fel vizual
pentru toate driverele; ce e în plus se arată ca extra, fiindcă e mai bine să arătăm mai mult,
după driver. Profilarea se face la nivel de driver, datele se aduc, se normalizează prin API-ul
nostru și afișăm frumos profilarea pentru interogări, tabele etc. Tot ce se poate pentru a
profila o interogare și a înțelege ce face motorul în spate, ca să optimizăm și să găsim
blocajele; poate și sfaturi despre ce se poate optimiza, arătate cu roșu acolo unde e
problema. Vrem o unealtă super profesionistă și vizuală.”

Pasul 39h rămâne cum e scris (parametrii și tab-ul „Plan” de bază). Secțiunea de față îl
continuă cu cinci pași, câte un commit fiecare: modelul normalizat și driverele (39h2), motorul
de analiză (39h3), vederea profilului (39h4), graful și flacăra (39h5), compararea, tabelele și
statisticile pe instrucțiune (39h6).

### Ce avem deja (din 39c / 39e) și ce lipsește

- **`database::plan::PlanNode`** (`crates/domain/database/database/src/plan.rs`): operația, obiectul,
  `detail` (condițiile lipite într-un singur text), costurile, rândurile estimate și reale,
  timpul pe buclă, buclele și copiii. `from_query_plan` citește rândurile SQLite (`id`,
  `parent`, `detail`) ca arbore, fără să piardă nimic la părinți lipsă sau la cicluri.
- **`Session::explain(statement, analyze, cancel) -> Vec<PlanNode>`**, cu `ConsoleCaps::explain:
  ExplainKinds { plan, analyze }`:
  - Postgres (`crates/infra/database/postgres/src/{console,plan}.rs`):
    - rulează `EXPLAIN (FORMAT JSON)` sau `(ANALYZE, BUFFERS, FORMAT JSON)`, aceleași opțiuni
      pe orice versiune;
    - un `ANALYZE` peste o scriere e anulat de driver (`BEGIN … ROLLBACK`, sau `SAVEPOINT
      console_explain` în tranzacția utilizatorului);
    - `plan::parse` citește doar `Plan` (nu și `Planning Time`, `Execution Time`, `Triggers`,
      `JIT`, `Settings`) și din fiecare nod câteva chei. **Se pierd** `Shared Hit/Read Blocks`,
      `Rows Removed by Filter`, `Sort Method` / `Sort Space`, `Hash Batches`, `Workers`,
      `Plan Width`, `Parent Relationship`, `Output` și restul.
  - SQLite (`crates/infra/database/sqlite/src/lib.rs`): `EXPLAIN QUERY PLAN`, fără analiză
    (`ExplainKinds { plan: true, analyze: false }`).
  - protocolul v2 (`db-protocol`): metoda `explain`, cu `dto::PlanNode`; driverul `toy` dă un
    `Toy Scan`.
- **`Runner::explain`** (`crates/modules/database/console/src/runner.rs`) scrie planul în
  Output, rând cu rând (`write_plan`), cu bara care arată numărul de pași.
- **UI, de refolosit:**
  - `ui_components::diagram`: `Layout` / `Card` / `Edge`, `Viewport` (pan, zoom, fit),
    `card_frame` / `card_header` / `card_row`, `edge` cu săgeți, etichetele așezate fără
    suprapuneri. Așezarea o face fiecare vedere (diagrama relațiilor are `layout.rs` ei, pe
    coloane).
  - `tree::TreeList` (rândurile virtualizate, chevroanele, ghidajele), `data_grid` (grila și
    regula barei care se strânge în „…”, `data_grid::toolbar::layout`), `log_view`, `toast`,
    `progress_ring`, `splitter`, `tooltip`, `code_tokens` (SQL colorat în afara editorului).
  - `EditorToolbar::highlight` (instrucțiunea evidențiată în consolă) și `marks`.
- **Ce n-avem:** un model care să arate la fel pentru orice motor, măsurătorile pe
  instrucțiune (planificare, execuție, I/O, JIT), ce e specific unui motor păstrat și arătat,
  versiunile Postgres tratate pe rând, măsurătorile reale pe SQLite, analiza cu sfaturi și o
  vedere vizuală.

### Principii

1. **Profilarea o face driverul, modelul e al nostru.** Fiecare driver știe ce poate cere
   motorului lui și cum îi citește răspunsul. Domeniul definește un singur `Profile`
   normalizat. UI-ul și regulile de analiză văd doar `Profile`, niciodată JSON-ul unui motor.
2. **Nimic nu se pierde.** O cheie pe care driverul nu o normalizează intră în `extra`, cu
   tipul, unitatea și eticheta ei, și se arată în detaliile nodului sau ale instrucțiunii.
   Răspunsul brut al motorului se păstrează (limitat) pentru export.
3. **Aceeași vedere, mai multă informație unde există.** Coloanele, secțiunile și graficele
   apar după ce e în profil, nu după numele driverului. Un SQLite fără timpi pe nod arată
   același arbore, cu „—” pe coloanele de timp, nu un ecran separat.
4. **„Necunoscut” nu e zero.** Toate măsurătorile sunt `Option`. Estimatul și realul se
   deosebesc mereu: altă coloană, altă culoare, alt indiciu.
5. **Analiza e pură, explicabilă și nu rulează nimic.** Regulile lucrează pe modelul
   normalizat, în domeniu, testate fără bază. Fiecare constatare spune pe ce dovezi se
   sprijină (cifrele), de ce contează și ce s-ar putea face. SQL-ul sugerat (un `CREATE INDEX`,
   un `ANALYZE`) se copiază sau se pune în consolă; nu se rulează niciodată singur.
6. **Siguranța, ca la 39g.** O profilare „rulată” chiar execută instrucțiunea: o scriere se
   anulează mereu, se confirmă, iar pe o sursă doar citire nu pleacă.
7. **Nimic greu pe firul UI.** Citirea, normalizarea, calculele și regulile rulează pe
   `db-console`. UI-ul primește un `Arc<ProfileReport>` imuabil; așezările sunt `Memo`.
8. **Versionat.** Modelul are `PROFILE_FORMAT`, protocolul primește versiunea 3, iar
   exportul JSON își scrie formatul. Un câmp nou e opțional, deci nu rupe nimic.

### Deciziile mari (cu motivul)

- **`Session::profile` e nou, `explain` rămâne.**
  - `Session::profile(statement, &ProfileOptions, cancel) -> Result<Profile, DbError>`. Implicit,
    profilul se face din `explain` (`Profile::from_plan`): un driver v2 sau `toy` primește
    gratuit vederea profilului, doar cu operațiile, costurile și rândurile.
  - Postgres și SQLite implementează `profile` de-adevăratelea. `explain` devine la ele
    `profile(..).plan_nodes()`, deci nu există două parsere.
  - `PlanNode` rămâne (Output-ul, tab-ul „Plan” din 39h, protocolul v2), ca proiecție a
    profilului.
- **Măsurătorile sunt câmpuri cu tip, nu o hartă de texte.** Un `HashMap<String, f64>` ar fi
  ascuns unitățile și ar fi făcut regulile fragile. Ce înțeleg toate motoarele (rândurile,
  timpii, blocurile, spill-ul, paralelismul) e un câmp `Option<…>` cu unitatea fixată. Ce e al
  unui singur motor e `Extra`, cu tip și unitate.
- **Timpul propriu (self time) îl calculează domeniul**, nu driverul (`profile::derive`), după
  aceeași regulă pentru toți: `total × bucle − Σ copii`, cu cazurile Postgres speciale
  (lucrătorii paraleli, `InitPlan` / `SubPlan`, `CTE Scan`) tratate explicit și marcate
  „aproximativ” când e cazul. Așa, culorile hărții de căldură înseamnă același lucru la
  orice motor.
- **Timpul măsurat de noi e separat de cel raportat de motor.** Runner-ul măsoară ceasul de
  perete (conectare exclusă, rețea inclusă); motorul dă planificarea și execuția. Diferența
  mare (rețea, serializare) e ea însăși o informație (Postgres 17: `SERIALIZE`).
- **Arborele e plat, într-un vector** (`Vec<ProfileNode>`, cu `NodeId(u32)` ca poziție, `parent`
  și `children`), nu recursiv: planurile Postgres pe tabele partiționate au mii de noduri, iar
  UI-ul virtualizează, sare și compară după index. Un profil e imuabil, deci poziția e o
  identitate stabilă în el. Profilul însuși are `ProfileId` (UUID v4).
- **Vederea profilului e a modulului `database-console`.** În `ui-components` intră doar
  piesele generice, refolosibile și de alte module: tabelul-arbore, graficul icicle,
  așezarea unui arbore pentru `diagram`, rampa de căldură și culorile severităților. Modulul nu
  depinde de `ui-workbench`.
- **Sfaturile sunt sugestii, nu acțiuni.** Un index pe producție, un `SET work_mem` sau un
  `VACUUM` au costuri pe care unealta nu le vede (scrierile încetinite, blocările). Constatarea
  le arată cu SQL-ul gata scris și cu motivul, iar utilizatorul decide.

### Organizarea pe foldere

```
crates/domain/database/database/src/
    profile/                nou, pur (doar `kernel`)
        mod.rs              Profile, ProfileId, PROFILE_FORMAT, Engine, ProfileDepth, ProfileNote,
                            from_plan, plan_nodes
        node.rs             ProfileNode, NodeId, OpKind, JoinKind, ChildRole, ObjectRef,
                            Condition, CondKind, Estimates, Actuals, Io, Memory, Spill,
                            Parallel, Wal
        statement.rs        StatementProfile, Jit, TriggerTime, Setting, EngineCounters,
                            StatementStats, Wait
        extra.rs            Extra, ExtraValue, Unit, Quantity
        metric.rs           Metric (cheile coloanelor), Metric::of(node), present(profile)
        derive.rs           Derived (timpul propriu, procentele, calea fierbinte, raportul
                            estimare / real), ProfileReport
        cond.rs             condițiile citite cu lexerul 39a: coloane, constante, funcții,
                            conversii, OR, LIKE
        sql_map.rs          nodul → locurile din textul instrucțiunii
        rules/              mod.rs (Rule, Finding, Severity, Suggestion, Thresholds, Facts)
                            + un fișier pe regulă (seq_scan.rs, misestimate.rs, …)
        compare.rs          ProfileDiff: nodurile potrivite, diferențele
        tables.rs           TableUsage (ce face interogarea cu fiecare tabel), TableStats
        export.rs           textul (ca EXPLAIN), JSON-ul nostru versionat (scris de mână),
                            Markdown-ul (rezumat + constatări)
        testing.rs          ProfileBuilder (doar sub `cfg(test)` și feature-ul `testing`)
    capabilities.rs         + ProfileCaps în ConsoleCaps
    sql/fingerprint.rs      nou: amprenta unei instrucțiuni (literalele scoase), pentru N+1
crates/infra/database/postgres/src/
    profile.rs              opțiunile pe versiune, JSON → Profile, extra
    stats.rs                pg_stat_statements, pg_stat_user_tables / _indexes, mărimile
crates/infra/database/sqlite/src/
    profile.rs              EQP normalizat, rularea măsurată, sqlite3_stmt_status, statisticile
crates/infra/database/protocol/src/
    dto.rs / convert.rs     + dto::Profile (format 1), ProfileCaps, TableStats; VERSIONS 1–3
crates/ui/ui-components/src/
    tree_table.rs           nou: tabel-arbore virtualizat (coloane, bare, insigne)
    icicle.rs               nou: grafic icicle / flacără, generic
    diagram/tidy.rs         nou: așezarea unui arbore (Buchheim / Walker, O(n))
    severity.rs             nou: Severity UI (iconiță + culoare, sigure la daltonism),
                            Palette::heat
crates/modules/database/console/src/profile/
    mod.rs  store.rs  view.rs  tree.rs  graph.rs  flame.rs  details.rs  findings.rs
    summary.rs  compare.rs  tables.rs  export.rs  layout.rs  sample.rs (snapshot)
```

- **Dependențele nu se schimbă:** domeniul rămâne doar pe `kernel`, iar `database-console`
  primește `serde_json`, ca explorer-ul, doar pentru „Copiază JSON-ul motorului” formatat.
  `check-deps` trece neschimbat.

### Modelul normalizat (`database::profile`)

#### `Profile`

- `id: ProfileId` (UUID v4), `format: u16` (`PROFILE_FORMAT = 1`).
- `engine: Engine { driver: String (kind-ul driverului), name: String, version: Option<String>,
  version_num: Option<u32> }`.
- `depth: ProfileDepth`:
  - `Estimated`: planul, fără rulare;
  - `Actual { timing: bool }`: rulat. `timing: false` înseamnă rândurile reale fără timpii pe
    nod (Postgres `TIMING off`, mai ieftin pe interogări cu multe rânduri).
- `statement: StatementProfile`.
- `nodes: Vec<ProfileNode>`, `roots: Vec<NodeId>`. Postgres poate da mai multe planuri pe o
  instrucțiune (regulile de rescriere), deci sunt mai multe rădăcini.
- `collected: ProfileCaps`: ce s-a cerut și s-a primit la această profilare (nu ce poate
  driverul). UI-ul arată de aici „I/O: indisponibil” față de „I/O: 0”.
- `notes: Vec<ProfileNote>`: ce trebuie știut când citești profilul, ca chei pentru locale cu
  argumente:
  - `RolledBack` (scrierea a fost anulată);
  - `IoTimingOff` (`track_io_timing` oprit: fără timpii I/O);
  - `InstrumentationOverhead` (timpii includ costul măsurării);
  - `PlannerSettingsChanged`;
  - `NewerFormat { format }` (profil de la un driver extern mai nou: câmpurile necunoscute au
    fost lăsate);
  - `Truncated { nodes }` (un plan peste limită, vezi „Performanță”).
- `raw: Option<RawPlan { format: RawFormat::{Json, Text, Rows}, text: Arc<str> }>`: răspunsul
  motorului, pentru export. Cel mult 8 MB, altfel `None`, cu nota.

#### `ProfileNode`

Toate câmpurile cifrice sunt `Option`.

- **Locul în arbore:** `parent`, `children`, `role: Option<ChildRole>`: `Outer`, `Inner`,
  `InitPlan`, `SubPlan`, `Member`, `Subquery`, `Cte` (Postgres `Parent Relationship`, SQLite
  `CO-ROUTINE` / `MATERIALIZE`). Plus `subplan_name` (`InitPlan 1 (returns $0)`).
- **Operația:**
  - `op: OpKind`, normalizat: `SeqScan`, `IndexScan`, `IndexOnlyScan`, `BitmapHeapScan`,
    `BitmapIndexScan`, `RowidLookup`, `TidScan`, `FunctionScan`, `ValuesScan`, `CteScan`,
    `SubqueryScan`, `NestedLoop`, `HashJoin`, `MergeJoin`, `Hash`, `Sort`, `IncrementalSort`,
    `TempBTree`, `Aggregate(AggStrategy::{Plain, Sorted, Hashed, Mixed})`, `WindowAgg`,
    `Unique`, `SetOp`, `Limit`, `Materialize`, `Memoize`, `Gather`, `GatherMerge`, `Append`,
    `MergeAppend`, `Result`, `ModifyTable(WriteKind)`, `LockRows`, `AutoIndex`, `BloomFilter`,
    `MultiIndexOr`, `Coroutine`, `Compound`, `Other`;
  - `name: String`: cuvintele motorului (`Seq Scan`, `SEARCH users USING INDEX …`,
    `Table scan`), arătate mereu lângă felul normalizat;
  - `join: Option<JoinKind>` (`Inner`, `Left`, `Right`, `Full`, `Semi`, `Anti`).
- **Pe ce lucrează:**
  - `object: Option<ObjectRef { kind: ObjectKind::{Table, Index, Cte, Function, Subquery},
    schema, name, alias }>`;
  - `index: Option<String>`, `scan_direction`;
  - `conditions: Vec<Condition { kind: CondKind, text: String }>`, cu `CondKind`: `IndexCond`,
    `Recheck`, `Filter`, `JoinFilter`, `HashCond`, `MergeCond`, `OneTimeFilter`, `SortKey`,
    `GroupKey`, `Presorted`, `CacheKey`, `Other(String)`;
  - `output: Vec<String>` (Postgres `VERBOSE`).
- **`Estimates`:** `startup_cost`, `total_cost`, `rows`, `width` (octeți pe rând).
- **`Actuals`:**
  - `rows` și `loops`, `startup_ms` și `total_ms`, toate pe buclă, cum le dau motoarele;
  - `never_executed`;
  - `rows_removed_by_filter`, `rows_removed_by_join_filter`, `rows_removed_by_recheck`;
  - `heap_fetches` și `index_searches` (Postgres 18);
  - `exact_blocks` / `lossy_blocks` (bitmap).
- **`Io`**, în blocuri și cu `block_size` pentru octeți:
  - `shared_hit`, `shared_read`, `shared_dirtied`, `shared_written`;
  - aceleași pentru `local_*`, plus `temp_read` / `temp_written`;
  - `read_ms`, `write_ms`, `temp_read_ms`, `temp_write_ms`.
- **`Memory`:** `peak_bytes`, `sort_method`, `sort_space: Option<(bytes, Place::{Memory, Disk})>`,
  `hash: Option<HashStats { buckets, original_buckets, batches, original_batches }>`,
  `cache: Option<CacheStats { hits, misses, evictions, overflows }>` (`Memoize`).
- **`Spill`**, calculat de driver când motorul spune: `kind` (`Sort`, `Hash`, `HashAgg`,
  `Material`, `Window`, `Cte`), `disk_bytes`, `batches`.
- **`Parallel`:** `planned`, `launched`, `aware`, `per_worker: Vec<WorkerStats>` (rândurile,
  timpii, I/O și spill-ul fiecărui lucrător, cu `VERBOSE`).
- **`Wal`** (Postgres 13+): `records`, `full_page_images`, `bytes`.
- **`flags`:** `disabled` (Postgres 18, sau costul 1e10 dinainte), `async_capable`, `partial`.
- **`extra: Vec<Extra>`.**

#### `StatementProfile`

- `planning_ms`, `execution_ms`: ale motorului (Postgres `Planning Time` / `Execution Time`;
  `Total runtime` sub 9.4).
- `wall_ms`, `first_row_ms`, `fetch_ms`: ale runner-ului.
- `rows_returned`, `rows_affected`.
- `planning_io: Option<Io>` (Postgres 13+), `io_total: Option<Io>` (suma, calculată).
- `planning_memory` (Postgres 17 `MEMORY`), `serialize: Option<Serialize { ms, bytes, format }>`
  (Postgres 17).
- `jit: Option<Jit { functions, generation_ms, inlining: Option<f64>, optimization, emission,
  total_ms }>`.
- `triggers: Vec<TriggerTime { name, relation, calls, ms }>`.
- `settings: Vec<Setting { name, value }>`: setările planificatorului diferite de cele
  implicite (Postgres 12+ `SETTINGS`).
- `query_id: Option<i64>` (Postgres 14+, cu `compute_query_id`).
- `counters: Option<EngineCounters>`: contoarele motorului pe instrucțiune. SQLite: `fullscan_steps`,
  `sorts`, `autoindex`, `vm_steps`, `reprepare`, `filter_hits`, `filter_misses`, `mem_used`.
  MySQL (mai târziu): `rows_examined`, `select_scan`, `sort_merge_passes`, `tmp_disk_tables`.
- `history: Option<StatementStats>`: statisticile cumulate ale aceleiași interogări pe server
  (`pg_stat_statements`): `calls`, `mean_ms`, `stddev_ms`, `min_ms`, `max_ms`, `rows`, `hit_ratio`,
  `temp_blocks`, `wal_bytes`.
- `waits: Vec<Wait { event, ms: Option<f64>, count: Option<u64> }>` (MySQL `performance_schema`
  mai târziu; câmpul există de acum, ca formatul să nu se schimbe).
- `extra: Vec<Extra>`.

#### `Extra`, `Quantity`, `Unit`

- `Extra { key: String, label: Option<String>, group: Option<String>, value: ExtraValue }`:
  - `key` e cheia motorului (`Sort Method`, `r_filtered`, `Async Capable`);
  - `label` vine din locale când cheia e cunoscută (`profile.extra.<driver>.<cheie>`), altfel e
    cheia motorului;
  - `group` e secțiunea din panoul de detalii („JIT”, „Triggers”, „Workers”, „Altele”).
- `ExtraValue::{Bool, Int(i64), Float(f64), Text(String), Quantity(Quantity),
  List(Vec<ExtraValue>), Group(Vec<Extra>)}`. Un obiect JSON necunoscut devine `Group`, o
  listă devine `List`: nimic nu se aruncă.
- `Quantity { value: f64, unit: Unit }`, cu `Unit::{Rows, Millis, Bytes, Blocks, Pages, Count,
  Percent, Cost, Ratio, None}`. Formatarea (`1,2 GB`, `845 ms`, `12 340 rânduri`) e a UI-ului;
  indiciul arată valoarea exactă.

#### `Metric` și ce se arată

- `Metric::{SelfTime, TotalTime, StartupTime, EstRows, ActualRows, Loops, RowsRatio, Cost,
  Width, SharedHit, SharedRead, Written, Temp, Spill, RemovedByFilter, Workers, Memory}`, cu
  `Metric::of(&ProfileNode, &Derived) -> Option<Quantity>`.
- `present(profile) -> Vec<Metric>`: coloanele pe care le are măcar un nod. Tabelul-arbore
  arată exact acestea, în ordinea fixă. Așa, Postgres cu `ANALYZE, BUFFERS` arată toate
  coloanele, iar SQLite în plan estimat doar operația, obiectul și condițiile, în aceeași
  vedere.

#### Ce calculează domeniul (`profile::derive`)

`derive(&Profile) -> ProfileReport { profile: Arc<Profile>, derived: Vec<Derived>, hot_path:
Vec<NodeId>, totals }`.

- **`inclusive_ms`** = `total_ms × loops`. La nodurile de sub `Gather` / `Gather Merge`,
  timpii Postgres sunt media pe lucrători, deci `× loops` ar număra de mai multe ori: se
  împarte la `launched + 1` (lucrătorul principal, dacă participă), marcat `approx`.
- **`self_ms`** = `inclusive − Σ inclusive(copii)`, fără valori negative:
  - `InitPlan` / `SubPlan` se scad doar o dată;
  - un `CTE Scan` care citește un CTE materializat nu-și scade CTE-ul (îl plătește primul
    cititor).
- **`self_pct`**: din `execution_ms` (sau din suma rădăcinilor). La `Estimated`, același calcul
  se face pe cost (`self_cost`, `cost_pct`), spus în antetul coloanei („cost, nu timp”).
- **`total_rows`** = `rows × loops`.
- **`misestimate`** = `actual_total / est_total`, ca factor cu semn (`×120 ↑`, `×0,01 ↓`).
  Rândurile 0 se tratează ca 1, ca să nu dea infinit.
- **`hot_path`**: de la rădăcină, mereu copilul cu cel mai mare `inclusive` (sau cost). E
  drumul evidențiat în toate vederile.
- **`depth`**, ordinea în adâncime (pentru export și comparare), `share_of_parent`.
- **Testat pe:** paralelism, `InitPlan`, CTE, bucle, noduri `never executed`, planuri doar
  estimate și rotunjirile motorului (timpi pe buclă × bucle care depășesc părintele: tăiați,
  cu `approx`).

#### Capabilitățile (`ProfileCaps`, în `ConsoleCaps::profile`)

- Câmpurile, `bool`: `estimated`, `actual`, `node_timing`, `buffers`, `io_timing`, `wal`,
  `memory`, `spill`, `parallel`, `settings`, `verbose_output`, `jit`, `triggers`,
  `engine_counters`, `node_actuals_by_scan` (SQLite `scanstatus`, vezi întrebarea 1),
  `statement_history`, `table_stats`, `undo_writes` (driverul anulează singur o scriere
  profilată).
- `ProfileCaps::from_explain(ExplainKinds)`: ce dă implicitul (`estimated` = `plan`, `actual` =
  `analyze`). `ExplainKinds` rămâne, pentru protocolul v2.
- Pe sesiune, după versiunea serverului, ca restul `console()`: Postgres 9.2 n-are `wal`, iar
  12 are `settings`.
- `ProfileOptions`:
  - `depth`;
  - `buffers`, `verbose`, `settings`, `wal`;
  - `statement_history` (citește `pg_stat_statements` dacă există);
  - `timeout: Option<Duration>`;
  - `max_raw_bytes`.

  Driverul ignoră ce nu poate și spune în `collected` ce a făcut.

#### Contractul nou al sesiunii (`crates/domain/database/database/src/lib.rs`)

- `fn profile(&mut self, statement: &str, options: &ProfileOptions, cancel: &Cancel) ->
  Result<Profile, DbError>`. Implicit: `explain` → `Profile::from_plan` (operația, obiectul,
  `detail` ca o singură condiție `Other`, costurile, rândurile, timpii).
- `fn table_stats(&mut self, tables: &[TableRef], cancel: &Cancel) -> Result<Vec<TableStats>,
  DbError>`. Implicit `Unsupported`; din `TableInfo` se văd oricum estimarea rândurilor.
- **`TableStats`:** tabelul, `live_rows`, `dead_rows`, `table_bytes`, `index_bytes`,
  `total_bytes`, `seq_scans`, `idx_scans`, `last_analyze`, `last_autoanalyze`, `last_vacuum`
  (ca text ISO dat de server: domeniul n-are ceas), `modified_since_analyze`, `stats_present`
  (SQLite `sqlite_stat1`), `indexes: Vec<IndexStats { name, columns, unique, bytes, scans }>`
  și `extra`.

### Cum se potrivește fiecare motor

#### PostgreSQL (`db-postgres`, `profile.rs` și `stats.rs`)

- **Opțiunile, după versiune** (`profile::options(version, &ProfileOptions) -> String`, testat pe
  fiecare versiune majoră de la 9.2 la 18):

  | Ce | De la | Folosit |
  |---|---|---|
  | `FORMAT JSON`, `ANALYZE`, `VERBOSE`, `COSTS`, `BUFFERS` | 9.0 | mereu (`BUFFERS` doar cu `ANALYZE` sub 13) |
  | `TIMING`, `Rows Removed by Filter`, `Heap Fetches`, `I/O Read/Write Time` | 9.2 | `TIMING off` la cerere |
  | `Planning Time` / `Execution Time` (înainte `Total runtime`) | 9.4 | citite pe ambele forme |
  | `Workers Planned` / `Launched`, `Gather`, `Parallel Aware` | 9.6 | |
  | `SUMMARY` | 10 | `SUMMARY on` și la planul estimat (planificarea măsurată) |
  | `JIT` | 11 | |
  | `SETTINGS` | 12 | da |
  | `WAL`, `BUFFERS` la planificare, `Incremental Sort`, `HashAgg` pe disc | 13 | `WAL` doar la scrieri |
  | `Query Identifier` (cu `VERBOSE` și `compute_query_id`), `Memoize` | 14 | |
  | `Temp I/O` cu timpi | 15–16 | |
  | `MEMORY`, `SERIALIZE`, numele `Shared/Local I/O … Time` | 17 | `MEMORY` da; `SERIALIZE TEXT` doar cu „măsoară trimiterea rândurilor” |
  | `BUFFERS` implicit cu `ANALYZE`, `Index Searches`, `Disabled`, rânduri reale cu zecimale, stocarea `Material` / `WindowAgg` / CTE | 18 | |

  Tabelul se verifică la implementare pe documentația fiecărei versiuni (`release notes`,
  pagina `EXPLAIN`), iar testele au câte un fișier pe formă.
- **Citirea (`profile::parse(json, block_size) -> Profile`):**
  - fiecare cheie cunoscută are un loc în model (tabelul `KEYS`, cheie → câmp, cu unitatea);
  - orice altă cheie, de la nod sau de la instrucțiune, devine `Extra` (grupată după prefixul
    ei: `Workers`, `JIT`, `Triggers`), deci o versiune 19 cu chei noi le arată fără cod nou;
  - `block_size` vine din `SHOW block_size`, citit o dată la conectare (implicit 8192).
- **`plan::parse` dispare:** `explain` devine `profile` → `plan_nodes`, iar testele lui trec pe
  noul parser.
- **Rularea:**
  - `ANALYZE` peste o scriere rămâne cum e (anulat de driver, cu `SAVEPOINT` în tranzacția
    utilizatorului), plus nota `RolledBack`;
  - o citire profilată în `Tx: Auto` rulează într-un `BEGIN READ ONLY` al profilării: o funcție
    care scrie, chemată dintr-un `SELECT`, cade în loc să scrie;
  - în ambele cazuri, `SET LOCAL statement_timeout` (setarea `console_profile_timeout`) se
    anulează odată cu tranzacția sau cu `ROLLBACK TO SAVEPOINT`;
  - instrucțiunile care nu pot rula într-o tranzacție (`VACUUM`, `CREATE INDEX CONCURRENTLY`,
    `CREATE DATABASE`, `ALTER SYSTEM`) sunt refuzate la profilarea rulată, cu motivul
    (`profile::actual_refused(kind, flavor)`, din `classify` 39a).
- **`pg_stat_statements`** (`statement_history`, doar dacă e instalată: `pg_extension` și
  view-ul se poate citi):
  - o singură interogare după `queryid` (14+, din `Query Identifier`); sub 14, fără potrivire
    (textul normalizat de noi nu e al serverului: nu ghicim);
  - coloanele pe versiuni: `total_time` până la 12, apoi `total_exec_time` / `total_plan_time`;
    `wal_*` de la 13, `jit_*` de la 15, `stats_since` de la 17;
  - nu se rulează niciodată `CREATE EXTENSION`. Lipsa ei e o constatare `Info`, cu ce înseamnă
    instalarea (`shared_preload_libraries`, repornirea).
- **`auto_explain` nu e nevoie:** profilăm instrucțiunea noastră, pe conexiunea noastră.
- **`table_stats` (`stats.rs`):** o interogare pentru toate tabelele profilului, după `regclass`:
  - `pg_stat_user_tables` (`seq_scan`, `idx_scan`, `n_live_tup`, `n_dead_tup`, `last_*analyze`,
    `last_*vacuum`, `n_mod_since_analyze` de la 9.4, `last_seq_scan` de la 16 în `extra`);
  - `pg_class.reltuples` / `relpages`;
  - `pg_total_relation_size`, `pg_relation_size`, `pg_indexes_size`;
  - indexurile din `pg_index` ⋈ `pg_stat_user_indexes` (coloanele, unic, mărimea, `idx_scan`).

#### SQLite (`db-sqlite`, `profile.rs`)

- **Planul estimat:** `EXPLAIN QUERY PLAN`, ca acum, iar textul rândului se normalizează
  (`sqlite_op(detail) -> (OpKind, ObjectRef, index, conditions)`):

  | Textul SQLite | Normalizat |
  |---|---|
  | `SCAN t`, `SCAN TABLE t` (sub 3.36) | `SeqScan` |
  | `SCAN t USING [COVERING] INDEX i` | `IndexScan` / `IndexOnlyScan` (fără condiție: o parcurgere a indexului) |
  | `SEARCH t USING INDEX i (a=? AND b>?)` | `IndexScan` cu `IndexCond` |
  | `SEARCH t USING COVERING INDEX i (…)` | `IndexOnlyScan` |
  | `SEARCH t USING INTEGER PRIMARY KEY (rowid=?)` | `RowidLookup` |
  | `… USING AUTOMATIC [PARTIAL] COVERING INDEX (a=?)` | `AutoIndex` |
  | `USE TEMP B-TREE FOR ORDER BY / GROUP BY / DISTINCT / RIGHT PART OF ORDER BY` | `TempBTree` (`SortKey`: pentru ce) |
  | `CORRELATED SCALAR SUBQUERY n`, `LIST SUBQUERY n`, `SCALAR SUBQUERY n` | `role: SubPlan` |
  | `CO-ROUTINE x`, `MATERIALIZE x` | `Coroutine` / `Materialize` (`role: Cte`) |
  | `MULTI-INDEX OR`, `INDEX n` | `MultiIndexOr` |
  | `COMPOUND QUERY`, `UNION ALL`, `LEFT-MOST SUBQUERY` | `Compound` |
  | `BLOOM FILTER ON t (a=?)` (3.38+) | `BloomFilter` |
  | `SCAN CONSTANT ROW` | `Result` |
  | orice altceva | `Other`, cu textul întreg |

- **Profilarea rulată** (SQLite n-are `EXPLAIN ANALYZE`; o facem noi):
  - pe conexiunea consolei: `prepare` (timpul măsurat ca planificare), apoi pașii până la capăt,
    cu rândurile numărate și aruncate (fără sink), și timpul primului rând;
  - la final, `Statement::get_status` (rusqlite, sigur, fără `unsafe`): `FullscanStep`, `Sort`,
    `AutoIndex`, `VmStep`, `RePrepare`, `Run`, `FilterHit` / `FilterMiss`, `MemUsed`, în
    `EngineCounters`;
  - o scriere rulează între `SAVEPOINT console_profile` și `ROLLBACK TO` + `RELEASE`, în
    tranzacția utilizatorului sau într-una a ei;
  - termenul: un `deadline` în `progress_handler`-ul existent (întrerupere), lângă `Cancel`;
  - `ProfileCaps`: `estimated`, `actual`, `engine_counters`, `table_stats`, `undo_writes`.
    `node_timing` nu, deci timpul e doar pe instrucțiune, spus în antetul coloanelor.
- **Rândurile reale pe nod (opțional, întrebarea 1):** cu SQLite compilat cu
  `SQLITE_ENABLE_STMT_SCANSTATUS` (3.42+), `sqlite3_stmt_scanstatus_v2` dă pe fiecare buclă
  `nLoop`, `nVisit`, estimarea, `NCYCLE` și id-ul rândului EQP. Îl potrivim pe nodurile
  `from_query_plan`, deci avem rânduri reale și o proporție de timp pe nod. Cere `unsafe` local
  (`rusqlite::ffi`, cu `// SAFETY:`, cum permite `Cargo.toml`) și un flag de compilare.
- **`table_stats`:**
  - `sqlite_stat1` (dacă s-a rulat `ANALYZE`: altfel `stats_present: false`, folosit de o
    regulă);
  - `pragma_index_list` / `pragma_index_info`;
  - `page_size` × `page_count`;
  - mărimea pe tabel și pe index din `dbstat`, doar dacă e compilat (verificat la rulare prin
    `pragma_compile_options`);
  - rândurile, din estimarea existentă a catalogului.

#### MySQL / MariaDB (driverul a venit în 39p: `db-mysql`; aici cum intră în model)

Nu scriem acum parserul: nu există driverul. Modelul se verifică însă pe forma lor (un test
de domeniu construiește cu `ProfileBuilder` un profil MySQL și unul MariaDB complet), ca
driverul de mai târziu să nu ceară câmpuri noi.

- **MySQL:**
  - `EXPLAIN FORMAT=JSON` (5.6.5+): `access_type` (`ALL` → `SeqScan`, `index` → parcurgerea
    indexului, `range` / `ref` / `eq_ref` / `const` → `IndexScan`), `key`, `used_key_parts`,
    `rows_examined_per_scan` → `Estimates::rows`, `filtered`, `cost_info.*` → costurile (restul
    în `extra`), `attached_condition` → `Filter`, `using_filesort` → `Sort`,
    `using_temporary_table` → `Materialize` cu `Spill` posibil, `nested_loop` → `NestedLoop`;
  - `EXPLAIN FORMAT=TREE` (8.0.16+);
  - `EXPLAIN ANALYZE` (8.0.18+, arbore text cu `actual time=a..b rows=r loops=l`; JSON de la
    8.3 cu `explain_json_format_version = 2`). Rulează instrucțiunea, deci o scriere doar în
    tranzacție anulată și doar pe tabele InnoDB. Un tabel netranzacțional (MyISAM) face
    profilarea rulată a unei scrieri refuzată, cu motivul;
  - pe instrucțiune:
    - diferența `SHOW SESSION STATUS` înainte și după (`Handler_read_*`, `Select_scan`,
      `Select_full_join`, `Sort_merge_passes`, `Created_tmp_disk_tables`) → `EngineCounters`;
    - `performance_schema.events_statements_history` (`ROWS_EXAMINED`, `NO_INDEX_USED`,
      `NO_GOOD_INDEX_USED`, `LOCK_TIME`);
    - `events_stages_history_long` → `waits` și etapele, ca `extra`, dacă sunt pornite
      consumatorii;
    - `SHOW PROFILE` (învechit) doar ca rezervă.
- **MariaDB:**
  - `ANALYZE FORMAT=JSON` (10.1+): `r_loops`, `r_rows`, `r_total_time_ms`, `r_filtered`,
    `r_table_time_ms` / `r_other_time_ms` → `Actuals`, iar restul în `extra`;
  - `EXPLAIN FORMAT=JSON` pentru planul estimat.

#### Driverele externe (protocolul v3)

- `VERSIONS` devine 1–3. Versiunea 3 adaugă:
  - metoda `profile` (`dto::Profile`, cu `format: 1`, aceleași nume de câmpuri ca exportul
    JSON);
  - metoda `table_stats`;
  - `dto::ConsoleCaps::profile`.
- Un driver v2 primește profilul din `explain`, prin implicitul domeniului, fără nimic de
  făcut.
- `dto::Profile::format` e separat de versiunea protocolului: câmpurile noi sunt opționale
  (`#[serde(default)]`), iar un `format` mai mare decât al nostru se citește cât se înțelege, cu
  nota `NewerFormat`.
- `extra` trece ca `{ key, label?, group?, value: { type, value, unit? } }`.
- Rândurile unei profilări rulate nu se trimit niciodată (`$/rows` nu se folosește).
- Driverul `toy` implementează `profile` (un plan cu timpi, I/O, un `extra` din fiecare tip și
  o cheie necunoscută).
- Contractul se documentează în `03-sistem-module.md` §6, cu un exemplu.

### Siguranța profilării

- **Profilarea rulată execută instrucțiunea.** Bara o spune în meniu („Explain Analyze —
  rulează instrucțiunea”), iar modalul de confirmare (39g, `view-console-confirm`) o arată:
  - **o scriere:** se cere mereu confirmarea, cu „modificările se anulează la final”.
    Avertismentul spune și ce nu se anulează: secvențele avansează, efectele din afara bazei
    (`dblink`, FDW, `COPY … PROGRAM`, fișiere) rămân, iar blocările pe rânduri se țin cât
    rulează;
  - **pe producție (semnul din 39g):** orice profilare rulată, și o citire, cere confirmare,
    fiindcă încarcă serverul (vezi întrebarea 2);
  - **o sursă doar citire:** profilarea rulată a unei scrieri e refuzată înainte să plece,
    chiar dacă s-ar anula, ca orice scriere. Citirile merg, într-o tranzacție `READ ONLY`.
- **Termenul și oprirea:** `console_profile_timeout` (implicit 300 s), pus de driver
  (`statement_timeout` local la Postgres, `deadline` în `progress_handler` la SQLite, `timeout`
  la protocol). ■ Stop anulează ca la orice rulare (`cancel_query`, apoi anularea tranzacției
  profilării).
- **Interogările auxiliare** (`pg_stat_statements`, statisticile tabelelor, `SHOW block_size`)
  sunt citiri mici, pe aceeași conexiune, după profil. Se pot opri din setări și nu se fac
  niciodată pe un driver `limited`.
- **Datele din profil:** condițiile conțin literale (`Filter: (email = 'x@y.ro')`), deci pot fi
  date personale.
  - Profilele stau doar în memorie, pe consolă, cât e deschis proiectul. Nu se scriu pe disc
    decât la un export cerut.
  - Istoricul (39i) nu ține profile.
  - Literalele cu parole sunt mascate în textul afișat (`mask_secrets`, aplicat condițiilor).
- **Sugestiile nu se rulează niciodată.** „Inserează în consolă” adaugă textul la capătul
  consolei, selectat, fără ▶. Pe producție, sugestia de index Postgres e `CREATE INDEX
  CONCURRENTLY` (cu nota că nu merge într-o tranzacție).

### Motorul de analiză (`database::profile::rules`, pur)

#### Contractul

- `trait Rule: Send + Sync { fn id(&self) -> &'static str; fn applies(&self, report:
  &ProfileReport) -> bool; fn check(&self, cx: &RuleContext) -> Vec<Finding>; }`. `id` e stabil și
  tehnic (`seq-scan-selective`): cheia în locale și a „ignoră regula”.
- `RuleContext`:
  - `report: &ProfileReport`;
  - `facts: &Facts`:
    - indexurile tabelelor atinse (coloanele, ordinea, unic, parțial, pe expresie);
    - cheile externe, ale motorului și cele virtuale acceptate (`RelationNotes`, planul 37);
    - `TableStats`, dacă au fost citite;
  - `statement: &PlannedStatement` (textul și tokenii 39a), `flavor`;
  - `recent: &RecentRuns` (amprentele instrucțiunilor rulate în sesiunea consolei, cu numărul
    și momentele);
  - `thresholds: &Thresholds`.
- `Facts` le adună modulul, în fundal, pe conexiunea consolei, doar pentru tabelele din profil
  (cel mult 50): `structure` (deja în contract), `ConsoleAccess::notes`, `table_stats`. Fără
  `Facts`, regulile care au nevoie de ele nu dau nimic. Nu ghicim un index care poate există.
- `Finding`:
  - `rule`, `severity: Severity::{Info, Warning, Critical}`;
  - `impact: f64`: partea din timp (sau cost) pe care o atinge, 0–1, după care se sortează;
  - `nodes: Vec<NodeId>`;
  - `evidence: Vec<(EvidenceKey, Quantity)>`: cifrele, arătate în text;
  - `suggestion: Option<Suggestion>`;
  - `sql_range: Option<Range<usize>>`: locul din instrucțiune, din `sql_map`.

  Textele vin din locale (`profile.finding.<id>.{title,why,fix}`), cu argumentele din
  `evidence`: domeniul nu are texte de UI.
- `Suggestion`:
  - `CreateIndex { table, columns, include, where_clause, expression, sql }`. SQL-ul e scris
    prin `Dialect::quote_identifier`, cu numele `idx_<tabel>_<coloane>` tăiat la 63 de octeți
    (limita Postgres);
  - `Analyze { table, sql }`, `Vacuum { table, sql }`, `Statistics { sql }` (`CREATE STATISTICS`,
    10+);
  - `Setting { name, value, sql }` (doar la nivel de sesiune: `SET work_mem = '64MB'`);
  - `Rewrite { hint: RewriteHint }` (`UnionAll`, `KeysetPagination`, `ListColumns`, `Batch`,
    `MatchLiteralType`), fără text generat.
- `analyze(cx) -> Vec<Finding>`: toate regulile care se aplică, sortate după severitate, apoi
  după impact. O constatare „Punctul fierbinte: N% din timp în nodul X” e mereu prima
  (`Info`), ca să se știe de unde să înceapă.
- **`cond` (condițiile, pur):** citește textul unei condiții cu lexerul 39a (pe flavor) și dă
  coloanele comparate cu o constantă sau un parametru (egalitate, interval, `IN`, `IS NULL`),
  coloanele învelite într-o funcție (`lower(email)`), coloanele convertite (`(col)::text`),
  `OR`-urile de la nivelul de sus, `LIKE` cu `%` la început și perechile de coloane din join.
  O coloană e atribuită tabelului nodului doar când e necalificată sau calificată cu aliasul
  lui.
- **`sql_map` (nodul → textul):** tabelul și aliasul nodului găsite în `FROM` / `JOIN` (tokenii
  39a), coloanele condiției în `WHERE` / `ON`. Doar când locul e unic, altfel `None`. Locul se
  duce în document prin `BindMap` (39h) și se arată cu `EditorToolbar::highlight`, doar cât
  textul consolei e cel profilat (`text_hash`).
- **`sql::fingerprint` (39a, nou):** instrucțiunea fără literale și parametri, cu spațiile
  normalizate, cu hash. Folosită de N+1 și de compararea profilelor aceleiași interogări.

#### Regulile

Fiecare regulă are un fișier, documentația ei (ce dovadă cere, pragul și de ce) și teste cu
cazuri pozitive și fals-pozitive. Pragurile sunt în `Thresholds` (constante cu nume, nu
setări, în prima versiune).

| id | Când | Severitate | Sugestia |
|---|---|---|---|
| `hot-spot` | mereu, nodul cu cel mai mare timp propriu (sau cost) | Info | — |
| `seq-scan-selective` | `SeqScan` pe ≥ 10 000 de rânduri citite (reale + eliminate, sau estimate) care păstrează ≤ 10 % | Warning; Critical peste 30 % din timp | `CreateIndex` pe coloanele de egalitate, apoi cea de interval; dacă indexul există: de ce nu e folosit (funcție, conversie, statistici, selectivitate mică) |
| `misestimate` | real / estimat ≥ 10 (în orice sens), cu ≥ 100 de rânduri pe una din părți; mai grav când nodul hrănește un join | Warning | `Analyze`; cu ≥ 2 coloane din același tabel în filtru, `CREATE STATISTICS` (10+); statistici vechi din `TableStats` ca dovadă |
| `nested-loop-many` | latura interioară a unui `NestedLoop` cu ≥ 1 000 de bucle și o parcurgere completă, sau ≥ 20 % din timp | Warning / Critical | `CreateIndex` pe coloana de join a laturii interioare; sau estimarea greșită a laturii exterioare |
| `spill-to-disk` | `Sort` pe disc (`external merge`), `Hash` cu `Batches > 1`, `HashAgg` pe disc (13+), `Material` / `WindowAgg` / CTE pe disc (18) | Warning | `Setting work_mem` la puterea lui 2 peste ~2× ce a ajuns pe disc, la nivel de sesiune, cu nota că se înmulțește cu nodurile și lucrătorii |
| `filter-removes-most` | ≥ 90 % din rânduri eliminate de filtru, ≥ 1 000, pe orice parcurgere (și pe index: indexul nu acoperă filtrul) | Warning | `CreateIndex` compus sau parțial (`where_clause` din filtrul constant) |
| `fk-join-no-index` | join pe o cheie externă (a motorului sau virtuală acceptată) fără niciun index care să înceapă cu coloanele ei, pe latura parcursă complet | Warning | `CreateIndex` pe coloanele cheii |
| `sqlite-scan` | SQLite `SCAN t` (nu printr-un index) în interiorul unei bucle, sau cu un `WHERE` pe `t`; întărit de `fullscan_steps` mare față de rândurile întoarse | Warning | `CreateIndex` |
| `sqlite-temp-btree` | `USE TEMP B-TREE FOR ORDER BY / GROUP BY / DISTINCT` peste ≥ 1 000 de rânduri estimate | Info / Warning | `CreateIndex` în ordinea din `ORDER BY` (din tokenii instrucțiunii) |
| `auto-index` | SQLite `AUTOMATIC … INDEX` sau contorul `autoindex` > 0 | Warning | `CreateIndex` permanent pe coloanele numite de SQLite |
| `function-on-column` | o condiție `f(col) op const` pe o coloană cu index simplu, fără index pe expresie | Warning | `CreateIndex` pe expresie (Postgres; SQLite 3.9+) sau rescrierea |
| `implicit-cast` | coloana convertită în condiție (`(col)::text = …`), cu un index pe ea | Warning | `MatchLiteralType` |
| `or-defeats-index` | `OR` între coloane diferite într-un filtru pe `SeqScan`, fiecare cu index | Info | `UnionAll` (sau nota despre `BitmapOr`); SQLite `MULTI-INDEX OR` nu e problemă |
| `leading-wildcard` | `LIKE '%…'` pe o coloană indexată | Info | Postgres: index `pg_trgm` (GIN), cu nota despre extensie |
| `select-star-wide` | `SELECT *` la nivelul de sus și lățimea rândului ≥ 500 de octeți, sau > 20 de coloane | Info | `ListColumns` (deschide calea unui `Index Only Scan`) |
| `limit-without-order` | `LIMIT` fără `ORDER BY` la același nivel | Info | ordinea nu e garantată |
| `big-offset` | `OFFSET` ≥ 10 000 | Info | `KeysetPagination` |
| `n-plus-one` | aceeași amprentă rulată de ≥ 20 de ori în sesiune în ≤ 60 s, sau `calls` mare cu `mean_ms` mic în `pg_stat_statements` | Info | `Batch` (`IN (…)` / `JOIN`) |
| `workers-not-launched` | `Workers Launched < Planned` | Warning | `max_parallel_workers` / `max_worker_processes` ocupate |
| `jit-dominates` | JIT ≥ 30 % din execuție și ≥ 50 ms | Warning | `Setting jit = off` pe sesiune, sau un `jit_above_cost` mai mare |
| `planning-dominates` | planificarea ≥ execuția și ≥ 10 ms | Info | partițiile, joinurile multe, instrucțiunile pregătite |
| `heap-fetches` | `Index Only Scan` cu `Heap Fetches` ≥ 10 % din rânduri | Info | `Vacuum` (harta de vizibilitate) |
| `lossy-bitmap` | `Lossy Heap Blocks` > 0 | Info | `Setting work_mem` |
| `dead-tuples` | `dead_rows` ≥ 20 % din `live_rows` (din `TableStats`) | Info / Warning | `Vacuum` |
| `stale-stats` | niciun `ANALYZE` sau `modified_since_analyze` > 20 %; SQLite cu joinuri fără `sqlite_stat1` | Info / Warning | `Analyze` |
| `cold-cache` | blocurile citite de pe disc ≥ 90 % din cele atinse și ≥ 1 000 | Info | prima rulare: rulează din nou ca să compari; fără `track_io_timing`: nota |
| `disabled-node` | un nod `Disabled` (18), costul ≥ 1e10 sau `enable_*` oprit în `SETTINGS` | Warning | setarea care l-a oprit |
| `trigger-time` | triggerele ≥ 30 % din timpul unei scrieri | Info | triggerul cu numele și timpul lui |

- Regulile MySQL viitoare (filesort, temporary, `ALL`) nu sunt reguli separate: aceleași
  `seq-scan-selective`, `spill-to-disk`, `sqlite-temp-btree` (redenumită atunci
  `temp-sort`) lucrează pe forma normalizată.
- **Testate:** fiecare regulă pe profile construite cu `ProfileBuilder` (pozitiv, la limită,
  fals-pozitiv):
  - un `SeqScan` pe un tabel mic, sau unul care întoarce aproape tot;
  - o buclă cu 1 iterație;
  - o estimare greșită pe 3 rânduri;
  - un index existent care acoperă deja;
  - `OR` pe aceeași coloană;
  - `LIMIT 1` cu `ORDER BY`.

  Plus textul exact al SQL-ului sugerat (citarea, numele lung tăiat, `CONCURRENTLY` pe
  producție). Bugetul: regulile pe 10 000 de noduri sub 20 ms (release).

### UI: profilul

#### Unde apare

- **În „Services”**, ca un tab de rezultat al sesiunii consolei: „Profil · orders” (iconița
  `plan`), cu aceleași reguli ca rezultatele: un tab nefixat e înlocuit de profilul următor, unul
  fixat rămâne.
  - Tab-ul „Plan” din 39h devine tab-ul „Profil”, cu tabelul-arbore pe aceleași coloane, ca să
    nu se arunce nimic (vezi „Pașii”).
  - Output-ul primește un rând: „Profil: 845 ms execuție, 3 probleme (1 critică)”, cu o
    legătură care deschide tab-ul.
- **Într-un tab de editor:** „Deschide în tab” (din bara profilului sau din meniul tab-ului)
  deschide un `Viewer` de felul `view-profile` (`host.open_view`) peste același
  `Arc<ProfileReport>`. Titlul e `profil [sursă] · orders`, cu culoarea sursei (`accent`) și
  breadcrumb-ul `Database Consoles › sursă › console › profil`.
- **Comenzile** (`When::View` pe vederea profilului): `database.profile.next_problem` (F2) /
  `prev_problem` (⇧F2), `hottest` (H), `expand_all` / `collapse_all`, `view_tree` /
  `view_graph` / `view_flame` / `view_tables`, `compare`, `copy_text`, `export`, `open_in_tab`.
  „Explain Plan” și „Explain Analyze” din meniul ▶ fac profile (estimat / rulat), iar ⚙
  primește opțiunile (`BUFFERS`, `TIMING`, `VERBOSE`, statisticile).

#### Așezarea (responsivă, ca bara grilei)

- **Bara profilului** (40 px), de la stânga la dreapta:
  - comutatorul vederilor (Arbore, Diagramă, Flacără, Tabele);
  - Compară ▾ (cu profilele consolei);
  - filtrul nodurilor, ⤢ „Deschide în tab”, Export ▾;
  - în dreapta, insignele: `ANALYZE` / `estimat`, `anulat`, motorul și versiunea.

  Ce nu încape intră în „…”, după regula `data_grid::toolbar::layout` (calculată o dată pe
  lățime și stare, `Memo`). Sub 560 px, comutatorul devine un meniu.
- **Bara rezumatului**, sub ea (`summary.rs`): pastile cu cifre, fiecare cu indiciu exact.
  - **Planificare** 1,2 ms · **Execuție** 845 ms · **Aducere** — · **Rânduri** 12 · **Buffere**
    1,2 GB din cache / 340 MB citiți · **Temp** 50 MB · **JIT** 120 ms · **Lucrători** 2/4;
  - contorii SQLite: pași VM, parcurgeri complete, sortări, indexuri automate;
  - în dreapta, constatările pe severitate (● 1 critică ▲ 2 avertismente ⓘ 3), care filtrează
    lista la click.

  Pastilele fără valoare nu apar.
- **Corpul:** vederea aleasă la stânga, iar la dreapta un panou (`splitter`, lățimea ținută în
  sesiunea proiectului) cu două secțiuni: „Ce se poate optimiza” (constatările) și „Detalii”
  (nodul ales).
  - Sub 820 px panoul devine un sertar deschis din butonul lui, peste vedere, nu modal.
  - Sub 520 px, constatările trec deasupra, ca o listă strânsă.

#### (1) Arborele (`tree.rs`, pe `ui_components::tree_table`)

- **Coloanele**, din `Metric::present` (ordinea fixă; lățimile ținute; antetul cu indiciu care
  spune ce e și unitatea):
  - **Operația:** iconița felului (`OpKind`: scanare, index, join, sortare, agregare…), numele
    motorului, obiectul îngroșat, indexul estompat, chevroanele și ghidajele `TreeList`;
  - **Timp propriu:** o bară orizontală pe celulă, cu lungimea = `self_pct` și culoarea din
    rampa de căldură, plus „412 ms · 48 %”;
  - **Timp total**, **Rânduri** (estimat → real, cu insigna `×120 ↑` colorată când factorul
    ≥ 10), **Bucle**, **Cost**;
  - **Buffere:** o bară mică în două părți (din cache / citite), cu cifrele în indiciu;
  - **Eliminate de filtru**, **Lățime**, **Spill** (iconița discului și mărimea), **Lucrători**.
- **Calea fierbinte:** o dungă în accent la stânga rândurilor ei și textul operației mai
  îngroșat. `H` sare la capătul ei.
- **Problemele:** o coloană-gutter cu iconița severității. Rândul are un fundal ușor în
  culoarea severității, iar celula metricii vinovate e conturată. Indiciul arată titlul
  constatării și cifrele.
- **Tastele:** ↑ ↓ ← → (deschide / închide), Home / End, PgUp / PgDn, ↵ (detaliile), `*`
  (deschide tot), F2 / ⇧F2 (problema următoare / anterioară), ⌘C (rândul ca text), tastarea
  filtrează (ca în „Database”).
- **Planurile mari:** peste 300 de noduri, cele sub 0,5 % din timp (sau cost) și mai adânci de
  12 niveluri pornesc închise, cu un rând „+ 1 240 de noduri (3 % din timp)”. Nodurile cu
  probleme și calea fierbinte rămân mereu deschise.

#### (2) Diagrama (`graph.rs`, pe `ui_components::diagram`)

- O carte pe nod:
  - antetul: iconița felului și operația, cu dunga de sus în culoarea căldurii;
  - rândurile: obiectul, timpul propriu / total, rândurile estimat → real, costul;
  - insigna severității în colț.
- Așezarea: `diagram::tidy` (nou, generic, O(n)), de sus în jos, copiii sub părinte. Muchiile
  sunt ortogonale, de la copil spre părinte (sensul datelor).
- Grosimea muchiei crește cu `log10` din rândurile reale (sau estimate), între 1 și 8 px
  (`EdgeStyle::width`, nou; apelurile existente pun valoarea de azi), cu eticheta rândurilor.
  Calea fierbinte e în accent.
- `Viewport`: pan, zoom, „potrivește”, „centrează pe nodul ales”. Se desenează doar cărțile din
  `visible()`. Subarborii închiși sunt o carte „+N noduri”. Click alege nodul în toate vederile.

#### (3) Flacăra / icicle (`flame.rs`, pe `ui_components::icicle`)

- De sus în jos, lățimea fiecărui nod = timpul lui inclusiv (sau costul, la estimat); partea
  proprie se vede ca segmentul neacoperit de copii. Culoarea vine din rampa de căldură după
  `self_pct`, iar problemele au conturul în culoarea severității.
- Copiii care, din rotunjiri sau paralelism, depășesc părintele sunt scalați proporțional în
  el, cu `approx` în indiciu.
- Click mărește subarborele (cu „înapoi” și breadcrumb-ul drumului), iar hover-ul arată
  indiciul exact. Dreptunghiurile sub 2 px se unesc într-unul „…”.

#### (4) „Ce se poate optimiza” (`findings.rs`)

- O carte pe constatare, sortată după severitate și impact:
  - iconița și culoarea severității, titlul;
  - textul „de ce” cu cifrele (`a citit 1 204 332 rânduri ca să păstreze 18 (0,001 %)`);
  - legătura spre nod;
  - sugestia, ca bloc SQL colorat (`code_tokens` cu `sql`), cu „Copiază” și „Inserează în
    consolă”;
  - „Ignoră în această consolă” (în memorie).
- Click pe carte alege nodul în vederea curentă (derulat la el) și evidențiază `sql_range` în
  consolă, dacă e deschisă și textul e același.
- Fără constatări: „Nimic de semnalat”, cu ce s-a verificat. La un profil doar estimat: „Rulează
  Explain Analyze pentru mai multe sfaturi”.

#### (5) Rezumatul instrucțiunii

E bara de mai sus. Sub 39h6, `history` (`pg_stat_statements`) adaugă o pastilă: „media pe
server 32 ms în 18 400 de apeluri”.

#### (6) Detaliile nodului (`details.rs`)

- Secțiunile, doar cele cu date:
  - **Operația:** felul normalizat și numele motorului;
  - **Obiectul:** schema, tabelul, aliasul, indexul;
  - **Condiții:** colorate ca SQL, câte una pe rând, cu felul ei;
  - **Estimat față de real:** un mic tabel cu rânduri, bucle, timpi pe buclă și în total, cost,
    lățime;
  - **I/O**, **Memorie și disc**, **Paralelism** (un rând pe lucrător), **Ieșire**
    (`VERBOSE`);
  - **Specific PostgreSQL** / **Specific SQLite** / al driverului: fiecare `Extra`, pe grupuri,
    cu eticheta, valoarea și unitatea;
  - **Probleme:** constatările nodului.
- Fiecare valoare se poate copia. Indiciul arată valoarea brută și unitatea.

#### (7) Compararea (`compare.rs`, 39h6)

- „Compară ▾” listează profilele ținute ale consolei (cel mult `console_profile_keep`, implicit
  20), cu ora, durata și amprenta. Recomandat: cele cu aceeași amprentă (aceeași interogare).
- **`profile::compare::diff(a, b) -> ProfileDiff`** (pur):
  - potrivește nodurile după drum (`OpKind` și obiect), cu LCS pe copii;
  - un `SeqScan on orders` care a devenit `Index Scan on orders` e „schimbat”, nu șters și
    adăugat;
  - dă diferențele pe metrici și pe instrucțiune.
- **Vederea:** două tabele-arbore aliniate pe rând, cu coloane Δ (timp, %, rânduri, buffere) și
  rezumatul diferențelor sus („845 ms → 12 ms, −98,6 %”).
  - Îmbunătățirea și regresia se arată cu albastru / portocaliu (sigure la daltonism) și cu
    ▼ / ▲, niciodată doar prin culoare.
  - Insigna „forma planului s-a schimbat”.

#### (8) Tabelele (`tables.rs`, 39h6)

- Un rând pe tabel atins (`profile::tables::usage`):
  - aliasurile;
  - cum a fost citit (parcurgere, index, index-only, bitmap, cu numele indexurilor);
  - rândurile citite și întoarse, partea din timp, problemele lui.
- Din catalog, cerute leneș la deschiderea tab-ului (`table_stats`, în fundal):
  - rândurile vii și moarte;
  - mărimea tabelului și a indexurilor;
  - ultimul `ANALYZE` / `VACUUM`, contorii `seq_scan` / `idx_scan`;
  - indexurile, cu „folosit aici” și „nefolosit niciodată” (`idx_scan = 0`);
  - SQLite: `sqlite_stat1` prezent, paginile.
- Click pe tabel alege nodurile lui. Meniul are „Deschide tabelul” și „Structura”
  (`ConsoleAccess::open_table`).

#### (9) Exportul (`export.rs` + `database::profile::export`)

- „Copiază ca text”: arborele indentat ca `EXPLAIN`, cu metricile noastre normalizate.
- „Copiază JSON-ul motorului”: `raw`, ca să poată fi dus în uneltele obișnuite. Noi nu urcăm
  nimic nicăieri.
- „Exportă JSON…”: formatul nostru versionat (`format: 1`, aceleași nume ca `dto::Profile`),
  scris de domeniu fără dependențe și salvat prin dialogul exportului existent.
- „Copiază ca Markdown”: rezumatul, constatările și sugestiile, pentru un PR sau un tichet.

#### Culorile și accesibilitatea

- **Severitățile** (`ui_components::severity`): Info albastru (cerc cu `i`), Warning portocaliu
  (triunghi), Critical vermilion (octogon cu `!`), din paleta Okabe–Ito, deci deosebite și la
  deuteranopie / protanopie. Mereu iconiță și culoare, niciodată doar culoare. Fiecare temă își
  pune nuanțele (`Palette::severity`), iar contrastul cu fundalul e verificat în teste (≥ 3:1
  pentru iconițe).
- **Rampa de căldură** (`Palette::heat(t)`): o singură nuanță cu luminozitatea crescătoare, deci
  ordinea se citește și în tonuri de gri, plus procentul scris. Una pe temă (închisă, deschisă).
- **Iconițe noi:** `flame`, `octagon-alert`, `hard-drive` (spill), `lightbulb` (sugestia),
  `route` (calea fierbinte). Restul există (`plan`, `graph`, `compare`, `table`, `index`,
  `memory`, `cpu`, `clock`, `info`, `alert`).
- **Tastatura** în toate vederile; indiciile cu valorile exacte; ambele teme în capturi.

#### Piesele generice noi din `ui-components`

- **`tree_table::TreeTable`:** rânduri cu adâncime, deschise / închise, peste `TreeList`
  (virtualizat). Coloane cu titlu, aliniere, lățime minimă și redimensionare. Celule
  `Text` / `Number { value, unit }` / `Bar { fraction, tone }` / `Split { a, b }` / `Badge` /
  `Icon`. Rândul ales, hover, gutter-ul de semne, indiciile pe celulă. Așezarea e `Memo` pe
  lățime și generație.
- **`icicle::Icicle`:** noduri `{ parent, value, self_value, label, tone, mark }`. Așezarea pe
  lățime (`Memo`), testul de lovire, mărirea pe subarbore și unirea celor sub 2 px.
- **`diagram::tidy`:** așezarea unui arbore cu mărimile cărților date, de sus în jos sau de la
  stânga la dreapta, fără suprapuneri, O(n). Muchiile ortogonale ca `Edge`.
- **`severity`:** `Level`, culorile și iconițele, plus `Palette::heat`.

Toate sunt testate în `ui-components/tests/unit/` (așezările pe forme de arbore, 10 000 de
noduri sub buget, testul de lovire, contrastul culorilor).

### Unde stau datele (modulul `database-console`)

- **`profile::store::Profiles`**, pe consolă (în `Runner`), deci pe proiect:
  - cel mult `console_profile_keep` (20) și `console_profile_memory` (32 MB). Cele vechi
    nefixate ies primele;
  - memoria se vede în Task Manager (`ResourceReporter`), iar „Clear and Rebuild Caches” le
    golește (`Purge`) pe cele nefixate;
  - nimic nu trece la alt proiect. Închiderea consolei le eliberează.
- **`Runner::profile(statement, options)`**, pe `db-console`, în ordine:
  1. confirmarea, ca la 39g;
  2. `Session::profile`;
  3. `derive`;
  4. `Facts` (structurile tabelelor atinse, `notes`, `table_stats` dacă e cerut);
  5. `rules::analyze`.

  Rezultatul ajunge ca un `Arc<ProfileReport>` cu constatările. UI-ul primește doar `poll`.
- **Setările** (`crates/modules/database/console/src/settings.rs`, blocul „Profilare” din
  „Console de interogări”): `console_profile_timeout` (300 s), `console_profile_buffers` (da),
  `console_profile_timing` (da), `console_profile_verbose` (nu),
  `console_profile_statement_stats` (da), `console_profile_keep` (20), `console_profile_memory`
  (32 MB).

### Performanță

- **Pe `db-console`:** citirea JSON-ului, normalizarea, `derive`, `Facts` și regulile. Bugetele,
  verificate în teste, release:
  - un plan Postgres de 5 000 de noduri (un `Append` pe 5 000 de partiții, generat în test)
    citit sub 50 ms;
  - `derive` + regulile pe 10 000 de noduri sub 20 ms;
  - `tidy` pe 10 000 de noduri sub 10 ms.
- **Limitele:** peste 50 000 de noduri, profilul se păstrează întreg, dar vederile pornesc cu
  subarborii mici închiși (`Truncated` spune câte). `raw` are cel mult 8 MB.
- **Pe cadru:** nimic recalculat dacă nu s-a schimbat nimic.
  - Rândurile arborelui, cărțile diagramei și dreptunghiurile flăcării sunt `Memo` pe
    `(ProfileId, lățime, generația deschiderilor, zoom)`.
  - Se desenează doar ce e în vedere.
  - Cât rulează o profilare se redesenează doar spinner-ul.
- **Windows:** nimic specific: căile exportului prin dialog, aceleași teste pe `windows-latest`,
  iar flag-ul de compilare SQLite (dacă se acceptă întrebarea 1) e verificat și în clippy-ul
  Windows din `check.sh`.

### Pașii (după 39h)

Fiecare pas e un commit, după `./scripts/check.sh`, cu capturile vechi identice în afara celor
numite.

- **39h — parametrii și planul (neschimbat).** Singura precizare: tab-ul „Plan” se face pe
  `ui_components::tree_table` (generic, intră aici), ca 39h4 să-l extindă, nu să-l refacă.
- **39h2 — profilul normalizat și driverele.**
  - **Ce intră:**
    - `database::profile` (modelul, `extra`, `Metric`, `derive`, `export` text / JSON,
      `ProfileBuilder`), `ProfileCaps` / `ProfileOptions`, `Session::profile` / `table_stats`
      cu implicite;
    - Postgres: `profile.rs`, opțiunile pe versiune, `block_size`, `READ ONLY` și
      `statement_timeout` local, `plan::parse` înlocuit;
    - SQLite: EQP normalizat, rularea măsurată, `get_status`, `SAVEPOINT`, `deadline`,
      `table_stats`;
    - protocolul v3 (`profile`, `table_stats`, `dto::Profile`), `toy`;
    - `Runner::profile` și `Profiles` (fără UI nou: Output scrie rezumatul și arborele, ca
      azi).
  - **Teste:**
    - **Fixtures Postgres**, scrise de mână după exemplele din documentație și din notele de
      versiune, nu de pe serverele utilizatorului: `crates/infra/database/postgres/tests/data/
      profile/{9.2-total-runtime, 9.6-parallel, 10-summary, 11-jit, 12-settings, 13-wal-update,
      13-incremental-sort, 13-hashagg-disk, 14-memoize-queryid, 16-temp-io, 17-memory-serialize,
      18-index-searches-disabled, sort-external, hash-batches, nested-loop-subplan, cte,
      unknown-keys}.json`. Fiecare are un `*.profile.txt` de aur (exportul text), rescris doar
      cu `EDITOR_UPDATE_GOLDEN=1`;
    - `unknown-keys` dovedește că nicio cheie nu se pierde;
    - opțiunile pe fiecare versiune 9.2–18; planul de 5 000 de noduri sub buget;
    - **SQLite pe fișiere temporare:** 50 000 de rânduri; un `SCAN`, un `SEARCH`, un index
      automat, un `TEMP B-TREE`; contoarele > 0 unde trebuie; o scriere profilată care lasă
      tabelul neschimbat (și în tranzacția utilizatorului); termenul care întrerupe;
      anularea; toate formele EQP, inclusiv `SCAN TABLE` dinainte de 3.36;
    - **domeniul:** `derive` (paralelism, `InitPlan`, CTE, `never executed`, estimat), forma
      MySQL / MariaDB construită complet, JSON-ul exportat (format, ordine stabilă);
    - **protocolul:** `profile` v3 dus și întors identic, un driver v2 cu profilul din
      `explain`, un format mai nou citit cu nota, `extra` din fiecare tip;
    - testul pe Postgres real, doar cu `EDITOR_TEST_POSTGRES_URL` (opțiunile acceptate de
      server, `pg_stat_statements` dacă există).
  - **Capturi:** niciuna nouă. Output-ul arată la fel.
- **39h3 — analiza.**
  - **Ce intră:** `profile::rules` (toate regulile din tabel), `cond`, `sql_map`,
    `sql::fingerprint`, `Facts` adunate de modul, `RecentRuns` din istoricul sesiunii,
    sugestiile SQL prin dialect, textele din locale (ambele limbi), Output-ul cu numărul de
    probleme.
  - **Teste:**
    - fiecare regulă: pozitiv, la limită, fals-pozitiv;
    - `cond` pe formele Postgres (`((status)::text = 'x'::text)`, `ANY ('{…}')`, `lower()`) și
      SQLite;
    - `sql_map` (unic, ambiguu, alias, după parametri puși);
    - `fingerprint`;
    - capăt la capăt, fixture JSON → constatările așteptate (în `db-postgres/tests`), SQLite
      temporar → `sqlite-scan` și `auto-index`;
    - bugetul.
  - **Capturi:** niciuna.
- **39h4 — vederea profilului.**
  - **Ce intră:**
    - `ui_components::{tree_table (extins), severity}` și `Palette::heat` în ambele teme;
    - vederea `profile::view`: bara, rezumatul, arborele cu căldura, calea fierbinte și
      problemele, detaliile cu `extra`, constatările cu „Copiază” / „Inserează în consolă” și
      evidențierea în consolă;
    - tab-ul „Profil” din „Services” (în locul „Plan”), „Deschide în tab” (`view-profile`);
    - comenzile și tastele, exportul text / JSON / Markdown, opțiunile din ⚙, setările;
    - iconițele noi.
  - **Teste:** vederea pe profile date (coloanele după `present`, calea fierbinte, F2 / ⇧F2,
    filtrul, planurile mari închise, lățimile responsive, „Inserează” care nu rulează, click pe
    constatare → nod și interval în consolă, textul schimbat → nicio evidențiere), contrastul,
    `Memo` (o așezare pe lățime).
  - **Capturi noi** (ambele teme, pe `profile::sample`, sub feature-ul `snapshot`: un profil
    Postgres construit de mână, cu paralelism, spill și trei constatări, plus unul SQLite real
    pe baza de probă): `services-profile`, `services-profile-narrow`, `profile-tab`,
    `profile-finding-selected` (consola cu intervalul evidențiat), `profile-sqlite`. Schimbate:
    `services-plan` (devine `services-profile`), `settings-caret` (blocul „Profilare”).
- **39h5 — diagrama și flacăra.**
  - **Ce intră:** `diagram::tidy`, `EdgeStyle::width`, `icicle`, `graph.rs`, `flame.rs`,
    selecția comună între vederi.
  - **Teste:** `tidy` (fără suprapuneri pe forme aleatoare, `proptest` cu sămânță fixă,
    buget), grosimile, `icicle` (proporțiile, scalarea în părinte, unirea sub 2 px, mărirea și
    înapoi), aceeași selecție în cele trei vederi.
  - **Capturi noi:** `services-profile-graph`, `services-profile-flame`. Diagrama relațiilor
    rămâne identică (grosimea implicită neschimbată).
- **39h6 — compararea, tabelele și statisticile pe instrucțiune.**
  - **Ce intră:** `profile::compare`, vederea comparării, `profile::tables` și vederea
    tabelelor, `table_stats` Postgres (`stats.rs`), `pg_stat_statements` (`history`, pastila
    din rezumat, `n-plus-one` și cu datele serverului), `stale-stats` / `dead-tuples` cu
    datele lor.
  - **Teste:** `diff` (același plan, un `SeqScan` → `Index Scan`, noduri adăugate / scoase,
    ordinea copiilor), tabelele (aliasuri, același tabel de două ori), interogările de
    statistici pe versiuni (fără server), SQLite `sqlite_stat1` prezent / lipsă.
  - **Capturi noi:** `services-profile-compare`, `services-profile-tables`.

### Ce nu facem acum

- Parserele MySQL / MariaDB: vin cu driverul lor (făcut în 39p, `db-mysql/src/profile.rs`).
- Așteptările și blocările urmărite în timp real (eșantioane din `pg_stat_activity` /
  `pg_locks` de pe o a doua conexiune, cât rulează interogarea). Câmpul `waits` există.
- Indexurile ipotetice („ce-ar fi dacă”, HypoPG), `pg_hint_plan` și planurile generice
  (`GENERIC_PLAN`, 16).
- Un tablou al celor mai scumpe interogări de pe server (top `pg_stat_statements`), o unealtă
  separată.
- Profilarea interogărilor venite din PHP (logul ORM, Xdebug) și potrivirea lor cu consola.
- Trimiterea planurilor la servicii externe și păstrarea automată a profilelor pe disc.
- Importul unui JSON exportat (doar exportul acum), sfaturile generate de un model AI, SQL
  Server și Oracle.

### Întrebări (hotărâte: recomandările acceptate)

1. **Rândurile reale pe nod la SQLite (`scanstatus`)?**
   - Ar cere SQLite-ul inclus compilat cu `SQLITE_ENABLE_STMT_SCANSTATUS` (prin
     `LIBSQLITE3_FLAGS` în `.cargo/config.toml`, deci pentru toate crate-urile cu `rusqlite`) și
     un `unsafe` mic în `db-sqlite`, cu `// SAFETY:`.
   - **Recomand da, în 39h2**, dacă binarul crește sub 100 KB și build-ul Windows trece. Fără
     el, SQLite are timpii și contoarele doar pe instrucțiune, iar arborele doar estimări:
     tocmai partea cea mai utilă lipsește.
   - Colectarea e oprită implicit (`SQLITE_DBCONFIG_STMT_SCANSTATUS`) și pornită doar pentru
     profilare, deci restul aplicației nu plătește nimic.
2. **O profilare rulată a unei citiri, pe producție, cere confirmare?**
   - **Recomand da, o dată pe consolă cât e deschis proiectul** („nu mai întreba pentru această
     consolă”). O scriere cere confirmare mereu.
   - O citire grea pe producție e un risc real (încărcarea serverului, `statement_timeout`
     ajută doar parțial), dar o întrebare la fiecare rulare ar face unealta incomodă exact unde
     e mai utilă.
3. **Cât de departe merg sugestiile?**
   - **Recomand:** doar „Copiază” și „Inserează în consolă” (textul adăugat la capăt, selectat,
     neexecutat), pe producție Postgres cu `CONCURRENTLY`. Fără „Aplică” cu un click.
   - Un index creat dintr-un click pe un tabel mare poate bloca scrierile minute în șir, iar
     costul lui (scrieri mai lente, spațiu) unealta nu-l vede. Decizia rămâne explicit a
     utilizatorului.

## Teste (strategia)

- **Domeniul** (pur, fără I/O):
  - lexer, împărțire, fel, parametri, pericole, scope, index, istoric, mașina tranzacției,
    plan;
  - multe cazuri de margine pe fiecare dialect și teste de proprietăți pe împărțire;
  - bugetele, cu limite generoase în debug.
- **Infra:**
  - SQLite pe fișiere temporare (`tempfile` sau scratchpad);
  - protocolul prin driverul `toy` servit în același proces și prin binarul de test
    `explorer/tests/protocol.rs`, extins cu consola;
  - Postgres real doar cu `EDITOR_TEST_POSTGRES_URL` (unul sau mai multe URL-uri, de
    exemplu un 9.6 și un 17), niciodată pornit de teste.
- **Modulele:**
  - `database-console` cu un `ConsoleAccess` fals și pe SQLite, cu pool-ul și ceasul
    manuale (ca 37b);
  - `services-panel` cu un furnizor fals;
  - `lang-sql` pe fișiere de exemplu din `tests/data`.
- **Aplicația:** bara peste editor, panoul de jos, tastele `EditorLanguage`, `DocumentAssist`
  în completare / definiție / hover, două proiecte despărțite (consolele, sesiunile,
  Services, istoricul).
- **Capturile:** cele numite la pași. Toate pe date făcute de captură, cu ceasul oprit,
  fără rețea. Cele vechi rămân identice în afara celor numite.
- **Unde stau testele:** în `tests/unit/` oglindind `src/`; integrarea între crate-uri în
  `tests/*.rs`.

## Riscuri

- **Gramatica e mare sau nu se potrivește cu ABI-ul:** criteriul măsurat în 39b, apoi
  regenerarea doar cu dialectele noastre. Rularea nu depinde de ea.
- **Fals-pozitive în diagnostice:** se raportează doar ce e sigur; erorile reale vin de la
  server, la locul lor.
- **Consola și grila pe același fișier SQLite:** blocările sunt explicate, iar `busy_timeout`
  și tranzacția deschisă se văd.
- **Rezultate uriașe:** limita de pagină, memoria limitată, cursoarele, anularea, celulele
  mari tăiate.
- **Scheme cu mii de tabele:** indexul pe două trepte, în bloc, cu buget, eliberat.
- **Ciocnirea tastelor (⌘↵):** `When::EditorLanguage`, testat față de grilă.
- **Protocolul extern:** v2 adăugat fără să rupă v1, cu versiunile negociate.

## Ce nu facem acum

- SQL-ul injectat în șirurile PHP (`$pdo->query("SELECT …")`, `DB::select`, Doctrine
  DBAL), cu completare din sursa asociată proiectului. Vine ca 39o, după 39k, prin
  `InjectionProvider` (`php.sql_strings`) și `DocumentAssist` pe stratul injectat. Doctrine
  DQL și Eloquent rămân separat.
- Driverele SQL Server și Oracle: un plan al lor. (MySQL / MariaDB: făcut în 39p.)
- Constructorul vizual de interogări, diagrama vizuală a planului (avem arborele) și
  „Execute to File”.
- Interogarea unui CSV prin SQLite în memorie.
- Lista de bază a funcțiilor Postgres fără conexiune, generată din imaginile Docker.
- Tipărirea jurnalului, Docker și configurațiile de rulare în „Services”.
- Widgetul de probleme din colțul editorului (numărul de erori, sus în dreapta). E generic,
  pentru toate fișierele, deci are planul lui; până atunci, bara de stare și marcajele de
  pe bara de derulare.
- Sesiuni împărțite între console și „single session mode”.
- Rezultatele sub editor, în tab (vezi întrebarea 4).
- Refactorizări SQL (redenumirea unei coloane în tot scriptul) și Find Usages pe aliasuri.

## Întrebări (hotărâte: recomandările acceptate)

1. **Unde stau consolele și istoricul?**
   - Recomand `<dosarul proiectului>/consoles/` și `…/database/history/`, local pe proiect,
     cum ai cerut: munca dintr-un proiect rămâne cu el, chiar pe o sursă globală.
   - Alternativa e un loc global (scratches), cu aceleași console în toate
     proiectele.
2. **Cum se știe că o sursă e de producție?**
   - Recomand un semn explicit „Producție” pe sursă sau pe folder, moștenit ca și culoarea.
   - Să nu-l deducem din culoarea roșie sau din numele `prod`: culoarea e decorativă, iar o
     regulă ghicită nu trebuie să decidă confirmările.
3. **Ce mod de tranzacție are implicit o sursă de producție?**
   - Recomand `Manual` pe producție (nimic nu e permanent fără Commit) și `Auto` în rest.
   - Utilizatorul îl poate schimba pe consolă.
4. **Rezultatele doar în „Services”, sau și sub editor?**
   - Recomand doar „Services” acum (ca în capturile tale): un singur loc, mai puțin cod.
   - Varianta „sub editor, în tab” vine ca setare după 39f, dacă o vrei.

## Stare

- **Aprobat**, cu cele patru recomandări de mai sus. Pașii se fac în ordine, câte un commit pe pas.
- **39a — făcut** (SQL-ul, pur, în `crates/domain/database/database/src/sql/`):
  - **`Flavor`** (`Generic`, `Sqlite`, `Postgres { version }`, `MySql`) și
    `Dialect::flavor` (implicit `Generic`; SQLite și Postgres dau pe al lor, Postgres cu
    versiunea serverului).
  - **`lex`** (`lex.rs`): un singur pas, tokenii sunt intervale de octeți (`Token { kind,
    start, end, unterminated }`), acoperă textul întreg, fără goluri și fără suprapuneri.
    Șirurile `'…'` (`''` dublat; backslash la MySql și în `E'…'` la Postgres), `X''`, `B''`,
    `N''`, `U&''`; identificatorii `"x"`, `` `x` `` (MySql, SQLite), `[x]` (SQLite), `U&"x"`;
    corpurile `$$` / `$tag$` (Postgres); comentariile `--` (la MySql doar urmat de spațiu), `#`
    (MySql), `/* */` imbricate la Postgres; parametrii `?`, `?N`, `$N`, `:name`, `@name`
    (nu la Postgres, unde `@` e operator), `$name` (SQLite); `::`, `:=`, operatorii JSON
    `?|` / `?&` la Postgres; `\r\n`, spațiile Unicode; ce e neînchis ține până la capăt,
    marcat, fără panică.
  - **`split`** (`split.rs`): `;` doar la nivelul de sus; corpul `BEGIN … END` al unui
    `CREATE TRIGGER` (SQLite, MySql, cu `CASE … END` înăuntru și `END IF` / `END LOOP` la
    MySql) și `BEGIN ATOMIC … END` (Postgres ≥ 14, sau versiune necunoscută) rămân întregi;
    `SplitOptions::line_separators` pentru liniile `GO` / `/`; instrucțiunile goale sau
    doar comentarii sunt sărite. `StatementSpan` are `range` (fără margini), `with_comments`
    (comentariile de pe rândurile dinainte; cel de pe rândul `;`-ului anterior rămâne al
    aceleia), `terminator`, `tokens`, `first_word` și `incomplete`. `parse` dă tokenii și
    instrucțiunile o dată (`Parsed`).
  - **`statement_at`**: cea care conține cursorul (cu comentariile și `;`-ul ei; la
    atingere, următoarea), altfel cea de pe același rând sau de pe rândul imediat următor,
    altfel niciuna.
  - **`classify`** (`classify.rs`): `Query`, `Write(WriteKind)`, `Ddl(DdlKind)`,
    `Transaction(TxControl)`, `Explain { analyze, writes }`, `Session`, `Other`, cu `writes()`,
    `touches_schema()` și `is_query()`. Un `WITH` care scrie într-o parte e scriere; `INSERT …
    ON CONFLICT DO UPDATE` / `INSERT OR REPLACE` / `REPLACE` / `ON DUPLICATE KEY UPDATE` sunt
    `Upsert`; `SELECT … INTO t` (Postgres) e `Create`; `PRAGMA` care citește față de cel care
    setează; `EXPLAIN (ANALYZE false)` nu rulează.
  - **`params` / `bind`** (`params.rs`): `ParamStyles` (pe flavor: Postgres fără `?`, care e
    operatorul `jsonb`, MySql fără `@name`, variabila utilizatorului; `ParamStyles::all`),
    `ParamName::{Positional, Numbered, Named}` (același nume de două ori e un parametru;
    `:id` și `@id` sunt același), `ParamValue::{Literal, Raw}`, `BindError::{Missing,
    Refused}` și `Bound { text, map }`. `BindMap::to_document` întoarce o poziție din textul
    trimis la document (în valoare: începutul parametrului).
  - **`mask_secrets`** (`secrets.rs`): literalul de după `PASSWORD` (cu `=`, `(`,
    `ENCRYPTED`, opțiunea `password` a unui server străin) și `IDENTIFIED BY` / `IDENTIFIED
    WITH … BY | AS` devine `'••••'`; un text fără parole rămâne împrumutat (`Cow`).
  - **`errors::locate`** (`errors.rs`): `EnginePosition::{Character, Byte}` (Postgres: caractere
    de la 1; SQLite: octeți de la 0) → `TextPosition { offset, line, column }` în document,
    prin `BindMap`; dincolo de capăt („at end of input”) e capătul instrucțiunii.
  - **`danger::statement_dangers`**: `UnfilteredWrite` (fără `WHERE` la nivelul lui, sau cu
    unul mereu adevărat: `true`, `1`, `1 = 1`, `'a' = 'a'`, `NOT FALSE`, sau oricare din ele
    într-un `OR`), `DropObject { kind: DropKind, name, cascade }` (câte unul pe nume),
    `Truncate`, `AlterDrop`. Se uită și în părțile unui `WITH` și sub `EXPLAIN ANALYZE` (care
    rulează), nu în corpul unui trigger creat și nici sub un `EXPLAIN` simplu. Pericolele
    vechi sunt neschimbate; `database-explorer` le spune pe cele noi în cuvinte
    (`schema.lose_*`, `schema.object_*`, în ambele limbi).
  - **Teste:** în domeniu, pe fiecare formă și dialect (inclusiv fals-pozitivele: `WHERE`
    în subinterogare, `DELETE … USING … WHERE`, `FOR UPDATE`, `ON CONFLICT DO UPDATE`, `ON
    DELETE CASCADE`); teste de proprietăți deterministe (`proptest`, sămânță fixă, 2 000 de
    cazuri): tokenii acoperă textul fără suprapuneri, instrucțiunile și ce e între ele dau
    textul înapoi, o instrucțiune citită singură e una, nimic nu panichează pe orice text.
    Bugetul: 1 MB lexat și împărțit în ~4 ms în release (limita testului 30 ms; 3 s în debug).
  - **Ce n-a intrat și de ce:**
    - setarea pentru liniile `GO` / `/` și cea a felurilor de parametri: setările sunt ale
      consolei (39e / 39h); domeniul are deja opțiunile (`SplitOptions`, `ParamStyles`);
    - `scope` și `format` sunt ale pașilor 39j și 39m, cum spune planul;
    - MySql e doar în lexer și împărțire (instrucțiunile compuse fără `DELIMITER`), fără un
      driver care să le folosească încă;
    - nicio captură nouă și niciuna schimbată (39a nu atinge UI-ul).
- **39b — făcut** (modulul `lang-sql`, `crates/modules/languages/sql`, id `lang.sql`):
  - **Gramatica**: `DerekStride/tree-sitter-sql` 0.3.11 (publicată ca `tree-sitter-sequel`,
    commit `7b51ecd`), licența MIT verificată, vendorizată în `third_party/tree-sitter-sql/`
    (`LICENSE` luat din commit — crate-ul nu-l are —, `grammar.js`, `src/`, `queries/`,
    `bindings/rust/`, `Cargo.toml` propriu cu `publish = false`, README cu versiunea și
    commit-ul). `parser.c` (17 MB) regenerat din `grammar.js` neschimbat cu `tree-sitter
    generate --abi 15` (CLI 0.27.1), pentru `tree-sitter` 0.27 din workspace.
  - **Criteriul de acceptare, măsurat**: binarul release (`lumen`, thin LTO) 52 275 632 →
    54 834 448 octeți, **+2,44 MiB** (pragul: 4 MB; tabelele gramaticii sunt ~2,4 MB în
    `__const`). Build curat: crate-ul gramaticii compilat de la zero în **2,6 s** (release) /
    **1,6 s** (debug), în paralel cu restul (pragul: +20 s). Nicio regenerare fără dialecte
    n-a fost nevoie. `[profile.dev.package."*"]` (opt-level 2) acoperă deja gramatica, ca pe
    celelalte: nicio intrare separată.
  - **`LanguageDescriptor`**: `sql`, „SQL”, extensiile `.sql`, `.ddl`, `.pgsql`, `.psql`
    (`x.sqlite.sql` e un `.sql`), `--` și `/* */`, ghilimelele `'` `"` `` ` ``, `$` în cuvinte,
    iconița `database`.
  - **`Grammar`**: `highlights.scm` al nostru (pe scope-urile temelor: `keyword`,
    `keyword.type` pentru tipuri, `keyword.operator`, `function.call`, `type` pentru tabele,
    `property` pentru coloane, `variable` pentru aliasuri și CTE-uri, `variable.quoted` pentru
    `"Nume"` — gramatica îl citește ca literal —, `string`, `number`, `boolean`, `null`,
    `constant.parameter`, `comment`, operatori, punctuație, `INTERVAL '…'`); `folds.scm`
    (instrucțiuni pe mai multe rânduri, subinterogări, CTE-uri, `CASE`, coloanele unui tabel,
    liste, argumente, corpul funcției, `BEGIN … END`, comentarii bloc); `indents.scm`
    (parantezele, `CASE`, blocurile, corpul). `embedded_errors: false`, `injected_errors:
    false` și câmpul nou **`Grammar::own_errors`** (`syntax-ts`): erorile gramaticii nu se arată
    nici în fișierele `.sql` (rădăcina raporta mereu); toate celelalte gramatici îl au `true`.
  - **PHP**: `<<<SQL` se colorează singur (injecția existentă); `normalize_language` duce și
    `PGSQL`, `PSQL`, `POSTGRES(QL)`, `PLPGSQL`, `MYSQL`, `SQLITE` la `sql`. ⌘/ în heredoc
    scrie `--` (limbajul de la cursor).
  - **Cuvintele pe dialect** (`dialect.rs`, liste ale noastre din documentația motoarelor):
    `Dialect::{Standard, Sqlite, Postgres, MySql}`, ales după numele fișierului
    (`.pgsql`/`.psql`, `x.pg.sql`, `x.sqlite.sql`, `x.mysql.sql`…; altfel standardul).
    `LanguageIntelligence` primește **`keywords_for(path)`** (implicit `keywords()`), folosit
    de completare. `KeywordInsert`: funcțiile ca apel (`COUNT()` cu cursorul între paranteze,
    `CAST()`), valorile exact (`NULL`, `TRUE`, `CURRENT_TIMESTAMP`), un cuvânt cheie care e și
    funcție SQLite (`REPLACE`, `LIKE`) rămâne cuvânt cheie. `completion_triggers` `.` `"`
    `` ` ``, `word_chars` `$`, `naming` fără majuscule. Contextul se citește din rândul curent
    (și un `/*` neînchis dinainte), deci merge și într-un heredoc: nimic în comentarii, șiruri,
    nume citate, după `.` (coloanele: 39j), pe parametri și numere.
  - **`cargo xtask sql-data`**: citește `pragma_function_list` din SQLite-ul inclus în
    `rusqlite` (aceeași dependență și aceleași feature-uri ca aplicația; 3.53.2, 125 de
    funcții) și scrie `data/sqlite.functions` (`NUME<TAB>feluri<TAB>arități`, cu majuscule ca
    restul cuvintelor). Citit leneș (`OnDemand`, `release_idle`, `memory`). Adăugat în
    `scripts/build.sh stubs` și în workflow-ul lunar `language-data.yml`.
  - **Teste**: `lang-sql` pe fișierele de exemplu `tests/data/sqlite.sql` și `postgres.sql`
    (colorarea fiecărui fel, corpul `$$` colorat ca SQL, numele citat, nicio eroare raportată,
    plierea, indentarea, comentarea liniilor, detectarea fișierelor), dialectele și listele
    (sortate, fără dubluri), datele (citite întregi, linii stricate sărite), completarea pe
    dialecte și inserarea; aplicația (`php_heredoc_sql_lights_up`: `<<<SQL` și `<<<PGSQL` în
    PHP sunt `sql`, colorate, fără erori, `--`); `syntax-ts` (etichetele heredoc); xtask
    (formatul și SQLite-ul inclus).
  - **Capturi noi**: `sql-editor` și `php-heredoc-sql` (ambele teme; fișierele stau lângă
    proiect, nu în el, ca arborele, căutarea și depozitul git al capturilor să rămână aceleași).
    Schimbate, cum era de așteptat: `about` și `settings-8` (59 de module în loc de 58). Toate
    celelalte identice.
  - **Ce n-a intrat și de ce**:
    - injecția `$$`: gramatica citește deja corpul `$$ … $$` (și `DO`) ca SQL, în același
      arbore, deci e colorat; o injecție `sql` în `sql` e oricum refuzată de motor (aceeași
      limbă). Un corp scris ca șir (`AS 'SELECT 1'`) rămâne șir;
    - `:name` nu e un nod `parameter` în gramatică (`?`, `?1`, `$1` sunt): se colorează ca o
      coloană; nicio eroare nu se arată;
    - perechile de paranteze nu au un câmp în `Grammar`: le face editorul, generic;
    - funcțiile Postgres fără conexiune (vezi „Ce nu facem acum”), deci Postgres și MySQL au
      doar funcțiile standardului.
- **39c — făcut** (contractul driverului pentru consolă):
  - **Domeniul** (`database`): `Capabilities::console: Option<ConsoleCaps>` (`cursors`,
    `explain: ExplainKinds`, `isolation: Isolations`, `read_only`, `switch_schema:
    SchemaSwitch::{None, Statement, Reconnect, NamesOnly}`, `databases`, `error_positions`,
    `notices`, `limited`; `ConsoleCaps::BASIC` și `LIMITED`); `Isolation` (cele trei niveluri
    Postgres și cele trei feluri de `BEGIN` SQLite, cu `id` / `sql`). Modulul nou `run`:
    `RunOptions { fetch_limit, keep_cursor }` (implicit 500), `RunOutcome { affected, rows,
    truncated, more: Option<CursorToken>, stopped, notices }`, `Notice`, `TxOptions`,
    `Place`, `ColumnBrief`, `TableColumns`, `Routine` / `RoutineKind`, sink-ul `Limited`
    (`Overflow::{Stop, Drop}`: o interogare se oprește un rând după pagină, o scriere merge
    până la capăt cu rândurile în plus aruncate), implicitele `*_by_execute`,
    `begin_statement` și `transaction_after` (urmărește `BEGIN` / `COMMIT` / `ROLLBACK` în
    textul rulat, nu în corpuri `$$`). Modulul nou `plan`: `PlanNode` (operația, obiectul,
    detaliul, costurile, rândurile estimate / reale, timpul, buclele, copiii) și
    `from_query_plan` (rândurile SQLite `id`/`parent`/`detail` ca arbore, nimic pierdut la
    părinți lipsă sau cicluri). `Session` primește `console`, `run`, `fetch`, `close_cursor`,
    `explain`, `begin` / `commit` / `rollback`, `in_transaction`, `set_schema`, `current`,
    `databases`, `catalog_columns`, `routines`, toate cu implicite (driverele vechi compilează
    neschimbate). `DbError::Sql(Box<SqlError { message, sqlstate, position, detail, hint }>)`,
    afișat exact ca `Query` („mesaj (detaliu)”), deci `grid_source::message` le arată la fel.
  - **SQLite**: `CONSOLE` (fără cursoare, planul fără `ANALYZE`, `DEFERRED` / `IMMEDIATE` /
    `EXCLUSIVE`, `NamesOnly`, pozițiile erorilor); `run` cu pagina; erorile de sintaxă
    (`SqlInputError`) cu offset-ul în octeți; `EXPLAIN QUERY PLAN` ca arbore; tranzacția
    deschisă = conexiunea nu e în autocommit (și un `BEGIN` scris de mână); `set_schema` /
    `current` (`main`) / `databases` (fișierul principal și cele atașate);
    `catalog_columns` într-o interogare pe schemă (`pragma_table_list` ⋈
    `pragma_table_info`; o schemă unde ea cade — un view stricat — e citită tabel cu tabel,
    cele care cad lăsate afară); `routines` din `pragma_function_list` (o dată pe nume și
    aritate). Anularea rămâne prin `progress_handler` (oprește cu `SQLITE_INTERRUPT`, ca
    `sqlite3_interrupt`; `Cancel` e un semn, fără mâner de chemat).
  - **PostgreSQL** (`console.rs`, `plan.rs`, `catalog.rs`): cursoarele sunt cursoare reale ale
    serverului, `DECLARE console_cursor_N NO SCROLL CURSOR FOR …` + `FETCH FORWARD`, cu un
    rând în plus ținut ca să se știe că mai sunt; în tranzacția utilizatorului sau, în afara
    ei, într-una `BEGIN READ ONLY` doar a cursorului, închisă (CLOSE + COMMIT) la
    instrucțiunea următoare, la `close_cursor` sau la capătul rezultatului. Doar pentru o
    interogare pe care `DECLARE` o ia (`SELECT`, `WITH … SELECT`, `VALUES`, `TABLE`), fără
    `FOR UPDATE / SHARE`; una refuzată de tranzacția doar citire (25006, înainte de orice
    rând) rulează simplu. Un `Flow::Stop` în timpul unui `FETCH` nu mai anulează (ar fi
    abandonat tranzacția): pagina se citește până la capăt. Fără cursor, o scriere peste
    pagină merge până la capăt. Notices: conexiunea e condusă prin `poll_message`, ultimele
    1 000 ținute și date cu rezultatul. Erorile: `SqlError` cu `position`
    (`ErrorPosition::Original`; una dintr-o interogare internă nu e un loc), codul, `detail`,
    `hint`; locul unei erori din `DECLARE` / `EXPLAIN` e întors la textul utilizatorului (fără
    prefix, cu comentariile dinainte numărate). `explain`: `EXPLAIN (FORMAT JSON)` / `(ANALYZE,
    BUFFERS, FORMAT JSON)` citit ca arbore (`Node Type`, `Join Type`, relația cu schema sau
    aliasul, indexul, condițiile, costurile, timpii); un `ANALYZE` peste o scriere e anulat de
    driver însuși (`BEGIN … ROLLBACK`, sau `SAVEPOINT` / `ROLLBACK TO` în tranzacția
    utilizatorului). `begin` cu izolarea și `READ ONLY` (`START TRANSACTION ISOLATION LEVEL …,
    READ ONLY`); `set_schema` = `SET search_path TO "x", public`; `current_database()` /
    `current_schema()`; `pg_database` fără șabloane; `catalog_columns` dintr-o interogare
    `pg_attribute` ⋈ `pg_class` ⋈ `pg_namespace` (+ `pg_attrdef`, `col_description`, locul în
    cheia primară prin `generate_series`, fără `array_position` / `LATERAL`), pe versiuni:
    `attgenerated` de la 12, `indnkeyatts` de la 11, partiționatele de la 10, view-urile
    materializate de la 9.3; `routines` din `pg_proc` cu `pg_get_function_arguments` /
    `pg_get_function_result`, `prokind` de la 11 și `proisagg` / `proiswindow` sub 11,
    variadicele fără aritate. O sursă doar citire primește la conectare `SET SESSION
    CHARACTERISTICS AS TRANSACTION READ ONLY` (orice sesiune a ei, nu doar a consolei).
  - **Protocolul v2** (`db-protocol`): `VERSIONS` 1–2; metodele `run`, `fetch`,
    `close_cursor`, `explain`, `begin`, `commit`, `rollback`, `set_schema`, `current`,
    `databases`, `catalog_columns`, `routines` (refuzate într-o sesiune de versiunea 1);
    notificarea `$/rows` (`columns` la începutul unui set, rândurile în bucăți de cel mult
    1 000 de rânduri sau ~1 MiB, înaintea răspunsului), primită de cititorul clientului și dusă
    cererii ei (`Connection::call_streaming`; timpul se numără de la ultimul mesaj; un sink
    care are destule trimite `cancel`, iar `cancelled` nu mai e eroare); erorile cu `data:
    { sqlstate, position | offset, detail, hint }` (la versiunea 1 fără `data`, cu detaliul în
    mesaj); `dto::Capabilities::console`, `RunOutcome::in_transaction`. `ProtocolSession`
    ține versiunea aleasă: la 2 folosește metodele noi, la 1 implicitele domeniului
    (`execute`, 100 000 de rânduri, fără flux și fără poziții) și `ConsoleCaps::LIMITED`
    (și `ProtocolDriver::capabilities` îl spune după manifest, fără să pornească nimic).
    `server` servește metodele noi prin `Session`, rândurile pe măsură ce vin. Driverul `toy`:
    `SELECT * | a, b FROM t` (tabel / coloană necunoscută cu locul și SQLSTATE), `SERIES n`,
    `WAIT`, `NOTICE`, cursoare, plan, tranzacții, o schemă. Documentat în
    `03-sistem-module.md` §6 (6.3.1, versiunile, exemplul).
  - **Teste:** domeniul (rularea implicită cu limita și „mai sunt”, oprirea față de tăiere,
    seturile cu limita lor, tranzacțiile prin cuvintele dialectului, `begin_statement`,
    catalogul implicit, `transaction_after`, izolările, `SqlError` afișat ca `Query`, planul
    SQLite ca arbore); SQLite pe fișiere temporare (pagina și „mai sunt”, o scriere peste
    pagină dusă la capăt, anularea, locul erorii, tranzacțiile pe feluri și un `BEGIN` scris,
    o tranzacție a consolei care face grila să primească `InUse`, `EXPLAIN QUERY PLAN`,
    schemele atașate, `catalog_columns` pe 2 000 de tabele, un view stricat, funcțiile);
    Postgres fără server (instrucțiunile cursorului, ce poate citi un cursor pe fiecare versiune
    9.2–18, locul erorii întors, `sql_error`, `EXPLAIN` JSON dintr-un fișier salvat
    `tests/data/explain-analyze.json`, interogările catalogului pe versiuni); protocolul
    (rândurile în flux identice cu cele directe, peste mai multe bucăți, cursorul citit până
    la capăt și închis, anularea în mijlocul fluxului și sesiunea care merge mai departe, un
    sink sătul, eroarea cu locul și codul, tranzacțiile, notices, planul, schema, catalogul,
    un driver v1 cu consola limitată, metodele v2 refuzate la v1, `data` erorilor pe ambele
    versiuni); `explorer/tests/protocol.rs` extins (toy-ul și SQLite servite ca programe:
    fluxul identic, cursorul, anularea, eroarea, un driver v1 instalat, consola SQLite prin
    protocol ca direct). Testul pe Postgres real (`the_console_on_real_servers`: cursorul,
    `search_path`, `pg_proc`, `position`, `RAISE NOTICE`, `ANALYZE` anulat, cancel, sesiunea
    doar citire) rulează doar cu `EDITOR_TEST_POSTGRES_URL` și n-a fost rulat (niciun server).
  - **Abateri de la plan, cu motivul:**
    - cursorul Postgres e `DECLARE` + `FETCH`, nu `bind` + `query_portal`: portalul lui
      `tokio-postgres` trăiește într-o `Transaction<'_>` împrumutată care nu poate ține între
      cereri; un cursor declarat e același portal al serverului. Un singur cursor deschis pe
      sesiune, închis la instrucțiunea următoare (și în `Manual`);
    - `ConsoleCaps::isolation` e un set `Isolations` (copiabil) în loc de
      `&'static [Isolation]`: driverele externe își spun nivelurile la rulare. Au intrat în
      plus `read_only`, `notices`, `limited` și `SchemaSwitch::NamesOnly` (SQLite);
    - `RunOutcome` are și `truncated` / `stopped` (rânduri rămase fără cursor);
      `Session::console()` dă capabilitățile pe sesiune (versiunea serverului, versiunea
      protocolului); `explain`, `catalog_columns` și `routines` primesc și `Cancel`;
    - SQLite dă locul doar pentru erorile pe care le plasează el (sintaxa); „no such table”
      vine fără loc. Tranzacțiile doar citire nu există în SQLite (refuzate cu motivul);
    - notices-urile unei rulări care cade se pierd (o eroare n-are unde să le ducă).
  - **Ce n-a intrat și de ce:** textul „driver vechi: rezultate limitate” e al barei
    consolei (39e), care citește `ConsoleCaps::limited`; nicio captură nouă și niciuna
    schimbată (39c nu atinge UI-ul).
- **39d — făcut** („Services”, generic):
  - **`ui_components::services`**: `ServiceId` (UUID v4), `ServiceNode` (id, părinte, iconiță,
    nume, detaliu, `ServiceState::{Idle, Running, Error, TxOpen}`, culoarea ca nume de
    `tag_colors`), `ServiceTab` (id, titlu, iconiță, fixat, `closable`: Output-ul unei sesiuni
    nu se închide), `ServicesContext` (proiectul, bus-ul lui, `wake`), `ServiceProvider` (id,
    ordine, iconiță, titlu, `open`) și `ProjectServices` (`nodes`, `generation`, `poll`,
    `tabs`, `open_tab`, `close_tab`, `pin_tab`, `activate`, `menu`, `command`, `take_reveal`,
    `purge`). Registrul (`register` / `unregister` / `providers`, unul pe id, după ordine) e
    umplut de aplicație din contribuții, ca `database::drivers`.
  - **`ui_components::log_view`**: `Log` (împărțit între cine scrie, de pe orice fir, și
    vederi; limita implicită 10 000 de rânduri, cele vechi ies; `generation`, `memory`,
    `text`), `LogLine` (ora ca text, `LogKind::{Info, Statement, Result, Warning, Error,
    Notice}`, prefixul `bază.schemă>`, textul colorat ca cod după extensie, legătura ca
    `ViewerAction`) și `LogView` (`Viewer`): butoanele laterale (încadrare, ține capătul,
    copiere, golire, cu indicii) și meniul lor; fontul monospațiat, deci încadrarea e o
    numărătoare de caractere (rupt după un spațiu), niciodată o măsurare; rândurile se așază
    doar când se schimbă liniile (cele noi doar adăugate la capăt), lățimea sau încadrarea; pe
    cadru se desenează doar rândurile în vedere. Capătul e ținut cât vin rânduri și nu mai e
    după o derulare în sus (din nou la capăt: din nou ținut). Click pe textul unei erori îi
    urmează legătura (și ↵ pe rândul ales); selecția pe rânduri (click, tragere, ↑ ↓, ⌘A),
    copiată ca `[ora] prefix text`. `code_tokens::runs` dă bucățile colorate pentru un rând
    încadrat. `lang-sql` dă acum și `code_style` (cuvintele standardului, `--`), ca SQL-ul să fie
    colorat și în afara editorului (jurnalul, diff-urile).
  - **Modulul `services-panel`** (`crates/modules/tools/services`, id `services`): un
    `ToolWindowProvider` (`services`, iconița nouă `services`, „Services”, `Dock::Bottom`,
    ordinea 5: după terminal, înaintea Git), `view.toggle_services` (⌘8 / Alt+8, și în
    presetarea JetBrains), `services.expand_all` / `services.collapse_all` (antetul și meniul
    butonului, un `MenuContribution` cu același id), `ToolContent::Panel` făcut la prima
    afișare. Panoul: arborele (filtrul, rândurile ferestrei „Database”: chevroane, ghidaje,
    semnul culorii, tastele ↑ ↓ ← → Home End PgUp PgDn ↵ Esc, tastarea pe rânduri filtrează;
    un nod e arătat când el sau un copil se potrivește, cu părinții, totul deschis cât se
    filtrează; un părinte necunoscut: sus; un ciclu nu se repetă), starea la capătul rândului
    (spinner, semnul erorii, insigna „TX”, cu indicii), despărțitorul (lățimea arborelui ținută
    în sesiunea proiectului), tab-urile nodului ales (fixat cu pin în loc de ×, meniul tab-ului:
    Pin / Close / Close Others) și vederea tab-ului activ. Contributorii sunt întrebați abia la
    prima desenare, o dată pe proiect; alt proiect: ce dăduseră se aruncă. Vederea unui tab se
    face la prima afișare și se eliberează când nodul nu mai e cel arătat (contributorul își
    păstrează datele). Mesajele și notificările vederilor merg la aplicație (`ToolPanelAction::
    View`, toast-uri). `Reveal` (prin `tell_panel`) și `ProjectServices::take_reveal` arată un
    nod și un tab. Gol: iconița ferestrei și „Niciun serviciu încă” cu de unde vin.
  - **Aplicația** învață un panou și jos: `ToolContent::Panel` cu `Dock::Bottom` e un
    `SidePanel` ca oricare (aceleași intrări, taste, meniuri, indicii, cursoare), așezat în zona
    de jos, unul o dată cu ferestrele de vederi (`set_bottom` pune deoparte panoul, arătarea
    panoului ascunde fereastra), butonul aprins cât se vede, antetul în stilul ferestrelor de jos
    (`PanelHeader::set_bottom`: 34 px, iconița în accent, linia de dedesubt) cu butonul „ascunde”
    al aplicației la capăt, cardul cu fundalul terminalului, despărțitorul de sus al zonei.
    `ToolPanel` primește `scroll_by` (roata în ambele direcții), `memento` / `PanelMemento` și
    `take_notifications`; sesiunea proiectului ține `panels` (`[panels]` în `session.toml`),
    date fiecărui panou când e făcut, iar un panou de jos arătat revine la pornire. Terminalul și
    celelalte ferestre de jos sunt neschimbate.
  - **Teste**: `ui-components` (registrul, identitățile; jurnalul: limita, încadrarea la lățime,
    capătul ținut și apoi nu, legăturile doar pe text, selecția copiată, butoanele, golirea),
    `services-panel` cu furnizori falși (nimic cerut înainte de afișare și o dată pe proiect,
    arborele și tab-urile, vederile făcute la afișare și eliberate, fixarea și închiderea cu
    Output-ul păstrat, tastele și filtrul, două proiecte care nu își văd nodurile, lățimea ținută,
    `Reveal` și comenzile butonului; rândurile pe forme de arbore, cicluri), aplicația (zona de
    jos împărțită cu terminalul, butonul aprins, antetul cu „ascunde” și indiciul lui, tastele
    ⌘8, panoul refăcut din sesiune cu lățimea lui, alt proiect fără nimic rămas), `storage`
    (`panels` dus și întors).
  - **Capturi noi**: `services-empty` și `services-fake` (ambele teme; un contributor de probă,
    `services_panel::sample`, doar sub feature-ul `snapshot` și în teste: două surse, consolele
    lor cu o tranzacție deschisă și o eroare, tab-urile Output / un rezultat fixat / altul,
    jurnalul cu ceasul oprit). Schimbate, cum era de așteptat: toate celelalte capturi diferă
    doar prin butonul nou din banda din stânga jos (iconița „Services”, între terminal și Git:
    o fereastră înregistrată își are butonul peste tot); `about` și `settings-8` numără în plus
    un modul (60).
  - **Abateri de la plan, cu motivul:**
    - capturile vechi nu rămân identice în afara `about`: butonul ferestrei noi e în banda
      fiecărui ecran (verificat: nicio altă diferență în afara benzii, a `about` și a
      `settings-8`);
    - contribuția bazei de date nu intră aici: e a modulului `database-console` (39f), care se
      face abia în 39e; până atunci fereastra arată starea goală. Contractul are deja ce-i
      trebuie (starea `TxOpen`, culoarea sursei, detaliul cu durata, tab-urile fixate,
      `take_reveal`);
    - `services-panel` depinde, pe lângă cele din plan, de `extensions` și `command-model`
      (modulul și comenzile lui), `ui-kit`, `render-api` și `text-layout` (desenul); nimic de
      infra, `check-deps` neschimbat;
    - în contract au intrat în plus `ServiceTab::closable`, `pin_tab`, `activate`,
      `take_reveal` și `purge`;
    - selecția din jurnal e pe rânduri întregi, nu pe caractere (ce se copiază dintr-un jurnal).
  - **Ce n-a intrat și de ce:** tipărirea jurnalului (vezi „Ce nu facem acum”); Docker și
    configurațiile de rulare (n-au modul).
- **39e — făcut** (consola în editor, rularea simplă):
  - **Domeniul** (`database::console`): `ConsoleId` (UUID v4), `TxMode` / `RunMode` (cu id-uri),
    `ConsoleSpec` (sursa, numele, fișierul relativ la dosarul proiectului, baza / schema alese,
    tranzacția, izolarea, modul, ultima durată, numele parametrilor), locul fișierelor
    (`folder`, `relative_file`, `identify`: `consoles/<source-uuid>/<nume>.sql`, `next_name`:
    `console`, `console_1`…), portul `ConsoleStore` (cu `NoConsoles`), `SourceBrief` (nume,
    driver, iconiță, culoare, producție — mereu `false` până la 39g —, doar citire, dialect,
    `flavor`, baza implicită, capabilitățile), `Overrides`, `Connector` și portul
    **`ConsoleAccess`** (`sources`, `source`, `connector`, `color`, `production`, `open_table`,
    `notes`, `selected`, `idle_close`) cu registrul pe proiect (`provide` / `access`), cererea
    „Jump” (`JumpRequest`, `ask_jump` / `take_jump`, `JUMP_COMMAND`, `NEW_COMMAND`),
    `table_query` (`SELECT * FROM <tabel>;` cu numele de selectat), **`plan`** (`PlanInput` →
    `RunPlan::{Run, Choose, Nothing}`: selecția în ambele moduri; în Playground instrucțiunea de la
    cursor după `statement_at`, altfel alegerea; în Script tot), `refused` (prima scriere pe o
    sursă doar citire) și `summary`.
  - **`storage::TomlConsoles`**: `<dosarul proiectului>/consoles/consoles.toml`, scris atomic,
    căile cu `/` pe orice sistem; un fișier stricat e o eroare și nu se scrie peste el.
  - **`command_model::When::EditorLanguage(id)`**: o parte din `Editor` (`overlaps`), căutată
    înaintea lui (`Workbench::key_places`: `[EditorLanguage("sql"), Editor]` într-un fișier SQL).
    ⌘↵ e `database.console.run` acolo și rămâne `database.submit` în grilă (`When::View`).
  - **`ui_components::editor_toolbar`**: `EditorToolbarProvider` (`open(ToolbarContext)`: o
    privire la cale, nimic pornit) și `EditorToolbar` (înălțimea, desenul, apăsarea, hover-ul,
    cursorul, indiciile, `poll`, `is_loading`, titlul, iconița, `accent`, `crumbs`,
    `may_close`, `command`, `take_action`, notificările, `follows_editor` / `sync_editor`,
    `marks` cu generația, `highlight`, `purge`), `ToolbarAction::{View, Menu}`, `EditorState`,
    `StatementMark` (intervalul și `text_hash`-ul textului rulat: semnul se arată doar cât textul
    e același) și `MarkState`.
  - **Aplicația** găzduiește bara (`workbench/editor_toolbars.rs`): cerută la deschiderea unui
    fișier text, așezată sub breadcrumb, în locul spațiului de deasupra editorului (textul sub ea,
    la aceeași lățime și același x ca în orice tab; bara de căutare sub ea), desenată cu dunga
    culorii sursei sus și punctul pe iconița tab-ului (ca `Viewer::accent`), cu clicurile, hover-ul,
    cursorul, indiciile, meniurile (`bar-menu:` + id, sub buton) și `poll`-ul ei; titlul tab-ului,
    iconița (a driverului; cea a bazei e chiar iconița fișierului `.sql`) și începutul
    breadcrumb-ului vin din ea; `may_close` e întrebat la închiderea unui tab. Semnele
    instrucțiunilor apar în gutter (un punct: inelul accentului cât rulează, verde gata, roșu
    eșuat; `Decorations::statements`), iar instrucțiunea pe care ar rula-o ▶ e evidențiată cât stă
    mouse-ul pe ▶ (`Decorations::highlight`). `WorkbenchHost`: `ActiveFile::cursor`,
    `EditorHost::select_active` / `open_file`, `workbench_model::CLOSE_TAB`.
  - **Modulul `database-console`** (`crates/modules/database/console`, id `database.console`):
    - `Consoles` pe proiect (`ConsolesService`): lista citită la prima consolă, scrisă în fundal
      (ultima schimbare scrisă ultima), fișierele consolelor urmărite pe disc (unul șters dispare,
      unul pus de mână apare), crearea (`console`, `console_1`…), runner-ele; la închiderea
      proiectului fiecare conexiune se închide, o tranzacție deschisă e anulată cu un toast.
    - `Runner`: conexiunea proprie a consolei prin `ConsoleAccess::connector`, făcută la prima
      rulare (nimic înainte), pe pool-ul `db-console`, o lucrare o dată: instrucțiunile pe rând
      (`Session::run` cu `console_fetch_limit`), oprirea la prima eroare (`console_stop_on_error`),
      Stop (`Cancel`), `Manual` (primul lucru care scrie deschide tranzacția), Commit / Rollback,
      un `BEGIN` scris de mână urmat, schema aleasă pusă o dată (`set_schema`), Explain (planul
      scris în Output), lista bazelor și schemelor; Output-ul (`ui_components::Log`: conectat,
      fiecare instrucțiune cu `bază.schemă>` și parolele mascate, rândurile cu timpii despărțiți în
      execuție și aducere, notices, erorile cu linia și coloana din document), istoricul din sesiune.
      Conexiunea e a barelor (`Lease`): ultima închisă, conexiunea se închide în fundal (ce rulează
      anulat, o tranzacție lăsată deschisă anulată de motor); neutilizată cât `[database]
      idle_close` (al explorer-ului, prin port), se închide — niciodată cu o tranzacție deschisă —,
      urmărită de un singur fir (`db-console-idle`).
    - **Bara** (`bar::ConsoleBar`, ca în captura de referință): ▶ verde și ▾ (Execute ⌘↵, Execute
      Script ⇧⌘↵, Explain Plan / Explain Analyze după `ConsoleCaps::explain`), istoricul (🕘:
      instrucțiunile rulate de curând, una pusă la cursor), parametrii `(p)`, `Tx: Auto ▾` (Auto /
      Manual, izolarea din `ConsoleCaps::isolation`; Commit și Rollback cât e o tranzacție
      deschisă, cu un punct pe pastilă), ■ Stop (roșu cât rulează), `Playground ▾` / `Script ▾`,
      ⚙ (rândurile aduse, oprirea la eroare: setările, salvate), comutatorul „Services”; în
      dreapta, ce a făcut ultima rulare (`9 rânduri · 28 ms`, eroarea în roșu, spinner-ul cât
      rulează), insignele „doar citire” și „driver vechi: rezultate limitate” (din
      `ConsoleCaps::limited` sau sesiune) și `bază.schemă ▾` (meniul schemelor și al bazelor,
      citite pe conexiunea consolei; schimbarea bazei cu o tranzacție deschisă e refuzată). Ce nu
      încape intră într-un „…” (`layout`, o dată pe lățime și stare, `kernel::Memo`). Toate
      butoanele au indicii, cu tastele comenzii. Tab-ul: `console [sursă]`, culoarea sursei;
      breadcrumb-ul `Database Consoles › sursă › console`. Ultima durată se ține în `ConsoleSpec`.
    - **Comenzile**: `database.console.run` (⌘↵), `run_script` (⇧⌘↵), `stop` (⌘F2), `explain`,
      `explain_analyze` (o scriere cere confirmare), `commit`, `rollback` — toate
      `When::EditorLanguage("sql")`, active doar pe o consolă —, `jump` (⇧⌘F10) și `new`; în
      presetarea JetBrains aceleași taste, în VS Code Ctrl+Enter. Fără instrucțiune la cursor:
      `host.choose` („Care instrucțiune?”); o sursă doar citire refuză scrierea înainte să plece.
    - **„Jump to Query Console…”**: cererea lăsată de fereastră / vedere, altfel sursa consolei
      active, altfel selecția ferestrei „Database” (`ConsoleAccess::selected`), altfel alegerea
      sursei. Fără console pe sursă (sau „New Query Console”): una nouă, deschisă; cu console: un
      popover cu „Consolă de interogări nouă”, apoi consolele cu ultima rulare. Deschisă de pe un
      tabel, consola primește schema lui și, goală, `SELECT * FROM <tabel>;` cu numele selectat.
      O sursă fără SQL (CSV) e refuzată cu motivul.
    - Setările (`settings.rs`, blocul „Console de interogări”): `[database] console_fetch_limit`
      (500), `console_stop_on_error` (da). Task Manager: conexiunile și memoria Output-urilor.
  - **Explorer-ul** implementează portul (`console::ExplorerConsoles`, dat cu
    `SessionsService::attach`): conexiunea făcută ca a lui (`Sessions::connect_fn` / `secret_fn`,
    aceeași parolă citită în fundal, același script de pornire, același „doar citire”, baza din
    `Overrides`), dar pe o conexiune proprie, nu pe worker-ul lui. Butonul nou din antetul
    ferestrei „Database” (iconița `console`, între `+` și reîmprospătare), rândurile „Consolă de
    interogări” / „Consolă de interogări nouă” în meniul unei surse, scheme, grup sau tabel (doar
    surse configurate cu dialect), butonul din antetul vederii de date (lângă Data / Structure,
    doar pentru o sursă configurată cu SQL) și rândurile din meniul listei ei de tabele; selecția
    ferestrei e ținută pentru paletă.
  - **Iconițe noi**: `play`, `stop`, `gear`, `console`, `parameter`, `plan`.
  - **Teste:** domeniul (identități, fișierele consolelor găsite înapoi, numele, interogarea
    tabelului, planul pe cursor / selecție / script / alegere / nimic, refuzul doar citire,
    registrul pe proiect, cererea „Jump”); `storage` (lista dusă și întoarsă, una stricată
    păstrată); `command-model` (`EditorLanguage` față de `Editor`, alt limbaj și o vedere);
    `ui-components` (`text_hash`, implicitele barei); modulul, pe SQLite temporar cu pool-ul și
    ceasul manuale (nimic conectat înainte de rulare, ordinea și semnele, eroarea care oprește
    scriptul cu locul ei, anularea, tranzacțiile `Manual` și `BEGIN` de mână, conexiunea închisă cu
    ultima bară și o tranzacție anulată, inactivitatea, schemele, Explain, parolele mascate,
    lista consolelor urmând discul, una stricată nescrisă, închiderea proiectului; bara: titlul,
    iconița, culoarea, breadcrumb-ul, butoanele și meniurile, indiciile, modurile ținute,
    evidențierea, ce a făcut rularea, închiderea cu tranzacție, istoricul; comenzile cu o gazdă
    falsă: rularea la cursor / selecție / script, alegerea, doar citire refuzat, sursa fără SQL
    refuzată, prima consolă făcută pe un tabel apoi lista, o consolă goală deschisă pe un tabel,
    închiderea cu tranzacție, Explain Analyze care întreabă, setările; layout-ul barei pe lățimi);
    explorer-ul (sursa văzută cu dialectul și culoarea, CSV fără SQL, conexiunea proprie doar
    citire, selecția); aplicația (bara deasupra textului, nu peste el: același x și aceeași lățime,
    bara de căutare sub ea; titlul și breadcrumb-ul; ⌘↵ al consolei față de al grilei, ⇧⌘F10).
  - **Capturi noi**: `console` (o consolă a bazei de probă, prima instrucțiune rulată: semnul
    verde, „9 rânduri · 28 ms”, ceasul oprit) și `database-jump-console` (lista consolelor sursei),
    ambele teme. Schimbate, cum era de așteptat: `database-tree`, `database-add`,
    `database-menu`, `database-folders`, `database-sources*`, `toast-over-modal` (butonul nou din
    antetul ferestrei „Database”, rândurile din meniu), `database-folder-grid*` (și butonul vederii
    de date, lângă Date / Structură), `settings-caret` (blocul nou „Console de interogări” în
    secțiunea editorului), `about` / `settings-8` (un modul în plus). Toate celelalte identice
    (în afara datelor relative ale capturilor git).
  - **Abateri de la plan, cu motivul:**
    - semnele din gutter vin prin bara consolei (`EditorToolbar::marks`), nu printr-un
      `LineDecorator`: aplicația arată decorațiile unui singur decorator pe document (git-ul ar fi
      putut acoperi semnele) și semnele sunt ale tab-ului consolei oricum;
    - înălțimea barei e 40 px, nu cea a grilei (46): deasupra editorului, sub tab-uri și
      breadcrumb, 46 ar fi luat prea mult din text;
    - alegerea bazei / schemei e un meniu al aplicației (schemele, bazele într-un submeniu), nu un
      popover cu filtru;
    - istoricul (🕘) arată deocamdată instrucțiunile rulate în sesiune (în memorie), iar
      parametrii `(p)` sunt doar un buton dezactivat cu indiciul lui: fereastra istoricului e a
      39i, modalul parametrilor al 39h;
    - Explain scrie planul în Output (tab-ul „Plan” e al 39h); Output-ul și rezultatele se văd în
      „Services” abia din 39f (contribuția bazei de date);
    - `Tx: Manual` și Commit / Rollback lucrează deja (cu semnul pe pastilă, închiderea tab-ului
      care întreabă și anularea la închiderea proiectului), fiindcă un `BEGIN` scris de mână le
      cere oricum; mașina de stări, semnul de pe tab și din arbore și `TransactionChanged` rămân ale
      39g;
    - ora din Output e UTC (`2026-10-31 10:00:23.698`): nu există încă o oră locală în aplicație;
    - în presetarea VS Code, Explain nu primește Ctrl+Shift+E (e al ferestrei proiectului acolo);
    - consolele sunt doar ale surselor configurate: un fișier deschis singur (fără sursă păstrată)
      cere întâi „Păstrează ca sursă” (identitatea lui e nouă la fiecare deschidere).
  - **Ce n-a intrat și de ce:** rezultatele și Output-ul în „Services”, sublinierea erorilor,
    `ConsoleRan` / `DataChanged` (39f); tranzacțiile complete și confirmările (39g); parametrii și
    tab-ul planului (39h); istoricul pe disc (39i).
- **39f — făcut** (rezultatele în „Services”):
  - **Domeniul** (`database`): `sql::tables` — `query_table` (tabelul unui `SELECT` dintr-un
    singur tabel: fără `JOIN`, fără al doilea tabel, subinterogare sau funcție în `FROM`, fără
    `UNION` / `WITH`; numele nescrise între ghilimele coborâte la Postgres), `written_tables`
    (`INSERT INTO`, `UPDATE`, `DELETE FROM`, `MERGE INTO`, `REPLACE`, `TRUNCATE`, plus
    `ALTER TABLE` / `DROP TABLE` cu structura schimbată, și în părțile unui `WITH`) și
    `rerunnable` (o interogare pură care se poate înveli: `SELECT`, `WITH … SELECT`,
    `VALUES`, `TABLE`, fără `FOR UPDATE / SHARE`); `Dialect::select_over` (`SELECT * FROM
    (\n<interogare>\n) AS console_result WHERE … ORDER BY … LIMIT … OFFSET …`, `;`-ul și un
    comentariu de la capăt neatinse); evenimentul `ConsoleRan { console, source, statements,
    writes, ddl }`.
  - **`ui-components`**: `Total::AtLeast(n)` („1–500 din 500+”, „500+ rânduri”: o pagină
    următoare de citit); `data_grid::export` — ce au în comun toate exporturile (formatul după
    extensie, `Target`, `ExportSink`: fișierul temporar ascuns redenumit peste țintă la
    capăt, șters la anulare sau eșec; limitele clipboard-ului, `ExportOutcome::told`), luat
    din explorer, care îl folosește acum (`export_menu` public); `services::set_shown` /
    `shown` (fereastra spune dacă e arătată, pe proiect: desenată / ascunsă).
  - **`services-panel`**: spune când e arătată (desen, `parked`, `Drop`, alt proiect); un nod
    care e chiar unul din tab-urile sale (un rezultat sub sesiunea lui) își arată tab-ul.
  - **Modulul `database-console`**:
    - `results` — `ResultBuffer` (rândurile în bucăți de 1 000 împărțite cu vederile prin
      `Arc`, octeții numărați, celulele peste 64 KB ținute tăiate cu mărimea spusă),
      `ResultSet` (titlul `bază.schemă.tabel` sau `Rezultat N`, instrucțiunea, coloanele,
      `More::{None, Cursor, Rerun, Lost}`, filtrul și ordinea serverului, execuția și aducerea,
      fixat, eliberat), `Results` (`replace_unpinned`, `cursors_closed`, `trim`: peste
      `[database] console_result_memory` — 64 MB implicit, setare nouă în blocul „Console de
      interogări” — cele mai vechi nefixate își lasă rândurile, apoi cele fixate, niciodată cel
      care se citește).
    - **Runner-ul** ține rezultatele: fiecare set ca un rezultat (mai multe pe instrucțiune și
      pe script), pe măsură ce vin; rularea următoare le înlocuiește pe cele nefixate. Doar
      ultima interogare a unei rulări își păstrează cursorul (următoarea instrucțiune îl
      închide; restul se citește apoi rulând din nou interogarea). Pagina următoare
      (`fetch_more`): din cursor, altfel interogarea pură învelită de la rândul următor
      (`LIMIT n+1 OFFSET m`); o scriere nu se rulează niciodată din nou (`More::Lost`, spus în
      Output). `requery`: interogarea pură citită din nou sortată și filtrată de server (și
      rândurile eliberate pentru memorie aduse înapoi). `export`: interogarea pură rulată din
      nou, rândurile scrise pe măsură ce vin (niciodată toate în memorie). Ce e cerut cât
      runner-ul lucrează se face când se eliberează. Output-ul: mesajele din locales,
      „N rânduri aduse începând cu {from} în X (execuție: Y, aducere: Z)” și la paginile
      următoare; o eroare e o legătură (`OpenFile`, linia și coloana din document, sau începutul
      instrucțiunii) și e subliniată în consolă prin `DiagnosticsFound` (sursa `sql-console`),
      ștearsă la rularea următoare sau când textul instrucțiunii se schimbă (bara urmărește
      editorul cât e o eroare arătată). Pe bus-ul proiectului: `ConsoleRan` la capătul fiecărei
      rulări, `DataChanged` pentru tabelele scrise (schema conexiunii pusă unde nu e scrisă;
      `main` al SQLite nu e o schemă a grilelor) — imediat în afara unei tranzacții, altfel la
      Commit (un Rollback nu spune nimic). O rulare de peste 10 s terminată cu fereastra
      „Services” ascunsă dă un toast cu „Arată Services”. Mai multe treziri (bara, fereastra,
      vederea). Dialectul pentru învelire: al sesiunii, altfel după `flavor`-ul sursei.
    - `result_source::ResultSource` (`GridSource`, doar citire): bucățile runner-ului luate doar
      când generația rezultatului se schimbă; rândurile de după cele aduse cerute runner-ului
      când grila le vrea; sortarea și filtrul `WHERE` / `ORDER BY` locale (în fundal, prin
      `database::eval`, o permutare, niciun rând copiat) când toate rândurile sunt aici — sau
      pentru rezultatul unei scrieri —, altfel de server (spus în bară); F5 rulează din nou o
      interogare pură, o scriere niciodată (spus).
    - `result_view::ResultView` (`Viewer`): grila (paginile, căutarea, transpunerea, copierea
      sunt ale ei) sub o bară subțire: Commit / Rollback cât e o tranzacție deschisă, timpul
      rulării (execuția și aducerea în indiciu), ce trebuie știut de rânduri (sortat de server,
      rânduri neaduse, eliberate, celule tăiate) și exportul (butonul: „Save As”, formatul după
      extensie; meniul: formatele grilei). Un rezultat complet se exportă din rândurile lui, în
      ordinea arătată, în fundal; o interogare cu rest e rulată din nou de runner; o scriere
      exportă doar ce a adus, spus.
    - `services::ConsoleServicesProvider` (contribuția, id `database-console`, grupul „Baze de
      date”): sursa (iconița, numele driverului, culoarea) › sesiunile consolelor (ultima
      durată, spinner, eroare, `TX`) › rezultatele lor; tab-urile unei sesiuni: Output (nu se
      închide) și câte unul pe rezultat (fixat / nefixat; închis, rândurile pleacă pe loc).
      La capătul unei rulări se arată ultimul rezultat (sau Output-ul, la o eroare).
    - Run / Explain arată fereastra „Services” (`show_side_panel`). Task
      Manager-ul numără și rândurile rezultatelor. Ora din Output e acum **ora locală**
      (`chrono`, deja în arbore prin alte crate-uri; doar ceasul cerut: `Cargo.lock` primește
      doar dependența).
  - **Teste**: domeniul (`query_table`, `written_tables`, `rerunnable`, `select_over`);
    `ui-components` (exportul: formatul, fișierul înlocuit doar complet, clipboard-ul limitat;
    `shown` pe proiect); modulul cu o sesiune scrisă de test (ceasul manual: execuția 43 ms față
    de aducere 368 ms, exact ca în Output), pe SQLite temporar și cu pool-ul manual: tab-ul
    nefixat înlocuit și cel fixat păstrat, închiderea care eliberează rândurile, pagina următoare
    prin rerulare (textul învelit exact) și prin cursor (închis de instrucțiunea următoare), o
    scriere niciodată rerulată (nici pagina, nici sortarea, nici exportul), sortarea pe server,
    limita de memorie și rândurile aduse înapoi, legătura erorii, diagnosticele și ștergerea lor
    (la rulare și la editarea instrucțiunii, prin bară), `DataChanged` / `ConsoleRan`, o scriere
    spusă abia la Commit, toast-ul rulării lungi doar cu fereastra ascunsă, bucățile și
    celulele tăiate, clasele coloanelor, sursa grilei (sortare locală, pagini cerute o dată,
    sortare de server, F5), exportul (complet, rerulat, parțial), arborele și tab-urile din
    „Services”, Run care arată fereastra, ceasul local.
  - **Capturi noi**: `services-output` (Output-ul unui script: orele, fiecare instrucțiune cu
    `main>`, „N rânduri aduse … (execuție, aducere)”, rândul afectat), `services-result`
    (`main.customers`, grila cu „12 rânduri”), `services-results-many` (trei rezultate, primul
    fixat, ultimul arătat), `console-error` (eroarea subliniată în consolă, roșie în Output,
    semnul din arbore, bara de stare), ambele teme; zona de jos făcută mai înaltă (400) în ele.
    Schimbate, cum era de așteptat: `console` (Run deschide acum „Services” cu rezultatul;
    zona de jos tot 400) și `settings-caret` (rândul nou „Memoria rezultatelor unei console”);
    toate celelalte identice.
  - **Abateri de la plan, cu motivul:**
    - bara tab-ului de rezultat e una a noastră deasupra grilei (Tx, timpul, nota, exportul);
      transpunerea și căutarea rămân în bara grilei. La înălțimea implicită a zonei de jos
      (280) grila arată puține rânduri; înălțimea se trage (și se ține);
    - celulele mari se țin tăiate; aducerea lor întregi în editorul mare rămâne pentru 39l
      (rezultatele sunt doar citire acum);
    - toast-ul rulării lungi vine după 10 s (prag fix, nu setare);
    - nodurile din „Services” nu au meniu (deschiderea consolei din arbore cere gazda, pe care
      contractul `ProjectServices` nu o are);
    - avertismentul „peste 100 000” vine din grilă doar când totalul e știut; cu „500+” nu se
      știe încă.
- **39g — făcut** (tranzacțiile și siguranța):
  - **Domeniul** (`database`): `console::TxState` — mașina de stări pură (`Idle`, `Open {
    since_statement, writes }`, `Failed`): `on_statement(index, kind, ok, mode, flavor)` (în
    `Manual` prima scriere reușită o deschide, scrierile se numără, o eroare o abandonează la
    Postgres — `Failed` până la `ROLLBACK` / `COMMIT` / `ROLLBACK TO`; SQLite o ține deschisă),
    `on_control` (un `BEGIN` / `COMMIT` / `ROLLBACK` / `SAVEPOINT` scris de mână, în ambele moduri),
    `follow(index, open)` (cuvântul motorului: un DDL care confirmă singur, o tranzacție deschisă
    de ceva ce regulile n-au văzut), `pending`, `ended`; `TxMode::default_for(production)`
    (`Manual` pe producție, `Auto` în rest, întrebarea 3); `console::safety(statements, flavor,
    production, tx, in_transaction) -> Safety { dangers, production_writes, production }`:
    pericolele fiecărei instrucțiuni (`danger::statement_dangers`), oriunde, și — pe producție, în
    `Tx: Auto`, în afara unei tranzacții (și a unui `BEGIN` scris de mână, până la `COMMIT`-ul lui)
    — fiecare scriere făcută definitivă pe loc. Evenimentul nou `TransactionChanged { console,
    source, open }`. Semnul **producție**: `DataSource::production`, `SourceFolder::production`,
    `folders::production_folder` / `folder_production` / `effective_production` (moștenit de la
    orice folder de deasupra, ca și culoarea, dar niciodată luat înapoi dinăuntru: siguranța
    înaintea comodității); `same_connection` îl ignoră (o sesiune rămâne deschisă).
  - **`storage::TomlDataSources`**: `production = true` pe sursă și pe folder (scris doar când e
    pus; un fișier vechi se citește „nu e producție”).
  - **Explorer-ul**: `Catalog::production_of`, `SourceBrief::production` adevărat acum; modalul
    surselor: bifa „Producție” în Opțiuni (sub „doar citire”, cu nota; moștenită de la un folder:
    bifată, dezactivată, „De la folderul „prod”…”), bifa în panoul folderului (moștenită de la
    părinte: la fel), insigna „PROD” pe rândurile listei (sursele, și cele moștenite, și folderul
    marcat); meniul folderului din fereastra „Database” are „Producție” (bifat cu ✓, comutat, cu un
    toast). `Sessions` urmărește `TransactionChanged` pe bus-ul proiectului (`console_transaction`):
    rândul sursei primește insignele „TX” (o consolă ține o tranzacție) și „PROD”
    (`tree::RowMarks`), iar o scriere a grilei refuzată cu `InUse` cât o consolă are tranzacția
    deschisă pe sursă spune exact asta (`Sessions::refusal`, `database.console_tx_in_use`), nu „alt
    program ține fișierul”.
  - **`ui-components` / aplicația**: `EditorToolbar::tab_badge` (un semn scurt după titlul tab-ului,
    în culoarea lui), `TabTitle::badge` desenat de bara de tab-uri (pastila „TX”, tab-ul mai lat cu
    ea); `ds::Tone::Warning`.
  - **Modulul `database-console`**:
    - **Runner-ul** urmărește `TxState` după fiecare instrucțiune (apoi cuvântul sesiunii, la
      fiecare `put_back`), `Snapshot::tx` / `tx_since`, instrucțiunile care au scris în tranzacția
      deschisă (`transaction_statements`, mascate); `TransactionChanged` la fiecare deschidere și
      închidere (Commit, Rollback, `abandon`, conexiunea închisă). `IDLE_CLOSE` ocolit și cu un
      cursor deschis (nu doar cu o tranzacție); o tranzacție deschisă de peste o oră e spusă o
      dată într-un toast (`LONG_TRANSACTION`, `long_due` / `tell_long_transaction`, prin firul
      `db-console-idle`, care doarme acum cel mult 30 s o dată). Închiderea ultimei bare cât lucrează
      ceva (un Commit cerut la închiderea tab-ului) lasă conexiunea lucrării și o închide la
      capătul ei: Commit-ul nu se mai pierde. Un Commit / Rollback eșuat e spus și într-un toast
      (tab-ul poate fi închis deja). `refused` (Output-ul), `acknowledged` / `acknowledge` (ce s-a
      confirmat o dată pe sesiunea consolei).
    - **Confirmarea** (`confirm.rs`, modalul `view-console-confirm`, `host.open_modal`, nu se
      închide la click afară): `ConfirmRequest` (`ConfirmKind::{Run, Analyze, Profile,
      CloseTransaction}`, consola, sursa cu culoarea ei și eticheta „producție”, instrucțiunile
      exacte colorate ca SQL prin `code_tokens`, motivele în cuvinte — `danger_text`, aceleași
      texte `schema.lose_*` —, o notă, `once`), `Answer::{Go, Commit, Rollback}`; `ask(host,
      request, then)` lasă ce se face cu răspunsul, iar butonul modalului îl închide și rulează
      `database.console.confirmed`; închis altfel (×, Esc) nu se face nimic, iar un răspuns vechi nu
      ajunge la altă întrebare (id-uri). Pe producție butonul se activează abia după „Am înțeles”
      (`danger_button`). **Pentru 39h2**: `ConfirmKind::Profile` și `ConfirmRequest::once = (consola,
      cheia)` — un „da” e ținut de runner-ul consolei cât e deschis proiectul și nu se mai întreabă.
    - **Comenzile**: Run — o sursă doar citire refuză scrierea fără modal (toast și Output), apoi
      `safety`: pericole sau scrieri pe producție în Auto → modalul, altfel rularea; Explain
      Analyze peste o scriere → modalul (`Analyze`, cu ce nu se anulează: secvențe, efecte în
      afara bazei, blocări cât rulează), refuzat pe o sursă doar citire; închiderea unei console cu
      tranzacția deschisă → modalul (`CloseTransaction`: ce ar păstra Commit-ul, „Commit și
      închide”, „Anulează și închide”, „Păstrează deschisă”). Consolele noi (făcute, puse de mână,
      găsite pe disc) ale unei surse de producție pornesc în `Manual`.
    - **Bara**: insigna „PROD” lângă `bază.schemă` (nu intră niciodată în „…”), `Tx: Manual · 2`
      (scrierile care așteaptă Commit), `Tx: eșuată` și punctul roșu pe pastilă la o tranzacție
      abandonată, indiciile pe stare; `tab_badge` „TX” (galben; roșu: eșuată). Arborele din
      „Services” arată `TX` (deja din 39f, acum urmând mașina de stări).
  - **Teste**: domeniul (mașina de stări: Auto, Manual și numărătoarea, abandonul Postgres și
    ieșirile din el, `BEGIN` / `COMMIT` / `SAVEPOINT` scrise de mână, un `COMMIT` refuzat, cuvântul
    motorului; `safety`: pericolele oriunde, scrierile pe producție doar în Auto și în afara unui
    `BEGIN`, citirile niciodată; producția moștenită, niciodată luată înapoi, `same_connection`);
    `storage` (semnul dus și întors, doar unde e pus); explorer-ul (producția din sursă sau din
    folder, tranzacția unei console urmată și mesajul `InUse` potrivit, insignele arborelui,
    bifele formularului și moștenirea lor); `ui-workbench` (insigna tab-ului îl lățește);
    modulul: runner-ul (starea urmată și spusă o dată pe deschidere / închidere, abandonul,
    nicio închidere pe inactivitate cu tranzacție sau cursor, toast-ul după o oră spus o dată,
    **SQLite blocat de tranzacția consolei: altă conexiune primește `InUse`, liberă după Commit**,
    refuzul în Output, confirmarea ținută pe sesiune), comenzile (pericolul confirmat întâi —
    renunțat nu rulează nimic, confirmat rulează —, producția: `Manual` implicit, fără întrebare
    în Manual, cu „Am înțeles” în Auto, citirile libere; doar citire refuzat fără modal, în Output;
    Explain Analyze întrebat și refuzat pe doar citire; închiderea cu Rollback și cu Commit, chiar
    cu bara plecată; întrebarea „o dată pe sesiune”), modalul („Am înțeles” înainte, ↵, Esc și ×
    fără efect, răspunsurile vechi ignorate, cele trei răspunsuri ale tranzacției, pericolele în
    cuvinte), bara (semnul tab-ului, numărătoarea, starea eșuată, insigna de producție pe orice
    lățime), layout-ul (insigna rămâne), „Services” (`TX` până la Commit).
  - **Capturi noi**: `console-tx-manual` (două scrieri în `Tx: Manual` care așteaptă Commit:
    `Tx: Manual · 2`, punctul pe pastilă, Commit / Rollback în bară și în tab-ul rezultatului,
    „TX” pe tab și în arborele din „Services”), `console-danger` (un `DELETE` fără `WHERE` și un
    `DROP TABLE` confirmate înainte să plece: motivele în cuvinte, instrucțiunile colorate),
    `console-prod-confirm` (o scriere pe o sursă de producție în `Tx: Auto`: eticheta
    „producție”, „Am înțeles” nebifat, butonul încă oprit; „PROD” pe bară), ambele teme.
    Schimbate, cum era de așteptat: `database-sources*` (sursa `shop-prod` marcată: insigna
    „PROD” în listă; `database-sources-folders`: folderul `prod` marcat, bifa „Producție” în
    panoul lui, insignele pe sursele moștenite și în fereastra „Database” din spate) și
    `toast-over-modal` (același modal al surselor). Toate celelalte identice.
  - **Abateri de la plan, cu motivul:**
    - `TransactionChanged` are și `source` (fereastra „Database” și grila știu așa ce sursă
      ține o consolă), nu doar `{ console, open }`;
    - producția moștenită de la un folder nu se poate scoate dintr-o sursă din el (bifa ei e
      bifată și dezactivată): culoarea se poate suprascrie, siguranța nu;
    - pe producție se confirmă orice scriere în `Tx: Auto` (cum spune planul), nu și una în
      `Manual` (nimic nu e definitiv până la Commit) și nici Commit-ul;
    - închiderea consolei cu tranzacție deschisă oferă și Commit (nu doar Rollback / Anulează):
      tab-ul se închide, Commit-ul se termină în fundal, iar un eșec e spus într-un toast;
    - mesajul potrivit pentru `InUse` vine la trimiterea modificărilor grilei (unde apare
      blocarea); citirile grilei (SQLite în WAL nu le blochează) păstrează mesajul general;
    - insigna „TX” din fereastra „Database” e pe rândul sursei (nu pe consolă, care nu e în
      arborele ei); în „Services” e pe sesiunea consolei, cum era din 39f;
    - în capturi, `database-folders` și `database-folder-grid*` rămân neschimbate: folderul
      `prod` e marcat ca producție doar în `database-sources-folders`.
  - **Ce n-a intrat și de ce:** profilarea pe producție o dată pe sesiune e a 39h2 (modalul și
    `once` sunt gata); bifa „producție” din Opțiuni nu are o captură a ei (`database-sources*`
    arată tab-ul General; insigna din listă și bifa folderului se văd).
- **După 39g — meniul editorului după context** (cerut după 39g, captura 373: într-o consolă
  meniul de click dreapta arăta refactorizările PHP):
  - Un rând al meniului (`ContextMenuItem`) spune acum și unde se aplică (`MenuWhen`: oricând,
    limbajul fișierului — `Language("sql")` —, ce oferă inteligența limbajului —
    `Provides(LanguageFeature)` — sau cât e activă altă comandă — `With(cmd)`); aplicația nu știe
    niciun limbaj: limbajele își declară `LanguageIntelligence::features` (PHP și JS / TS: tot;
    CSS / HTML: navigarea și redenumirea; SQL: nimic; un fișier fără inteligență: nimic). Rename,
    Change Signature, Move, Go to Declaration, Find Usages și Show Diagram apar doar unde limbajul
    le oferă; „Istoric local ›” doar pentru fișierele proiectului; secțiunile goale dispar.
  - **Secțiunea consolei, prima** (`MenuSection::Run`): Execute (⌘↵), Execute Script, Explain Plan;
    apoi, printre cele de navigare, Jump to Query Console… (doar într-o consolă) și comanda nouă
    **Open in Services** (`database.console.open_in_services`: sesiunea consolei în „Services”,
    cu Output-ul ei, fără nicio conexiune). În orice fișier `.sql`, „Comentează rândul” (`--`,
    contribuit de `lang-sql`, secțiunea nouă `MenuSection::Code`). Profile și Format vin cu 39h2 /
    mai târziu.
  - Modalul de confirmare are caseta instrucțiunilor cât conținutul (între 2 și 12 rânduri, apoi
    derulare), iar fereastra modalului e cât îi trebuie (`Viewer::modal_height`, `ModalSpec::fit`).
  - Teste: meniul unei console, al unui `.sql` simplu, al unui text simplu, al unui fișier din
    afara proiectului, PHP neschimbat (`context_menus.golden` identic); condițiile; rândurile
    modulului; Open in Services; înălțimile modalului. Captură nouă: `console-menu`; schimbate:
    `console-danger`, `console-prod-confirm` (modalul compact).
- **39h — făcut** (parametrii și planul):
  - **Domeniul** (`database`): `PlannedStatement::bound` (`Option<sql::Bound>`), `sent()` (textul
    trimis, cu valorile puse) și `map()` (`BindMap`: poziția motorului întoarsă în document);
    `text` rămâne textul din document (semnele din gutter, istoricul). `console::parameters`
    (`AskedParam`: numele, cum e scris prima oară, câte locuri; o dată pe nume, `?` numerotat în
    instrucțiunea lui), `console::bind_statements` (valorile puse prin `Dialect::literal`, o
    expresie brută cum e tastată, un parametru fără valoare e eroare, nu `NULL`) și
    `console::dialect_for(flavor)` (dialectul înainte ca sesiunea să-l dea pe al ei; folosit și la
    învelirea interogărilor 39f). `plan`: `inclusive_ms`, `total_rows`, `own_detail`, `Measure`
    (timpul, altfel costul), `flatten` (`FlatNode`: adâncimea, părintele, în adâncime),
    `PlanColumns::of` (doar coloanele pe care le are măcar un pas), `weight`, `hot_path` (ramura
    cea mai scumpă: de la rădăcina cea mai grea, de fiecare dată copilul cel mai greu; goală fără
    măsură, ca la SQLite), `to_text` / `line` (planul ca text, pentru „Copiază”).
  - **`ui_components::tree_table`** (nou, generic, ca 39h4 să-l extindă): `TreeTable` cu
    `TableColumn` (titlu, indiciu, aliniere, lățimea minimă și cea dată; prima coloană e arborele
    și ia ce rămâne), `TableRow` (părintele, iconița, celulele, `marked` — dunga accentului și
    textul mai îngroșat —, `sign` în gutter, `tint`), `TableCell` (`CellValue::{Empty, Text(spans
    Normal / Strong / Muted / Code), Number { value, decimals, unit }, Bar { fraction, tone, label },
    Split { a, b, label }, Badge, Icon}` și indiciul lui); rândurile arătate ținute în `Memo` pe
    generația rândurilor și a deschiderilor, desenate doar cele în vedere (`TreeList`); ghidajele
    și chevroanele `tree`; antetul cu marginea coloanelor trasă (redimensionare); tastele (↑ ↓,
    PgUp / PgDn, Home / End, → deschide / primul copil, ← închide / părintele, ↵, `*` deschide tot
    sub rândul ales); alegerea păstrată la închidere (trece la părinte); `row_text` / `text`
    (copierea); `number_text` (cifrele grupate cu spațiu subțire).
  - **Modulul `database-console`**:
    - **`params.rs`**: modalul `view-console-params` (`host.open_modal`, nu se închide la click
      afară): sursa în culoarea ei (și „producție”), instrucțiunile cu parametri colorate ca SQL, cu
      locurile parametrului editat luminate, apoi un rând pe parametru — numele cum e scris (și
      `×2` când apare de mai multe ori), valoarea (`TextInput`), felul (Text / Număr / NULL /
      Expresie, segmentat; până e ales, felul urmează valoarea: un număr tastat e număr) și „Ține
      minte” —, problema (un număr care nu e, o expresie goală) sub listă, Renunță / Rulează
      (Explică, Păstrează valorile). ↵ dă valorile, Esc și × renunță; Tab / ↑ ↓ trec la valoarea
      următoare; peste 6 parametri lista derulează. Ca la confirmare, `ask` lasă ce se face cu
      valorile, iar `database.console.params_given` (intern) le folosește doar pentru întrebarea
      lor.
    - **Comenzile**: Run (și Execute Script) — refuzul doar citire, apoi parametrii (dacă sunt),
      apoi pericolele și producția confirmate pe textul trimis, apoi rularea; Explain / Explain
      Analyze la fel (parametrii întâi, apoi confirmarea analizei unei scrieri). Comanda nouă
      `database.console.params` („Editează parametrii…”, butonul `(p)` al barei, activ acum):
      valorile instrucțiunilor pe care le-ar rula Run (altfel ale întregii console) editate și
      ținute, fără nicio rulare; o consolă fără parametri o spune.
    - **Runner-ul**: rulează `sent()` (Output-ul arată exact ce a rulat, cu parolele mascate;
      rezultatul își reține textul trimis, deci paginile următoare și sortarea pe server merg cu
      valorile), istoricul ține șablonul; eroarea (și la Explain acum) e locată prin harta
      instrucțiunii, deci sublinierea și legătura din Output cad în textul utilizatorului.
      Valorile ținute minte stau în `Runner` (`remembered_params` / `remember_params`), cât e
      deschis proiectul; `ConsoleSpec::params` primește doar numele. Explain face un tab „Plan”
      (`results::PlanSet`: titlul `Plan · tabel` pentru interogarea unui singur tabel, nodurile
      împărțite prin `Arc`, `analyze`, `rolled_back` pentru o scriere măsurată, durata), după
      aceleași reguli ca rezultatele: un plan nefixat e înlocuit de planul următor (și de rularea
      următoare), unul fixat rămâne; Output-ul păstrează planul scris rând cu rând, ca înainte.
    - **`plan_view.rs`**: tab-ul „Plan” (`PlanView`) pe `TreeTable`: Operația (iconița după fel,
      cuvintele motorului), Pe (obiectul), Condiția (doar când nu e deja în cuvintele operației,
      ca la SQLite), Cost (bară cu partea din cost la un plan estimat; costul de pornire în
      indiciu), Rânduri, Rânduri reale (`×120 ↑` lângă ele când estimarea greșește de 10× sau
      mai mult), Bucle, Timp (bara părții din timpul instrucțiunii, în toate buclele) — doar
      coloanele pe care le are planul; ramura cea mai scumpă marcată (și bara ei portocalie).
      Bara de deasupra: `EXPLAIN` / `ANALYZE`, „anulat” pentru o scriere măsurată, instrucțiunea
      trimisă colorată, „Ramura cea mai scumpă e marcată”, pașii și durata, Deschide tot /
      Închide tot / Copiază (planul ca text); meniul rândului (Copiază rândul, planul, deschide /
      închide tot), ⌘C rândul ales.
    - **`services.rs`**: nodurile și tab-urile planurilor (iconița `plan`) sub sesiunea consolei.
    - **Setarea nouă** `[database] console_param_styles` (`engine`: cum îi citește motorul — la
      Postgres `?` rămâne operatorul `jsonb` —; `all`: orice formă), în blocul „Console de
      interogări” (`settings-caret` arată rândul nou).
  - **Teste**: domeniul (`parameters` pe toate formele, `?` pe instrucțiune, `:id` = `@id`,
    șiruri / comentarii neatinse, `?` la Postgres; `bind_statements`: valorile puse, citarea,
    expresia brută, lipsa unei valori, eroarea motorului întoarsă exact în document; `flatten`,
    `hot_path` pe cost, pe timp cu bucle, gol la SQLite, `PlanColumns`, `line`); `db-postgres`
    (`EXPLAIN (FORMAT JSON)` dintr-un JSON salvat nou, `tests/data/explain.json`: arborele, coloanele,
    ramura pe cost, textul; cel analizat cântărit pe timp); `ui-components` (`tree_table`: rândurile
    ascunse și deschise, alegerea păstrată, tastele, `*`, textul copiat, cifrele, desenat: chevronul,
    dublu-clic, marginea trasă, indiciul antetului, derularea); modulul (fiecare formă de parametru
    cerută, scrisă și rulată pe SQLite temporar, istoricul cu șablonul; renunțat nu rulează nimic;
    valorile ținute doar în memorie — niciun fișier al proiectului nu le conține, istoricul nici
    el, numele în listă —, una nebifată uitată; eroarea după valori la linia și coloana
    utilizatorului; Explain cere valorile și deschide tab-ul „Plan”; confirmarea arată textul
    trimis; `(p)` editează fără rulare; modalul: felul care urmează valoarea, problemele, Tab, ↵,
    Esc, ×, înălțimea și derularea; tab-ul „Plan”: coloanele analizei, ramura, factorul, timpul
    pe bucle, planul estimat cu bara costului, planul SQLite real ca arbore desenat, închis /
    deschis / copiat; planurile înlocuite / fixate ca rezultatele; bara: `(p)` rulează comanda).
  - **Capturi noi**: `console-params` (o instrucțiune cu patru parametri, înainte de rulare:
    `:customer` citit ca număr, `:status` editat cu locul luminat în instrucțiune, o expresie
    `(SELECT avg(total) FROM orders)`, `:limit` nețiut minte) și `services-plan` (Explain Plan al
    unei interogări cu un CTE și o subinterogare corelată pe baza de probă: `CO-ROUTINE`, `SEARCH`,
    `CORRELATED SCALAR SUBQUERY` ca arbore), ambele teme. Schimbate, cum era de așteptat:
    `settings-caret` (rândul „Parametrii ceruți”) și capturile cu bara consolei — `console`,
    `console-error`, `console-tx-manual`, `console-danger`, `console-prod-confirm`, `console-menu`,
    `services-output`, `services-result`, `services-results-many` — doar butonul `(p)`, activ
    acum; toate celelalte identice.
  - **Abateri de la plan, cu motivul:**
    - un `?` e numerotat în instrucțiunea lui (regula 39a): într-un script, primul `?` al fiecărei
      instrucțiuni e același parametru, cu o singură valoare;
    - „ține minte” e bifat implicit (cerut: valorile se țin pe sesiune); debifat, valoarea e uitată
      după rulare;
    - `(p)` editează valorile instrucțiunilor de la cursor (altfel ale întregii console) fără să
      ruleze, cu butonul „Păstrează valorile”;
    - pe SQLite planul n-are costuri, rânduri sau timpi (`EXPLAIN QUERY PLAN`): tab-ul arată doar
      operația și obiectul, fără ramură marcată; captura `services-plan` e deci planul SQLite real
      (planul Postgres cu toate coloanele e verificat în teste, nu în captură, fiindcă testele și
      capturile nu pornesc servere);
    - Explain Analyze pe Postgres rămâne anulat de driver (39c), cu confirmarea 39g; „anulat” se
      vede pe tab-ul planului.
  - **Ce n-a intrat și de ce:** normalizarea profilului, regulile și vederea profilului sunt ale
    39h2–39h6 (tab-ul „Plan” și `tree_table` sunt baza lor); felurile de parametri ca setări pe
    fiecare formă (o singură alegere „motor / toate” acum).
- **39h2 — făcut** (profilul normalizat și driverele):
  - **Domeniul** (`database::profile`, pur, doar pe `kernel`):
    - `Profile` (`ProfileId` UUID v4, `PROFILE_FORMAT = 1`, `Engine`, `ProfileDepth::{Estimated,
      Actual { timing }}`, `StatementProfile`, nodurile plate `Vec<ProfileNode>` cu `NodeId`,
      `roots`, `collected: ProfileCaps`, `ProfileNote` — `RolledBack`, `IoTimingOff`,
      `InstrumentationOverhead`, `PlannerSettingsChanged`, `NewerFormat`, `Truncated` peste
      50 000 de noduri, `RawTooLarge` —, `raw` de cel mult 8 MB); `push`, `walk` (în adâncime, un
      nod întâlnit de două ori o singură dată), `sum_io`, `memory`, `keep_raw`;
    - `node.rs` (`OpKind` cu toate felurile planului, `guess` din cuvintele unui motor,
      `JoinKind`, `ChildRole`, `ObjectRef`, `CondKind`, `Estimates`, `Actuals`, `Io`, `Memory`,
      `Spill`, `Parallel` / `WorkerStats`, `Wal`, `NodeFlags`), `statement.rs` (`Jit`,
      `TriggerTime`, `Setting`, `EngineCounters`, `StatementStats`, `Wait`, `PlanningMemory`,
      `Serialize`), `extra.rs` (`Extra`, `ExtraValue` cu `Group` / `List`, `Quantity`, `Unit`),
      `metric.rs` (`Metric::of`, `present`), `tables.rs` (`TableStats`, `IndexStats`);
    - `derive` → `ProfileReport` (`Arc<Profile>`, `Derived` pe nod: `inclusive_ms`, `self_ms`,
      `self_pct`, `self_cost` / `cost_pct` pe un plan estimat, `total_rows`, `misestimate`,
      `depth`, `share_of_parent`, `approx`; `hot_path`, `order`, `Totals`): sub un `Gather`
      timpii sunt împărțiți la lucrători + conducător (`approx`), un CTE materializat îl plătește
      primul cititor (părintele nu-l scade a doua oară), rotunjirile care trec de părinte sunt
      tăiate la zero (`approx`), un nod `never executed` are 0;
    - `export`: textul ca `EXPLAIN` (`to_text`, fișierele de aur ale driverelor) și JSON-ul nostru
      scris de mână (`to_json`: `format` primul, ordinea câmpurilor fixă, doar ce se știe);
    - `testing::ProfileBuilder` (sub `cfg(test)` și feature-ul `testing`);
    - `Profile::from_plan` / `plan_nodes` (planul 39h ca proiecție: cuvintele, felul joinului,
      `using index`, condițiile pe care cuvintele nu le spun deja), `by_explain` (implicitul),
      `actual_refused` (`VACUUM`, `CREATE INDEX CONCURRENTLY`, `CREATE / DROP DATABASE`,
      `ALTER SYSTEM`, `REINDEX CONCURRENTLY`, controlul tranzacției, `ATTACH` la SQLite);
    - `ProfileCaps` (cele 18 câmpuri, `from_explain`, `fields` / `set`) în `ConsoleCaps::profile`;
      `ProfileOptions` (`ESTIMATED`, `ACTUAL`, `timeout`, `max_raw_bytes`, plus `serialize`);
      `Session::profile` (implicit din `explain`) și `Session::table_stats` (implicit
      `Unsupported`).
  - **PostgreSQL** (`db-postgres/src/profile.rs`, `plan.rs` scos): tabelul versiunilor în
    documentația modulului, `options(version, …, writes)` pe fiecare versiune 9.2–18 (`BUFFERS` la
    planul estimat de la 13, `TIMING OFF` la cerere, `SUMMARY` de la 10, `SETTINGS` de la 12, `WAL`
    doar la scrieri de la 13, `MEMORY` de la 17, `SERIALIZE TEXT` la cerere de la 17, `BUFFERS OFF`
    la 18 când nu e cerut), `caps(version)`, `parse(json, block_size)` cu `KEYS` / `STATEMENT_KEYS`
    (orice altă cheie, a nodului, a lucrătorului, a JIT-ului, a trigger-ului, a planificării sau a
    instrucțiunii, devine `Extra` cu tipul ei, grupată după ce ține), spill-urile (sortare pe disc,
    `HashAgg` pe disc, `Hash` cu mai multe loturi, `Storage: Disk` la 18), `Disabled` (18, și costul
    `disable_cost` dinainte). `block_size` din `SHOW block_size` la conectare. Rularea: `BEGIN READ
    ONLY` pentru o citire (`BEGIN` pentru o scriere sau un `FOR UPDATE`), `SAVEPOINT console_profile`
    în tranzacția utilizatorului, `SET LOCAL statement_timeout`, totul anulat la final;
    `track_io_timing` citit pentru nota `IoTimingOff`; `explain` e acum `profile(…).plan_nodes()`.
  - **SQLite** (`db-sqlite/src/profile.rs`): `sqlite_op` pe toate formele (inclusiv `SCAN TABLE` de
    dinainte de 3.36, `AS alias`, `CREATE AUTOMATIC INDEX`), `from_rows`, `measure` (pregătirea =
    planificarea, pașii numărați, primul rând, contoarele `get_status`, `SAVEPOINT console_profile`
    pentru o scriere, termenul în `progress_handler`), `scanstatus` (vezi abaterile),
    `table_stats` (`sqlite_stat1`, `pragma_index_list` / `pragma_index_info`, paginile fișierului,
    `dbstat` dacă e compilat). `ConsoleCaps::explain.analyze` e acum `true` la SQLite: Explain
    Analyze e profilul măsurat.
  - **Întrebarea 1 (scanstatus) — făcut, măsurat:** `LIBSQLITE3_FLAGS =
    "-DSQLITE_ENABLE_STMT_SCANSTATUS"` în `.cargo/config.toml` (toate crate-urile cu `rusqlite`,
    și ținta Windows). Binarul unui program minim cu `rusqlite` crește cu **16 768 octeți pe macOS**
    și **14 144 pe Windows** (`x86_64-pc-windows-gnu`, build-ul trece), mult sub 100 KB.
    Colectarea e oprită la deschiderea conexiunii (`SQLITE_DBCONFIG_STMT_SCANSTATUS`) și pornită
    doar cât rulează o profilare. `unsafe` doar în `scanstatus` (cu `// SAFETY:`).
  - **Protocolul v3** (`db-protocol`): `VERSIONS` 1–3, `PROFILE_VERSION`, metodele `profile` și
    `table_stats` (`SINCE_3`, refuzate unei sesiuni de versiunea 2), `dto::Profile` (aceleași nume
    ca exportul JSON, `format` separat de protocol), `dto::ProfileCaps` în `ConsoleCaps`,
    `dto::TableStats`; `convert::profile_from` citește un format mai nou cât se înțelege (nota
    `NewerFormat`, felurile necunoscute `Other`, un nod sau o rădăcină din afara listei lăsate
    afară). Un driver v2 primește profilul din `explain`. `toy`: un profil cu timpi, I/O, un
    `extra` din fiecare tip și o cheie necunoscută, plus statisticile tabelelor. Documentat în
    `03-sistem-module.md` §6.3.2, cu un exemplu și cu `[driver.capabilities.console.profile]`.
  - **Modulul `database-console`**: `profile::store::Profiles` (pe runner, cel mult
    `console_profile_keep` și `console_profile_memory`, cele vechi nefixate ies primele, cel mai nou
    rămâne, golite de „Clear and Rebuild Caches”, numărate în memoria consolei din Task Manager);
    `Runner::profile` (Explain și Explain Analyze trec prin el, cu opțiunile din setări): profilul
    driverului, ceasul de perete al runner-ului (`wall_ms`), `derive`, păstrat, tab-ul „Plan” din
    proiecție (`PlanSet::profile`, „anulat” din nota `RolledBack`), Output-ul cu arborele ca înainte
    (literalele parolelor din condiții mascate) și un rând de rezumat (planificarea, execuția,
    totalul, rândurile, anularea, contoarele motorului, blocurile, JIT, pasul cel mai scump).
    Setările noi `[database] console_profile_timeout` (300 s), `_buffers`, `_timing`, `_verbose`,
    `_statement_stats`, `_keep` (20), `_memory` (32 MB), citite prin `profile_options` /
    `profile_limits`. Comenzile: Explain Analyze al unei citiri pe producție cere confirmarea o
    dată pe consolă (`ConfirmKind::Profile`, `ConfirmRequest::once`, cheia `profile-production`,
    cu termenul spus); al unei scrieri, mereu (ca înainte); ce nu poate rula într-o tranzacție e
    refuzat înainte să plece (toast și Output). Răspunsul „o dată” se ține după ce întrebarea a
    făcut ce avea de făcut (runner-ul consolei poate apărea abia atunci).
  - **Teste**: domeniul (proiecția înapoi identică, `walk` pe o listă stricată, nota și limita
    `raw`, `Truncated`, `sum_io`, `guess`, toate id-urile, `actual_refused`, implicitul din
    `explain`, `ProfileCaps`; `derive` pe bucle, paralelism, `InitPlan`, CTE, `never executed`,
    rotunjiri, plan estimat, SQLite fără măsură, `present`, bugetul pe 10 000 de noduri; forma
    MySQL și MariaDB construite complet; exportul: textul exact, JSON-ul cu formatul primul,
    ordinea, scăparea, aceleași texte de două ori); `db-postgres` (cele 17 fixtures scrise de mână
    după documentație, fiecare cu `*.profile.txt` de aur rescris doar cu `EDITOR_UPDATE_GOLDEN=1`;
    `unknown-keys` verifică cheie cu cheie că nimic nu se pierde; opțiunile pe fiecare versiune;
    planul de 5 000 de noduri sub buget; testul pe server real extins, doar cu
    `EDITOR_TEST_POSTGRES_URL`); `db-sqlite` (toate formele EQP, arborele cu id-urile lui, 50 000 de
    comenzi pe un fișier temporar: `SCAN`, `SEARCH`, indexul automat cu buclele și rândurile lui
    reale, `TEMP B-TREE`, contoarele, timpii pe nod din cicluri fără să treacă de instrucțiune; o
    scriere profilată care lasă tabelul neschimbat, și în tranzacția utilizatorului; `VACUUM`
    refuzat; termenul care întrerupe, anularea; `sqlite_stat1` lipsă / prezent; SQLite compilat cu
    `ENABLE_STMT_SCANSTATUS`); protocolul (profilul v3 identic cu al driverului direct, un driver v2
    din `explain`, o sesiune v2 refuzată, dus și întors identic, exportul JSON citit ca
    `dto::Profile`, un format mai nou cu nota, opțiunile, capabilitățile din manifest și
    statisticile); modulul (profilul păstrat, tab-ul care îl numește, rezumatul în Output, Explain
    Analyze măsurat pe SQLite, o scriere anulată, memoria și golirea; magazinul cu limitele lui;
    confirmarea o dată pe producție, scrierea mereu, `VACUUM` refuzat fără conexiune; setările).
  - **Capturi:** niciuna nouă; toate cele vechi identice.
  - **Abateri de la plan, cu motivul:**
    - id-urile `scanstatus` nu sunt ale `EXPLAIN QUERY PLAN` (cele două se pregătesc separat și
      adresele programului diferă cu unu): elementele se potrivesc după cuvintele lor, în ordine, iar
      cele pe care planul nu le listează (`CREATE AUTOMATIC INDEX ON …`) se adaugă ca noduri;
    - `NCYCLE` e al fiecărui element, nu și al copiilor: timpul unui nod SQLite e partea lui din
      ciclurile instrucțiunii (ale lui și ale copiilor) din timpul execuției, aproximativ;
      `node_timing` rămâne `false`, `node_actuals_by_scan` `true`;
    - Explain și Explain Analyze sunt profilarea (fără o comandă „Profile” nouă): vederea, comenzile
      și opțiunile din ⚙ vin cu 39h4, iar setările profilării sunt declarate și citite acum, dar
      blocul lor din fereastra de setări vine tot cu 39h4 (`settings-caret` rămâne identic);
    - Output-ul are în plus rândul de rezumat, iar condițiile pe care cuvintele SQLite le spun deja
      nu se mai repetă după ele; în proiecția planului, planul unui CTE (`CO-ROUTINE x`) nu se mai
      numește și ca obiect (modelul îl are), deci tab-ul „Plan” rămâne cum era;
    - durata unui Explain (și `wall_ms`) se măsoară de la cerere, fără conectare (ceasul runner-ului
      citit de două ori, ca înainte);
    - `ProfileOptions::serialize` (nu era în listă) pentru `SERIALIZE TEXT`; `Io` are și timpii
      `local_*` (Postgres 17); `JoinKind` are `RightSemi` / `RightAnti` (17–18).
  - **Ce n-a intrat și de ce:** `pg_stat_statements` (`statement_history`) și `table_stats` la
    Postgres sunt ale 39h6 (`stats.rs`), cum spun pașii; regulile, `Facts` și `sql_map` sunt ale
    39h3; vederea profilului ale 39h4.
- **39h3 — făcut** (analiza profilului):
  - **Domeniul** (`database::profile`, pur, tot doar pe `kernel`):
    - **`sql::fingerprint`** (`sql/fingerprint.rs`): instrucțiunea fără valori — literalele,
      parametrii, `TRUE` / `FALSE`, corpurile `$$` și semnul unui număr devin `?`, o listă de
      valori de orice lungime (`IN (1, 2, 3)`) un singur `?`, comentariile scoase, spațiile
      normalizate, cuvintele necitate cu litere mici, ultimul `;` lăsat; `NULL` rămâne (`IS NULL`
      e altă interogare); hash FNV-1a pe 64 de biți, același pe orice mașină.
    - **`cond`**: o condiție citită cu lexerul 39a după flavor — `Predicate` (coloana cu
      calificativul ei, `Compare::{Eq, In, Range, IsNull, Like, Not}`, funcția care o învelește,
      `Wrapper`, conversia `(col)::tip` / `CAST`, literal sau parametru, `LIKE '%…'`, în interiorul
      unui `OR`, locul), `JoinPair` (cu `side_of`), `or_branches` (coloanele fiecărei ramuri a unui
      `OR` de sus), toate coloanele pomenite; formele Postgres (`((status)::text = 'x'::text)`,
      `= ANY ('{…}'::text[])`, `lower((email)::text)`, `~~`, `(SubPlan 1)`) și SQLite (`a=? AND
      b>?`), `BETWEEN … AND`, listele `IN (…)`, valorile tipizate (`DATE '…'`), funcțiile de valori
      (`now()`); niciodată o panică pe un text stricat.
    - **`sql_map::SqlMap`**: tabelele din `FROM` / `JOIN` / `UPDATE` / `INTO` cu aliasurile lor
      (și `FROM a, b`, subinterogările, fără funcții), clauzele cu adâncimea și grupul lor
      (`WHERE`, `ON`, `HAVING`, `ORDER BY` fără al ferestrelor, `GROUP BY`, `LIMIT`, `OFFSET`,
      `FETCH`, `SELECT`); `resolve` (un alias sau un nume, doar unic), `table_range`,
      `predicate_range` (comparația întreagă, ieșind din apelul sau parantezele din jurul coloanei:
      `lower(email) = 'x'`), `sort_columns`, `select_star`, `limit_without_order`, `offset` (și
      `LIMIT a, b`). Locurile sunt în textul trimis; constatarea le duce în document prin
      `BindMap` (testat după parametri puși).
    - **`rules`**: `Rule` (`id`, `applies`, `check`, `variants`), `RuleContext` (raportul,
      `Facts`, `PlannedStatement`, flavor, `RecentRuns`, `Thresholds`, producția; calculate o dată:
      `SqlMap`, condițiile fiecărui nod citite, `WHERE`-ul de sus, perechile de join ale
      instrucțiunii, dialectul), `Finding` (regula, `Severity::{Info, Warning, Critical}`,
      `impact` 0–1, nodurile, dovezile, varianta textelor, sugestia, `sql_range` în document),
      `Thresholds::DEFAULT` (constante cu nume), `analyze` (punctul fierbinte primul, apoi după
      severitate și impact), `problems`, `text_keys`; `facts.rs` (`Facts`, `TableFacts` cu
      structura, statisticile și rândurile, `index_starting_with`, `expression_index_on`,
      `KeyFact` — cheile motorului și cele virtuale acceptate —, `tables_of` cel mult 50,
      `node_table` cu aliasul dus la tabel, `RecentRuns` cel mult 256 de amprente × 64 de momente);
      `suggest.rs` (`Suggestion::{CreateIndex, Analyze, Vacuum, Statistics, Setting, Rewrite}`,
      `RewriteHint`, `IndexSpec`, numele `idx_<tabel>_<coloane>` tăiat la 63 de octeți, `CREATE
      INDEX` prin dialect — schema pe index la SQLite, `INCLUDE`, parțial, pe expresie, `USING gin
      (… gin_trgm_ops)`, `DESC`, `CONCURRENTLY` doar Postgres pe producție —, `ANALYZE`, `VACUUM
      (ANALYZE)`, `CREATE STATISTICS` 10+ cu `mcv` de la 12, `SET` verificat, `memory_value`).
    - **Cele 28 de reguli**, câte un fișier documentat (dovada, pragul și de ce): `hot-spot`,
      `seq-scan-selective`, `misestimate`, `nested-loop-many`, `spill-to-disk`,
      `filter-removes-most`, `fk-join-no-index`, `sqlite-scan`, `sqlite-temp-btree`, `auto-index`,
      `function-on-column`, `implicit-cast`, `or-defeats-index`, `leading-wildcard`,
      `select-star-wide`, `limit-without-order`, `big-offset`, `n-plus-one`, `workers-not-launched`,
      `jit-dominates`, `planning-dominates`, `heap-fetches`, `lossy-bitmap`, `dead-tuples`,
      `stale-stats`, `cold-cache`, `disabled-node`, `trigger-time`.
    - `ProfileReport::findings` (gol după `derive`, pus de modul); `testing::Check` (instrucțiunea,
      faptele, rulările, producția: `rule`, `analyze`, `context`), `testing::table`, `on`, `cond`.
  - **Modulul `database-console`** (`profile/analysis.rs`): `Analysis` (cheile acceptate ale
    sursei din `ConsoleAccess::notes`, producția), pusă de Explain prin `Runner::set_analysis`;
    `gather` pe conexiunea consolei, după profil, pe `db-console`: structura fiecărui tabel al
    profilului (cel mult 50) și, cu interogările auxiliare permise (`console_profile_statement_stats`)
    și un driver care nu e `limited` și le are, `table_stats`; ce cade rămâne necunoscut. `analyze`,
    `with_findings` (constatările merg cu `Arc<ProfileReport>`), `texts` (titlul, de ce, ce se poate
    face, din locale, cu dovezile ca argumente: rândurile grupate pe mii, timpii, octeții, procentele,
    factorii `×120 ↑` / `×0,01 ↓`), `severity`, `problems_text`. Runner-ul ține amprenta fiecărei
    instrucțiuni rulate (`RecentRuns`, din momentul făcut al runner-ului; `recent_runs`), iar
    Output-ul primește în rândul de rezumat „N probleme (M critice)” și câte un rând pe problemă
    (`Avertisment · titlu: de ce`), cu SQL-ul sugerat ca bloc de cod după el — scris, niciodată
    rulat. Textele în ambele limbi: `[profile.severity]`, `[profile.finding.<id>]` (`title`, `why`,
    `fix`, variantele), `console.profile_problems*`, `console.profile_finding`.
  - **Teste**: fiecare regulă cu cazuri pozitive, la limită și fals-pozitive pe profile
    `ProfileBuilder` (un `SeqScan` pe un tabel mic sau care întoarce aproape tot, o buclă cu o
    iterație, o estimare greșită pe 3 rânduri, un index existent care acoperă deja, `OR` pe aceeași
    coloană, `LIMIT 1` cu `ORDER BY`, varchar-ul Postgres convertit la `text`, sub un `LIMIT`, fără
    fapte nimic ghicit), cu textul exact al SQL-ului sugerat (citarea, numele lung tăiat, schema,
    `CONCURRENTLY` pe producție, `DESC`, parțial, pe expresie, trigrame, statisticile pe versiuni);
    `cond` pe formele Postgres și SQLite; `sql_map` (unic, ambiguu, alias, după parametri puși,
    clauzele); `fingerprint`; `Facts` și `RecentRuns`; ordinea constatărilor, o formă MySQL
    normalizată, bugetul (10 000 de noduri sub 20 ms în release); `db-postgres`: fixture-urile 39h2
    → constatările așteptate (`18-index-searches-disabled`: scanarea selectivă critică cu locul ei și
    `enable_seqscan`, `sort-external` / `16-temp-io` / `13-hashagg-disk` / `hash-batches`: spill cu
    `work_mem`, `nested-loop-subplan`: `heap-fetches`, `12-settings`: setarea oprită) și cele
    sănătoase fără probleme; `db-sqlite`: un fișier temporar real → `sqlite-scan` cu indexul sugerat
    și locul lui, `auto-index`, `stale-stats` fără `sqlite_stat1`, faptele citite pe conexiune;
    modulul (toate cheile textelor în ambele limbi, cu argumente doar dintre dovezi; cifrele scrise;
    faptele adunate și oprite; Explain Analyze cu problemele în raport, numărate și scrise în Output
    cu SQL-ul, indexul necreat; rulările numărate după amprentă și `n-plus-one`).
  - **Capturi:** niciuna nouă; toate cele vechi identice.
  - **Abateri de la plan, cu motivul:**
    - dovezile sunt `Evidence::{Quantity, Text}` (numele tabelului, al coloanei, al pasului sunt și
      ele argumente ale textelor), iar `Finding::variant` alege textele unei variante (`why_<v>`,
      `fix_<v>`: de ce nu e folosit un index existent, cheia virtuală, `LIMIT`-ul SQLite…);
    - un index se sugerează doar când indexurile tabelului se știu și niciunul nu începe deja cu
      acele coloane (fără fapte constatarea rămâne, fără SQL): „nu ghicim un index care poate
      există”; o coloană text pe care Postgres o scrie `(col)::text` contează ca neconvertită;
    - `misestimate` se spune doar unde începe (un copil care greșește la fel e cauza), nu și sub un
      `LIMIT` când vin mai puține; `sqlite-scan` tace pentru tabele sub 1 000 de rânduri citite (sau
      după statistici); `disabled-node` spune și un `enable_*` oprit fără nod dezactivat; `cold-cache`
      citește blocurile rădăcinilor (la Postgres includ copiii: suma nodurilor le-ar număra de
      două ori);
    - `RewriteHint::UnwrapColumn` (o funcție de mai multe argumente), `IndexSpec::descending`,
      `Suggestion::CreateIndex::{method, concurrently}`, `VACUUM (ANALYZE)` (SQLite n-are `VACUUM`
      pe tabel: niciuna);
    - `RuleContext` mai ține producția, harta, dialectul și condițiile citite; punctul fierbinte nu
      e numărat ca problemă;
    - în Output, pe lângă număr, câte un rând pe problemă și SQL-ul ei, cât lipsește panoul 39h4.
  - **Ce n-a intrat și de ce:** panoul constatărilor, „Copiază” / „Inserează în consolă”, „Ignoră
    în această consolă” și evidențierea în consolă sunt ale 39h4; `table_stats` la Postgres și
    `pg_stat_statements` sunt ale 39h6, deci `dead-tuples`, `stale-stats` (la Postgres) și latura
    serverului din `n-plus-one` sunt gata, testate pe date construite, dar primesc date reale abia
    atunci.
- **39h4 — făcut** (vederea profilului):
  - **`ui-components`**: `severity` (`Level` Info / Warning / Critical cu iconițele `info`,
    `alert`, `octagon-alert`; `Palette::severity` din Okabe–Ito, nuanțe pentru tema închisă și
    cea deschisă, contrastul ≥ 3:1 verificat; `Palette::heat`, o singură nuanță caldă cu
    intensitatea crescătoare; `luminance` / `contrast`); `tree_table` extins, nu refăcut: `Ink`
    (ton, severitate sau căldură), `SpanStyle::{Ink, Pill}`, `TableCell::{lead, outline}` (iconița
    din față, conturul cifrei vinovate), `TableRow::{sign_tip, folded_note}` (indiciul semnului din
    gutter, ce ascunde un rând închis), `set_filter` (potrivirile și părinții lor, oricum ar fi
    închise; `Memo` pe generații); `Viewer::{actions_view, crumbs}`, `ToolPanel::keyed_view`,
    `ServiceId::link` / `TAB_LINK`; iconițele `flame`, `octagon-alert`, `hard-drive`, `lightbulb`,
    `route`, `panel-right`.
  - **Aplicația**: o vedere dintr-un panou lateral (un tab din „Services”, apăsat) primește
    tastele acțiunilor ei (`key_places`: felul ei, apoi al panoului); breadcrumb-ul unei vederi
    vine din `Viewer::crumbs`. **„Services”**: `keyed_view`, legăturile `service-tab:<id>` (rândul
    din Output deschide tab-ul profilului), `TabCommand`, tastele simple trimise vederii.
  - **Modulul** (`profile/`): `view.rs` (`ProfileView`, în „Services” și în tab-ul `view-profile`,
    peste același `Arc<ProfileReport>`): bara (comutatorul vederilor — Arbore, cu Diagramă,
    Flacără, Tabele arătate dar încă nefăcute, cu indiciul lor; meniu sub 560 px —, filtrul pașilor,
    Expand / Collapse, „Deschide în tab”, Export ▾, sertarul panoului, insignele `ANALYZE` /
    `EXPLAIN`, „anulat”, motorul și versiunea, instrucțiunea; ce nu încape intră în „…”),
    rezumatul (`summary.rs`: pastilele planificare, execuție, total, aducere, rânduri, buffere din
    cache / citite, temp, JIT, lucrători, contorii SQLite, cu valoarea exactă în indiciu;
    constatările pe severitate, care filtrează lista la click), corpul (`layout.rs`: panoul
    alături de la 820 px, cu splitter și lățimea ținută în sesiunea proiectului; sertar sub 820
    px; sub 520 px constatările deasupra, ca listă strânsă); `tree.rs` (coloanele din
    `Metric::present` în ordinea fixă, lăsate afară după importanță când nu încap; timpul propriu
    ca bară de căldură „412 ms · 48 %”, costul propriu la un plan estimat; rândurile estimat →
    real cu factorul `×153 ↑` într-o pastilă de la ×10 — nu sub un `LIMIT` care întoarce mai
    puține; buferele în două părți; spill-ul cu iconița discului; lucrătorii porniți / planificați;
    ramura fierbinte marcată; semnul severității în gutter, nuanța rândului și cifra vinovată
    conturată, punctul fierbinte cu flacăra; peste 300 de noduri, cele adânci și mici închise cu
    „+N pași · x %”, niciodată ramura fierbinte sau problemele); `findings.rs` (o carte pe
    constatare: iconița și culoarea, titlul, partea din timp, de ce cu cifrele, legătura spre pas,
    ce se poate face, SQL-ul colorat și împărțit pe rânduri, „Copiază”, „Inserează în consolă”,
    „Ignoră în această consolă”, „Arată N ignorate”; „Nimic de semnalat” cu ce s-a verificat, iar
    la un plan estimat „Rulează Explain Analyze”; așezarea `Memo` pe lățime); `details.rs`
    (operația, obiectul, condițiile ca SQL, estimat față de real, I/O, memorie și disc,
    paralelism, WAL, ieșirea, „Specific PostgreSQL / SQLite” cu fiecare `Extra` pe grupuri,
    problemele pasului; fără pas ales, ale instrucțiunii: motorul, timpii, JIT, trigger-ele,
    setările, contorii, notele; fiecare valoare copiată la click, valoarea brută în indiciu);
    `export.rs` (text ca `EXPLAIN`, JSON-ul motorului formatat cu `serde_json`, JSON-ul nostru
    salvat prin dialogul „Save As” în fundal, Markdown cu rezumatul, constatările și SQL-ul);
    `figures.rs`; `sample.rs` (profilul Postgres 17 construit de mână al capturilor și testelor).
  - **Click pe o constatare** (sau F2 / ⇧F2): pasul ales și adus în vedere, `Runner::set_focus`
    (`FocusMark`: intervalul instrucțiunii, hash-ul textului ei, locul constatării); bara consolei
    îl evidențiază prin `EditorToolbar::highlight` cât textul instrucțiunii e același, iar la o
    editare îl lasă (și o rulare nouă). **„Inserează în consolă”**: comanda
    `database.profile.insert` deschide consola, adaugă SQL-ul la capăt după un rând gol și îl
    selectează — nu rulează nimic. **„Ignoră”**: regulile ignorate stau pe runner, în memorie,
    văzute de toate vederile consolei.
  - **Comenzile** (`ViewAction` pe `view-profile`, remapabile): `database.profile.next_problem`
    (F2), `prev_problem` (⇧F2), `hottest` (H), `expand_all`, `collapse_all`, `copy_text`, `export`,
    `open_in_tab`, `view_tree`; interne: `database.profile.open`, `database.profile.insert`. Tab-ul
    „Plan” e acum „Profil · tabel” (`PlanSet` ține `report`, `table`, `document_hash`), iar
    Output-ul are un singur rând de rezumat cu problemele numărate și legătura spre tab (fără câte
    un rând pe problemă, ca la 39h3). Tab-ul de editor: `profil [sursă] · tabel`, culoarea sursei,
    breadcrumb-ul `Console de baze de date › sursă › consolă › profil`.
  - **Opțiunile din ⚙** (meniul barei consolei, „Profilare”: BUFFERS, timpii, VERBOSE,
    statisticile) și **blocul „Profilare”** din setări (`ProfileSettings`, după „Console de
    interogări”): cele 7 setări `console_profile_*`.
  - **Teste**: `ui-components` (severitățile pe ambele fundaluri ≥ 3:1, rampa crescătoare,
    filtrul arborelui, semnul cu indiciu, conturul, nuanța, nota rândului închis); modulul
    (coloanele după `present` și ordinea lor, planul SQLite estimat doar cu operația; ramura
    fierbinte, semnul, nuanța și cifra conturată, bara de căldură, factorul și lipsa lui sub
    `LIMIT`, spill-ul, lucrătorii; coloanele lăsate după importanță; planurile mari închise cu
    ramura fierbinte deschisă; așezarea pe lățimi și limitele panoului; detaliile doar cu ce au,
    `extra` pe grupuri; exportul în patru feluri; cărțile așezate o dată pe lățime și împărțite
    când e îngust; F2 / ⇧F2 circular cu nodul și locul din consolă, H, filtrul tastat și Esc, `*`,
    ignorarea văzută de altă vedere și reluarea, filtrul severității, aspectul la 1400 / 700 / 450
    px și sertarul, „Inserează” care nu rulează și selectează exact SQL-ul, exportul și scrierea în
    fundal, „Deschide în tab” cu breadcrumb-ul lui, tab-ul închis, profilul SQLite real ca tab;
    bara: locul evidențiat cât textul e același și lăsat la editare, meniul ⚙ cu opțiunile; blocul
    de setări); „Services” (tastele vederii, `TabCommand`, legătura spre alt tab); aplicația (tab-ul
    `view-profile` cu breadcrumb-ul lui, F2 / ⇧F2 / H acolo și nu în editor).
  - **Capturi noi** (ambele teme): `services-profile`, `services-profile-narrow` (lista strânsă
    deasupra), `profile-tab`, `profile-finding-selected` (prima problemă aleasă, locul ei luminat
    în consolă), `profile-sqlite` (Explain Analyze real pe baza de probă). `services-plan` e
    înlocuită de `services-profile`; schimbată, cum era de așteptat: `settings-caret` (blocul
    „Profilare”).
  - **Abateri de la plan, cu motivul:**
    - Diagrama, flacăra și tabelele (39h5–39h6) apar în comutator, estompate, cu indiciul „vine
      într-o versiune următoare”, ca locul lor să existe; Compară ▾ lipsește până la 39h6;
    - Export ▾, „…” și comutatorul îngust sunt meniuri desenate de vedere (o vedere nu poate deschide
      un meniu al aplicației la click stâng);
    - lățimea panoului e ținută pe proiect în memorie (`Consoles`), cât e deschis proiectul, nu în
      fișierul sesiunii;
    - „Ignoră” e un buton cu iconiță (`eye-off`) și indiciu; un tab de editor redeschis dintr-o
      sesiune salvată nu are profil (profilele nu se scriu pe disc);
    - Markdown-ul îl face modulul (textele constatărilor sunt în locale), nu
      `database::profile::export`;
    - capturile profilului Postgres folosesc profilul construit de mână peste consola sursei de
      probă SQLite (fără server), cum spune planul.
  - **Ce n-a intrat și de ce:** diagrama și flacăra (39h5), compararea și tabelele (39h6), cum
    împart pașii.
- **39h5 — făcut** (diagrama și flacăra):
  - **`ui-components`**: `diagram::tidy` (nou, `diagram/tidy.rs`): `tidy(children, roots, sizes,
    Flow, Gaps)` așază o pădure cu mărimile date ale cărților, de sus în jos (`Flow::Down`) sau de
    la stânga la dreapta (`Flow::Right`), părintele centrat peste copii, nivelurile aliniate (cât
    cea mai adâncă carte a nivelului), subarborii strânși după contururi, fără suprapuneri, în
    O(n) (conturul fiecărui subarbore ținut de la cel mai adânc nivel, cu decalaj leneș: o unire
    costă cât cel mai scurt); `links` dă muchiile ortogonale ca `Edge`, de la copil spre părinte,
    fiecare copil cu banda lui pe latura părintelui. `EdgeStyle::width` (nou; `EdgeStyle::LINE` =
    1,5, pus de toate apelurile existente, deci diagrama relațiilor și cea a claselor rămân
    identice): capul săgeții crește cu o linie groasă, colțurile ei umplute. `icicle` (nou):
    `IcicleNode { parent, value, self_value, label, detail, tone, mark, sign, tip }`, `layout`
    (pur: lățimea după valoarea inclusivă în părinte, copiii care depășesc părintele scalați în
    el și marcați `approx`, vecinii sub 2 px uniți într-o bară „…” și neurmați în jos), `Icicle`
    (așezarea `Memo` pe noduri, lățime și mărire; click alege și mărește subarborele, banda de sus
    cu ← și breadcrumb-ul drumului, fiecare pas apăsabil, legenda cu rampa de căldură, axa de
    timp cu pași „frumoși” 1 / 2 / 5; tastele ↑ ↓ ← → ↵ ⌫ / Esc Home; indiciul exact, al barei
    unite cu numărul și valoarea; textul barei negru sau alb după contrast; doar rândurile în
    vedere desenate). `TreeTable::tree_width` (cât cere coloana arborelui ca rândurile arătate
    să-și păstreze cuvintele, `Memo` pe generație). `EditorToolbar::focus` (locul arătat, cu
    severitatea lui).
  - **Modulul**: `graph.rs` (`GraphView` pe `diagram::Viewport`): o carte pe pas — dunga de sus
    în căldura timpului propriu, iconița felului, cuvintele motorului (îngroșate pe ramura
    fierbinte), semnul problemei în colț (marginea în culoarea severității, fundalul ușor
    nuanțat) sau flacăra punctului fierbinte; rândurile: obiectul (sau indexul, condiția,
    subplanul), timpul propriu / total cu pastila părții în căldură, rândurile estimat → real cu
    factorul de la ×10, costul și buclele; muchiile copil → părinte cu grosimea
    `edge_width(rows) = 1 + log10(rows)`, între 1 și 8 px, eticheta rândurilor pe o pastilă
    așezată fără suprapuneri (`place_labels`), ramura cea mai scumpă în accent; ce e strâns în
    arbore e strâns și aici, ca o carte „+N pași · x %” (click o deschide; dublu click pe o carte
    strânge / deschide pasul, același cu arborele); filtrul arborelui estompează ce nu se
    potrivește; grila de puncte a pânzei; controalele plutitoare (−, %, +, potrivește, centrează
    pe pasul ales, sensul așezării) și legenda (grosime = rânduri, ramura scumpă, rampa); pan cu
    tragerea și rotița, zoom cu + / − / 0 și ⌘⌥ + rotița în tab-ul de editor
    (`Viewer::zoom`); doar cărțile din vedere desenate, așezarea `Memo` pe profil, generația
    arborelui și sens. `flame.rs` (`FlameView` pe `Icicle`): o bară pe pas, lățimea = timpul
    inclusiv (costul la un plan estimat), culoarea din rampa de căldură după partea proprie,
    problemele conturate în culoarea severității cu semnul lor, indiciul cu cifrele exacte
    (același cu al cărții). **Selecția comună**: `ProfileView::select_node` — o carte sau o bară
    apăsată e rândul arborelui (părinții deschiși), constatarea pasului aleasă (locul ei luminat în
    consolă), iar pasul ales rămâne cel apăsat chiar dacă constatarea începe la altul; F2 / ⇧F2 /
    H în diagramă aduc cartea în vedere, flacăra iese din mărire dacă pasul e în afara ei.
    Comutatorul are acum Arbore, Diagramă, Flacără (Tabele rămâne estompat până la 39h6), comenzile
    noi `database.profile.view_graph` / `view_flame`, meniul de click dreapta al fiecărei vederi
    (potrivește, centrează, sensul; mărește, înapoi, toți pașii).
  - **Reparate din 39h4**: (1) coloana operației nu mai e tăiată: `tree::fitting` dă întâi locul
    cuvintelor (cât cere `tree_width`, între 230 și 460 px), apoi îngustează cifrele
    (`Col::compact_width`) și abia apoi lasă afară coloanele mai puțin importante; numele întreg e
    și în indiciu. (2) locul unei constatări în consolă se vede limpede în ambele teme: textul
    nuanțat în culoarea severității (22 %) cu un contur de 1 px, rândurile lui ușor nuanțate, ca
    evidențierile de căutare ale editorului (`Decorations::focus`, separat de evidențierea lui Run).
    (3) antetul arborelui și capetele panoului („Ce se poate optimiza”, „Detalii”) au aceeași
    înălțime (`tree_table::HEADER_H`, o singură constantă, și pentru banda flăcării), deci linia
    lor trece continuu peste splitter, lat și în sertar; `TreeTable::header_rect` și
    `ProfileView::panel_heads` o fac verificabilă în test.
  - **Teste**: `tidy` (copiii sub părinte, centrat, nivelurile aliniate, contururile, pădurea,
    muchiile spre părinte pe benzi, fără suprapuneri pe 300 de arbori aleatori cu `proptest` și
    sămânța fixă 395, în ambele sensuri; 10 000 de noduri sub 10 ms în release); `icicle`
    (proporțiile, partea proprie, scalarea în părinte cu `approx`, unirea sub 2 px și neurmarea,
    mărirea și înapoi, selecția din afara măririi, tastele, click și breadcrumb, indiciul barei
    unite, 10 000 de noduri și `Memo`); modulul (`edge_width`, o carte și o muchie spre părinte pe
    pas în ambele sensuri fără suprapuneri, grosimea și eticheta după rânduri, ramura fierbinte,
    cartea „+N”, indiciul cu cifrele și constatarea, săgețile după sens; barele flăcării după
    timpi, cost și probleme, făcute o dată; aceeași selecție în cele trei vederi — click pe carte,
    tastele în flacără, F2 în diagramă cu cartea în vedere —, strângerea comună cu arborele, zoom-ul;
    coloanele care dau prioritate arborelui; liniile anteturilor la același y, lat și în sertar;
    bara consolei cu `focus` separat de `highlight`).
  - **Capturi noi** (ambele teme): `services-profile-graph` (diagrama, prima problemă aleasă,
    așezată de la stânga la dreapta), `services-profile-flame` (flacăra, pasul cel mai fierbinte
    ales). Schimbate, cum era de așteptat: `services-profile`, `services-profile-narrow`,
    `profile-tab`, `profile-sqlite` (comutatorul cu Diagramă și Flacără active, coloana operației
    întreagă), `profile-finding-selected` (și locul luminat în consolă).
  - **Abateri de la plan, cu motivul:**
    - `tidy` e Reingold–Tilford cu contururi (O(n)), nu Buchheim / Walker complet: aceleași reguli
      (fără suprapuneri, părintele centrat, ordinea copiilor), fără distribuirea uniformă a
      subarborilor mici dintre doi mari — la planuri (lanțuri cu puține ramificări) nu se vede;
    - sensul diagramei e ales singur (cel care arată mai mult în spațiul dat: de la stânga la
      dreapta într-un panou lat și scund, ca „Services”), cu buton să-l schimbi; potrivirea nu
      coboară sub 75 % (textul rămâne citibil) și centrează pasul ales, ținând diagrama în vedere;
    - un click pe flacără alege pasul și îl și mărește (ca în uneltele de flame graph), iar
      zoom-ul diagramei din „Services” e din controalele ei și din taste (panourile laterale nu
      primesc ⌘⌥ + rotița);
    - în diagramă, dublu click strânge / deschide pasul (comun cu arborele).
  - **Ce n-a intrat și de ce:** tabelele și compararea sunt ale 39h6, cum împart pașii.
- **Reparat după 39h5 — consolele își urmează fișierul** (redenumit din tab, „Rename File…” / ⇧F6):
  - **Cauza:** aplicația muta doar calea tab-ului (`Document::set_path`); bara consolei își ținea
    numele din `ConsoleSpec` (citit la deschidere), pe care nimeni nu-l schimba, iar modulul nu
    asculta `FileRenamed` — titlul, breadcrumb-ul, nodul din „Services” și `consoles.toml` rămâneau
    pe numele vechi, iar la următoarea listare fișierul nou ar fi intrat ca o consolă nouă (alt
    `ConsoleId`, pierzând schema, Tx, parametrii). Un panou lateral nu era anunțat de calea nouă
    (cheia lui nu o cuprindea). Watcher-ul ignora tot folderul proiectului (`.lumen`), deci și
    consolele redenumite de alt program.
  - **Ce s-a făcut:** `Consoles::follow` ascultă pe magistrala proiectului `FileRenamed` (oricine
    l-a făcut: tab, arbore, refactorizare), `EntriesDeleted` și `EntriesChanged`: în folderul sursei
    e aceeași consolă (același id, setările, runner-ul cu rezultatele și istoricul, `Runner::renamed`
    cu fișierul și numele noi); în folderul altei surse devine a ei (baza și schema lăsate, vechea
    conexiune închisă); scoasă din `consoles/` e un `.sql` obișnuit (iese din listă, tranzacția
    anulată și spusă, utilizatorul anunțat — atașarea la o sursă e 39n); ștearsă, iese din listă.
    Schimbările de pe disc ale altui program: un fișier de consolă dispărut cu un `.sql` nou lângă
    el (sau, singur, oriunde) e o redenumire, anunțată ca `FileRenamed`; dispărut fără pereche e o
    ștergere, anunțată ca `EntriesDeleted` (tab-ul salvat se închide). `ProjectService::own_dirs`
    și `WatchedDirs` lasă watcher-ul să audă `consoles/` din folderul proiectului. În aplicație,
    `follow_move` (tab, arbore, refactorizare, disc) mută toate tab-urile fișierului, îi spune
    vederii (`Viewer::moved`) și barei (`EditorToolbar::moved`: o urmează, altfel providerii sunt
    întrebați din nou — un `.sql` mutat în folderul unei console primește bara), apoi reîmprospătează
    panourile laterale și breadcrumb-ul. Vederea de date a unui `.sqlite` / `.csv` redenumit:
    `Sessions::file_moved` (sesiunea redeschisă la calea nouă, fișierul deschis singur cunoscut după
    ea, sursa configurată pe el cu fișierul — și numele, când era al fișierului — schimbat,
    layout-urile grilelor păstrate), `DataView::moved` (titlul, citirea și urmărirea discului ale
    fișierului nou). Diagrama unui fișier își urmează fișierul (tabelele deschise din el, numele).
  - **Teste:** modulul (redenumit: același id, schemă, Tx, parametri, runner, lista scrisă; spus
    întâi de bară; mutat la altă sursă; scos din folder cu tranzacția anulată și cele două mesaje;
    șters; redenumirea / ștergerea / mutarea altui program, perechi ambigue ca ștergeri, fișierul
    listei ignorat; nodul din „Services”); explorer (`file_moved`: sesiunea redeschisă la calea nouă,
    lease-ul, layout-urile, sursele configurate și numele lor; vederea unui CSV redenumit; diagrama);
    `WatchedDirs::internal`; aplicația (Rename File… pe o consolă: calea, titlul, breadcrumb-ul,
    același `ConsoleId` cu schema și Tx, același runner, „Select in Project View” și butonul de
    localizare al panoului; redenumirea, scoaterea și readucerea de pe disc / din arbore, ștergerea
    de pe disc; vederea de date a unui SQLite redenumit și sursa ei). Nicio captură nouă; toate
    identice.
  - **Abateri:** titlul tab-ului unei diagrame e cel dat la deschidere (de obicei numele tabelei);
    antetul ei și deschiderea tabelelor urmează fișierul. Mai multe console dispărute și apărute în
    același folder deodată, pe disc, nu pot fi împerecheate sigur: sunt ștergeri, iar fișierele noi
    intră ca console noi.
- **39h6 — făcut** (compararea, tabelele și statisticile pe instrucțiune):
  - **Domeniul** (`database::profile`, tot doar pe `kernel`):
    - **`compare::diff(înainte, după) -> ProfileDiff`**: pașii potriviți după locul lor în arbore
      — felul și obiectul (`similarity`: același obiect cântărește cel mai mult, apoi același fel,
      apoi aceeași familie: scanări, joinuri, agregări, sortări, `Gather`, `Append`) —, copiii a doi
      pași potriviți aliniați în ordine (LCS cu ponderi); copiii inversați (laturile unui hash join)
      se găsesc și așa; un pas care doar învelește (un `Gather` adăugat, un `Hash` devenit `Sort`)
      e văzut prin el, până la 4 niveluri, candidatul cu cea mai mare potrivire luat primul. Un
      `Seq Scan on orders` devenit `Index Scan on orders` e `Change::Changed`, nu șters și adăugat;
      ce rămâne e `Added` / `Removed` cu tot subarborele. Pe fiecare rând `Delta` (timpul în toate
      buclele, cel propriu, partea, rândurile, bufferele, citirile, costul), pe instrucțiune
      (`StatementDiff`: timpul, planificarea, ceasul runner-ului, rândurile, bufferele din cache și
      citite, temp, costul), `Delta::trend` (mai bine / mai rău peste `NOISE` = 5 %), `FindingsDiff`
      (rezolvate, noi, rămase, după regulă și tabel), `shape_changed`. Peste 40 000 de celule (mii de
      partiții) potrivirea e lacomă după fel și obiect, apoi după obiect.
    - **`tables::usage(raport, sql_map) -> Vec<TableUsage>`**: un rând pe tabel (aliasurile, același
      tabel de două ori o singură dată), `TableScan` pe fiecare pas (`Access::{Scan, Index,
      IndexOnly, Bitmap, RowId, AutoIndex, Write}`, indexul — al unui bitmap din scanările lui de
      index —, aliasul), rândurile citite (păstrate + eliminate de filtru / recheck) și întoarse în
      toate buclele, timpul propriu și partea (costul la un plan estimat), constatările lui (după
      noduri sau după tabelul din dovezi), cel mai greu primul.
    - Regula nouă **`no-statement-stats`** (Info, a 29-a): nota nouă `ProfileNote::NoStatementStats`
      (serverul nu ține istoricul, `pg_stat_statements` lipsă sau neîncărcată), cu ce înseamnă
      instalarea (`shared_preload_libraries`, repornirea, `CREATE EXTENSION`), fără SQL: nu
      instalăm nimic.
    - `ConsoleAccess::open_structure` (implicit `None`; explorer-ul îl are, ca `open_table`).
  - **PostgreSQL** (`db-postgres/src/stats.rs`, pur, testat fără server):
    - `tables_sql(version, n)`: o singură interogare pentru toate tabelele profilului (`VALUES ($1,
      $2), …`, schema goală = cea vizibilă), `pg_class` ⋈ `pg_stat_user_tables`: rândurile vii și
      moarte, `reltuples` / `relpages`, scanările, ultimul (auto)analyze și (auto)vacuum,
      `n_mod_since_analyze` de la 9.4, `n_ins_since_vacuum` de la 13, `last_seq_scan` /
      `last_idx_scan` de la 16 (în `extra`), mărimile (`pg_relation_size`, `pg_indexes_size`,
      `pg_total_relation_size`); `indexes_sql`: `pg_index` ⋈ `pg_stat_user_indexes` (coloanele din
      `pg_get_indexdef` pe `indnkeyatts` de la 11, `indnatts` înainte; unic, primar, mărime, scanări,
      predicatul, `indisvalid`); `history_sql` (`pg_stat_statements` după `queryid`, doar de la 14,
      agregat pe baza curentă, coloanele pe versiuni: `*_exec_time` de la 13, `wal_bytes` de la 13);
      parserele rândurilor (`table_row`, `index_row`, `assemble`, `history_row`).
    - Sesiunea: `table_stats` (două interogări; în tranzacția utilizatorului sub `SAVEPOINT
      console_stats`, ca o interogare auxiliară căzută să nu-i strice tranzacția). `profile` cu
      `statement_history` pe 14+: extensia căutată o dată pe sesiune; dacă e, `VERBOSE` adăugat doar
      pentru `Query Identifier` (ieșirile pașilor șterse când `VERBOSE` nu e cerut), istoricul citit
      după `queryid` în `statement.history`; dacă nu e, nota `NoStatementStats`. `caps`:
      `statement_history` de la 14, `table_stats` mereu. Nu rulăm niciodată `CREATE EXTENSION`.
  - **SQLite**: `table_stats` exista (39h2); vederea arată `sqlite_stat1` prezent / lipsă, mărimea
    paginii și a fișierului în indiciu.
  - **Modulul `database-console`**:
    - `StoredProfile` ține și faptele (`facts`, cu statisticile citite mai târziu), amprenta, ora și
      durata; `PlanSet::facts`, `ProfileShown::{facts, fingerprint}`; `Runner::profile_list`
      (`ProfileBrief`), `profile_facts`, `load_table_stats` (o dată pe profil, în fundal, pe
      conexiunea consolei, doar cu interogările auxiliare permise și un driver care le are).
    - **„Compară ▾”** în bara profilului (iconiță singură când comutatorul e meniu): profilele
      ținute ale consolei, cele ale aceleiași interogări (amprenta) primele, „Oprește compararea”.
      **`profile/compare.rs`** (`CompareView`): capul (cele două profile cu ora și durata, „845,1 ms
      → 12,4 ms” cu pastila „−98,5 % mai rapid”, planificarea, rândurile, citirile, cache-ul, temp,
      „Forma planului s-a schimbat” / „Același plan”, inversează, închide), arborele comun al celor
      două planuri pe rânduri (pasul — un pas schimbat spune ce a fost: `Parallel Seq Scan →
      Index Scan …` —, înainte, după, schimbarea, rândurile și bufferele înainte → după; semnul din
      gutter pentru schimbat / adăugat / scos, rândurile mult mai bune sau mai rele nuanțate) și, cu
      loc, constatările comparate alături (rezolvate, noi, rămase; una a profilului arătat se alege
      la click). Mai bine albastru cu săgeata în jos, mai rău portocaliu cu săgeata în sus
      (Okabe–Ito, niciodată doar culoarea). Un rând ales e pasul profilului arătat; Esc sau altă
      vedere ies din comparare; „Copiază compararea” ca text.
    - **„Tabele”** în comutator (acum activ): **`profile/tables.rs`** — un rând pe tabel (aliasurile,
      cum a fost citit ca pastile, indexurile folosite, rândurile citite → păstrate, timpul ca bară
      de căldură, problemele cu semnul celei mai grave și rândul nuanțat) și, din statisticile
      serverului, rândurile vii și moarte (partea moartă pe pastilă, portocalie de la 20 %),
      mărimea tabelului și a indexurilor, ultimul analyze (sau „Niciodată”, la SQLite `sqlite_stat1`
      da / „Fără statistici”), ultimul vacuum, parcurgerile și scanările prin index; indexurile sunt
      copiii rândului (unic, „folosit aici”, „nefolosit niciodată” la `idx_scan = 0`, mărimea,
      scanările). Coloanele apar doar dacă au date. Statisticile lipsă se cer leneș la deschiderea
      vederii; jos se spune că se citesc sau că nu sunt disponibile. Un tabel ales alege primul lui
      pas (și constatarea lui) în celelalte vederi și invers; meniul: „Deschide tabelul”,
      „Structura”, copiază rândul.
    - Rezumatul are pastila **„Pe server”** (`pg_stat_statements`: media × apelurile; în indiciu
      cea mai rapidă / lentă, abaterea, rândurile, partea din cache).
    - Comenzile noi `database.profile.view_tables` și `database.profile.compare` (remapabile).
    - `Runtime::frozen` (doar sub `snapshot` / teste) și `sample::freeze_times`: timpii măsurați de
      motor (planificare, execuție, primul rând, timpul fiecărui pas — partea lui după rândurile
      subarborelui) fixați, deci **`profile-sqlite` e acum aceeași la fiecare rulare**.
    - `sample`: `report_with` (și cu statisticile tabelelor), `indexed` (aceeași interogare după un
      index pe `orders (status, created_at)`: index scan și nested loop, 12,4 ms), `show_compare`,
      `show_tables`.
  - **Locale** în ambele limbi (`profile.compare.*`, `profile.tables.*`, pastila istoricului,
    constatarea `no-statement-stats`, nota, comenzile).
  - **Teste**: `diff` (același plan, `SeqScan` → `Index Scan` schimbat, pași adăugați / scoși cu
    subarborii, un `Gather` adăugat deasupra și un `Hash` devenit `Sort`, ordinea copiilor
    inversată, `Delta::trend` și zgomotul, constatările rezolvate / noi / rămase, planuri estimate
    pe cost, 5 000 de partiții repede); `usage` (aliasuri, același tabel de două ori, SQLite cu
    aliasul, bitmap cu indexul lui, o scriere); `no-statement-stats`; `db-postgres` (`stats.rs` pe
    fiecare versiune 9.2–18, parserele, testul pe server real extins, doar cu
    `EDITOR_TEST_POSTGRES_URL`); modulul (tabelele cu statistici, fără ele, indexul „folosit aici”,
    SQLite `sqlite_stat1` prezent / lipsă; compararea înainte / după index; vederea: tabelul ales ↔
    pasul ales, meniul; statisticile citite leneș o singură dată pe un SQLite real; „Compară ▾”,
    rândul ales, copierea, inversarea, Esc, altă vedere).
  - **Capturi noi** (ambele teme): `services-profile-compare`, `services-profile-tables`.
    Schimbată, cum era cerut: `profile-sqlite` (timpii fixați; acum stabilă). Schimbate de bara
    nouă (butonul „Compară ▾” și „Tabele” activ în comutator): `services-profile`,
    `services-profile-narrow`, `profile-tab`, `profile-finding-selected`, `services-profile-graph`,
    `services-profile-flame`.
  - **Abateri de la plan, cu motivul:**
    - compararea e un singur arbore cu coloanele „înainte” și „după” alăturate pe același rând (nu
      două tabele separate): alinierea pe rând rămâne exactă la derulare și la deschiderea /
      închiderea pașilor;
    - ▼ / ▲ sunt iconițele `arrow-down` / `arrow-up` (fontul interfeței nu le are sigur);
    - compararea și tabelele folosesc și locul panoului când e alături (tabelele lor sunt late);
      constatările comparate stau într-o coloană a comparării; detaliile pasului rămân în celelalte
      vederi; în sertar panoul se deschide și peste tabele;
    - mai vechiul dintre cele două profile e „înainte”, oricare ar fi cel arătat (se poate inversa);
    - istoricul `pg_stat_statements` se potrivește doar de la 14 (`Query Identifier`), cu `VERBOSE`
      adăugat doar pentru el; extensia căutată o dată pe sesiune;
    - lipsa extensiei e o constatare `Info` (regula nouă), cum cere planul, deci numărată ca
      problemă (se poate ignora în consolă);
    - statisticile tabelelor sunt citite deja de analiză când interogările auxiliare sunt permise;
      cererea leneșă de la deschiderea vederii acoperă profilele fără ele.
  - **Ce n-a intrat și de ce:** nimic din pas; o captură a `pg_stat_statements` pe server real nu
    există (testele nu pornesc servere).
- **39i — făcut** (istoricul și consolele păstrate):
  - **Domeniul** (`database::history`, nou, doar pe `kernel`): `HistoryId` (UUID v4), `Outcome`
    (`ok` / `error` / `cancelled`), `HistoryEntry` (sursa, consola care a rulat-o ultima, baza și
    schema, textul **așa cum e scris** — șablonul: parametrii rămân `:id` / `$1` / `?`, numele lor
    în `params`, niciodată o valoare —, cu secretele mascate prin `sql::mask_secrets`, ora dată de
    modul, durata, rândurile, cum s-a încheiat; `key`: textul cu spațiile strânse și `;` final
    scos), `QueryHistory` (aceeași instrucțiune rulată din nou urcă sus păstrându-și identitatea,
    cel mult `limit` pe sursă, cele vechi ies; `from_blocks` citește blocurile fișierului în ordine,
    ultimul bloc al unei instrucțiuni câștigă; `record` → `Write::Append` sau `Write::Compact`;
    `remove`, `clear`, `search`), `needs_compaction` (fișierul se rescrie abia peste limită + un
    sfert, cel puțin 16 blocuri — nu la fiecare rulare odată plin), `Search` (fiecare cuvânt,
    orice majuscule, și locurile de luminat), portul `HistoryStore` cu `NoHistory` și
    `MemoryHistory` (testele, capturile). `console::check_name` / `NameProblem` / `NAME_MAX`
    (numele unei console ca nume de fișier: gol, caractere interzise pe un sistem, punct la început
    sau la sfârșit, numele rezervate de Windows, lungimea, luat de altă consolă a sursei în orice
    majuscule; `.sql` tastat e scos). `JumpRequest::list` („Database Consoles…”: lista chiar fără
    console).
  - **`storage::TomlQueryHistory`**: `<dosarul proiectului>/database/history/<source-uuid>.toml`;
    fiecare rulare un bloc `[[entry]]` adăugat la capăt (fișierul rămâne TOML valid, nerescris;
    un fișier fără ultimul rând încheiat primește întâi unul); rescris atomic doar la compactare
    sau după o ștergere (gol: fișierul dispare); un fișier stricat e **păstrat deoparte** ca
    `<uuid>.toml.broken` (`.broken-2`… când există deja unul), blocurile care se pot citi luate
    din el și scrise din nou; o intrare fără id sau text e lăsată deoparte, nu fișierul.
  - **Modulul `database-console`**:
    - `history::Histories` (pe proiect, în `Consoles`, în memorie până primește magazinul
      proiectului la `attach`): istoricul unei surse citit o dată — de o rulare pe firul
      runner-ului, de fereastră în fundal —, `record` (înregistrat și scris sub un singur lacăt,
      blocurile în ordinea rulărilor), `entries` (`None` cât se citește, cititorii treziți),
      `remove` / `clear` (fișierul rescris în fundal cu ce ține **atunci**, deci și o rulare
      venită între timp), `purge` („Clear and Rebuild Caches”), un fișier stricat spus o dată
      (toast la deschiderea ferestrei). Setarea nouă `[database] console_history_size` (1000;
      100 / 500 / 1000 / 5000) în blocul „Console de interogări”.
    - **Runner-ul** înregistrează fiecare instrucțiune rulată (și una refuzată de motor, una
      oprită), cu locul conexiunii, durata, rândurile aduse sau schimbate; istoricul din sesiune
      al 39e (lista din meniul 🕘) a plecat. `RunRequest::detached`: o instrucțiune care nu e textul
      consolei (din istoric, rulată din nou) nu pune semne în gutter, nu subliniază și nu leagă
      eroarea de un loc din consolă. `Runtime::with_wall` / `wall_ms` / `time_text` (ora locală;
      UTC când e înghețat, în capturi).
    - **Fereastra istoricului** (`history_view.rs`, modalul `view-console-history`, nu se închide
      la click afară): butonul 🕘 al barei (și „…” › Istoric) rulează comanda nouă
      `database.console.history` (⌥⌘E, și în presetarea JetBrains); câmpul de căutare are
      tastele (câte s-au găsit din câte, în dreapta lui), lista cea mai nouă întâi — semnul
      încheierii (✓ verde, eșuată roșie, oprită galbenă), prima linie colorată ca SQL cu potrivirile
      luminate, dedesubt ora, durata, rândurile, `bază.schemă` —, desenată doar unde se vede (orice
      număr de intrări), instrucțiunea aleasă întreagă dedesubt; ↑ ↓ / PgUp / PgDn, ↵ (sau dublu
      clic, „Inserează”) o pune în consolă după textul ei, selectată, niciodată rulată; „Rulează din
      nou” o rulează așa cum e pe conexiunea consolei (`commands::run_text`: aceleași reguli ca Run —
      doar citire, parametrii ceruți din nou, pericolele și producția confirmate); „Șterge” / ⌦ la
      capătul câmpului o scoate din istoric; meniul rândului (Inserează, Rulează din nou, Copiază,
      Șterge); indiciul unui rând arată instrucțiunea întreagă.
    - **Popover-ul consolelor** (`jump.rs`, `view-console-jump`, `host.open_overlay` ca popover;
      un click afară îl închide — nu e modal, nimic nu se pierde): „Query consoles of …” (⇧⌘F10)
      are un câmp de filtrare deasupra listei, cu tastele în el de la deschidere. `jump::filter`
      (pur): fuzzy după nume (`kernel::fuzzy_match`, cuvintele despărțite de `_ - . spațiu`) și
      după text (prima linie care conține ce s-a tastat, altfel prima în care literele sunt
      apropiate — cel mult de trei ori lungimea căutării, din oricare dintre primele 32 de locuri
      ale primei litere), linia arătată sub nume cu potrivirea luminată, ultima rulare atunci în
      dreapta numelui; numele potrivite înaintea textelor, apoi după scor, apoi în ordinea lor;
      „New Query Console” mereu primul; tastat, prima potrivire e aleasă (niciuna: „New”) și ↵ o
      deschide. Textele: ale tab-urilor deschise (cu modificările nesalvate), celelalte citite din
      fișiere în fundal (cel mult 256 KB fiecare, `TEXT_MAX`), filtrul refăcut doar când se schimbă
      căutarea sau textele; lista e virtualizată (7 rânduri, derulare, rândul ales ținut în vedere).
      Rândul ales sau sub mouse are creionul (Redenumește…) și coșul (Șterge…); și în meniul lui.
      O gazdă fără popover-ele modulelor primește lista ei (`host.choose`), ca înainte.
    - **Redenumirea și ștergerea din UI** („Database Consoles”, `rename.rs`): modalul
      `view-console-rename` (numele selectat, `.sql` după el, problema spusă cum se tastează,
      ↵ / Redenumește, Esc / Anulează); `Consoles::rename` redenumește fișierul și publică
      `FileRenamed` pe magistrala proiectului, deci merge pe **aceeași cale** ca redenumirea din tab
      sau din arbore (d8d4c9d): același `ConsoleId`, aceleași setări și același runner, tab-urile
      urmând fișierul; `Consoles::delete` întreabă întâi (`host.confirm`, periculos; o tranzacție
      deschisă spusă în întrebare), șterge fișierul și publică `EntriesDeleted` (tab-ul salvat se
      închide, consola iese din listă, tranzacția anulată și spusă). Din: popover-ul consolelor,
      meniul unei sesiuni în „Services” (Deschide consola, Redenumește…, Șterge…) și al sursei de
      acolo („Consolele bazei de date…”), plus rândul nou „Consolele bazei de date…” în meniul
      sursei din fereastra „Database” (popover-ul chiar fără console). Comenzi interne:
      `database.console.history_picked`, `jump_picked`, `rename`, `delete`, `renamed`.
  - **`ui-components` / `services-panel`**: `ProjectServices::take_action` (ce cere un rând al
    meniului unui nod aplicației — o comandă cu gazda, un fișier deschis), trimis de panou după
    `command`.
  - **Explorer-ul**: `console::ask_listed`, `DatabaseWindow::console_listed`, rândul
    „Consolele bazei de date…” (doar pe sursă).
  - **Teste**: domeniul (mascarea, numele parametrilor fără valori, dublurile urcate cu
    identitatea lor, limita, compactarea abia peste prag, blocurile citite înapoi cu ultimul
    câștigând, ștergerea, căutarea și locurile ei, `MemoryHistory`; `check_name` pe fiecare
    problemă); `storage` (adăugarea la capăt cu fișierul valid după fiecare bloc și neatins în
    față, un fișier fără ultimul rând încheiat, compactarea citită înapoi identic cu memoria pe 40
    de rulări, un fișier stricat păstrat ca `.broken` / `.broken-2` niciodată scris peste și
    blocurile bune păstrate, nicio parolă și nicio valoare de parametru pe disc, o intrare
    incompletă lăsată deoparte); modulul (`Histories`: citit o dată, rulările adăugate, ștergerea
    rescrisă cu o rulare venită între timp, compactarea peste limită, citirea în fundal și
    `purge`, fișierul stricat spus o dată; runner-ul: fiecare instrucțiune înregistrată cu
    rândurile, schema, consola și încheierea, una refuzată de motor, o rulare `detached` fără
    semne și urcată sus; fereastra: lista, căutarea și alegerea, ↵ care inserează fără să ruleze,
    „Rulează din nou” fără să atingă textul consolei, ștergerea cu ⌦ doar la capătul câmpului,
    citirea în fundal, detaliul rândului, desenul a 500 de intrări doar unde se văd; popover-ul:
    filtrul gol, numele înaintea textelor, linia potrivită luminată, fuzzy în nume și în text
    dar nu risipit, `TEXT_MAX`, tastat → prima potrivire și ↵, nicio potrivire → „New”, textul
    nesalvat al unui tab, 40 de console derulate, „Database Consoles…” fără console, redenumirea
    și ștergerea dintr-un rând, desenul doar al rândurilor văzute și click-ul afară; redenumirea:
    același id, schema și Tx, fișierul mutat și `FileRenamed` anunțat, runner-ul spus, numele
    refuzate fără nicio schimbare, ștergerea întrebată cu tranzacția anulată și `EntriesDeleted`,
    meniul din „Services”; parametrii și parolele niciodată în istoric; bara); aplicația (⌥⌘E în
    consolă, fereastra și popover-ul ca vederi ale modulului cu tastele în câmp, o consolă
    redenumită cu tab-ul urmând fișierul și titlul `daily [shop]`).
  - **Capturi noi** (adăugate în `scripts/ui-snapshots.sh` și `snapshot.rs`, **nerandate încă**:
    pasul s-a făcut în modul rapid, la cererea utilizatorului): `console-history` (`CONSOLE=history`:
    fereastra istoricului unei console, căutată după `orders`), `database-jump-console-filter`
    (`CONSOLE=jump-filter`: popover-ul filtrat după `ord`, nume și texte potrivite), ambele teme.
    De așteptat schimbate: `database-jump-console` (popover-ul modulului, cu câmpul de filtrare, în
    locul listei aplicației), `settings-caret` (rândul nou „Instrucțiuni ținute în istoricul unei
    surse”), capturile cu bara consolei doar dacă indiciul 🕘 apare (nu apare în ele).
  - **Abateri de la plan, cu motivul:**
    - istoricul e pe sursă, în proiect, cum spune planul; fereastra arată istoricul sursei consolei
      (al tuturor consolelor ei), nu doar al consolei;
    - „Rulează din nou” rulează instrucțiunea separat de textul consolei (fără semne în gutter și
      fără sublinieri, care ar fi căzut în alt loc); erorile ei rămân în Output;
    - ⌘↵ nu ajunge la modale (doar ↵, Esc, săgețile și câmpul): rularea din nou e un buton și un rând
      de meniu, ștergerea e ⌦ la capătul câmpului (nu ⌘⌫), redenumirea din popover e creionul
      rândului (F2 nu ajunge la o vedere a unui modul);
    - compactarea nu se face la prima depășire, ci peste limită + un sfert (cel puțin 16): altfel,
      odată plin, fiecare rulare ar fi rescris tot fișierul; în memorie limita e exactă;
    - eroarea motorului nu e păstrată în istoric (doar că a eșuat): mesajul poate conține date ale
      rândurilor;
    - nodurile din „Services” au acum meniu (prin `ProjectServices::take_action`), iar dublu-clicul
      pe o sesiune încă nu deschide consola (rândul „Deschide consola” din meniu o face);
    - „Database Consoles” e popover-ul consolelor cu redenumire / ștergere pe rând (nu o fereastră
      separată).
  - **Ce n-a intrat și de ce:** capturile nu sunt randate și comparate (modul rapid cerut de
    utilizator: doar `cargo fmt`, `cargo build -p app` și testele crate-urilor atinse; capturile se
    randează la sfârșit); `./scripts/check.sh` (clippy pe Windows inclus) nu a rulat din același
    motiv; „Golește istoricul” nu are încă un buton (`Histories::clear` există, fără o confirmare
    în fereastră).
- **39j — făcut** (indexul schemei și completarea):
  - **Domeniul** (`database`): **`sql::scope`** (`scope.rs`, pur, pe tokenii instrucțiunii, cu
    `scope_in` care găsește instrucțiunea cursorului — după `;`, una nouă): cuvântul tastat e
    scos din ce se citește (o „gaură”), deci un nume pe jumătate nu devine tabel și nu înghite
    clauza următoare; `QueryScope` cu `Expect` (`Nothing`, `Keyword(KeywordPlace)`, `Schema`,
    `Table { schema, join }`, `Column { qualifier }`, `JoinCondition { right, left }`,
    `InsertColumns`, `TableColumns`, `UsingColumns`, `Value`), `Clause`, tabelele vizibile
    (`ScopeTable`: `Table`, `Cte` cu coloanele listei sau ale interogării, `Derived` cu coloanele
    listei `SELECT`, `Function` — `generate_series(…) g`; aliasul, nivelul, locul), CTE-urile
    (și cele recursive, care se văd pe ele), aliasurile listei `SELECT` (în `ORDER BY`; în
    `GROUP BY` la PostgreSQL / SQLite / MySQL; în `HAVING` la SQLite / MySQL), `*` chiar înainte
    de cursor (`Star`) și toate tabelele numite (`named`, pentru coloanele cerute leneș). Nivelurile:
    un subselect din `WHERE`, `IN (…)`, `EXISTS (…)` sau din lista `SELECT` își are scope-ul și
    vede tabelele din afară (corelat, cele apropiate întâi); aliasurile dinăuntru nu ies; un tabel
    derivat nu vede afară decât cu `LATERAL` (și atunci doar ce e înaintea lui); fiecare ramură a
    unui `UNION` are `FROM`-ul ei. Locurile: lista `SELECT`, `WHERE`, `ON`, `GROUP BY`, `HAVING`,
    `ORDER BY`, `SET` (stânga lui `=`: coloanele țintei), `RETURNING`, `VALUES`, `ON CONFLICT (…)
    DO UPDATE SET`, `MERGE … USING … ON … WHEN [NOT] MATCHED THEN UPDATE SET / INSERT (…)`,
    `DELETE … USING`, `UPDATE … FROM`, `CREATE INDEX … ON t (…)`, `REFERENCES`, `ALTER TABLE …
    DROP / ALTER / RENAME COLUMN`, `SET search_path TO`; `FROM` în argumente (`EXTRACT(… FROM …)`)
    și `IS DISTINCT FROM` nu schimbă clauza. **`schema_index`**: `SchemaIndex` imuabil (din
    `IndexParts`), numele cu litere mici în vectori sortați (prefixul prin căutare binară, apoi
    fuzzy pe cel mult 600 de nume cu aceeași inițială și pe cuvintele numelor — `items` găsește
    `order_items`), `find` pe schemă (exact, apoi fără majuscule; fără schemă: cea implicită,
    apoi cea unică), coloanele compacte (tipurile împărțite: 5 000 × 30 de coloane sub 20 MB,
    testat), relațiile (`Real` — cheile motorului, cu cea fără coloane a SQLite rezolvată la cheia
    primară —, `Virtual` — acceptate în `RelationNotes` —, `Inferred` — `relations::infer` prin
    `SchemaGraph::with_notes`, cele puse deoparte lăsate afară), rutinele, `memory`, `missing`,
    `with_columns` (a doua treaptă: o generație nouă, cea veche neatinsă). **`completion`** (pur):
    `complete(CompletionRequest) → Completion { items, replace, missing }` — cuvintele cheie ale
    locului (scrise cu litere mici când așa e tastat; ale dialectului: `ILIKE` doar la Postgres,
    `GLOB` / `PRAGMA` doar la SQLite), tabelele și view-urile (CTE-urile întâi, apoi schema
    curentă, apoi celelalte scheme ca `billing.`, apoi tabelele lor calificate; `schema.|` doar
    ale ei; numele citate unde trebuie — `quote`, Postgres pliază majusculele), coloanele (după
    `alias.` ale tabelului în ordinea lor; necalificate ale tuturor tabelelor din scope, cele
    ambigue inserate calificate, `o.id`; detaliul „tabel alias · tip”; ale unui tabel care nu e în
    scope, `users.`), `INSERT INTO t (` cu „toate coloanele” întâi oricând, `JOIN` cu tabelele
    legate întâi (cele spre care arată, apoi cele care arată spre ele, apoi cele deduse) și
    condiția întreagă (`customers c ON c.id = o.customer_id`, aliasul din inițiale și nu unul
    luat — `alias_for`), `ON` cu condițiile întregi (și compuse, `AND`) apoi coloanele, `USING (`
    cu cele comune întâi, `*` desfăcut (`replace` pe `*` / `t.*`), funcțiile (ale serverului și
    ale noastre pe dialect, cu semnătura în detaliu; doar când s-a tastat ceva), `Loading` când
    coloanele lipsesc; bugetul: 5 000 de tabele sub 2 ms în release (testat cu limita 60 ms în
    debug). `Session::catalog_keys` (implicit prin `structure`; tabelele fără chei lăsate afară),
    `TableKeys`, evenimentul `SchemaInvalidated { source }`.
  - **Driverele**: SQLite `catalog_keys` într-o interogare (`pragma_table_list` ⋈
    `pragma_foreign_key_list`, tabel cu tabel unde cade); Postgres `catalog_keys` într-o
    interogare pe `pg_constraint` pe care o citește și 9.2 (fără `LATERAL`, `WITH ORDINALITY`,
    `array_position`: locurile dintr-un `generate_series(1, 32)` constant), cu rândurile citite
    înapoi în chei. Protocolul folosește implicitul domeniului.
  - **`code_intelligence::DocumentAssist`** (`assist.rs`): `AssistContext { project, root,
    path, language, text, offset, version }`, `complete` → `FrameworkCompletion`, `definition`
    → `AssistTarget::{Location, Command}`, `hover`, `generation(project)`, `languages()`.
    `CompletionKind` primește `Table`, `View`, `Column`, `Schema`, `Snippet` (insignele `T`, `V`,
    `c`, `S`, `s` în listă).
  - **Aplicația**: `LanguageSupport::assists` (contribuții); `workbench/assists.rs` le întreabă
    pe limbajul de la cursor înaintea framework-urilor în `update_completion`, în
    `find_definition` (o `Location`) și `goto_definition` (o comandă rulată), în cardul de hover;
    o listă făcută de un assist ține generația lui și e refăcută la cadrul următor când se
    schimbă (`poll_assists` în `on_wake`, `refresh_assisted_completion` înainte de desen);
    `CompletionPopup::items`; „Clear and Rebuild Caches” golește și barele de deasupra
    editorului (`EditorToolbar::purge`).
  - **Modulul `database-console`**: `schema::Indexes` (pe proiect, în `Consoles`): un index pe
    sursă și bază, citit pe pool-ul nou `db-schema-index` (`Runtime::spawn_index`) pe o conexiune
    a lui prin `ConsoleAccess::connector` (niciodată a consolei), doar la prima cerere; prima
    treaptă — `tables`, apoi `catalog_columns` + `catalog_keys` dacă sunt cel mult
    `console_eager_tables` tabele și încap în `console_index_memory`; altfel coloanele cerute
    leneș (`request`, pe conexiunea păstrată a indexului, o dată pe tabel, lista refăcută când
    vin); cel vechi servește până vine cel nou; invalidat de `SchemaInvalidated` (DDL din consolă —
    runner-ul îl anunță când `ddl > 0` —, „Refresh” din fereastra „Database” pe sursă sau pe
    toate, o sursă ștearsă sau cu altă conexiune — `same_connection` fals, spus de explorer),
    `DataChanged { structure: true }`, `DataSourcesChanged` (sursele dispărute uitate, cele cu alte
    relații decise citite din nou); o citire devenită veche pe drum e aruncată și urmată de alta
    (`epoch`, bilete); `ReindexRequested` / `Consoles::purge` îl golesc; eliberat după
    `idle_close` fără console deschise pe sursă (`sweep` / `next_due`); eroarea spusă în listă;
    `busy` pentru capturi. `assist::ConsoleAssist` (`DocumentAssist` pe `sql`): consola după
    fișierul ei (`attachment`, gata pentru fișierele `.sql` atașate din 39n), tokenii citiți o
    dată pe versiune (`kernel::Memo`), sugestiile domeniului cu sursa în detaliu (`main · shop`,
    ca în captura de referință), „dedusă” la relațiile deduse, „Se citește schema …” cât se citește.
    Comanda nouă `database.console.expand_columns` („Expand Column List”, ⌥↵, și în presetarea
    JetBrains): `*`-ul de la cursor (sau de dinaintea lui) înlocuit cu coloanele lui. Setările
    noi `[database] console_eager_tables` (1 000) și `console_index_memory` (64 MB), în blocul
    consolelor. Firul `db-schema-index` în Task Manager.
  - **Explorer-ul**: `refresh_asked` publică `SchemaInvalidated` (rândul sursei, antetul, meniul);
    `apply` (fereastra surselor) îl publică pentru o sursă ștearsă sau cu altă conexiune.
  - **Teste**: `scope` pe zeci de forme pe SQLite și Postgres (aliasul și punctul, textul
    neterminat, șiruri / comentarii / numere / parametri, fiecare loc unde vine un tabel, cuvintele
    după un tabel / un `JOIN` / un `GROUP` / o valoare, coloanele în fiecare clauză, `JOIN … ON`,
    `USING`, `INSERT`, `UPDATE … SET`, `ON CONFLICT`, `CREATE INDEX`, `ALTER … COLUMN`, CTE-urile
    simple / cu listă / recursive / mai multe / cu `*`, tabelele derivate cu `LATERAL`,
    subselecturile corelate pe două niveluri și aliasurile ascunse, ramurile unui `UNION`,
    aliasurile listei pe dialecte, `*` desfăcut, scheme / nume citate / funcții, `MERGE`,
    `search_path`, instrucțiunea cursorului; proprietăți deterministe: nimic nu panichează pe 600 de
    texte aleatoare și indicii sunt în tabele); indexul (găsirea pe schemă și fără majuscule,
    schema implicită, prefix / fuzzy / cuvinte, relațiile reale / virtuale / deduse / puse
    deoparte, a doua treaptă, 5 000 × 30 sub 20 MB); completarea (coloanele aliasului, ambiguitatea,
    subselectul, `JOIN` cu tabelele legate și aliasul liber, deduse ultimele, acceptate și puse
    deoparte, condițiile compuse, schemele, cuvintele pe loc și dialect, `INSERT`, `*`, CTE și
    derivate, `ORDER BY`, coloanele lipsă, fără index, citarea, bugetul pe 5 000 de tabele);
    SQLite `catalog_keys` pe un fișier temporar (compuse, `REFERENCES t`, `ON DELETE`); Postgres
    interogarea cheilor; modulul pe SQLite real (citit o dată în fundal pe conexiunea lui, cel vechi
    servind, DDL / `DataChanged` / relațiile decise, o citire veche aruncată, golirea, eliberarea,
    a doua treaptă, o eroare spusă; assist-ul: „Se citește…” apoi coloanele, tabelele cu sursa,
    `JOIN` din cheia fișierului și cea dedusă, un fișier care nu e consolă, coloanele încărcate
    leneș, `*` desfăcut în consolă, tokenii o dată pe versiune; DDL-ul runner-ului anunțat);
    explorer-ul (`Refresh` pe o sursă și pe toate); aplicația (o consolă reală: `CREATE TABLE`
    rulat, apoi `select * from events e where e.` completat din schemă, lista refăcută când vine,
    acceptarea, tabelul cu `main · shop`, un `.sql` obișnuit fără lista modulului).
  - **Capturi noi** (adăugate în `scripts/ui-snapshots.sh` și `snapshot.rs`, **nerandate**: modul
    rapid cerut de utilizator): `console-completion` (`CONSOLE=completion`: lista după `FROM`, cu
    tabelele, schema și sursa, ca în captura de referință) și `console-join` (`CONSOLE=join`: după
    `JOIN`, `customers c ON c.id = o.customer_id` întâi), ambele teme. De așteptat schimbată:
    `settings-caret` (cele două rânduri noi în blocul consolelor).
  - **Abateri de la plan, cu motivul:**
    - `Expect` nu are `Function` separat: funcțiile vin cu `Column { qualifier: None }` și cu
      `Value` (unde merge o expresie), cum le oferă de obicei completarea SQL; `Schema` e doar la `search_path`
      (după `FROM` schemele vin printre tabele, ca `public.`);
    - `AssistContext` are în plus `project` și `root` (indexul e al proiectului);
    - acțiunea ⌥↵ pe `*` e o comandă a consolei (aplicația n-are încă intenții ⌥↵ generice) și,
      cerută cu ⌃Space chiar după `*`, singura sugestie a listei;
    - limita de memorie a indexului decide treapta (un index care n-ar încăpea citește coloanele
      leneș); coloanele citite leneș nu sunt scoase după aceea (le cer doar instrucțiunile scrise);
    - sursele cu altă conexiune ajung la index prin `SchemaInvalidated` publicat de explorer (care
      știe `same_connection`), nu prin compararea în consolă;
    - completarea nu se face pe cadru: la fiecare tastă (un `Memo` pe versiune pentru tokeni), iar
      lista unui assist se reface doar când îi crește generația.
  - **Ce n-a intrat și de ce:** literalele de tip enum Postgres (`pg_enum`), „mai târziu, dacă e
    scump”, cum spune planul; ajutorul de semnătură ca card pentru funcțiile SQL (semnătura e în
    detaliul sugestiei); definiția / hover-ul / diagnosticele sunt ale lui 39k (punctul de
    extensie și locul lor în aplicație sunt gata); capturile nerandate, `./scripts/check.sh` și
    clippy pe Windows nerulate (modul rapid).
- **39k — făcut** (rezolvarea și diagnosticele):
  - **Domeniul** (`database`): **`sql::resolve`** (`resolve.rs`; scanerul lui `scope` notează,
    într-o singură trecere, ce e fiecare nume — deci amândouă văd la fel ce e în scope): `Name`
    (locul, textul fără ghilimele, nivelul, clauza, dacă e al unei schimbări de schemă) cu
    `NameRole` — `Table` (unde merge un tabel), `Schema` (înaintea numelui), `Alias` (unde e dat),
    `CteName`, `Column { qualifier, certain }` (`certain`: necalificat, sigur un operand — după
    virgulă, paranteză, operator sau un cuvânt după care vine o expresie; nu un tip după `::` /
    `AS`, nu un literal cu tip `DATE '…'`, nu `EXTRACT(YEAR FROM …)`, nu un alias dat), `Qualifier`,
    `Function`, `TargetColumn` (`INSERT INTO t (…)`, `UPDATE t SET c =`, `ON CONFLICT (…)`, `CREATE
    INDEX … ON t (…)`); `LevelScope` pe fiecare nivel (tabelele vizibile, CTE-urile, aliasurile
    listei, `USING` / `NATURAL`), `resolve_at` (instrucțiunea cursorului), `qualified`, `alias_of`;
    cuvintele limbajului și numele după `AS` nu sunt nume. `sql::created_tables` (`CREATE [TEMP]
    TABLE [IF NOT EXISTS]`, `CREATE [MATERIALIZED] VIEW`, `CREATE SEQUENCE`, `CREATE VIRTUAL TABLE`,
    `SELECT … INTO` la Postgres, `ALTER TABLE … RENAME TO`). **`navigation::lookup`**: `Target` —
    `Table { alias }` (folosirea unui alias: unde e dat), `Column`, `Local` (CTE, tabel derivat,
    rândurile unei funcții: unde e definit, coloanele), `LocalColumn`, `Ambiguous`; o coloană
    necalificată e a singurului tabel de pe nivelul cel mai apropiat care o are (unul mai apropiat
    cu coloane necunoscute oprește căutarea), coloanele necitite spuse (`missing`). **`schema_check`**
    (doar ce e sigur): tabel necunoscut doar la SQLite și Postgres, niciodată unul creat oriunde în
    text (o consolă își rulează instrucțiunile în orice ordine) sau de sesiunea consolei, un CTE,
    unul dintr-o schemă necitită (`temp.`, o bază atașată, `information_schema`), al motorului
    (`sqlite_…`, `pg_…`), într-o schimbare de schemă (`DROP TABLE IF EXISTS`, `REFERENCES`), într-o
    instrucțiune neterminată sau în SQL dinamic (șiruri, corpuri `$$`), iar la SQLite niciunul
    necalificat după un `ATTACH`; coloană necunoscută doar cu o calificare rezolvată sigur la un
    tabel al bazei cu coloanele citite, sau în lista țintei (`INSERT`, `SET`), coloanele motorului
    deoparte (`rowid`, `ctid`…), niciodată pe CTE-uri sau tabele derivate (lista lor poate fi
    incompletă); ambiguitatea (avertisment) pe un nume necalificat sigur, în două tabele ale
    nivelului cel mai apropiat, nu cu `USING` / `NATURAL`, nu pe un alias al listei, nu în `ORDER
    BY` (poate fi o coloană a rezultatului). `Session::catalog_relations` (implicit nimic; Postgres:
    secvențele și partițiile — de la 10 — într-o interogare) → `IndexParts::readable`;
    `SchemaIndex::exists`, `knows_schema`, `column`, `points_to`. `ConsoleAccess::open_column`
    (implicit structura).
  - **Explorer-ul**: `open_column` deschide structura tabelului cu coloana arătată
    (`view::want_column`, `OpenSource::column`; rândul ei luminat în accent și adus în vedere o
    dată; altă tabelă arătată o uită).
  - **Modulul `database-console`**: `ConsoleAssist::definition` (pe un tabel sau o coloană a sursei
    comanda internă `database.console.goto` — „Go to Table or Column”, care rezolvă din nou la
    cursor și deschide datele tabelului prin `ConsoleAccess::open_table` sau structura cu coloana
    prin `open_column`, fișierul sau comanda explorer-ului prin `request_command_named`; pe un
    alias folosit, un CTE, un tabel derivat și coloanele lor `Location` în text; pe definiția
    însăși nimic) și `hover` (tabelul: `TABLE nume`, schema și sursa, rândurile estimate, coloanele
    — cel mult 40 — cu tipul, `PK`, `NOT NULL`, `DEFAULT`, `→ tabel.coloană`, comentariul; coloana:
    `tabel.coloană` și pe linia a doua tipul cu `PRIMARY KEY` / `NOT NULL` / `DEFAULT`, „Trimite la
    …”; CTE-ul cu coloanele lui; ambiguitatea spusă), din indexul construit (cerut dacă lipsește,
    coloanele necitite cerute). **`checks::Checks`** (pe proiect, în `Consoles`): bara îi dă textul
    la fiecare versiune (`follows_editor` e acum mereu adevărat pe o consolă cu SQL), verificarea
    după o pauză de 400 ms (cel mult 3 s, `kernel::queue::Coalescing`) pe pool-ul `db-schema-index`,
    pe indexul construit (cerut când lipsește; nimic spus până vine), din nou la fiecare sosire a
    unui index sau a unor coloane (`Indexes::on_arrival`); se publică `DiagnosticsFound` (sursa
    `sql-schema`) doar când se schimbă (un text curat prima dată: nimic), o versiune mai nouă venită
    între timp își are verificarea ei; consola închisă (`Drop` pe bară) sau mutată își ia
    constatările; „Clear and Rebuild Caches” le golește; `busy` pentru capturi. Runner-ul ține ce au
    făcut instrucțiunile lui (`Runner::made`: tabelele temporare create, un `ATTACH`). Setarea nouă
    `[database] console_grammar_hints` (oprită) în blocul consolelor: `EditorToolbar::grammar_hints`
    (nou, generic) cere aplicației erorile gramaticii pe care limbajul nu le raportează
    (`SyntaxSnapshot::quiet_errors`, nou), arătate estompat, ca avertismente „O îndoială a
    gramaticii”. Meniul editorului unei console are acum „Go to Declaration”
    (`workbench_model::GOTO_DECLARATION`, cu `MenuWhen::With(RUN)`: doar în consolă).
  - **Aplicația**: `WorkbenchHost::request_command_named` (comanda unui modul după textul id-ului
    ei); `HoverInfo::params_label` (rândurile cardului numite „Coloane:”); `multiline_signature`
    păstrează un cod scris deja pe mai multe linii; legătura de sub ⌘ pe un tabel are eticheta
    comenzii; erorile gramaticii estompate când bara le cere.
  - **Teste**: `resolve` (rolurile într-un `JOIN`, aliasurile, schemele, numele citate, CTE-urile,
    tabelele derivate și funcțiile, listele țintă, `ON CONFLICT … excluded`, siguranța numelor
    necalificate, DDL, `USING` / `NATURAL`, subselecturile și ramurile `UNION`, instrucțiunea
    cursorului, `created_tables`, textul neterminat); `navigation` (tabelul și aliasul lui, coloana
    calificată, necalificată, din `INSERT`, necitită, ambiguitatea, nivelul mai apropiat, CTE-uri și
    tabele derivate, nimic pe cuvinte / șiruri / funcții / nume necunoscute); `schema_check`
    (tabelul necunoscut cu locul lui; fără fals-pozitive: `CREATE TEMP` și `CREATE TABLE IF NOT
    EXISTS` oriunde în script, view-uri, CTE-uri simple și recursive, `temp.`, o schemă atașată,
    `ATTACH`, `sqlite_master`, `pragma_table_info`, `json_each`, `DROP TABLE IF EXISTS`, `ALTER …
    RENAME`, SQL în șiruri, o instrucțiune neterminată, ce a făcut sesiunea; Postgres cu partiții,
    secvențe, altă schemă, una necitită, `pg_class`, `information_schema`, `generate_series`, `DO
    $$ … $$`, `SELECT … INTO TEMP`; alte dialecte fără tabele necunoscute; coloanele necunoscute
    calificate, din `INSERT`, din `SET`, în orice majuscule, `rowid`, `excluded.`, CTE-uri și
    derivate tăcute, cele necitite cerute; ambiguitatea și excepțiile ei; un script întreg);
    indexul (`exists`, `knows_schema`, `column`, `points_to`); interogarea Postgres a relațiilor;
    `quiet_errors`; `multiline_signature`; explorer-ul (coloana arătată în structură); modulul pe
    SQLite real (definiția pe tabel / coloană / alias / CTE și nimic pe cuvinte, „Go to” deschizând
    datele și structura cu coloana, cardurile tabelului / coloanei / CTE-ului / ambiguității, nimic
    înainte de schemă; verificările: nimic până vine schema, apoi tabelul necunoscut cu linia și
    coloana, curățate când textul se repară, aceeași versiune verificată o dată, uitate la închidere,
    ce creează textul și ambiguitatea ca avertisment, un text curat fără nimic publicat, coloanele
    citite leneș, o schimbare de schemă, numele citate; runner-ul ținând tabelele temporare și
    `ATTACH`; meniul); aplicația (o consolă reală: cardul tabelului după ce se citește schema,
    aliasul dus la locul lui, tabelul necunoscut subliniat ca eroare la linia și coloana lui și
    curățat după reparare, tabelul deschis în vederea fișierului; meniul consolei cu „Go to
    Declaration”).
  - **Capturi noi** (adăugate în `scripts/ui-snapshots.sh` și `snapshot.rs`, **nerandate**: modul
    rapid cerut de utilizator): `console-hover` (`CONSOLE=hover`: cardul tabelului `orders` cu
    coloanele, tipurile și cheile) și `console-unknown-table` (`CONSOLE=unknown-table`: `order_lines`
    subliniat cu mesajul lui, CTE-ul de alături nu), ambele teme. De așteptat schimbate:
    `console-menu` (rândul „Go to Declaration”), `settings-caret` (rândul nou al îndoielilor
    gramaticii).
  - **Abateri de la plan, cu motivul:**
    - ⌘-click pe un tabel deschide datele lui (structura e la un clic, în același tab); pe o coloană,
      structura cu coloana arătată, cum spune planul;
    - „Find Usages” nu apare în consolă: nu e implementat (planul îl lasă pentru mai târziu), deci nu
      se arată ce nu merge; rândul „Go to Declaration” e al consolei (`MenuWhen::With(RUN)`), nu
      prin `LanguageFeature`, pentru că un `.sql` obișnuit nu rezolvă nume;
    - tabelul necunoscut se spune doar la SQLite și Postgres (un driver extern poate să nu listeze
      tot) și niciodată în schimbările de schemă; coloana necunoscută și în listele țintă (`INSERT`,
      `SET`: sigure), nu pe CTE-uri / tabele derivate; ambiguitatea nu în `ORDER BY`;
    - „indexul schemei complet” = prima treaptă (toate tabelele tuturor schemelor) plus relațiile
      pe care `tables` nu le listează (`catalog_relations`: partițiile și secvențele Postgres);
    - doar textul consolei active e urmărit (prin bară); celelalte console deschise își păstrează
      constatările și sunt verificate din nou la sosirea unui index;
    - erorile gramaticii, cu setarea pornită, sunt avertismente estompate (nu „informații”), ca să se
      vadă pe bara de derulare;
    - cardurile n-au comentariul tabelului (indexul nu-l citește încă).
  - **Ce n-a intrat și de ce:** „Find Usages” pe aliasuri și redenumirea (planul: mai târziu);
    capturile nerandate, `./scripts/check.sh` și clippy pe Windows nerulate (modul rapid cerut de
    utilizator: doar `cargo fmt`, `cargo build -p app` și testele crate-urilor atinse — trec; două
    teste ale indexului PHP din aplicație, `a_bulk_paste_ends_indexed` și
    `the_index_follows_saves_and_the_disk`, au căzut o dată sub încărcare și trec rulate din nou).
- **39l — făcut** (rezultatele editabile):
  - **Domeniul** (`database`): `Edits` (modificările în așteptare ale unui tabel: celule după
    cheia rândului cu valorile citite, rânduri noi, rânduri șterse, `change_set` în ordinea în
    care rulează) **mutat din explorer în domeniu** (`database::edits`), ca grila tabelului și
    rezultatul consolei să-l folosească la fel (explorer-ul îl reexportă, `summary` rămâne al
    lui). **`sql::editable::query_shape`** (pur): un `SELECT` al unui singur tabel
    (`query_table`), fără `DISTINCT`, `GROUP BY` / `HAVING`, agregate (`count`, `sum`,
    `string_agg`, `json_group_array`…) sau ferestre (`OVER`) în listă, fără `FOR UPDATE`
    (`Locking`: nu se rulează din nou ca să-și recitească rândurile); fiecare element al listei
    — `SelectItem::All` (`*`, `t.*`, `schema.t.*`), `Column` (numele, calificat cu tabelul sau
    aliasul, redenumit cu `AS` sau fără, coborât la Postgres) sau `Expression`.
    **`edits::editable_result`** (pur, pe structura tabelului): fiecare coloană a rezultatului
    dusă la a tabelului (`*` desfăcut în ordinea tabelului, numele verificate; o coloană arătată
    de două ori: doar prima se editează; `rowid` / `_rowid_` / `oid` ca id de rând), cheia
    după care se găsesc rândurile (`ResultKey`: cheia primară toată printre coloane, altfel id-ul
    rândului selectat, altfel o cheie unică fără predicat pe coloane `NOT NULL`), `key_of`
    (un `NULL` în cheie: rândul nu se editează), `writable` (nu generată, a tabelului),
    `column_infos` (o expresie contează ca generată: nu se scrie niciodată). `NotEditable` cu
    motivele (`NotQuery`, `NotOneTable`, `Distinct`, `Grouped`, `Aggregate`, `Locking`, `NoKey` —
    un view, de pildă —, `KeyMissing(coloanele)`, `Unmatched`). **`transaction::nested`**:
    pașii aceluiași `Script` între `SAVEPOINT` și `RELEASE`, cu `ROLLBACK TO SAVEPOINT` la eșec
    (doar modificările lor anulate, tranzacția păstrată, niciodată abandonată la Postgres).
    `ConsoleAccess::open_rows` (implicit datele tabelului).
  - **`ui-components`**: `GridSource::whole` / `Whole::{Here, Coming, Failed}` — o celulă ținută
    tăiată e cerută întreagă înainte de editare; grila spune „Se citește valoarea întreagă…” și
    deschide editorul mare când vine (dacă cursorul e tot pe ea). `DataGrid::row_of` /
    `source_column` (locul selecției ca rând / coloană a sursei).
  - **Explorer-ul**: `open_rows` — vederea fișierului (`view::want_rows`) sau a sursei
    (`OpenSource::filter`) arată tabelul filtrat `coloană = valoare`, aplicat abia când tabelul
    cerut e cel arătat.
  - **Modulul `database-console`**:
    - `results`: celulele tăiate știute cu locul lor (`cut_at`), `epoch` (rândurile lăsate și
      citite din nou: locurile nu mai sunt aceleași); `ResultSet::shape` (calculat la rulare din
      textul trimis), `editing: Editability::{Unknown, Reading, Ready, Refused, Failed}`,
      `submitting`, `submitted`, `wholes`.
    - **Runner-ul**: coada lucrărilor așteptate e acum o listă (fără dubluri; o rulare păstrează
      ce au cerut tab-urile), `describe` (structura tabelului citită o dată, pe conexiunea
      consolei, când tab-ul rezultatului e arătat), `read_whole` (`SELECT coloana FROM tabel
      WHERE cheia`, prin dialect), `submit(SubmitJob)` — exact scriptul arătat: în `Tx: Auto`
      într-o tranzacție a lui, confirmată pe loc (`DataChanged` imediat); în `Manual` (sau într-o
      tranzacție deschisă de un `BEGIN` scris) sub savepoint-ul `console_edit` în tranzacția
      consolei — deschisă de el dacă nu era, cu izolarea consolei —, numărate ca scrieri
      (`TxState`, `Tx: Manual · N`, lista Commit-ului), `DataChanged` la Commit; un eșec al unei
      tranzacții deschise de el o anulează. Instrucțiunile și cum s-a terminat în Output; reușit,
      rezultatul e citit din nou (interogarea lui pură, cu ordinea și filtrul serverului) —
      nicio scriere rulată din nou.
    - `result_source::ResultSource`: editarea grilei pe `Edits` (după cheia rândului;
      rândurile cu modificări știute după locul lor, găsite din nou după cheie când rândurile
      se recitesc), coloanele cu cheia / `NOT NULL` / doar citire după structură, rânduri noi
      (clonate fără cheie, coloanele generate, expresiile și celulele mari necitite), ștergeri,
      revenire, celulele mari citite întregi (originalul comparat la `UPDATE` e valoarea
      întreagă), motivul doar-citire în cuvinte (`refused_text`), sursa doar citire / driverul
      fără editare (`Rights`, urmate la fiecare schimbare a runner-ului), `reference` (cheile
      externe ale tabelului).
    - `result_view::ResultView`: Submit (bara grilei, ⌘↵ prin `actions_view` =
      `view-console-result` și comenzile `database.console.submit_result` / `add_row` /
      `clone_row` / `delete_rows` (⌘⌫) / `revert`), toast-ul trimiterii (confirmat, în
      tranzacție, conflict, eșec), în meniul celulei „Deschide tabelul” și, pe celula unei chei,
      „Mergi la rândul referit” (prin `ConsoleAccess::open_table` / `open_rows`).
    - **`edit`** (nou): comanda internă `database.console.submit_edits` — sursa doar citire
      refuză înainte (Output și toast), o tranzacție eșuată cere Rollback întâi, apoi
      `apply_script` cu structura tabelului și modalul consolei (`ConfirmKind::Submit`, „Trimite
      modificările rezultatului”): instrucțiunile exact cum rulează, rândurile șterse, nota
      Auto / Manual; pe producție „Am înțeles” doar în `Tx: Auto` (`ConfirmRequest::pending`: în
      `Manual` nimic nu e definitiv înainte de Commit). Închis altfel decât prin buton: nimic.
  - **Teste**: domeniul (`query_shape`: `*`, `t.*`, calificări, aliasuri, expresii, `NULL`,
    numele citate și coborâte; refuzurile cu motivul — `DISTINCT`, `GROUP BY`, `HAVING`,
    agregate, `OVER`, `JOIN`, două tabele, subinterogare, `UNION`, `WITH`, `FOR UPDATE`, scrieri,
    `VALUES`; `editable_result`: coloanele duse la tabel, redenumite, expresii, arătate de două
    ori, cheia primară, `rowid`, cheia unică `NOT NULL` și una nulabilă refuzată, cheia lipsă,
    un tabel fără cheie, coloanele nepotrivite, `change_set` pe coloanele tabelului;
    `transaction::nested`); grila (valoarea mare cerută întreagă, editorul deschis când vine,
    nu și cu cursorul mutat, eșecul spus); explorer-ul (rândurile cerute filtrate); modulul pe
    SQLite real (Auto: o celulă, un rând nou, o ștergere, previzualizarea exactă, nimic înainte
    de confirmare, aplicat, recitit, `DataChanged`; ce nu se poate edita și de ce — cheia lipsă,
    agregat, `DISTINCT`, `JOIN`, o scriere, un view, o expresie; sursa doar citire fără nicio
    citire a structurii; **Manual**: în tranzacția consolei fără „Am înțeles” pe producție,
    numărată, apoi **conflictul** — un rând schimbat de altă instrucțiune a consolei între timp —
    cu toate modificările păstrate și tranzacția păstrată, Commit-ul cu ambele; producția în
    Auto cu „Am înțeles” și ștergerea în motive, nimic fără răspuns; `rowid`; celula mare citită
    întreagă și comparată fără conflict; rândul referit și tabelul deschise; ordinea felurilor).
  - **Captură nouă** (adăugată în `scripts/ui-snapshots.sh` și `snapshot.rs`, **nerandată**:
    modul rapid cerut de utilizator): `services-result-edit` (`CONSOLE=result-edit`: rezultatul
    `main.customers` cu un nume schimbat, o țară schimbată, un rând nou și al treilea marcat
    pentru ștergere, Submit în bara grilei), ambele teme. De așteptat schimbate:
    `services-result` și `services-results-many` (rezultatele unui tabel sunt acum editabile:
    semnul cheii pe `id`, butoanele de editare ale grilei), poate și `console-tx-manual` (tab-ul
    rezultatului lui).
  - **Abateri de la plan, cu motivul:**
    - editabil și cu `rowid` selectat sau o cheie unică `NOT NULL` (nu doar cheia primară), ca
      grila tabelului; interogarea nu e niciodată schimbată ca să aducă cheia (o rulăm cum e
      scrisă): fără cheie printre coloane, motivul spune ce coloane să fie selectate;
    - `FOR UPDATE` nu e editabil (rândurile s-ar reciti rulând din nou o interogare care
      blochează);
    - previzualizarea e modalul consolei (cu sursa, producția și nota tranzacției), nu cel al
      explorer-ului (modulele nu depind unul de altul); bifa pentru ștergeri a explorer-ului
      (`confirm_delete`) e a lui: aici ștergerile sunt spuse în motivele modalului;
    - în `Manual` modificările rulează sub un savepoint (un conflict nu strică tranzacția
      deschisă); după reușită rezultatul e recitit (prima pagină), nu peticit pe loc;
    - modificările grilei nu intră în istoricul consolei (nu sunt textul utilizatorului); sunt
      în Output;
    - un rezultat nefixat înlocuit de rularea următoare își pierde modificările netrimise (ca
      tab-ul lui); unul fixat le păstrează;
    - o clonă lasă motorului celulele mari necitite întregi.
  - **Ce n-a intrat și de ce:** un test al aplicației pe editarea din „Services” (modulul o
    testează cap-coadă pe SQLite real, prin aceleași comenzi; aplicația nu are cod nou aici, doar
    cazul capturii); capturile nerandate, `./scripts/check.sh` și clippy pe Windows nerulate
    (modul rapid cerut de utilizator: `cargo fmt`, `cargo build -p app` și testele crate-urilor
    atinse).
- **39m — făcut** (formatarea):
  - **Domeniul** (`database::sql::format`, pur și determinist, în `sql/format/`: `mod.rs`
    intrările și verificarea, `layout.rs` așezarea, `doc.rs` limbajul așezării și tipărirea,
    `words.rs` cuvintele): **se schimbă doar spațiile dintre tokeni și literele cuvintelor
    cheie**; orice alt token (nume, șiruri, numere, parametri, comentarii, corpurile `$$` /
    `$tag$`) iese octet cu octet, în ordinea lui. Asta se **verifică**, nu se speră: fiecare
    instrucțiune așezată e citită din nou (`same_meaning`: aceiași tokeni fără spații, un cuvânt
    indiferent de litere, aceleași instrucțiuni citite cu liniile `GO` / `/` și fără), altfel
    rămâne cum era scrisă; la fel textul întreg la sfârșit. **Idempotent**: așezarea ține doar de
    tokeni și de ce se păstrează (un comentariu după tokenul lui pe același rând sau pe rândul
    lui, două șiruri pe rânduri diferite — Postgres le lipește doar așa —, un semn lipit de
    operandul lui, rândurile goale dintre instrucțiuni).
    - `FormatOptions { keyword_case: KeywordCase::{Upper, Lower, Preserve}, indent, width,
      commas: CommaStyle::{Trailing, Leading}, lists: ListStyle::{Fit, OnePerLine} }` (implicit
      majuscule, 4, 120, virgulele după, listele rupte doar peste lățime); `format` (textul
      întreg → `Formatted { text, left }`), `format_range` (instrucțiunile atinse de selecție —
      niciodată o bucată de instrucțiune), `format_at` (instrucțiunea de la cursor, după regula
      lui `statement_at`) → `RangeFormatted { range, text, left }`; rândurile de după primul
      încep ca el (indentarea instrucțiunii păstrată); sfârșiturile de rând `\r\n` păstrate.
    - **Așezarea** (stilul obișnuit de formatare SQL): fiecare clauză pe rândul ei (`SELECT`, `FROM`, fiecare
      join, `WHERE`, `GROUP BY`, `HAVING`, `ORDER BY`, `LIMIT`, `OFFSET`, `FETCH`, `FOR UPDATE`,
      `UNION [ALL]` singur pe rând, `INSERT [OR …] INTO`, `VALUES`, `UPDATE`, `SET`, `DELETE
      FROM`, `RETURNING`, `ON CONFLICT … DO …`, `MERGE` cu `USING` / `WHEN`, `SELECT … INTO`);
      lista clauzei pe rândul ei dacă încape, altfel câte un element pe rând, indentat;
      condițiile rupte înainte de `AND` / `OR` (nu `AND`-ul unui `BETWEEN`); `ON`-ul unui join
      pe rândul lui sau sub el; subinterogările în paranteze pe un rând dacă încap, altfel
      indentate; corpurile CTE-urilor mereu indentate (`WITH a AS (` … `),`); `CASE` pe un rând
      sau cu fiecare `WHEN` / `ELSE` pe rândul lui; apelurile și listele dintre paranteze rupte
      peste lățime; `CREATE TABLE` cu o coloană / constrângere pe rând; `CREATE VIEW … AS`,
      `CREATE TABLE … AS`, `EXPLAIN …` cu interogarea sub ele; corpul unui trigger SQLite /
      `BEGIN ATOMIC` cu câte o instrucțiune pe rând (așezate la rândul lor); celelalte
      instrucțiuni pe rândul lor, rupte după virgule peste lățime. Spațiile: nimic înainte de
      `,` / `;` / `)`, după `(`, în jurul lui `.` și `::`, între o funcție și `(` (`count(*)`),
      un spațiu între un nume de tabel și lista lui de coloane (`INSERT INTO t (a)`,
      `REFERENCES t (id)`, CTE-ul `x (a) AS (`), operatorii binari cu spații, un semn unar lipit
      cum era (`-1`), parantezele drepte `[ ]` ale Postgres cum erau.
    - **Cuvintele cheie** schimbate doar cele rezervate (care nu pot fi nume necitate oricum,
      deci literele lor nu schimbă nimic), cele care sunt cuvinte doar după altele (`PRIMARY
      KEY`, `NO ACTION`, `WITHOUT ROWID`, `SKIP LOCKED`…) și tipurile acolo unde stă un tip
      (după `::`, în coloanele lui `CREATE TABLE`, după `ADD COLUMN x`, `TYPE`, `RETURNS`, în
      `CAST(… AS t)`); niciodată un nume după / înainte de `.`, un identificator citat, numele
      unei funcții.
    - **Rămân cum sunt scrise** (numărate în `left`): o instrucțiune neterminată (un șir sau o
      paranteză deschisă), una cu o comandă psql (`\gset`: ține de rândul ei), una cu peste 100
      de paranteze imbricate, una a cărei așezare n-ar trece verificarea.
  - **Contractul nou, generic** (`code_quality::builtin`): `TextFormatter` (limbajele lui,
    `format(FormatRequest) → Formatted::{Changed { edit, left }, Unchanged, Nothing}`),
    `FormatRequest { project, path, language, text, scope: FormatScope::{Whole, Range, At} }`,
    `narrowed` (schimbarea strânsă la ce diferă: cursorul și marcajele rămân);
    `ProjectContext::text_formatters` (contribuțiile modulelor, adunate de aplicație);
    `workbench_model::format_active` (textul activ așezat ca un singur pas de undo; un text de
    peste 256 KB așezat în fundal și schimbat doar dacă e tot cel activ, neschimbat —
    `FormatOutcome::Moved` altfel) și `workbench_model::REFORMAT_FILE`.
  - **Code quality**: „Reformat File” (⌥⌘L) pe un limbaj cu formator propriu și fără unealtă
    aleasă de proiect îl folosește pe acela, în editor (selecția, altfel tot fișierul), fără
    salvare; o unealtă aleasă (de pildă `sqlfluff`) are întâietate; ce a rămas neformatat, „e
    formatat deja”, „textul s-a schimbat între timp” spuse în toast.
  - **`lang-sql`**: `format::SqlFormatter` (formatorul SQL, după dialectul documentului: al
    sursei consolei — spus de modulul consolelor prin `database::console::tell_flavors` /
    `document_flavor`, fără ca limbajul să știe de console —, altfel al numelui fișierului:
    `.pgsql`, `x.sqlite.sql`), comanda nouă `sql.format_statement` („Reformat Statement”, fără
    taste: instrucțiunea de la cursor sau cele ale selecției) și, în meniul de click dreapta al
    oricărui fișier SQL (deci și al consolei), „Reformat Code” și „Reformat Statement” după
    „Comentează rândul” (`MenuSection::Code`, `MenuWhen::Language("sql")`); setările noi
    `[sql] format_keyword_case` (`upper` / `lower` / `preserve`), `format_indent` (4),
    `format_width` (120), `format_commas` (`trailing` / `leading`), `format_lists` (`fit` /
    `always`), declarate în `lang-sql/src/settings.rs` și arătate în blocul nou „Formatarea
    SQL” din secțiunea editorului (`FormatSettings`).
  - **`database-console`**: `assist::flavor_of` (dialectul fișierului unei console: al sursei
    ei), dat domeniului la activare.
  - **Locale** în ambele limbi (blocul setărilor și cuvintele lui de căutare, comanda,
    mesajele `sql.format_*` și `quality.*`).
  - **Teste**: 37 de cazuri de aur pe SQLite și Postgres (select / join / group / insert pe mai
    multe rânduri / `INSERT OR REPLACE` / upsert / update / delete / `CREATE TABLE` cu `[x]`,
    `"order"` și `WITHOUT ROWID` / trigger cu corp / view / index parțial / CTE-uri / `CASE` /
    ferestre / `PRAGMA` și `ATTACH` / `EXPLAIN QUERY PLAN` / subinterogări / `UNION ALL` …
    `EXCEPT`; funcția plpgsql cu `$body$` păstrat octet cu octet, cast-uri și operatorii JSON,
    `DISTINCT ON`, `RETURNING`, `FOR UPDATE SKIP LOCKED`, `LATERAL`, `FILTER`, comentarii
    imbricate, `E'…'` / `U&'…'` / `$x$`, `DO $$`, `BEGIN ATOMIC`, șirurile pe două rânduri,
    `MERGE`, `EXPLAIN (…)`, `SELECT INTO`, `ALTER TABLE`, liste și condiții lungi), fiecare
    verificat și ca aceiași tokeni și idempotent; opțiunile (literele, indentarea și lățimea,
    virgulele înainte, câte un element pe rând), rândurile `VALUES` lungi, subinterogarea și
    `CASE` rupte, `ON` sub join; comentariile (după token, pe rândul lor, după virgulă, înainte
    de `)`, între și după instrucțiuni, doar comentarii, cele pe mai multe rânduri), rândurile
    goale, `\r\n`, textul gol; ce nu se schimbă niciodată (șirurile, corpurile, numele citate),
    instrucțiunile neterminate, comanda psql, imbricarea prea adâncă, liniile `GO` / `/` (și o
    coloană `go` care nu ajunge singură pe rând), semnele și operatorii lipiți, șirurile
    alăturate ale Postgres; instrucțiunea de la cursor, selecția, indentarea păstrată și
    idempotentă pe loc; **proprietăți** cu proptest pe sămânță fixă: 700 de texte aleatoare din
    bucăți de SQL (cuvinte, nume, șiruri, comentarii, corpuri, paranteze, separatori, text
    neterminat) pe toate dialectele și cu opțiuni aleatoare — aceiași tokeni, același sens,
    idempotent, `format_at` la fel —, așezarea independentă de spațierea textului (aceiași
    tokeni spațiați aleator dau același text), punctele fixe în toate combinațiile de opțiuni
    (rulat și pe 40 000 de cazuri cu alte semințe, local); **bugetele**: un `INSERT` de peste 1 MB
    (26 000 de rânduri; ~55 ms în release), un `SELECT` cu 5 000 de coloane și 5 000 de
    instrucțiuni (~40 ms), 90 de niveluri de apeluri cu 2 000 de condiții. Doc-ul (grupuri,
    ruperi, marginea, textele pe mai multe rânduri, fără rânduri goale sau spații la capăt).
    `narrowed`; `format_active` (pe loc, în fundal, textul schimbat între timp); Code quality
    (formatorul propriu doar fără unealtă aleasă); `lang-sql` (fișierul întreg cu schimbarea
    strânsă, instrucțiunea de la cursor, selecția, dialectul după nume, `left`, setările salvate
    și date formatorului, rândurile meniului); consola (dialectul sursei); domeniul
    (`document_flavor`); aplicația (⌥⌘L într-o consolă reală o așază după dialectul SQLite —
    `[a  b]` rămâne un nume —, „Reformat Statement” doar instrucțiunea de la cursor, un `.sql`
    simplu; meniul consolei și al unui `.sql` cu cele două rânduri noi).
  - **Abateri de la plan, cu motivul:**
    - „Reformat Code” e comanda „Reformat File” a modulului Code quality (⌥⌘L, aceleași taste),
      care folosește formatorul limbajului prin contractul generic `TextFormatter` când
      proiectul n-a ales o unealtă — nu o a doua comandă pe aceleași taste;
    - pe lângă selecție și fișier, „Reformat Statement” (cerut: instrucțiunea de la cursor) e o
      comandă a limbajului, fără taste implicite;
    - o selecție se extinde la instrucțiunile întregi pe care le atinge (o bucată de
      instrucțiune nu se poate așeza sigur);
    - liniile `GO` / `/` sunt mereu păstrate pe rândul lor (textul e citit cu ele), oricum ar fi
      setarea consolei: nicio instrucțiune nu ajunge pe sau de pe un asemenea rând;
    - rândurile goale din interiorul unei instrucțiuni nu se păstrează (doar cel mult unul între
      instrucțiuni); un comentariu pe mai multe rânduri sau unul de rând într-o listă o rupe
      (comentariul rămâne unde era);
    - SQLite numește o coloană-expresie fără alias după textul ei: după formatare numele
      coloanei rezultatului poate avea alte spații / litere (sensul interogării nu se schimbă),
      ca la orice formator.
  - **Ce n-a intrat și de ce:** „Format Project” rămâne doar al uneltelor externe (formatorul
    propriu lucrează pe textul din editor); nicio captură nouă (planul nu numește una); de
    așteptat schimbate: `console-menu` (cele două rânduri noi după „Comentează rândul”) și
    capturile secțiunii editorului din setări (`settings-caret` și pagina editorului: blocul
    nou „Formatarea SQL”); capturile nerandate, `./scripts/check.sh` și clippy pe Windows
    nerulate (modul rapid cerut de utilizator: `cargo fmt`, `cargo build -p app` și testele
    crate-urilor atinse).
- **39n — făcut** (un `.sql` din proiect atașat unei surse):
  - **Domeniul** (`database::console`): `ConsoleKind::{Console, Attached}` pe `ConsoleSpec`
    (`kind`; `ConsoleSpec::attached` — numele e al fișierului, `file` relativ la rădăcina
    proiectului, `path` după fel; `is_attached`), `SQL_FILES` (`sql-files.toml`),
    `SQL_EXTENSIONS` (ale limbajului `sql`: `sql`, `ddl`, `pgsql`, `psql`) și `attachable`
    (un fișier SQL din proiect, niciodată din dosarul propriu al proiectului — fișierul unei
    console e o consolă —, calea lui relativă).
  - **`storage::TomlConsoles::sql_files`**: același adaptor, `<dosarul proiectului>/sql-files.toml`,
    câte un `[[file]]` cu identitatea (UUID v4), sursa, `file` (calea în proiect, mereu cu
    `/`), baza, schema, Tx, izolarea, modul, ultima durată și numele parametrilor; scris atomic,
    unul stricat e o eroare și nu se scrie peste el.
  - **`Consoles`**: lista fișierelor atașate alături de a consolelor (`List`: citită o dată,
    scrisă în fundal, cea mai nouă ultima; acum și lista consolelor), `attach` (aceeași
    atașare pe aceeași sursă; pe alta: baza și schema lăsate, Tx după producție, conexiunea
    veche închisă — o tranzacție anulată și spusă), `detach`, `attached_of`, `attached_files`,
    `attachments` (un număr schimbat la fiecare atașare, detașare sau mutare: barele fișierului
    se uită din nou; aplicația trezită). `spec`, `spec_of_path` și `update` le văd pe amândouă,
    deci rularea, rezultatele din „Services”, istoricul, parametrii, Explain / profilul,
    completarea, rezolvarea (`assist::attachment`, pregătită în 39j), diagnosticele din schemă
    (`checks`) și formatarea după dialectul sursei (`assist::flavor_of` → `document_flavor`)
    lucrează pe un fișier atașat ca pe o consolă. Redenumirea / ștergerea din popover și din
    „Services” rămân ale consolelor.
  - **Urmărirea fișierului** (ca la consolele din d8d4c9d): `FileRenamed` (tab, arbore,
    refactorizare) — redenumit sau mutat în proiect rămâne aceeași atașare (id, setări,
    runner-ul cu rezultatele și istoricul, `Runner::renamed`), mutat în folderul consolelor
    unei surse devine consola ei (aceeași; altă sursă: baza și schema lăsate), scos din
    fișierele SQL ale proiectului e detașat (spus); `EntriesDeleted` — iese din listă cu
    runner-ul; schimbările discului ale altui program — unul dispărut cu un singur `.sql` nou în
    același folder e o redenumire și atașarea îl urmează (fără `FileRenamed`: fișierele
    proiectului sunt ale aplicației), unul dispărut fără pereche rămâne atașat (poate reveni:
    altă ramură scoasă). **Nota din 39e / d8d4c9d, făcută:** o consolă scoasă din `consoles/`
    în proiect își păstrează sursa — devine un fișier atașat, cu același id, aceleași setări și
    același runner (tranzacția deschisă și conexiunea rămân: aplicația face bara nouă înainte
    s-o lase pe cea veche), spus o dată; mutată în afara fișierelor proiectului (de exemplu în
    dosarul lui propriu) e, ca înainte, un fișier simplu (tranzacția anulată, spus).
  - **Bara** (`file_bar::SqlFileBar`, oferită de `ConsoleBarProvider` pentru orice fișier SQL al
    proiectului): neatașat, „Fără sursă de date” și butonul „Atașează o sursă de date… ▾” cu
    sursele care vorbesc SQL (sau motivul că nu sunt); atașat, chiar bara consolei
    (`ConsoleBar`) pe sursă, cu titlul, iconița și breadcrumb-ul fișierului păstrate, culoarea
    sursei pe tab și, în stânga schemei, pastila sursei (iconița driverului, numele tăiat cu „…”
    pe o bară îngustă, niciodată pusă în „…”) cu meniul ei: celelalte surse și „Detașează sursa
    de date”. Toate tab-urile aceluiași fișier urmează atașarea (`attachments`, un număr citit,
    nimic pe cadru altfel); o tranzacție deschisă oprește detașarea și mutarea pe altă sursă
    (spus). Mutat: aceeași bară cât rămâne un fișier SQL al proiectului, altfel providerii sunt
    întrebați din nou (o consolă scoasă în proiect primește bara fișierului, un fișier mutat în
    `consoles/` pe a consolei).
  - **Comenzile**: `database.console.attach` („Atașează o sursă de date…”, alegerea prin
    `host.choose`, sursa de acum marcată) și `database.console.detach`, ambele
    `When::EditorLanguage("sql")`, în meniul editorului după Explain doar unde sunt active;
    ⌘↵ / ⇧⌘↵ (`OnSqlFile`) pe un fișier neatașat întreabă întâi sursa, apoi rulează
    (rândurile Execute apar deci și pe un `.sql` neatașat); Go to Declaration și Jump
    apar acum cât e activ Explain (o consolă sau un fișier atașat). În „Services”, sesiunea unui
    fișier atașat are iconița unui fișier și meniul „Deschide fișierul” / „Detașează”.
  - **Locale** în ambele limbi (bara, meniurile, alegerea, mesajele `kept_attached`,
    `detached_on_move`, `tx_open_attach`, comenzile).
  - **Teste**: domeniul (`attachable` — extensiile, fără nume, alt proiect, `..`, fișierul unei
    console, dosarul propriu —, `ConsoleSpec::attached`); `storage` (lista fișierelor dusă și
    întoarsă cu `/`, separată de a consolelor; una fără cale e o eroare nescrisă); modulul
    (atașat / găsit / scris în lista lui / aceeași atașare / altă sursă cu schema lăsată și
    runner-ul închis / detașat / ce nu se atașează; lista citită înapoi; redenumit, mutat cu
    folderul, mutat în folderul consolelor ca aceeași consolă, scos din proiect și detașat cu
    mesajul, șters; discul: redenumirea urmată, dispărut fără pereche păstrat; consola scoasă
    în proiect cu tranzacția și conexiunea păstrate și consola scoasă în dosarul propriu ca
    înainte; bara — neatașat fără titlu / culoare / urmărire, butonul cu sursele, atașat din
    meniu, barele aceluiași fișier care urmează, pastila sursei cu celelalte surse și Detach,
    lease-urile lăsate, mutarea —; layout-ul cu pastila lângă schemă pe lățimi; comenzile —
    ⌘↵ pe un fișier neatașat cere sursa, nimic înainte, apoi rulează; Attach cu sursa de acum
    marcată, Detach; tranzacția deschisă refuză; doar fișierele SQL ale proiectului —; completarea
    și dialectul unui fișier atașat; diagnosticele lui șterse la detașare; sesiunea din
    „Services” cu meniul ei; rândurile meniului); aplicația (bara pe un `.sql` din proiect la
    locul barei consolei, un `.txt` fără bară; atașat din bară: titlul fișierului, ⌘↵ rulează,
    completarea din schemă, ⌥⌘L după dialectul SQLite, detașat fără lista modulului; consola
    redenumită pe disc scoasă în proiect rămâne atașată cu același id și redevine consolă;
    meniul unui `.sql` neatașat; condițiile meniurilor).
  - **Capturi noi** (nerandate încă): `sql-file` (`CONSOLE=file`: „Fără sursă de date —
    Atașează…”) și `sql-file-attached` (`CONSOLE=file-attached`: atașat, o instrucțiune rulată,
    pastila sursei, rezultatul în „Services”); fișierul `report.sql` și `sql-files.toml` sunt
    șterse de script după fiecare caz. De așteptat schimbate: niciuna dintre cele vechi
    (`sql-editor` deschide un fișier din afara proiectului, deci fără bară); `console-menu` nu
    se schimbă (rândurile noi apar doar unde comenzile lor sunt active).
  - **Abateri de la plan, cu motivul:**
    - atașarea se ține pe identitate (UUID v4) cu calea fișierului relativă la proiect, nu pe
      cale singură: urmează redenumirile și mutările ca o consolă;
    - fișierele cu extensiile limbajului `sql` (și `.ddl`, `.pgsql`, `.psql`), nu doar `.sql`;
      fișierele deschise singure (în afara proiectului) n-au bară — atașarea e a proiectului;
    - titlul tab-ului rămâne numele fișierului (sursa se vede în pastilă și în culoarea
      tab-ului), breadcrumb-ul e al fișierului;
    - ⌘↵ pe un fișier neatașat întreabă sursa în loc să nu facă nimic;
    - o redenumire pe disc a unui fișier atașat nu se anunță ca `FileRenamed` (doar atașarea
      urmează), iar unul dispărut fără pereche rămâne atașat;
    - o consolă scoasă din `consoles/` în proiect nu mai e „un fișier simplu”: își păstrează
      sursa (nota din 39e / d8d4c9d).
  - **Ce n-a intrat și de ce:** popover-ul „Jump to Query Console” listează doar consolele;
    capturile nerandate, `./scripts/check.sh` și clippy pe Windows nerulate
    (modul rapid cerut de utilizator: `cargo fmt`, `cargo build -p app` și testele
    crate-urilor atinse).
- **39o — făcut** (extractoarele de date, „Data Extractors”; partea întâi: cele
  de bază, alegerea, exportul — șabloanele utilizatorului sunt 39o2):
  - **Domeniul nou, pur: crate-ul `extractors`** (`crates/domain/database/extractors`, doar `kernel` și
    `i18n`): `Cell` / `Kind` / `Column` (cheia, `computed` — `GENERATED ALWAYS AS`, expresia unei
    interogări —, `generated` — auto-increment / identity), `SqlStyle` (numele și literalele;
    `AnsiSql` implicit), `Extractor` (id, nume, grup `Sql` / `Delimited` / `Other` / `Scripted`,
    extensia, `skips_columns`, `problem`, `origin`, `open` → `Stream` cu `begin` / `rows` /
    `finish`, un bloc de rânduri odată), `Extraction` (coloanele sărite prin `Options { skip_computed,
    skip_generated }` — niciodată toate —, rândurile numărate; ce dau `begin`, fiecare `rows` și
    `finish` e textul întreg, `extract` dintr-odată), `Lines`.
    - **SQL** (`sql_writers`): `SqlInserts { multirow }` (`INSERT INTO t (a, b) VALUES (…);` câte
      unul pe rând, sau pe mai multe rânduri cu `VALUES` aliniat, o instrucțiune nouă la fiecare
      `MULTIROW_ROWS` = 1000), `SqlUpdates` (`UPDATE t SET … WHERE cheia;` — fără cheie printre
      coloane: toate găsesc rândul, `NULL` prin `IS NULL`; doar cheia: e și setată), `WhereClause`
      (`id IN (…)` pe o coloană, cu `OR id IS NULL` pentru `NULL`-uri, altfel condițiile rândurilor
      unite prin `OR`, câte un rând pe linie; cheia dacă e printre coloane, altfel coloanele
      selectate). Skip-urile se aplică inserturilor și update-urilor.
    - **Separate** (`delimited`): `CsvFormat` (id, nume, separatorul — orice text fără rând nou —,
      ghilimelele sau niciunele, antetul, textul lui `NULL` — o valoare egală cu el e citată —,
      `LineEnd::{Lf, CrLf}`), `encode` / `decode` pentru setări (`cheie=valoare;…`, `\` `;` `=`
      escapate, tab ca `\t`), `is_valid`; cele implicite: CSV, TSV (fără antet: ⌘C de până acum),
      TSV cu antet (fostul „Tab-separated, with the header” și exportul `.tsv`), `|`, `;`.
    - **Altele** (`formats`): JSON (exact ca înainte), Markdown (ca înainte), HTML (`<table>` cu
      `<th>` / `<td>`, rândurile noi `<br>`), XML (`<data table="…">` de `<row>`-uri cu `<column
      name="…">`, `NULL` ca `null="true"`, caracterele pe care XML nu le poate ține înlocuite),
      One-Row (toate valorile pe un rând, separate prin virgulă, citate CSV), Pretty (tabelul text
      aliniat, numerele la dreapta, rândurile noi ca `\n`; lățimile din antet și primul bloc).
    - **`catalog`**: cele de bază (`builtin`), ale modulelor (`register`), formatele CSV configurate,
      opțiunile, șabloanele fiecărui proiect (`set_scripted`, pentru 39o2), alegerea pe proiect
      (`choose` / `chosen_id` / `chosen`: una care nu mai e oferită sau are o problemă → implicitul
      `tsv`, apoi primul format separat), `forget` la închiderea proiectului, `generation`; `catalog()`
      al aplicației, `Catalog::new()` pentru teste. Id-urile comenzilor modulului (`SAVE_COMMAND`,
      `CONFIGURE_CSV_COMMAND`, `OPEN_DIRECTORY_COMMAND`) stau aici: grila le cere după id.
  - **`database::extract`**: `DialectSql` (un `Dialect` ca `SqlStyle`: ce refuză — un NUL, un
    float infinit — scris ca SQL standard), `sql_style`, `value_of` / `cell_of`.
  - **`ui-components`**: `data_grid::copy` rescris pe extractoare (`copy_text`, `CopyStream`,
    `column_of`, `cell_of`; **`CopyFormat` și `EXPORT` scoase**); `GridColumn::{computed, generated}`;
    `GridSource::sql` (dialectul sursei, implicit `None`: SQL standard); `data_grid::export`:
    `ExportFormat::{Text(extractor), Xlsx}` (`find`, `chosen`, `offered` — XLSX doar pentru fișier —,
    `format_of(path, project, chosen)`: extensia celui ales îl păstrează, altfel după extensie, `.txt`
    → Pretty, `.xlsx` → Excel), `ExportWriter` (text sau registru, un bloc odată), `ExportSink::
    write_bytes` și `complete(finish, rows, text)`; **`data_grid::xlsx`** (`XlsxWriter`: zip-ul
    scris de noi — tipurile, relațiile, registrul, stilul antetului îngroșat, foaia cu antetul fixat
    —, foaia comprimată pe măsură ce vin rândurile, numerele / booleenii / zecimalele-text ca
    numere, `NULL` fără celulă, textul tăiat la 32 767 caractere, cel mult 1 048 576 de rânduri și 4
    GB — fără zip64, spus —, aceeași ieșire oricum ar fi blocurile, data fixă). **Mărimea:** niciun
    crate nou în `Cargo.lock` — `flate2` (backend-ul `miniz_oxide`) și CRC-ul lui erau deja legate
    prin `history-fs`; crește doar cu codul nostru (~400 de rânduri).
  - **Grila** (`DataGrid`): `set_project` (alegerea e a proiectului), `extractor()`, `copy(extractor)`
    / `copy_as(id)`; **⌘C scrie cu extractorul ales**; meniul celulei: „Copiază ca {nume}” (cel ales,
    cu tastele) și „Copiază ca ›” cu toate extractoarele pe grupuri despărțite; „Exportă toate
    rândurile ›” (toate + Excel) și „Copiază toate rândurile ca ›” (doar textul) din `export_menu
    (project)`; `GridAction::Export { format: id }` și **`GridAction::Command`** (comenzile modulului,
    rulate de vederi); comenzile `grid:copy:<id>`, `grid:export:<id>`, `grid:export-copy:<id>`,
    `grid:extractor:<id>`, `grid:extractors`. **Pastila extractorului** (`Tool::Extractor`: numele
    celui ales și ▾, în dreapta, înaintea butoanelor vederii; intră în „…” după Find, înainte de
    Refresh; în „…” deschide popover-ul ancorat acolo; tooltip-ul o numește). **Popover-ul**:
    titlurile grupurilor, extractoarele (cel ales bifat; unul cu problemă estompat, cu
    semnul și motivul în tooltip), după SQL bifele „Skip Computed / Generated Columns” (popover-ul
    rămâne deschis), după CSV „Configure CSV Formats…”, nota că Excel e doar la export; derulat cu
    rotița când e mai înalt decât grila; Esc sau un click în afară îl închide (nu e un modal).
  - **Explorer-ul**: `TableSource::with_dialect` (SQL-ul copiat prin dialectul sursei),
    `grid_columns` cu `computed` / `generated` din structură, vederea dă proiectul grilei,
    `ExportRequest { format: ExportFormat, sql, options }` scris prin `ExportWriter` (și XLSX).
  - **`database-console`**: `ResultSource::sql` (dialectul sesiunii), coloanele rezultatului cu
    `computed` (expresii, coloanele generate) / `generated` după structură, `ResultView` cu proiectul
    runner-ului, butonul de export al barei folosește **extractorul ales** (până acum CSV mereu),
    meniul lui `export_menu(project)`, `write_rows(…, Writing { format, sql, options }, …)`, exportul
    rerulat al runner-ului (`ExportRequest { columns: Vec<GridColumn>, format, sql, options }`) prin
    `ExportWriter`.
  - **Modulul nou `data-extractors`** (`crates/modules/database/extractors`, id `database.extractors`,
    fără să depindă de alte module): setările `[extractors] skip_computed`, `skip_generated`,
    `csv_formats` (`Setting<Vec<String>>`, câte un `CsvFormat::encode`; cele implicite nu se scriu)
    și, în `project.toml` al proiectului, `[extractors] chosen`; `apply` le dă catalogului;
    `ExtractorsService` (alegerea proiectului dată catalogului la deschidere, uitată la închidere);
    comenzile `extractors.save` (opțiunile în setări, alegerea în setările proiectului — rulată de
    popover), `extractors.configure_csv` și cea internă `extractors.csv_formats_given`; blocul
    „Extractoarele de date” din secțiunea editorului (cele două bife, formatele, unde se alege);
    **modalul „Formatele CSV”** (`view-csv-formats`: lista în stânga cu Adaugă / Șterge — niciodată
    ultimul —, numele, separatorul ca pastile `,` `;` `|` Tab sau oricare altul tastat, ghilimelele
    `"` / `'` / fără, antetul, textul lui `NULL`, sfârșitul de rând LF / CRLF și previzualizarea a
    două rânduri; „Revino la cele implicite”, Renunță, OK; ce nu se poate salva spus — fără nume,
    fără separator, ghilimelele în separator, două cu același nume; Esc lasă întâi câmpul, apoi
    închide; închis doar prin ×, Esc sau butoane). Aplicația înregistrează modulul și ia
    contribuțiile `Arc<dyn extractors::Extractor>` ale modulelor în catalog.
  - **Locale** în ambele limbi (secțiunea nouă `[extract]`, `grid.copy_with`, `grid.tip_extractor`,
    blocul setărilor); cheile vechi `grid.copy_*` / `grid.export_*` ale formatelor fixe scoase.
  - **Teste**: domeniul (fiecare extractor pe `NULL`, ghilimele, virgule și rânduri noi, binar, numere,
    float-uri ne-finite, booleeni, JSON-ul motorului, date și zecimale-text; dialectul — ghilimele
    inverse, booleenii SQLite; multirow pe blocuri și împărțit la 1000; skip-urile, inclusiv „toate
    sărite: niciuna”, și nu pentru CSV; Updates cu / fără cheie / doar cheia; Where pe o coloană cu
    `NULL`, doar `NULL`, gol, pe mai multe coloane; formatele CSV cu separator de mai multe caractere,
    ghilimele `'`, fără ghilimele, `\N`, CRLF; dus și întors prin setări, cele stricate refuzate;
    fiecare format scris la fel pe blocuri; catalogul — ordinea, ale modulelor și ale proiectelor
    despărțite, alegerea pe proiect, una dispărută sau cu problemă, formatele configurate);
    `database::extract`; grila (⌘C după alegere, alt proiect cu a lui, „Copiază ca”, meniul celulei
    pe grupuri fără XLSX, pastila cu tooltip, popover-ul — rândurile, bifa care îl lasă deschis,
    alegerea care îl închide, comanda, Esc, click în afară —, popover-ul înalt derulat, pastila în
    „…”, **selecția parțială** — celulele nealese `NULL`, Where pe coloana aleasă, pe cheie cu rândul
    întreg —, exportul `xlsx` doar în fișier); bara (pastila înaintea butoanelor, lipsă fără lățime,
    intrată în „…” înaintea lui Refresh, nimic suprapus la nicio lățime); `xlsx` (părțile citite
    înapoi cu CRC-ul verificat, celulele și tipurile lor, aceeași ieșire pe blocuri diferite, literele
    coloanelor, numele foii, prea multe rânduri); `export` (formatul după numele fișierului, XLSX doar
    pentru fișier, scriitorul de text și de registru); explorer-ul (50 000 de rânduri SQLite exportate
    în 12 extractoare exact ca copia lor, plus XLSX; CSV-ul scris înapoi identic); consola (export
    XLSX, Where pe clipboard, comenzile modulului ajunse la aplicație); modulul (formatele citite
    înapoi fără cele stricate sau repetate, cele implicite nescrise, bifele din setări, editorul
    formatelor — adaugă / schimbă / previzualizare / șterge / implicite / problemele —, modalul dă
    formatele, Esc fără nimic, alegerea în setările proiectului și înapoi la redeschidere, uitată la
    închidere, formatele salvate și oferite).
  - **Capturi noi** (adăugate în `scripts/ui-snapshots.sh` și `snapshot.rs`, **nerandate**: modul
    rapid cerut de utilizator): `grid-extractors` (`DATA=extractors`: tabelul `orders`, o celulă
    aleasă, popover-ul deschis) și `services-result-extractors` (`CONSOLE=result-extractors`:
    rezultatul `main.customers` cu popover-ul deschis). **De așteptat schimbate:** toate capturile cu
    o grilă (`data-*`, `database-folder-grid*`, `services-result*`, `services-results-many`,
    `console-tx-manual`, `sql-file-attached`…: pastila „TSV ▾” în bară; la lățimi mici „…” are un
    rând în plus), `data-menu` și meniurile celulei (rândul „Copiază ca TSV”, „Copiază ca ›” cu toate
    extractoarele), `about` (un modul în plus), `settings-caret` și pagina editorului din setări
    (blocul „Extractoarele de date”).
  - **Abateri de la plan, cu motivul:**
    - modelul pur e un crate de domeniu al lui (`extractors`), nu o parte din `database`: grila e
      generică (CSV-ul, orice listă tabelară), iar `database` îi dă doar dialectul (`DialectSql`);
    - implicitul rămâne TSV fără antet (⌘C de până acum, lipit în foaia de calcul ca celule), nu CSV:
      nimic din ce făcea ⌘C nu se schimbă până nu alege utilizatorul; „TSV cu antet” e un format al
      lui (ca exportul `.tsv` de până acum);
    - binarul e scris acum ca `0x…` hexazecimal în CSV / TSV / Markdown / HTML / XML / Pretty (până
      acum „(N octeți)”: se pierdeau datele);
    - butonul de export din bara rezultatului folosește extractorul ales (cum cere planul), nu CSV-ul
      fix; un fișier al cărui nume are extensia celui ales îl păstrează (SQL Updates într-un `.sql`);
    - Pretty pe export își ia lățimile din antet și din primul bloc de rânduri (o valoare mai lată
      mai târziu își lărgește doar rândul): exportul nu ține toate rândurile în memorie;
    - Where Clause folosește cheia când e printre coloanele alese, altfel coloanele alese; SQL Updates
      fără cheie găsesc rândul după toate coloanele (un șablon de pornit);
    - multirow împarte la 1000 de rânduri (limitele motoarelor);
    - XLSX e scris de noi (niciun crate nou: `flate2` era deja în arbore), fără zip64 (spus peste 4 GB);
    - setările „Skip…” și formatele CSV sunt ale tuturor proiectelor (setările utilizatorului); doar
      alegerea extractorului e a proiectului (`project.toml`).
  - **Ce n-a intrat și de ce:** extractoarele utilizatorului (șabloanele `*.extractor`, dosarele,
    exemplele, „Go to Extractors Directory”, colorarea și completarea) sunt **39o2**, commit separat,
    cum s-a cerut la o fază prea mare; capturile nerandate, `./scripts/check.sh` și clippy pe Windows
    nerulate (modul rapid cerut de utilizator: `cargo fmt`, `cargo build -p app` și testele
    crate-urilor atinse).
- **39o2 — făcut** (extractoarele utilizatorului; a doua parte a lui 39o, commit separat):
  - **Domeniul** (`extractors::template`, pur: nu citește fișiere, nu rulează programe, nu atinge
    rețeaua — scrie doar rândurile primite): `parse` (antetul opțional `---` cu `name:` și
    `extension:` — altfel numele fișierului și `txt` —; buclele `{{#rows}}…{{/rows}}` — o dată, la
    primul nivel: rândurile scrise pe măsură ce vin — și `{{#columns}}…{{/columns}}` — oriunde: în
    rând celulele lui, în afară doar coloanele, un antet —; condițiile `{{#first}}` / `{{#last}}`
    (ale buclei celei mai din interior), `{{#null}}`, `{{#number}}`, `{{#key}}`, fiecare inversat cu
    `^`; câmpurile `{{value}}`, `{{column.name}}`, `{{column.type}}`, `{{column.kind}}`,
    `{{column.index}}`, `{{table}}`, `{{row.index}}` (de la 0), `{{row.number}}` (de la 1);
    filtrele în lanț `|sql` (literalul dialectului sursei; un nume citat), `|json`, `|csv`,
    `|html`, `|xml`, `|md`, `|php`, `|py`, `|upper`, `|lower`, `|trim`, `|pad:N` / `|pad:-N`;
    comentariile `{# … #}` și `{{! … }}`; `\{{` și `\{#` literale; o etichetă de secțiune singură pe
    rândul ei își ia rândul cu ea, ca în Mustache), `Template { name, extension, before, row, after
    }`, `TemplateError { line, column, message }` (1-based, coloana în caractere; prima eroare:
    etichetă / comentariu / secțiune neînchise, închise în altă ordine sau fără să fie deschise,
    câmp / secțiune / filtru necunoscute, argumentul lui `pad`, un câmp în afara locului lui —
    `value` doar în coloanele unui rând, `column.*` în coloane, `row.*` în rânduri —, `rows` de două
    ori sau într-o secțiune, `columns` în `columns`, `first` / `last` fără buclă, o buclă inversată,
    peste 16 niveluri, peste 256 KB, antetul greșit), `TemplateExtractor` (grupul „Scripted”, id-ul
    dat, originea, extensia antetului; cu o eroare: `problem` „Rândul R, coloana C: …” și nu scrie
    nimic), `TemplateStream` (ce e înainte la început, fiecare rând când vine — unul ținut până vine
    următorul, ca `{{#last}}` să știe —, ce e după la sfârșit: aceeași ieșire oricum ar fi blocurile),
    `complete` (în eticheta de la cursor: câmpurile după `{{`, secțiunile după `#` / `^` — fără
    bucle după `^` —, cea de închis întâi după `/`, filtrele după `|`; nimic în comentarii sau în
    afara etichetelor). Filtrele `|php` și `|py` sunt în plus față de listă (exemplele PHP / Python
    au nevoie de literalele lor: `null` / `None`, `true` / `True`, ghilimele simple escapate).
  - **Modulul `data-extractors`**:
    - **`templates`**: `read_dir` (fișierele `*.extractor` ale unui dosar, sortate, cel mult 200;
      unul peste 256 KB sau care nu e text e o eroare, nu o cădere; id-urile `template:project:<fișier>`
      / `template:user:<fișier>`), `Templates` per proiect (dosarul propriu `<dosarul proiectului>/
      extractors/` și cel din dosarul de configurare, `config/extractors/`): citite pe pool-ul
      `extractors` (o citire cerută cât rulează una o mai face o dată), date catalogului ca
      șabloanele proiectului, erorile celor din proiect publicate ca `DiagnosticsFound` (sursa
      `extractor-templates`, la rândul și coloana lor; șterse când sunt reparate sau fișierul
      dispare); urmate prin evenimentele proiectului (`EntriesChanged`, `FilesWritten`,
      `EntriesDeleted`, `FileRenamed`, `EntriesRescan` pe dosarul lor — `extractors` e unul din
      `own_dirs`, deci watcher-ul proiectului îl aude), iar dosarul de configurare printr-un watcher
      al lui (`fs-local`), unul singur pentru toate proiectele, cât e deschis unul; `busy` pentru
      capturi.
    - **`examples`**: SQL-Insert-Multirow, Python-DataFrame, PHP-Array, Laravel-Seeder, Markdown-List,
      scrise în dosarul proiectului când „Go to Extractors Directory” îl face (cele existente nu sunt
      niciodată suprascrise).
    - **Comanda `extractors.open_directory`** („Go to Extractors Directory”, și în popover-ul grilei,
      în grupul „Scripted”): face dosarul proiectului cu exemplele prima dată (spus), îl listează din
      nou, recitește șabloanele, deschide primul șablon în editor (sau spune că nu e niciunul).
    - **`language`**: limbajul `extractor` (`*.extractor`, comentariul `{# #}`, iconița `table`)
      colorat prin **gramatica Twig** (deja în arbore: etichetele `{{ }}` devin directive, câmpurile
      și filtrele colorate, etichetele de secțiune ca un cuvânt cheie întreg, `{# #}` comentariu;
      erorile gramaticii nu sunt raportate — cele adevărate sunt ale limbajului de șablon),
      `TemplateIntelligence` (doar declanșatorii completării `{#^/|.`) și `TemplateAssist`
      (`DocumentAssist` pe `extractor`: sugestiile lui `template::complete`, cu explicația lor).
    - `ExtractorsService { user: UserDir }` (dosarul de configurare, unul anume — testele — sau
      niciunul), `own_dirs`, `busy`; firul `extractors` în Task Manager; blocul setărilor spune unde
      stau șabloanele.
  - **Grila**: grupul „Scripted” al popover-ului apare mereu, cu „Go to Extractors Directory”;
    rândul pastilei în „…” are o etichetă scurtă („Extractorul de date: TSV”, `grid.more_extractor`),
    tooltip-ul pastilei pe cea lungă (meniul „…” nu mai iese din grilă — testul lui pica de la 39o,
    cu localele încărcate).
  - **Locale** în ambele limbi (mesajele erorilor de șablon — acoladele citate ICU —, explicațiile
    completării, comanda, rândul setărilor).
  - **Teste**: limbajul (bucle și separatori, antetul, coloanele ca antet și rândurile, condițiile pe
    `NULL` / număr / cheie / primul, toate filtrele cu escapările lor, `|php` / `|py`, numele citate,
    `pad` la dreapta / stânga, rândurile singure ale secțiunilor și comentariile, acoladele escapate,
    aceeași ieșire pe blocuri, erorile cu rândul și coloana lor — 20 de forme —, nimic în afara
    datelor, un șablon cu eroare nu scrie nimic, completarea); modulul (dosarul citit sortat cu
    erorile, fișiere uriașe sau binare, șabloanele proiectului și cele comune în catalog doar pentru
    proiectul lor, diagnosticele spuse / șterse la reparare / la ștergere, recitirea la o schimbare
    în dosar și nu în rest, dosarele, exemplele — fiecare citit fără eroare și scriind exact ce
    trebuie pe rânduri cu `NULL`, ghilimele și booleeni, scrise o dată fără să le suprascrie pe ale
    utilizatorului —, interogările colorării compilate și fiecare etichetă a exemplelor citită de
    gramatică ca una, completarea prin assist, mesajele ICU citite cum sunt scrise).
  - **Captură nouă** (adăugată, **nerandată**: modul rapid): `extractor-template` (`EXTRACTOR=template`:
    „Go to Extractors Directory” face dosarul cu exemplele, `Python-DataFrame.extractor` deschis,
    colorat; scriptul șterge dosarul după fiecare caz). De așteptat schimbate: `grid-extractors` și
    `services-result-extractors` (grupul „Scripted” cu comanda lui), `about` (nimic în plus față de
    39o: același modul).
  - **Abateri de la plan, cu motivul:**
    - colorarea vine din gramatica Twig (vendorizată), nu dintr-o gramatică a noastră: generarea uneia
      cere CLI-ul tree-sitter, pe care nu l-am putut rula aici; Twig citește fiecare `{{ }}` ca o
      directivă (verificat pe exemple), cu interogările noastre;
    - comentariile se scriu și `{# … #}` (pe lângă `{{! … }}`), ca gramatica să le coloreze; `\{#`
      e un `{#` literal;
    - în plus `|php`, `|py` și `{{row.number}}`;
    - erorile se arată în editor la citirea fișierului de pe disc (la salvare), nu la fiecare tastă;
      ale șabloanelor din dosarul de configurare (în afara proiectului) apar doar în popover (cu
      motivul în tooltip);
    - „Go to Extractors Directory” e al proiectului (dosarul de configurare e spus în setări); un
      dosar de configurare făcut după deschiderea proiectului e urmărit de la proiectul următor.
  - **Ce n-a intrat și de ce:** capturile nerandate, `./scripts/check.sh` și clippy pe Windows
    nerulate (modul rapid cerut de utilizator: `cargo fmt`, `cargo build -p app` și testele
    crate-urilor atinse).
- **39p — făcut** (driverul MySQL / MariaDB: `crates/infra/database/mysql` = `db-mysql`,
  `crates/modules/database/mysql` = `database-mysql`, id `database.mysql`, înregistrat ca
  `database-postgres`; două fabrici, `mysql` și `mariadb`, același driver):
  - **Clientul: `mysql_async` 0.37** (cu `mysql_common`; MIT / Apache-2.0, pur Rust), cu
    `default-features = false` și `minimal-rust` (deflate prin `miniz_oxide`, fără zlib în C),
    `rustls-tls` + `ring` + `tls12`: același rustls 0.23 / ring / tokio pe care îi aduce deja
    `tokio-postgres`, deci nimic în C și nimic nou de compilat pe Windows (clippy pe
    `x86_64-pc-windows-gnu` curat). Are `caching_sha2_password` (și peste o conexiune
    necriptată: cheia RSA a serverului, cu RSA-ul lui `mysql_common`) și
    `mysql_native_password`; MariaDB 10.11 / 11.x și MySQL 5.7 / 8.0 / 8.4 se conectează toate.
    Alternativele: `mysql` (sincron) aduce alt I/O și `native-tls` implicit; `sqlx` e mult mai
    mare și ar duce un al doilea strat de abstracție. Creșterea binarului, măsurată pe un binar-sondă release (`opt-level 3`, LTO thin, `codegen-units 1`, ca aplicația) cu `db-postgres` față de `db-postgres` + `db-mysql` (tokio, rustls, ring comune, ca în aplicație): +2,3 MB fără simboluri (2,87 → 5,18 MB); în aplicație ar trebui să fie mai puțin (`regex`, `url`, `flate2`, `serde_json` sunt deja acolo), nemăsurat pe aplicația întreagă (două build-uri release). `Cargo.lock`: doar pachete noi (`crypto-common` ținut la 0.1.6 ca `generic-array` să rămână 0.14.9).
  - **Dialectul pe versiuni** (`database::mysql`, pur, testat): `ServerVersion` (motorul și
    versiunea din `VERSION()`, și forma `5.5.5-…-MariaDB` a handshake-ului), `Feature`
    (`RenameColumn` 8.0 / 10.5.2, `DefaultExpression` 8.0.13 / 10.2.1, `FunctionalIndex`,
    `DescendingIndex`, `ExplainTree`, `ExplainAnalyze` 8.0.18, `AnalyzeJson` MariaDB 10.1,
    `Returning` MariaDB 10.5, `Sequences`, `Checks`, `WindowFunctions`); numele cu backtick,
    textul cu backslash sau NUL scris hex cu setul de caractere (`_utf8mb4 X'…'`: citit la fel
    în orice `sql_mode`), filtrul „conține” cu `CAST(… AS CHAR)` și `ESCAPE '!'` (o nouă metodă
    `Dialect::contains_condition`, implicit cea standard), `AUTO_INCREMENT` la capătul
    coloanei, implicitele-expresie între paranteze (refuzate pe 5.7, afară de literale și
    `CURRENT_TIMESTAMP`), redenumirea tabelului în schema lui (necalificat, MySQL l-ar muta în
    baza sesiunii), `CHANGE COLUMN` pentru o coloană modificată, `DROP INDEX … ON`, `DROP
    FOREIGN KEY`, indexul parțial refuzat, `RETURNING` doar pe MariaDB 10.5+, iar pe un server
    fără `RENAME COLUMN` (5.7, MariaDB < 10.5.2) redenumirea devine `CHANGE` cu definiția
    citită (`Dialect::simplified`). `console::dialect_for(Flavor::MySql)` dă acum dialectul
    MySQL (nu cel generic): sugestia de index a analizei se scrie cu backtick.
  - **Catalogul** (`catalog.rs`, din `information_schema`, numele scrise ca literale, totul prin
    protocolul text): schemele = bazele (fără `information_schema`, `mysql`,
    `performance_schema`, `sys`; MySQL arată doar ce are utilizatorul vreun drept, deci
    `editor_private` nu apare), tabele / view-uri cu estimarea motorului, coloanele
    (`COLUMN_TYPE`, implicitul normalizat ca expresie — MySQL îl scrie gol, MariaDB îl citează
    și scrie `NULL` —, `auto_increment`, coloanele generate STORED / VIRTUAL, colația),
    indecșii (ordinea, unic, descrescător, prefixul, expresia indexurilor funcționale 8.0.13+),
    cheile străine cu regulile lor; în bloc pentru consolă: coloanele cu comentariul și locul
    în cheia primară, cheile, rutinele cu parametrii (`PARAMETERS`) și 226 de funcții ale
    serverului (agregate și de fereastră marcate) pentru completare, secvențele MariaDB ca
    relații. `lang-sql` are mai multe cuvinte cheie MySQL (`SHOW … PROCESSLIST`, `KILL`,
    `DEFINER`, `FULLTEXT`…).
  - **Sesiunea:** un runtime tokio pe un fir, al ei (ca la PostgreSQL); rândurile în flux,
    tipizate din metadatele coloanelor (întregi, `bigint unsigned` peste i64 ca text, float,
    `json`, text, binar, `bit` ca număr, datele / zecimalele / enum-urile ca scrise); paginile
    grilei după cheie (`(k1, k2) > (…)`); oprirea: o a doua conexiune scurtă trimite `KILL QUERY
    <id>` (un fir care urmărește `Cancel` și termenul profilării; și când grila / consola are
    destule rânduri dintr-o interogare — restul golit, conexiunea păstrată; `SLEEP` oprit e tot
    „anulat”); `CLIENT_FOUND_ROWS`, ca „exact un rând” al grilei să nu fie conflict la o valoare
    neschimbată; starea tranzacției cum o spune serverul (`SERVER_STATUS_IN_TRANS` din ultimul
    OK; după o eroare, întrebat cu `DO 0`), deci un DDL care face commit singur închide
    tranzacția și în consolă; conexiunea închisă politicos (`COM_QUIT`) la final.
    `sslmode`: `prefer` încearcă TLS și, dacă serverul n-are sau nu se poate negocia (MySQL 5.7
    al matricei: cifruri pe care rustls nu le are), se conectează necriptat, ca libpq;
    `require` refuză atunci; `verify-ca` / `verify-full` cu fișierul autorității (doar el e de
    încredere) sau rădăcinile Mozilla; certificatul clientului cu cheia lui.
  - **Consola (39c):** fără cursor pe server (`ConsoleCaps::cursors = false`: pagina următoare
    e interogarea rulată din nou, sortată / filtrată de motor), avertismentele ca notices
    (`SHOW WARNINGS`), erorile cu SQLSTATE (fără `HY000`, care nu spune nimic), codul în
    detaliu și locul găsit din cuvintele MySQL (`near '…' at line N`, căutat de la acel rând),
    mutat înapoi peste un `EXPLAIN …` adăugat; tranzacțiile cu izolarea (`SET TRANSACTION
    ISOLATION LEVEL …` apoi `START TRANSACTION [READ ONLY]`), schema prin `USE`, locul
    (`DATABASE()` ca schemă: MySQL n-are un nivel deasupra), `databases()` = schemele.
  - **Profilarea (39h2, `profile.rs`):** planul din `EXPLAIN FORMAT=JSON` (5.7+, MariaDB);
    ce a rulat din `EXPLAIN ANALYZE` (MySQL 8.0.18+, arborele text citit rând cu rând: adâncimea
    din indentare, costul, rândurile, `actual time=a..b rows=r loops=l` pe buclă, `never
    executed`, rânduri cu zecimale) și din `ANALYZE FORMAT=JSON` (MariaDB: `r_loops`, `r_rows`,
    `r_filtered` → rândurile scoase de filtru, `r_table_time_ms` + `r_other_time_ms` ale tuturor
    buclelor → pe buclă, `r_total_time_ms` al blocului → timpul instrucțiunii,
    `query_optimization` → planificarea). Corespondența din „Cum se potrivește fiecare motor”:
    `access_type` (`ALL` scanare, `index` parcurgerea indexului, `range` / `ref` / `eq_ref` /
    `const` căutări, `using_index` → acoperitor), `key`, `rows_examined_per_scan` / `rows`,
    `attached_condition` → `Filter`, `cost_info` (grupul „Cost”), `nested_loop`,
    `ordering_operation` / `filesort` → sortare, `grouping_operation` (cu tabel temporar:
    hash), `duplicates_removal`, `windowing`, `union_result` cu membrii, `temporary_table`,
    subinterogările de toate felurile ca sub-planuri (cele derivate ca `Subquery`); în arbore
    filtrul de deasupra unei scanări îi dă condiția și rândurile scoase (cum le citește
    analiza). Orice cheie necunoscută rămâne `Extra` (`used_columns`, `possible_keys`,
    `r_engine_stats`…). Rularea: într-o tranzacție a ei (`READ ONLY` pentru o citire fără
    blocări) sau sub savepoint-ul `console_profile` în a utilizatorului, anulată la final;
    refuzate DDL-ul, instrucțiunile tranzacției / sesiunii și scrierile pe tabele fără
    tranzacții (`information_schema.ENGINES`); termenul prin `KILL QUERY`. Pe instrucțiune:
    contoarele `SHOW SESSION STATUS` înainte / după (`Handler_read_*` → rândurile examinate,
    `Select_scan`, `Sort_merge_passes`, `Created_tmp_disk_tables`, toate și ca `Extra` în
    „Session status”), înregistrarea din `performance_schema.events_statements_history`
    (`ROWS_EXAMINED`, `NO_INDEX_USED`, `LOCK_TIME`…) când utilizatorul o poate citi.
    Statisticile tabelelor (`stats.rs`): `information_schema.TABLES` (estimarea, datele,
    indecșii, motorul, schimbarea), `STATISTICS` (indecșii cu coloanele, cardinalitatea), și
    `performance_schema.table_io_waits_summary_by_index_usage` când se poate.
  - **Arborele:** sursă › baze (ca scheme) › tabele, o singură sesiune pentru toate (vezi
    abaterile); o sursă care numește `shop` sau nicio bază vede aceleași baze.
  - **Teste:** domeniul (versiunile, ce are fiecare, literalele, filtrul, coloanele, implicitele
    pe versiuni, DDL-ul, redenumirea pe 5.7, `RETURNING`); `db-mysql` fără server (29: valorile
    după tip, erorile, locul erorii, transacțiile, capabilitățile pe versiuni, interogările
    catalogului, implicitele, planurile reale capturate pe matricea locală în `tests/data` —
    MySQL 8.4 JSON / arbore, 5.7 cu subinterogări și uniune, 8.0 cu hash join, MariaDB 10.11 /
    11.4 / 11.8 —, contoarele, `performance_schema`, statisticile, TLS); modulul (cele două
    fabrici, nimic conectat); integrarea pe matricea locală (`EDITOR_TEST_MYSQL_URLS`, doar dacă
    e dată): `db-mysql/tests/matrix.rs`, 12 teste — bazele ca scheme, una refuzată ca
    autentificare, tabele / view-uri, structurile (chei compuse, coloană generată, indecșii
    funcțional și descrescător pe 8.0+), paginile grilei, un view DEFINER citibil și un tabel
    refuzat (42000), filtrul, consola (pagini, o interogare uriașă oprită la timp, erori cu loc,
    avertismente, `USE`, tranzacții cu izolare, commit-ul implicit al unui DDL, tranzacția doar
    citire), anularea de pe a doua conexiune, planuri și profile pe fiecare versiune (5.7:
    refuzat cum trebuie), o scriere profilată anulată (și în tranzacția utilizatorului), DDL
    refuzat, statistici, catalogul în bloc într-un index de completare care completează,
    utilizatorul doar citire și sursa doar citire, TLS, modificările grilei (`CLIENT_FOUND_ROWS`,
    conflictul) și DDL-ul dialectului pe un tabel temporar — **12 din 12 pe fiecare: MySQL 5.7,
    8.0, 8.4, MariaDB 10.11, 11.4, 11.8**; `database-explorer/tests/mysql.rs`: fereastra
    „Database” pe fiecare server, sursa pe `shop` și fără bază: bazele ca scheme (fără cele ale
    serverului și `editor_private`), tabelele și view-urile altei baze, structura unui tabel,
    rândurile prin worker-ul sursei — trece pe toate 6.
  - **Captură nouă** (adăugată, **nerandată**: modul rapid): `database-tree-mysql`
    (`DATABASE=mysql`: un server MySQL inventat, în memorie, sursa pe `shop`, bazele ca scheme,
    `shop` deschisă până la tabele). De așteptat schimbate: `database-add` (MySQL și MariaDB în
    lista driverelor), `about` (un modul în plus).
  - **Abateri de la plan, cu motivul:**
    - arborele MySQL e sursă › baze-ca-scheme › tabele printr-o singură sesiune, nu nivelul de
      baze cu câte o sesiune pe bază al PostgreSQL-ului (b63f000): în MySQL o bază *este* o
      schemă și o sesiune le citește pe toate cu nume calificate (și alți clienți le arată tot ca
      scheme); fără conexiuni în plus, consola, grila și completarea merg peste baze;
      `ConsoleCaps::databases = false`;
    - fără cursor pe server (protocolul text n-are; `mysql_async` nu deschide cursoare de
      instrucțiune): pagina următoare e rularea din nou, ca la orice sursă fără cursor;
    - `READ UNCOMMITTED` nu e oferit (domeniul n-are nivelul);
    - pe MySQL 8.0 / 8.4 o scriere pe un singur tabel nu are arbore rulat (`<not executable by
      iterator executor>`): profilul ei e planul estimat cu timpul și contoarele instrucțiunii
      rulate (și anulate); MySQL 5.7 n-are profil rulat deloc (`ConsoleCaps::explain.analyze`
      fals, refuzat cu motivul);
    - `performance_schema` nu e citibil de un utilizator obișnuit (implicit): contoarele vin din
      `SHOW SESSION STATUS`; `events_stages_history_long` (așteptările) și `SHOW PROFILE` nu sunt
      folosite; `ProfileCaps::statement_history` rămâne fals (nu e istoricul între rulări al
      `pg_stat_statements`);
    - MariaDB `ed25519` / `parsec` nu sunt activate (n-ar fi fost pure Rust fără crate-uri în
      plus); `mysql_clear_password` refuzat cu motivul;
    - în arbore schemele urmează ordinea primului tabel (ca înainte, pentru orice sursă), nu
      ordinea alfabetică; baza sursei nu e marcată „implicită” între scheme.
  - **Ce n-a intrat și de ce:** capturile nerandate și `./scripts/check.sh` nerulat (modul
    rapid cerut de utilizator: `cargo fmt`, `cargo build -p app` și testele crate-urilor
    atinse); clippy pe Windows rulat doar pentru `db-mysql` și `database-mysql`, cum s-a cerut.
- **Corecție: tooltipurile profilului (imaginile 385, 386) — făcut:**
  - **Cauza:** un tooltip cu mai multe rânduri (pasul din arborele profilului: cuvintele lui,
    apoi felul lui — `SCAN` / „Sequential scan”; instrucțiunea; o constatare) era desenat
    de `ds::tooltip` ca un singur rând: toate rândurile pe aceeași linie de bază, una peste
    alta, într-o pilulă cât rândul cel mai lat. Și un meniu contextual deschis nu oprea
    tooltipurile din spate: `Workbench::tooltip_at` se oprea doar la un modal.
  - **Generic:** `ds::tooltip` așază rândurile unul sub altul (`\n`), fiecare rupt între
    cuvinte peste `TOOLTIP_MAX_W` (cel mult 12), pilula cât cel mai lat și cât toate, în
    marginile ferestrei (întoarce dreptunghiul ei; `ds::tooltip_lines`); e tot un singur
    tooltip pe fereastră (`Tooltips`). Cât e deschis orice strat peste editor (meniu
    contextual, popover, modal), doar acela poate da un tooltip — al lui
    (`OverlaySlot::tooltip_at`, implicit niciunul; vederea modală pe al ei, înăuntrul
    cardului: tooltipurile vederilor modale, până acum nedesenate, apar) — nimic din spate.
    Auditul `tooltip_at`: profilul (dropdown-ul lui), fereastra surselor (lista deschisă)
    nu mai dau tooltipuri prin ce au deschis peste ele (grila, schema, diagrama o făceau
    deja).
  - **Teste:** ui-components (rândurile unul sub altul, pilula cât ele; rândul lung rupt în
    margini; rândurile goale lăsate), app (niciun tooltip cu meniul contextual deschis,
    reveniți după închidere), database-console (niciunul cu dropdown-ul profilului deschis).
    Fără capturi noi (modul rapid).
- **Corecție: mărimile docurilor și ale despărțitoarelor, per proiect (imaginea 391) — făcut:**
  - **Cauza:** fereastra Services (și orice fereastră de jos) se deschidea la 280 px: un
    rezultat arăta un singur rând. Mărimile trase de utilizator erau doar în memorie
    (lățimea docului din stânga / dreapta, înălțimea celui de jos, lista de tabele a vederii
    de date, panoul profilului — 39h4 —, felia de detaliu a Task Managerului din dreapta în
    setări globale, cea de jos deloc, coloanele comparației, panourile logului Git) și
    treceau neschimbate în proiectul deschis după.
  - **Generic:** `workbench_model::LayoutSizes` — mărimile trase ale unui proiect, după cheie
    (`dock.left` / `dock.right` / `dock.bottom`, `database.tables-list`,
    `database.profile-panel`, `task-manager.detail-right` / `-below`, `compare.diff` /
    `compare.merge-left` / `compare.merge-result`, `git.log-branches` / `git.log-details`),
    în `ProjectData` (făcute cu proiectul, înlocuite cu el), salvate în sesiune (tabelul
    `[sizes]` din `session.toml`) și citite înapoi înainte să se refacă vederile. Fiecare
    vedere făcută de aplicație (tab, fereastră de jos, modal, refăcută din sesiune) și
    fiecare panou primesc mânerul (`set_data`); Services îl dă vederilor taburilor lui.
    Vederile își citesc mărimea din el și o scriu când e lăsat despărțitorul; dublu clic
    dă implicitul înapoi (cheia uitată). Lățimea arborelui Services rămâne în memento-ul
    panoului (era deja per proiect). Task Managerul scrie felia din dreapta și în setări
    (implicitul proiectelor unde n-a fost trasă).
  - **Un doc, o mărime:** fiecare doc (stânga, dreapta, jos) are o singură mărime, a
    tuturor ferestrelor lui (terminal / Services / Remote…; Structure / Task Manager /
    Database…): trecerea de la una la alta nu sare.
  - **Implicitul de jos:** netras niciodată în proiect, docul de jos e 45% din înălțimea de
    sub bara de sus, editorul păstrând cel puțin 30% (un rezultat arată ~8–10 rânduri), după
    fereastră; tras o dată, mărimea proiectului.
  - **Teste:** workbench-model (mânerul), storage (`[sizes]` dus și întors, sesiunile vechi
    fără el, numere întregi), app (implicitul de jos la mai multe înălțimi, editorul ≥ 30%;
    trecerea între ferestrele aceluiași doc păstrează mărimea, trasă cu una o au toate;
    trase, proiectul redeschis în altă fereastră le are la fel, alt proiect pe ale lui,
    dublu clic — implicitul), database-explorer, database-console, task-manager, compare,
    vcs-git (fiecare vedere: mărimea trasă scrisă în proiect, următoarea vedere o ia),
    services-panel (vederile taburilor primesc mânerul). Capturile care arată docul de jos
    fără înălțime dată se schimbă (mai înalt).
- **Corecție: colțurile de jos ale cardurilor (imaginile 394 vs 395) — făcut:**
  - **Cauza:** fereastra Database își umple tot conținutul cu fundalul ei (dreptunghi
    pătrat, până la marginile cardului): colțurile de jos ieșeau tăiate drept, peste
    rotunjirea cardului; Structure nu desena nimic acolo. Orice panou / vedere care umple
    până jos (o grilă, un terminal cu fundal propriu) făcea la fel.
  - **Generic:** randerele taie doar la dreptunghiuri, așa că după conținutul fiecărui card
    (panourile din stânga și dreapta, fereastra de jos, cardul editorului) aplicația pune
    un strat tăiat la card cu un inel din fundalul ferestrei a cărui margine interioară e
    exact conturul rotunjit al cardului (un quad mai mare cu `r` pe fiecare latură, rază
    `2r`, chenar `r`) și chenarul cardului desenat din nou: tot ce iese din contur, în
    colțuri, e acoperit (`Workbench::round_card_corners`). Doar de sub antetul aplicației
    (colțurile de sus sunt ale antetului, sub nuanța ferestrei); la cardul editorului doar
    jumătatea de jos. Două quad-uri și un strat pe card, fără nimic pe cadru în plus.
  - **Audit:** Database, Services, Task Manager, Git, proiect, Structure, Remote,
    terminal — în ambele teme, prin capturi (grupurile database, windows, chrome).
  - **Teste:** app (fiecare card — stânga / dreapta, jos, editorul — are stratul după tot
    ce umple colțul lui de jos, cu chenarul din nou, în ambele teme).
