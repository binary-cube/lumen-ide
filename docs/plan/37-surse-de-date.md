# Surse de date: CSV, SQLite și driverele (planul 37)

Cerut: „baze de date și fișiere de date, ca într-un IDE complet”, pe module și drivere, gândit de la
început și pentru driverele din extensiile terților, cu firele care nu blochează niciodată
UI-ul, totul DDD, curat și extensibil pentru driverele care vor mai apărea.

Planul 13 (`13-baze-de-date.md`) rămâne viziunea de ansamblu: consola SQL, `lang-sql`, SQL-ul
din PHP, MongoDB, Redis. Planul 37 e drumul concret până acolo, pornind de la ce există deja.

## Ce avem deja (nu se pierde nimic)

- **Domeniul `crates/domain/database/database`** e pur și conține:
  - `DriverFactory`: id, nume, `Kind`, port implicit și `create`. Nu rulează nimic până la
    conectare.
  - `Driver::connect` → `Session`, cu `children` (arborele exploratorului) și `execute`
    (rândurile vin în flux într-un `RowSink`, cu `Flow::Stop`).
  - `Cancel`, `Value`, `Column`, `Outcome`, `DbError`, `Target` / `Tls` și `Secret` (din
    `kernel`).
- **`crates/infra/db-postgres`** implementează contractul:
  - își face runtime-ul doar la conectare;
  - anulează interogările prin protocol;
  - arborele lui e scheme › tabele / view-uri › coloane / indecși.

  Nu e încă legat în aplicație.
- **`crates/infra/system/secrets`** are două seifuri:
  - `Keychain`;
  - `ProjectVault`: un fișier al proiectului criptat cu ChaCha20-Poly1305, cu cheia în
    keychain.

  Portul lor, `SecretStore`, stă acum în domeniul `remote`.
- **Mecanismele din aplicație pe care se construiește:**
  - `ViewerProvider` / `Viewer`, cu felurile `Replace` și `Preview`;
  - `ToolWindowProvider`;
  - `SettingsContribution`;
  - `kernel::work`, `kernel::Latest` și `kernel::queue`;
  - bus-ul de evenimente al proiectului.
- **Regulile 36h** se aplică și aici:
  - domeniul fără I/O și fără ceas;
  - modulele fără fire proprii;
  - modulele folosesc doar crate-urile infra din listă, deci fiecare driver își declară
    crate-ul în `MODULE_OWNED_INFRA`.

## Principii

1. **Domeniul se numește tot `database`.** E deja contextul surselor de date. Primește
   modelul `DataSource` și porturile, nu se face un al doilea domeniu.
2. **Un driver e un modul.** Modulul aduce un `DriverFactory`, iar crate-ul lui infra aduce
   implementarea.
   - Exemple: `database-sqlite` cu `db-sqlite`, `database-csv` cu `db-csv`,
     `database-postgres` cu `db-postgres`.
   - Modulul de UI (`database-explorer`) nu depinde de niciun driver. Le descoperă prin
     contribuții.
   - Un driver nou înseamnă un crate infra, un modul și o linie în lista de module, fără
     nicio schimbare în UI.
   - Totul stă grupat sub folderul `database` (cerut: „să le găsesc simplu pe toate”), vezi
     „Organizarea pe foldere”.
3. **Contractul driverului e „WASM-friendly”** (ADR-0006, `03-sistem-module.md` §4):
   - tipuri simple și DTO-uri serializabile;
   - cererile și răspunsurile ca mesaje.

   Un driver terț rulează la fel ca unul de-al nostru, printr-un adaptor de protocol (37i):
   e un component WASM când vin modulele dinamice, sau un proces separat care vorbește
   protocolul.
4. **Nimic pe firul UI.** Fiecare sesiune deschisă e un „actor”:
   - cererile ei merg într-o cutie poștală și rulează pe rând, pe pool-ul numit `db` din
     `kernel::work`, nu pe un fir al modulului;
   - răspunsurile se întorc prin wake.
   - Pentru paginile de rânduri contează doar ultima cerută (`kernel::Latest`): la derulare
     rapidă nu se strâng cereri vechi.
   - Fiecare cerere poate fi anulată (`Cancel`).
5. **Orice schimbare de date sau de structură trece printr-o tranzacție.** Pașii sunt
   aceiași peste tot:
   1. schimbările se adună în așteptare;
   2. se arată ce va rula (SQL-ul sau, la CSV, rezumatul);
   3. la „Trimite” se aplică toate sau niciuna;
   4. „Anulează” le aruncă.
6. **Leneș și cu resursele eliberate:**
   - nimic nu pornește până nu se deschide un fișier de date sau o sursă;
   - o sesiune nefolosită se închide după un timp (setare);
   - paginile din cache se eliberează la „Clear caches” (`Purge`) și cu tab-ul închis.
7. **Siguranța e înaintea comodității:**
   - nimic distructiv fără previzualizare și confirmare;
   - o sursă poate fi „doar citire”;
   - un tabel fără cheie se poate doar citi, cu explicația în UI.
8. **Parolele** (cerința utilizatorului) se salvează local, la nivel de proiect, și
   securizat, ca la remote: `ProjectVault`, criptat, cu cheia în keychain. Alternativele
   sunt keychain-ul sau „nu salva”.
   - Configurarea surselor nu conține niciodată secrete.
   - Parolele nu apar în loguri și nici în sesiune.

## Organizarea pe foldere

Tot ce ține de baze de date stă sub același nume, `database`, în fiecare strat:

```
crates/domain/database/database/            domeniul (crate-ul `database`, cum e acum)
crates/infra/database/
    postgres/                      `db-postgres` (mutat aici din crates/infra/db-postgres)
    sqlite/                        `db-sqlite`
    csv/                           `db-csv`
crates/modules/database/
    explorer/                      `database-explorer`: grila, fereastra „Database”, modalul
    sqlite/                        `database-sqlite`: driverul SQLite
    csv/                           `database-csv`: driverul CSV
    postgres/                      `database-postgres`: driverul PostgreSQL
```

- Folderul `database` e doar un grup, nu un crate.
- Un driver nou intră în ambele foldere: `infra/database/<driver>/` și
  `modules/database/<driver>/`.
- `cargo xtask check-deps` știe grupurile (făcut odată cu `languages/` și `frameworks/`): un crate din `crates/modules/<grup>/<crate>` e
  tot modul, iar unul din `crates/infra/<grup>/<crate>` e tot infra. Citirea numelor de
  comenzi și fire ale modulelor coboară și ea în grupuri. Fișierele `clippy.toml` din
  `crates/modules` și `crates/domain` se aplică și în subfoldere, pentru că clippy le caută
  urcând prin foldere.
- `MODULE_OWNED_INFRA`: `db-sqlite` → `database-sqlite`, `db-csv` → `database-csv`,
  `db-postgres` → `database-postgres`.
- Mutarea lui `db-postgres` se face cu `mv` simplu, nu cu `git mv`, în 37a.
- Celelalte familii sunt grupate deja la fel (cerut): `crates/modules/languages/<limbaj>`
  (`lang-php`, `lang-css`…) și `crates/modules/frameworks/<framework>` (`fw-laravel`,
  `fw-symfony`…). Numele crate-urilor au rămas aceleași; doar folderele s-au mutat.
- Și restul modulelor sunt pe familii: `viewers/` (image, markdown, binary),
  `appearance/` (themes, theme-studio, icon-packs, file-icons), `editing/` (emmet, colors,
  refactoring, quality, language-levels), `insight/` (structure, canvas, diagrams),
  `project/` (tree, indexing, local-history, remote), `vcs/` (git), `tools/` (terminal,
  task-manager, compare). Un modul nou intră în familia lui.

## Domeniul (`crates/domain/database/database`)

### Modelul

- **`DataSource`** e o sursă configurată:
  - `id` (UUID v4), `name`, `driver` (id-ul din factory);
  - `scope`: `Global` sau `Project`;
  - `Target` (exista deja), `read_only`, etichetele cu culoarea lor;
  - pentru fișiere: calea, relativă la proiect când e în el;
  - `SecretPolicy`: `Never`, `Session`, `ProjectVault` sau `Keychain`.
- **`Capabilities`**, ce poate un driver:
  - `transactions`, `edit_rows`, `schema_changes` (ce fel: tabele, coloane, indecși,
    redenumiri);
  - `schemas`, `file_based`, `cancel`, `returning`.

  UI-ul arată doar ce se poate.
- **Structura:** `TableInfo` (nume, fel: tabel sau view, estimarea numărului de rânduri) și:
  - `ColumnInfo`: nume, tip declarat, `TypeClass` (text, număr întreg / zecimal, bool,
    dată / timp, binar, JSON, altul), nullable, implicit, parte din cheie, auto-increment;
  - `IndexInfo`: nume, coloane, unic;
  - `ForeignKey`.
- **`RowKey`**, cum e găsit un rând la editare:
  - cheia primară;
  - `rowid` la SQLite;
  - numărul liniei, plus o amprentă a rândului, la CSV.

  Fără cheie, tabelul se poate doar citi.
- **`Page`:** offset, limită, ordonare (coloană și sens) și filtru (`Filter` structurat,
  nu text liber, la început).
- **`ChangeSet`:** `Insert`, `Update` (cheia și coloanele schimbate, cu valoarea veche
  pentru conflict) și `Delete` (cheia).
- **`SchemaChange`:**
  - `CreateTable`, `RenameTable`, `DropTable`;
  - `AddColumn`, `RenameColumn`, `AlterColumn`, `DropColumn`;
  - `CreateIndex`, `DropIndex`.

### Porturile

- **`Driver`:** `capabilities()` și `connect(target, secret)`. Se păstrează `kind()` și
  `language()`.
- **`Session`** păstrează ce are și primește:
  - `tables(path)`, `structure(table)` și `read(table, page, sink)`;
  - `apply(changes) -> Applied` și `alter(changes) -> Applied`, într-o singură tranzacție
    fiecare.

  Implementările implicite întorc `Unsupported`, deci `db-postgres` compilează neschimbat
  și câștigă metodele pe rând.
- **`Dialect`**, pur, adus de fiecare driver SQL:
  - cum se citează identificatorii și literalele;
  - paginarea;
  - SQL-ul pentru un `ChangeSet` și DDL-ul pentru un `SchemaChange`.

  Așa, previzualizarea arată exact ce rulează.
- **`DataSourceStore`:** sursele proiectului (`<project dir>/data-sources.toml`) și cele
  globale (`config_dir()/data-sources.toml`), fără secrete.
- **`SecretStore`** urcă din `remote` într-un loc comun (`kernel` sau un mic crate de
  domeniu `credentials`).
  - `remote` îl re-exportă, ca nimic din remote să nu se schimbe.
  - `secrets` îl implementează o singură dată, pentru amândouă.

### Regulile pure (testate fără nicio bază)

- SQL-ul generat din `ChangeSet` și `SchemaChange`, pe fiecare dialect.
- **Planul de reconstruire a tabelului la SQLite.** SQLite nu știe `ALTER COLUMN`, iar
  `DROP COLUMN` are limite. Se urmează procedura oficială în 12 pași: tabel nou, copiere,
  ștergere, redenumire, indecșii și triggerele refăcute, toate în tranzacție și cu
  `foreign_keys` verificate.
- **Detectarea operațiilor periculoase:** `DROP`, ștergeri în masă, o coloană ștearsă
  care are date.
- **Inferența tipurilor la CSV:**
  - se ia un eșantion de N rânduri și se deduc întreg, zecimal, bool, dată sau text;
  - se ghicesc separatorul (`,` `;` tab `|`), ghilimelele, antetul și BOM-ul.

### Evenimentele (pe bus-ul proiectului)

- **`DataSourcesChanged`**: o sursă a fost adăugată, schimbată sau ștearsă.
- **`DataChanged { source, table }`**: după un commit. Celelalte vederi ale aceluiași tabel
  se reîmprospătează.
- **Fișierele schimbate pe disc** vin pe evenimentul de disc existent:
  - un SQLite sau un CSV schimbat din afară se reîncarcă;
  - commit-ul nostru e marcat ca „al nostru”, ca la salvări, deci nu se face buclă.

## Componenta tabel și repository-ul (cerut)

Listarea tabelară e o singură componentă, refolosită peste tot, care primește o sursă de date
și face singură tot ce face o grilă de date completă. Nu știe de baze de date.

### Repository-ul (în domeniul `database`, folosit din `database-explorer`)

- **`TableRepository`** este, pentru un tabel (sau pentru rezultatul unei interogări),
  singurul loc prin care se citește și se scrie:
  - `count(filter)`: numărul total de rânduri, pentru „1–500 din N”;
  - `page(page)`: o pagină, cu ordonarea și filtrul ei;
  - `structure()`;
  - `apply(changes)`: schimbările, într-o tranzacție;
  - `refresh()`.
- Rulează peste `SessionWorker`, deci pe pool-ul `db`, niciodată pe firul UI.
  - Paginile vin în cache (`kernel::cache`, cu limită de memorie). Pentru fiecare tabel
    contează doar ultima pagină cerută (`Latest`).
  - Pagina următoare se poate cere dinainte, în fundal.
- Același contract îl implementează și CSV-ul, și orice driver nou. Vederea nu știe ce e
  dedesubt.

### Componenta (`ui-components::data_grid`, generică)

- **`DataGrid`** primește un **`GridSource`**, un port din `ui-components`, cu tipuri simple:
  - coloanele: nume, tip afișat, aliniere, dacă e cheie, dacă poate fi `NULL`;
  - `total()`, `row(i)` (`None` cât timp pagina încă vine), `request(range, sort, filter)`;
  - opțional, editarea: `editable(col)`, `set(row, col, value)`, `insert()`, `delete(rows)`,
    `pending()`, `submit()`, `revert()`.

  `database-explorer` adaptează `TableRepository` la `GridSource`. Aceeași grilă poate servi
  mai târziu și rezultatele consolei SQL, Task Manager-ul sau orice listă tabelară.
- **Ce face grila:**
  - **Paginarea:**
    - bara de sus cu `|<  <  1–500 din N  >  >|`;
    - mărimea paginii, aleasă dintr-o listă (10, 100, 500, 1000, „toate”, cu avertisment la
      tabele mari); implicit setarea modulului;
    - total exact sau estimat, la cerere, pentru tabele uriașe.
  - **Sortarea:**
    - click pe antet: crescător → descrescător → fără;
    - cu ⇧, sortare pe mai multe coloane, cu numărul ordinii pe antet;
    - sortarea se face la sursă (`ORDER BY`), nu doar pe pagina vizibilă.
  - **Filtrarea:**
    - câmpul `WHERE`, structurat (`Filter`);
    - filtru rapid din meniul unei celule („doar valoarea asta”, „fără valoarea asta”,
      `IS NULL`);
    - câmpul `ORDER BY` lângă el.
  - **Coloanele:** redimensionare, mutare prin tragere, ascundere / afișare dintr-o listă,
    potrivirea lățimii la conținut (dublu-click pe margine). Așezarea se ține minte pe tabel,
    în sesiunea proiectului.
  - **Rândurile și celulele:**
    - numerele rândurilor;
    - `NULL` arătat distinct;
    - selecție de celule, rânduri sau coloane, cu mouse-ul sau tastele (⇧ / ⌘);
    - căutarea în grilă (⌘F);
    - vederea transpusă (un rând ca listă coloană–valoare).
  - **Editarea:**
    - editorul celulei se alege după tip (registrul `ValueEditorProvider`);
    - valorile lungi (text, JSON) se editează într-un editor mare, deschis separat;
    - rândurile se adaugă, se clonează sau se șterg;
    - schimbările în așteptare sunt colorate (nou, schimbat, de șters);
    - „Submit” (⌘↵) arată previzualizarea SQL-ului, „Revert” le aruncă;
    - un tabel fără cheie e doar citire.
  - **Copiere și export:** selecția ca TSV, CSV, JSON, `INSERT`-uri sau Markdown, iar exportul
    întregului rezultat în fundal.
  - **Agregatele selecției** în bara de jos: număr, sumă, medie, minim, maxim.
  - **„Refresh”** și reîmprospătarea automată la `DataChanged` sau când fișierul se schimbă pe
    disc.
- **Performanța:**
  - se desenează doar rândurile vizibile, cu textul celulelor tăiat și măsurat o singură dată
    pe pagină;
  - nu se lucrează nimic pe fiecare cadru dacă nu s-a schimbat nimic;
  - paginile ieșite din vedere se eliberează după limita de memorie.
- Textele (butoane, meniuri, mesaje) sunt în `locales`.

## Execuția (aplicarea, în modulul `database-explorer`)

- **`SessionWorker`:**
  - deține o `Session`;
  - își rulează cererile una câte una, pe pool-ul `db` din `kernel::work`;
  - **cererile** sunt `Tables`, `Structure`, `ReadPage` (prin `Latest`, pe tabel), `Apply`,
    `Alter` și `Close`;
  - **răspunsurile** se pun într-o coadă, iar vederea le ia la `poll`.
- **`Sessions`:** câte un worker pentru fiecare sursă deschisă, în `ProjectData`, deci se
  duc odată cu proiectul.
  - Se închid când nu mai sunt folosite (setarea `IDLE_CLOSE`) și la închiderea
    proiectului (garda `on_end`).
- **Rândurile citite** stau într-un cache pe pagini (`kernel::cache`), cu limită de memorie,
  pentru Task Manager.

## Infra

- **`db-sqlite`** folosește `rusqlite` cu `bundled`, adică SQLite inclus și compilat cu `cc`.
  - `ring` face deja la fel, deci mingw e prezent pentru clippy pe Windows.
  - Tranzacțiile sunt `BEGIN IMMEDIATE … COMMIT`, cu `busy_timeout`.
  - Fișierele WAL sunt respectate.
  - Paginile se citesc după `rowid` când nu e altă ordonare, ca să nu fie `OFFSET` mare.
- **`db-csv`** citește cu regulile CSV ale domeniului (`database::csv`: `RecordScanner`,
  `split_fields`, `field_text`, `encode_field`), nu cu un crate în plus — același parser
  pentru ghicit, citit și scris.
  - Un index al offseturilor rândurilor se face în fundal, pe bucăți, ca o pagină să se
    citească prin `seek`. Așa, un CSV de sute de MB nu stă tot în RAM.
  - **Tranzacția** se aplică așa:
    1. se scrie un fișier temporar în același folder;
    2. se verifică amprenta fișierului original (dacă s-a schimbat pe disc între timp, e
       conflict și nu se scrie nimic);
    3. se face o redenumire atomică peste original. Pe Windows, cu reîncercare dacă fișierul
       e ținut deschis.
  - Totul trece prin porturile `FileSystem`, nu direct prin `std::fs` în domeniu.
- **`db-postgres`** e un singur driver pentru toate serverele, de la 9.2 la cele mai noi:
  protocolul (v3) e același, autentificarea e cea cerută de server (trust, cleartext, md5 la
  cele vechi, SCRAM-SHA-256 la cele noi), iar ce diferă (catalogul, SQL-ul pe care îl
  acceptă o versiune) se citește din `server_version_num` la conectare. Două drivere ar fi
  dublat tot restul pentru câteva interogări de catalog.

## UI (modulul `database-explorer`, doar cu `ui-components`)

- **Vederea de date** este un `Viewer` cu grila desenată pe GPU, virtualizată (se desenează
  doar rândurile vizibile).
  - **Antetul** arată numele și tipul fiecărei coloane, plus o iconiță pentru cheie.
    Ordonarea se schimbă cu click pe antet.
  - **Panoul „Structură”** arată coloanele, tipurile, indecșii și cheile.
  - **Editorii de celule** se aleg după `TypeClass`: text, număr, bool, dată, JSON, NULL, iar
    BLOB-ul doar se arată ca mărime. E un registru extensibil (`ValueEditorProvider`), ca un
    driver să-și aducă tipurile lui (de exemplu array-urile din Postgres).
  - **Rândurile:** se adaugă, se șterg (cu selecție multiplă) sau se editează în celulă.
    Schimbările în așteptare sunt colorate.
  - **Bara de jos:** „Trimite” (cu previzualizarea), „Anulează”, numărul de schimbări în
    așteptare și numărul total de rânduri.
- **Deschiderea automată a fișierelor:**
  - **`.sqlite`, `.sqlite3`, `.db`**, recunoscute după antetul `SQLite format 3\0`, nu doar
    după extensie, se deschid ca `Replace`.
    - Acum le ia `binary-files`. Providerul de date trebuie întrebat înaintea lui, deci
      `binary-files` devine ultima variantă (o prioritate a providerului, în
      `ui-components`).
    - În stânga apare lista de tabele, iar în dreapta grila.
  - **`.csv` și `.tsv`** se deschid ca `Preview`, cu „Date” ca vedere implicită.
    - Textul rămâne editabil exact ca acum: comutatorul Editor / Split / Preview păstrează
      ce aveam.
    - Cât timp textul are modificări nesalvate, grila le arată, dar e doar citire, cu
      mesajul „salvează întâi”. Altfel cele două editări s-ar suprascrie una pe alta.
    - După commit, documentul nemodificat se reîncarcă singur de pe disc, ca acum.
- **Managementul SQLite (37f):**
  - **Ce se poate face:** creare și ștergere de tabel; adăugare, redenumire, modificare și
    ștergere de coloană; creare și ștergere de index.
  - **Cum se face:** în modale ale modulului (`host.open_modal`), care nu se închid la un click
    afară. Fiecare modal arată DDL-ul care va rula și cere confirmare pentru ce e distructiv.
- **Fereastra „Database” (37g):**
  - **Locul:** un `ToolWindowProvider` pe dreapta, cu iconița cilindrului (`database`). Se
    face doar la prima afișare.
  - **Arborele:** sursă › schemă › `tables` › tabel › coloane / indecși, cu numerele lângă
    noduri. Folosește `TreeList` din 36i.
  - **Meniul `+`:** „Data Source” › lista de drivere, cu cele recente sus, apoi toate. Lista
    vine din contribuții.
- **Modalul „Data Sources and Drivers” (37h)**:
  - **În stânga:** sursele globale și cele ale proiectului, care se pot muta dintr-o parte în
    alta.
  - **În dreapta:** „Name”, apoi tab-urile General / Options / SSH/SSL / Schemas / Advanced.
    - Câmpurile vin de la driver, printr-o descriere declarativă (`FieldSpec`). Exemple: un
      fișier la SQLite; host, port, user, parolă și bază de date la servere.
    - URL-ul se generează și se citește în ambele sensuri.
  - **Butoanele:** „Test Connection” (în fundal, cu anulare), Cancel / Apply / OK.
  - **Textele:** toate în `locales`.
- **Setările** sunt declarate în `crates/modules/database/explorer/src/settings.rs` (`Setting<T>`)
  și arătate printr-un `SettingsContribution` (blocul „Date”, în secțiunea editorului: o pagină
  nouă ar fi mutat navigarea din toate capturile setărilor):
  - mărimea paginii;
  - închiderea sesiunilor nefolosite;
  - eșantionul CSV pentru tipuri;
  - confirmarea operațiilor distructive.

## Extensii terțe (37i)

- **`database::protocol`:** cererile și răspunsurile sesiunii, ca DTO-uri versionate
  (`serde`, cu versiunea protocolului în handshake).
  - Un driver declară `FieldSpec`, `Capabilities`, `Dialect` (sau „fără SQL”) și metodele
    sesiunii.
- **`ProtocolDriver`** este adaptorul care face dintr-un capăt de protocol (un canal) un
  `Driver` obișnuit. Restul aplicației nu știe că driverul e extern.
- **Capetele posibile:**
  - un component WASM, odată cu modulele dinamice (ADR-0006);
  - un proces separat, cu protocolul pe stdin / stdout, care poate fi și o punte JDBC. Puntea
    JDBC are nevoie de Java, deci doar dacă e instalat; nu e o dependență a aplicației.
- **În 37i se face:** protocolul, adaptorul și un driver de test care trece prin protocol
  (în teste). Cel puțin unul dintre drivere trebuie să ruleze identic și direct, și prin
  protocol. Gazda WASM vine cu planul modulelor dinamice.

## Diagrama relațiilor (37j, cerut)

- **Ce arată:** tabelele unei surse sau scheme, ca niște carduri (numele, coloanele cu tipul,
  cheia primară marcată), și liniile dintre ele, cu coloanele care leagă tabelele
  (`order.user_id → users.id`) și cardinalitatea: 1–n, sau 1–1 dacă cheia e unică.
- **Două feluri de relații:**
  - **Reale**, din cheile externe citite din structură, cu linie continuă.
  - **Deduse din nume**, cu linie punctată și eticheta „dedusă”. Nu dublează niciodată o
    cheie reală.
- **Regulile de deducere** (pure, în domeniu, `database::relations`, testate):
  - **Coloana:** `userId`, `user_id`, `userid`, `UserID`, `user_fk`, `fk_user` se reduc toate la
    „user”, oricum ar fi scrise (snake_case, camelCase, litere mari).
  - **Tabelul țintă:** `user`, `users`, `User`, `tbl_user`. Singular și plural se potrivesc
    amândouă: `category`/`categories`, `box`/`boxes`, `person`/`people`.
  - **Cheia țintă:** `id`, sau cheia primară cu o singură coloană, cu un tip compatibil (un
    text nu se leagă de un întreg).
  - **Auto-referințele:** `parent_id` se leagă de același tabel, dacă nu există un tabel
    `parent`.
  - **Nesiguranța:** fiecare relație dedusă primește un nivel de încredere. Potrivirea exactă
    și cea cu plural sunt sigure; cea cu prefix e mai slabă.
- **Acceptat sau ignorat:** o relație dedusă se poate accepta și devine o „cheie virtuală”,
  salvată pe sursa de date. Una ignorată nu mai apare. Cheile virtuale sunt
  folosite și de grilă, pentru a sări la rândul legat.
- **Comutatoarele:** chei reale, deduse sau ambele.
- **Scheme mari, cu sute de tabele:**
  - pornești de la un tabel și vezi doar vecinii lui, pe 1–2 niveluri;
  - filtrezi după nume;
  - apropii, depărtezi, potrivești în fereastră.

  Aranjarea în pagină se face pe un fir din fundal, o singură dată pe structură.
- **Interacțiunea:** un click pe un tabel îl centrează; un dublu-click îi deschide datele în
  grilă.
- **De unde se deschide:** din meniul contextual al unei surse, scheme sau tabel din
  fereastra „Database” („Show Diagram”), și din vederea fișierului SQLite. Merge pentru orice
  driver care dă structura.
- **Organizarea:**
  - Partea comună a modulului `diagrams` (aranjarea, desenul cardurilor și al liniilor,
    panoramarea, zoom-ul) se mută în `ui-components` (`diagram`), pentru că modulele nu
    depind unele de altele.
  - Diagrama claselor PHP o folosește fără nicio schimbare vizibilă: capturile ei rămân
    identice.
  - Modulul nou e `database-diagram`, în `crates/modules/database/diagram`, încărcat doar la
    cerere.

## Pașii

Fiecare pas e un commit, după `check.sh` și cu capturile identice. Capturile noi (grila,
modalele, fereastra „Database”) se adaugă ca scenarii în spatele feature-ului `snapshot`.

1. **37a — domeniul.**
   - Modelul (`DataSource`, `Capabilities`, structura, `RowKey`, `Page`, `ChangeSet`,
     `SchemaChange`), cu porturile extinse, cu implementări implicite.
   - `Dialect`, SQL-ul generat și planul de reconstruire SQLite, toate pure și testate.
   - `SecretStore` mutat într-un loc comun, cu re-export în `remote`.
   - `db-postgres` mutat în `crates/infra/database/postgres` (`check-deps` știe deja
     grupurile de foldere).
   - `db-postgres` compilează neschimbat.
2. **37b — execuția.**
   - Modulul `database-explorer` (gol de UI): `SessionWorker`, `Sessions` în `ProjectData`, pool-ul
     `db`, `Latest` pentru pagini, anulare, închidere la inactivitate, evenimentele.
   - `TableRepository` peste worker: total, pagini cu ordonare și filtru, cache-ul
     paginilor, cererea dinainte a paginii următoare, `apply` cu verificarea „exact un
     rând” (altfel `Conflict` și rollback), ordinea pașilor din `transaction::rebuild_script`.
   - Testat cu un driver fals.
3. **37c — SQLite.**
   - `db-sqlite` și modulul `database-sqlite` (factory, recunoașterea fișierului).
   - Tabelele, structura, paginile, `apply` în tranzacție.
   - Teste pe o bază temporară în scratchpad sau în `tempdir`.
4. **37d — CSV.**
   - `db-csv` și modulul `database-csv`: dialectul ghicit, tipurile inferate, indexul
     offseturilor în fundal, commit-ul cu fișier temporar, amprentă și redenumire atomică.
5. **37e — componenta tabel și vederea de date.**
   - `DataGrid` și `GridSource` în `ui-components`, cu tot ce e în „Componenta tabel”. Se poate
     împărți în două commit-uri: întâi citirea (paginare, sortare, filtru, coloane, selecție,
     copiere), apoi editarea.
   - Grila, structura, editorii de celule, adăugare / editare / ștergere cu „Trimite” și
     previzualizare.
   - Deschiderea automată (SQLite ca `Replace`, înaintea lui `binary-files`; CSV ca
     `Preview`).
   - Setările.
6. **37f — managementul SQLite.**
   - Tabele, coloane, indecși, cu DDL arătat și confirmare; reconstruirea tabelului unde
     trebuie.
   - Datele pentru confirmare vin din domeniu: `transaction::alter_plan` dă `Script`-ul (ce
     rulează e ce se arată), cu `Script::stale` (indecșii, triggerele și view-urile care
     numesc o coloană ștearsă și nu se mai refac) și `danger::script_dangers`
     (`Danger::LostObject`), lângă `schema_dangers`. Modalul doar le afișează și cere
     confirmarea.
7. **37g — fereastra „Database”.**
   - `DataSourceStore` (proiect și global), arborele cu numere, meniul `+` cu driverele.
8. **37h — modalul „Data Sources and Drivers”.**
   - `FieldSpec`, tab-urile, URL-ul, „Test Connection”, parolele după `SecretPolicy`
     (implicit `ProjectVault`).
   - `database-postgres` legat, cu TLS implicit `prefer` și `sslmode` respectat (din TODO,
     review 29).
9. **37i — protocolul pentru drivere externe.**
   - DTO-urile versionate, `ProtocolDriver`, driverul de test prin protocol, documentarea
     contractului în `03-sistem-module.md`.

10. **37j — diagrama relațiilor.** Două commit-uri:
    - componenta `diagram` comună în `ui-components` (diagrama claselor mutată pe ea, capturi
      identice) și modulul `database-diagram` cu cheile reale;
    - relațiile deduse din nume, nivelul de încredere, acceptarea ca chei virtuale și
      ignorarea lor.

## Windows

- Toate driverele sunt Rust, plus SQLite compilat din sursă. TLS-ul trece prin `rustls`.
- Redenumirea atomică la CSV se reîncearcă dacă fișierul e ținut deschis de alt proces
  (Excel): de 6 ori, cu pauze care se dublează de la 50 ms (cod doar pe Windows, `cfg`),
  apoi `DbError::InUse`, cu mesajul din `locales` (`database.file_in_use`).
- Blocarea fișierelor SQLite pe Windows e acoperită de `busy_timeout`, cu mesajul „fișierul
  e folosit de alt program”.
- Căile trec prin `config_dir()` și prin directorul proiectului, fără presupuneri Unix.

## Riscuri

- **CSV-uri uriașe:** indexul de offseturi e în fundal, iar paginile se citesc prin `seek`.
  Un commit pe un fișier mare se face în flux (copiere cu schimbări), fără să încarce tot
  fișierul.
- **SQLite deschis și de alt program:** `busy_timeout`, conflictul arătat, nimic pierdut. La
  o schimbare făcută din afară, fișierul se reîncarcă.
- **Același CSV, editat ca text și ca date:** când există modificări nesalvate, grila e doar
  citire (mai sus).
- **Diferențele de dialect:** tot SQL-ul se generează din model, cu teste pe fiecare dialect,
  nimic concatenat de mână.
- **Contractul pentru terți se schimbă:** protocolul e versionat de la început. O versiune
  necunoscută e refuzată, cu un mesaj clar.

## Stare

- **37a, gata.** Domeniul `database` are modelul pe fișiere (`source`, `capabilities`,
  `structure`, `changes`, `schema`, `dialect`, `sqlite`, `danger`, `csv`, `events`), cu
  porturile extinse și implementări implicite `Unsupported`. `Dialect` are SQL-ul standard
  (`GenericSql`) și SQLite, planul de reconstruire în 12 pași, detectarea operațiilor
  periculoase și inferența CSV, toate pure și testate. `SecretStore` stă acum în `kernel`,
  lângă `Secret` (re-exportat de `remote`; `secrets` depinde doar de `kernel`).
  `db-postgres` e în `crates/infra/database/postgres`, neschimbat. Domeniul rămâne fără
  `serde` (ca restul domeniilor): DTO-urile serializabile vin cu protocolul, în 37i.
- **37b, gata.** Domeniul are `TableRepository` (`count`, `page`, `structure`, `preview`,
  `apply`, `refresh`, `poll`, fără blocare: `Fetch::Pending` până vine) și regulile pure din
  `transaction` (ordinea: ștergeri, actualizări, inserări; „exact un rând”; `Script` = ce se
  previzualizează și ce rulează; reconstruirea SQLite pas cu pas). `Session` a primit `count`
  și `rebuild_source`, `Dialect` a primit `needs_rebuild`, toate cu implicite. Modulul
  `database-explorer` (fără UI): `SessionWorker` cu cutie poștală pe pool-ul `db` (o cerere pe
  job, ultima pagină a unui tabel câștigă, citirile dinainte la urmă, anulare), `Sessions` pe
  proiect (garda `on_end`), închidere la inactivitate (`IDLE_CLOSE`, un singur job pe pool-ul
  `db-idle`), `SessionRepository` cu cache-ul paginilor limitat în octeți, `DataChanged` după
  commit. `Latest` din `kernel` are pool-ul lui: regula lui („ultima câștigă”) e în cutia
  poștală a worker-ului, ca ordinea față de commit-uri să se păstreze. Setarea e declarată;
  blocul ei apare în setări cu 37e. Testat cu un driver fals, cu pool-ul și ceasul manuale.
- **37c, gata.** `db-sqlite` (`rusqlite` 0.40 cu `bundled`, compilat și pentru Windows): fișierul
  se deschide citire-scriere sau doar citire (opțiunea `read_only` a țintei, pusă de `Sessions`
  pentru o sursă doar citire); un fișier lipsă e o eroare (se face doar cu `create=true`), unul
  care nu e bază e refuzat la deschidere, `busy_timeout` 5 s, WAL-ul lăsat cum e, cheile
  străine pornite. Tabelele din toate schemele (`main`, `temp`, cele atașate), cu estimarea din
  `sqlite_stat1` (nimic numărat); structura din pragma-uri (ordinea cheii în
  `ColumnInfo::key_position`, AUTOINCREMENT din SQL, indecșii impliciți, cheile străine,
  `WITHOUT ROWID`); paginile cu cheile rândurilor, după `rowid` fără altă ordonare, pagina
  următoare pornind după ultimul `rowid` citit; valorile după clasa de stocare; rândurile
  afectate numărate; anularea prin progress handler. `rebuild_source`: `CHECK`-urile,
  triggerele, view-urile (și cele peste ele), indecșii pe expresii refăcuți din SQL-ul lor.
  Modulul `database-sqlite` aduce `DriverFactory` și `DataFiles` (`.sqlite`, `.sqlite3`, `.db`,
  `.db3`, confirmate de antetul de 16 octeți), înregistrate de aplicație în
  `database::drivers`, de unde `Sessions::open_source` găsește driverul după id. Teste pe
  fișiere temporare, plus un test de integrare al worker-ului și al repository-ului pe SQLite.
- **37c, completări la reconstruire.** Coloanele redenumite odată cu o reconstruire păstrează
  numele vechi cât se reface tabelul și se redenumesc la final cu `ALTER TABLE … RENAME
  COLUMN` (SQLite rescrie view-urile, triggerele, indecșii și `CHECK`-urile); o redenumire
  singură (sau un `AlterColumn` care schimbă doar numele) nu mai reconstruiește. Dacă un nume
  s-ar ciocni pe drum (două coloane care își schimbă numele), redenumirea se face în tabel.
  Ce numește o coloană ștearsă (indecși, triggere, view-uri și view-urile peste ele) nu se mai
  reface și apare în `RebuildPlan::stale` / `Script::stale`, iar `danger` îl semnalează.
  `ColumnInfo` / `ColumnDef` au `collation` și `generated` (`Generated { expression, stored }`),
  citite din `table_xinfo` (hidden 2/3) și din definiția tabelului, scrise de dialect;
  coloanele generate nu se copiază la reconstruire și nu se scriu la editare. Obiectele unei
  scheme atașate se refac calificate în schema lor. Indecșii pe expresii sau parțiali stau în
  `SqliteTable::written_indexes`, refăcuți din SQL-ul lor; unul șters rămâne șters. Un tabel
  cu o coloană `rowid` e găsit prin `_rowid_` sau `oid` (`Structure::row_id_name`); doar cu
  toate trei ascunse și fără cheie e doar citire. Verificarea cheilor străine compară
  `PRAGMA foreign_key_check` de dinainte cu cel de după (pe tabel copil și părinte, doar
  pentru tabelele schimbate): încălcările vechi nu mai blochează, cele noi anulează
  tranzacția. Inserările întorc cheile rândurilor noi (`RETURNING`, în `Applied::inserted`).
  Mai multe tabele de reconstruit merg într-o singură tranzacție (`transaction::alter_plan`,
  tabel cu tabel, în ordinea în care apar), cu cheile străine oprite în jurul ei; rularea unui
  `Script` (`transaction::run`) e acum în domeniu, iar explorer-ul doar o apelează.
- **37d, gata.** `db-csv` (fără dependențe noi: regulile CSV ale domeniului) și modulul
  `database-csv` (id `database.csv`; `DriverFactory` `csv` și `DataFiles` pentru `.csv`,
  `.tsv`, `.tab`, confirmate de un început de text, fără octeți NUL în primul KB). Un
  fișier e o sursă cu un singur tabel (numele fișierului). La deschidere se citesc doar
  primii 256 KB: dialectul (`guess_with`, tab impus la `.tsv` / `.tab`; opțiunile țintei
  `delimiter` și `header` îl pot spune), stilul (`CsvStyle`: CRLF sau LF, toate câmpurile
  între ghilimele sau doar cele care trebuie), coloanele (din antet, `column_1`… fără el,
  numele repetate numerotate) cu tipurile din `infer_types`. `OffsetIndex` ține un semn la
  fiecare 1024 de rânduri (8 octeți), se face pe bucăți de 4 MB cât cere o pagină (până la
  sfârșitul ei, ca rândurile arătate să fie în amprentă) sau până la capăt pentru total,
  se anulează între bucăți și continuă de unde a rămas; înainte de total, `TableInfo::rows`
  e o estimare din eșantion. Fișierul se deschide pe cerere, nu e ținut deschis. Sortarea
  și filtrarea (reguli pure noi în `database::eval`: `NULL` primul, numerele după valoare,
  comparațiile cu `NULL` false) se fac pentru fișiere de cel mult 64 MB (o trecere care
  ține doar locul rândurilor și cheile de sortare); peste, `Unsupported`, spus în
  `Capabilities::sorting` (`Sorting::UpTo`, câmp nou; SQLite are `Any`). Rândurile se
  găsesc după `RowKey::Line` (locul printre rândurile de date și amprenta valorilor);
  `Structure::lines` (câmp nou) dă `KeyKind::Line`. Commit-ul (`apply`, și `alter` pentru
  coloane adăugate, șterse, redenumite; restul `Unsupported`) copiază fișierul în flux
  într-un temporar `.<nume>.<uuid>.tmp` în același folder: ce nu se schimbă se copiază octet
  cu octet (BOM, ghilimele, rânduri goale, terminații, lipsa ultimei terminații), rândurile
  schimbate se verifică după amprentă, apoi `sync_all`, permisiunile originalului,
  verificarea că originalul e neschimbat (mărime, timp și hash-ul a tot ce a citit indexul)
  și redenumirea peste el. Orice eșec lasă originalul neatins și șterge temporarul. Sursa
  doar citire refuză scrierile. `DbError::InUse` e nou (și SQLite îl dă acum pentru un fișier
  ocupat). `DataChanged` vine din worker, ca la SQLite. Teste: domeniul (scanner pe bucăți
  de orice mărime, câmpuri, stil, `eval`), `db-csv` (dialecte, antet, ghilimele cu rânduri
  noi, 50 000 de rânduri citite prin `seek`, anulare și reluare, octeții exacți după
  commit, conflict la schimbare din afară — și la aceeași mărime și același timp, prin
  hash —, temporarul șters, doar citire, coloane), plus integrarea worker-ului și a
  repository-ului pe un CSV real. Fișierul temporar e vizibil o clipă pentru watcher-ul
  proiectului; marcarea commit-ului ca „al nostru” vine cu vederea, în 37e.
- **37e, citirea, gata.** `ui_components::data_grid` (generic, nu știe de baze de date):
  portul `GridSource` (coloane, `total` — știut, estimat, în curs —, starea și valorile
  rândurilor împrumutate prin `CellRef`, `request(query, rânduri)`, `poll`, `capabilities`,
  `cancel`, `refresh`, `count`) și `DataGrid`: paginarea `|< < 1–500 din N > >|` cu mărimea
  aleasă (10, 100, 500, 1000, „toate”, cu avertisment peste 100 000), totalul estimat numărat
  la click; sortarea din antet (crescător → descrescător → fără; cu ⇧ pe mai multe coloane,
  numerotate), câmpurile `WHERE` și `ORDER BY` citite într-un filtru structurat (`GridFilter`:
  comparații, `IS [NOT] NULL`, `LIKE '%text%'`, `AND` / `OR` / `NOT`, paranteze, numele
  citate; eroarea arătată sub câmp) și scrise înapoi ca text; filtrele rapide din meniul
  celulei; coloanele redimensionate, mutate prin tragere, ascunse / arătate („Coloane ›”),
  potrivite la conținut (dublu-click pe margine) și măsurate singure la prima pagină, cu
  așezarea ținută pe fișier și tabel (peste sesiunea proiectului: vezi completările); numerele rândurilor, `NULL`
  distinct, selecția de celule / rânduri / coloane cu mouse-ul (⇧, ⌘) și tastele, ⌘F în
  rânduri, rândul transpus, copierea ca TSV (⌘C), CSV, JSON, `INSERT`-uri SQL sau Markdown,
  agregatele selecției în bara de jos, anularea unei pagini lente, stările de încărcare și
  eroare. Se desenează doar rândurile din vedere; textul unei celule e modelat o dată (îl
  ține sistemul de text) și tăiat la coloană fără căutări repetate; cererile pleacă doar
  când se schimbă rândurile din vedere. Meniul de click dreapta e al aplicației
  (`Viewer::context_menu`, rândurile întoarse ca `view-menu:` → `Viewer::command`).
- **Vederea de date** (`database-explorer`, `view`): `DataViewProvider` (prin
  `database::drivers::for_file`; `DataFiles::text` nou spune dacă fișierul e text) deschide
  SQLite ca `Replace` — `ViewerProvider::priority` nou, `binary-files` e `Fallback` și
  aplicația îi ordonează — și CSV ca `Preview` cu datele întâi (`Viewer::opens_alone`); un
  CSV prea mare pentru editor se deschide doar ca date. Fiecare fișier are sursa lui în
  `Sessions` (`open_file`: un id UUID v4 pe fișier și proiect, împărțită de vederi, închisă
  cu ultima prin `FileLease`); un fișier din afara proiectelor are `Sessions` proprii.
  `TableSource` adaptează `TableRepository` la grilă: blocuri aliniate cerute pe rând
  (worker-ul păstrează doar ultima pagină a unui tabel), doar cele din jurul vederii ținute,
  totalul numărat în fundal (peste 1 000 000 estimat, numărat la cerere), un bloc scurt dă
  totalul. În stânga lista tabelelor (tabele, apoi view-uri, cu numerele lor), în dreapta
  „Date” / „Structură” (coloane cu tipul, nul, implicit, cheie, auto-increment, generată,
  colaționare; indecși; chei externe), doar citire. Cât timp textul CSV are modificări
  nesalvate, grila arată ce e pe disc, cu mesajul „salvează întâi” (`Viewer::reads_file` /
  `set_unsaved`: aplicația nu mai trimite textul întreg la fiecare tastă). Un fișier schimbat
  de alt program (sau jurnalul SQLite de lângă el) se recitește; commit-ul nostru nu
  (mărimea și timpul fișierului după el). Tab-urile cu previzualizare (Markdown, CSV) sunt
  acum fișiere text pentru salvarea automată și reîncărcare (`Tab::holds_text`), ca textul
  CSV să urmeze commit-urile.
- **Setări noi:** `[database] page_size` (10, 100, 500, 1000, toate; implicit 500),
  `csv_sample` (înregistrările din care se ghicesc tipurile; opțiunea `sample_rows` a
  driverului CSV, cu eșantionul în octeți crescut pe măsură, până la 8 MB), `confirm_delete`
  (folosită la editare), lângă `idle_close`; se aplică pe loc (grilele deschise iau mărimea
  nouă; pornirea închiderii cu sesiuni deschise pornește acum supravegherea, nu abia la
  următoarea conexiune). Comenzile vederii: `database.refresh` (F5), `database.transpose`.
- **Capturi noi** (grupul `data`, fișiere făcute de captură într-un folder temporar):
  `data-sqlite`, `data-structure`, `data-query`, `data-transpose`, `data-menu`, `data-csv`,
  `data-csv-split`.
- **37e, editarea, gata.** `GridSource` a primit editarea (cu implicite „doar citire”):
  `editing` (permis, sau motivul), `mark` (schimbat / nou / de șters), `set`, `insert` (gol
  sau clonat: cheia și coloanele generate lăsate motorului), `delete` (un rând nou doar
  dispare; din nou: nemarcat), `revert` (selecția sau tot), `pending`, `new_rows` (id-uri de
  la `NEW_ROWS` în sus, arătate după rândurile paginii, numerotate `+1`…), `is_default`.
  Grila: editorul în celulă (↵, dublu-click sau tastat peste celulă; ↵ / ⇥ / ↑ ↓ aplică și
  merg mai departe, Esc renunță), valoarea citită după clasa coloanei (`parse_value`: întreg,
  zecimal, bool, dată, JSON, text; gol la o coloană nulabilă e `NULL`; o valoare greșită ține
  editorul, cu motivul), spațiul comută un bool, ⌫ / ⌦ pun `NULL` (sau șterg rândurile
  întregi selectate), editorul mare pe mai multe rânduri (`TextArea` nou, ⇧↵ aplică, nu se
  închide la un click alături), rânduri adăugate / clonate / șterse, „Revert” pe selecție sau
  pe tot, schimbările colorate (celula, rândul nou, rândul de șters tăiat, bara de lângă
  număr), „Trimite (N)” în bara de unelte, numărul în bara de jos, motivul când nu se poate
  edita. Coloanele generate și valorile binare nu se editează.
- **Schimbările în așteptare** stau în `TableSource` (`edits::Edits`: rândurile după
  `RowKey`, cu valorile citite pentru conflict; rândurile noi; cele de șters), deci rămân la
  altă ordine, alt filtru sau o reîmprospătare; `change_set` le face `ChangeSet` (ștergeri,
  actualizări, inserări). Nu se poate edita: sursa doar citire, un view, un driver fără
  `edit_rows`, un tabel fără cheie, textul CSV nesalvat, un submit în curs.
- **„Trimite” (⌘↵)** arată întâi ce rulează: modalul modulului (`database.preview` →
  `host.open_modal`, extensia `view-data-preview`; nu se închide la click afară) cu
  instrucțiunile exacte din domeniu (`TableRepository::preview`, același `Script` care
  rulează), cuvintele cheie colorate, sau, la o sursă fără SQL (CSV), câte o linie pe
  schimbare. Ștergerile cer o bifă (`[database] confirm_delete`). Răspunsul vine înapoi
  printr-un slot comun; modalul închis altfel e „nu”. Apoi `apply` într-o tranzacție:
  reușit — schimbările dispar, rândurile se recitesc, banda spune câte rânduri s-au schimbat
  și cheile rândurilor noi (`Applied::inserted`); refuzat (conflict, fișier ocupat) — nimic
  pierdut, banda spune de ce. Abatere: cheile întoarse sunt arătate, nu folosite la
  re-cheierea rândurilor noi, pentru că editarea e oprită cât rulează submit-ul și totul se
  recitește după el. (Registrul `ValueEditorProvider`, exportul întregului rezultat,
  așezarea ținută peste sesiune și tastele grilei în Split au venit în completările de mai
  jos.)
- **Comenzi noi** ale vederii: `database.submit` (⌘↵), `database.delete_rows` (⌘⌫),
  `database.add_row`, `database.clone_row`, `database.revert`; capturi noi: `data-edit`,
  `data-preview`.
- **37e, completări, gata.**
  - **Editorii de valori** sunt un punct de extensie: `data_grid::value_editor`
    (`ValueEditorProvider`: `fit` după coloană — clasă, tip numit, prioritate —, textul de
    pornire, citirea înapoi cu motivul, editorul mare, „aceeași valoare”). Un modul îl
    contribuie (`Arc<dyn ValueEditorProvider>`), aplicația îl înregistrează la pornire;
    câștigă cel mai potrivit, cele înregistrate înaintea celor incluse la egalitate.
    `ValueEditorSpec` e un furnizor doar din date (clase, tipuri, `ValueFormat`: text,
    întreg, zecimal, bool, dată / oră / amândouă, JSON, o listă de valori, brut) — ce poate
    declara un driver extern (37i) fără cod aici; editorii incluși sunt astfel de
    specificații. Noi: JSON (arătat indentat în editorul mare, verificat, păstrat compact;
    același JSON scris altfel nu e o schimbare) și data / ora verificate (ISO, cu `/` sau
    `.`, anul la sfârșit ca într-un CSV; un tip numit `date` sau `time` doar atât).
  - **Exportul întregului rezultat** (`export`): din meniul antetului „Exportă toate
    rândurile ›” (CSV, TSV, JSON, `INSERT`-uri SQL, Markdown) și comanda `database.export`;
    fișierul se alege cu „Save As” al sistemului (`AppRequest::PickSavePath`, răspunsul
    `picked-save:`), extensia aleasă spune formatul. Toate rândurile, cu ordinea și filtrul
    grilei, coloanele cum sunt arătate, se cer câte 2000 printr-un client propriu al
    worker-ului (paginile grilei își păstrează rândul) și se scriu de joburi pe pool-ul
    `db`, cu formatele copierii (`CopyStream`: aceiași octeți ca `copy_text`, pe bucăți),
    într-un fișier ascuns lângă țintă, redenumit peste ea la final; anulat, eșuat sau cu
    vederea închisă, temporarul se șterge și ținta rămâne cum era. Progresul: o bandă cu
    rândurile scrise din total, bara și „Anulează”, plus operația în Task Manager
    (`ActivityKind::Export`, unitatea `Rows`). „Copiază toate rândurile ca ›” pune un
    rezultat mic în clipboard (cel mult 10 000 de rânduri).
  - **Așezarea grilei** (lățimi, ordine, coloane ascunse, sortarea, filtrul `WHERE`,
    mărimea paginii aleasă în grilă) se ține pe sursă și tabel în
    `<dosarul proiectului>/data-grids.toml` (`database::layouts`: modelul și regulile, cel
    mult 400, cele mai noi ultimele; `storage::TomlGridLayouts` scrie atomic), scrisă de un
    job după 600 ms fără altă schimbare; la redeschidere revine, coloanele care nu mai sunt
    (și un filtru care nu se mai înțelege) se ignoră. Fișierele din afara proiectului doar
    cât e proiectul deschis.
  - **Tastele în Split:** jumătatea clicată le are (`Viewer::keys_in_split`, grila îl
    cere): săgețile, ⇧, ↵, tastatul peste celulă, ⌘↵, ⌘C, ⌘F, F5, ⌫ / ⌦ merg la grilă, iar
    editorul textului nu primește nimic (nici editările lui, ⌘D); un clic în text i le dă
    înapoi. Previzualizările fără taste (Markdown) lasă totul editorului, ca înainte.
  - **Câmpurile `WHERE` / `ORDER BY`:** ⌘A, ⌘X, ⌘Z / ⇧⌘Z (istoric nou în `TextInput`,
    tastatul la rând un pas, niciodată pentru parole), dublu-clic pe cuvânt, triplu-clic tot,
    ⇧-clic și tragerea selectează; ⌘A în câmp sau în editorul celulei nu mai selectează
    celulele. Motivul unui filtru neînțeles e o fereastră mică sub câmp, deasupra antetului
    (nimic nu o mai taie).
  - **Capturi noi:** `data-csv-split-keys`, `data-json`, `data-where-error`, `data-export`,
    `data-exported`.
- **37f, gata.** Managementul structurii, din vederea de date, cât poate driverul
  (`SchemaChanges`, cu `foreign_keys` și `reorder` noi și `allows(change)`; worker-ul refuză
  ce nu e permis): la SQLite totul, la CSV coloane adăugate, redenumite, mutate și șterse.
  - **Domeniul:** `SchemaChange::CreateTable(TableDef)` (coloane, cheie primară compusă,
    chei externe, `UNIQUE`, `CHECK`, `WITHOUT ROWID`), `AddForeignKey`, `DropForeignKey`,
    `MoveColumn`; `IndexInfo` are ordinea coloanelor (`descending`) și condiția unui index
    parțial (`predicate`, citită de `db-sqlite` din SQL-ul indexului prin
    `sqlite::index_predicate`). Dialectul le scrie (`create_table(&TableDef)`,
    `index_columns`, `index_predicate`, `type_suggestions`, `collations`); la SQLite cheile
    externe și ordinea coloanelor merg prin reconstruire. O coloană ștearsă pe care o
    numește un view, un trigger, un index scris sau un `CHECK` reconstruiește tabelul
    (`transaction::drops_named`: `DROP COLUMN`-ul SQLite ar refuza), cu ce se pierde în
    `Script::stale`. `transaction::alter_preview` dă `Script`-ul și ce rescrie SQLite la o
    redenumire (`sqlite::rewritten`); `Danger::DropIndex` nou. `layouts::follow` /
    `GridLayout::follow`: așezarea grilei urmează coloanele și tabelele redenumite, le
    uită pe cele șterse (filtrul rescris prin `sqlite::rename_name`).
  - **Worker-ul:** `Request::PlanAlter` (planul, fără să ruleze) și
    `Request::AlterAsShown` (replanificat și refuzat ca `Conflict` dacă nu mai e exact ce
    s-a arătat); `SessionWorker::on_altered`, pus de `Sessions::open_file`, mută așezările
    înainte de `DataChanged`.
  - **Modulul** (`schema`: `form` pur, modalul, desenul): un singur modal
    (`host.open_modal`, extensia `view-data-schema`, comanda `database.schema`) pentru tabel
    nou / redenumit / șters, coloană adăugată / schimbată / mutată / ștearsă, index nou /
    șters, cheie externă adăugată / ștearsă. Câmpurile sunt `TextInput` (⌘A, ⌘Z / ⇧⌘Z,
    ⌘X), tipurile și colaționările sugerate de dialect, listele de ales (coloane, tabelul
    țintă, acțiuni, locul) deschise peste formular; ⇥ / ↓ ↑ trec prin câmpuri, ↵ trimite
    când se poate, Esc închide. Sub formular, ce rulează exact (instrucțiunile din domeniu,
    împărțite pe rânduri la lățimea cardului) sau, la CSV, schimbările în cuvinte; motivele
    de validare traduse; ce se pierde (date, view-uri, triggere, indecși, rândurile când se
    știu) listat și bifat în subsol înainte ca butonul să se activeze; ce rescrie SQLite,
    ca informație. Rulează într-o tranzacție; eșuat, modalul rămâne cu motivul, fișierul
    neatins. Reușit: banda vederii, lista tabelelor și grila refăcute (și în celelalte
    vederi ale sursei, prin `DataChanged { structure: true }`), tabelul creat sau redenumit
    arătat.
  - **De unde:** meniul listei de tabele (tabel nou, redenumește, șterge), butoanele
    „+ Coloană / + Index / + Cheie externă” din titlurile secțiunilor „Structură”, meniul
    unui rând al structurii (schimbă, mută, șterge), dublu-click pe o coloană.
  - **Capturi noi:** `data-create-table`, `data-alter-column`, `data-drop-column`,
    `data-create-index`; `data-structure` arată acum butoanele secțiunilor.
- **37g, gata.** Fereastra „Database”, sursele configurate și fișierul lor.
  - **Domeniul:** `DataSource::is_configured` (o sursă pe fișier are fișierul, una de server
    are host-ul sau baza), `recent_drivers` (driverele surselor, cele mai noi întâi, fiecare o
    dată), `unique_name` (`shop`, `shop (2)`…), `is_secret_option` (opțiunile cu nume de
    parolă nu se scriu niciodată) și `NoStore` (o listă care nu ține nimic: un fișier din
    afara proiectelor, testele, capturile).
  - **`storage::TomlDataSources`** implementează `DataSourceStore`: sursele proiectului în
    `<dosarul proiectului>/data-sources.toml`, cele globale în `config_dir()/data-sources.toml`,
    scrise întregi și atomic, fără nicio parolă (nu există câmpul; o opțiune numită ca una e
    lăsată afară). Fișierul unei surse din proiect e relativ, cu `/`. Un fișier care nu se
    poate citi ca listă (TOML stricat, o sursă fără `id` sau fără driver, un port greșit) e o
    eroare, nu o listă goală: nu se scrie niciodată peste el.
  - **`catalog::Catalog`** (în `Sessions`, deci al proiectului): listele se citesc în fundal
    abia când fereastra le cere, o dată; o schimbare se face pe loc în memorie, se anunță
    (`DataSourcesChanged`) și se scrie în fundal, o scriere pe rând, cea mai nouă la urmă; o
    scriere eșuată e spusă în fereastră. Lista globală schimbată într-un proiect ajunge și în
    celelalte (fiecare o anunță pe bus-ul lui); sursele proiectului nu ies din el.
    `Sessions::source_worker` deschide o sursă configurată după id (driverul din registru,
    printr-o funcție pe care testele o înlocuiesc), cu așezările grilelor ținute după id;
    un fișier configurat ca sursă, deschis din arborele proiectului, e chiar acea sursă (id-ul
    și „doar citire” ei), iar fișierele deschise singure sunt listate (`open_files`) și
    anunțate când apar și dispar.
  - **Fereastra** (`window`, `ToolWindowProvider` pe dreapta, după Structură, Task Manager și
    pânză, iconița `database` nouă în setul inclus, alături de `layers`, `column`, `index`;
    comanda `database.window`; meniul butonului e un `MenuContribution` cu același id:
    reîmprospătează, restrânge tot) se face doar la prima afișare. Antetul aplicației are
    `+`, reîmprospătarea și „restrânge tot”; dedesubt, filtrul (`ListFilter`: ce se
    potrivește dintre nodurile citite, cu drumul până la ele), apoi arborele (`tree`, model
    pur): sursă › schemă (doar când driverul dă una: SQLite-ul își numește schema principală
    „fără schemă”, deci tabelele lui stau direct sub sursă; o bază atașată apare ca schemă) ›
    tabele / view-uri › tabel › coloane / indecși / chei externe, cu numerele (tabelele unei
    surse și ale unei scheme, rândurile estimate ale unui tabel când motorul le ține, coloanele
    lui odată citit), iconițe pe fel (cheie primară, cheie externă, view). Un nod se citește
    abia când e deschis, prin worker-ul sursei (cereri `Tables` / `Structure`, în fundal), cu
    rândul „Se încarcă…” sau eroarea lui; o sursă neconfigurată nu se conectează (o notă).
    Evenimentele proiectului (`DataSourcesChanged`, `DataChanged` cu structură) intră într-o
    coadă care le contopește (`kernel::queue::Coalescing`), luată la `poll`. Ascunsă, își
    lasă cererile (le reia la afișare); sesiunile se închid ca oricare, la inactivitate.
  - **Tastele:** un click îi dă tastatura (`ToolPanel::takes_keys`, nou); ↑ ↓ ⇞ ⇟ Home End,
    → deschide / intră, ← închide / urcă la părinte, ↵ deschide tabelul sau nodul, tastatul
    pornește filtrul, Esc îl golește, ⌘C copiază numele, F5 reîmprospătează
    (`database.refresh_sources`, un `ViewAction` al ferestrei: `ToolPanel::keyed`, nou, iar
    aplicația îi trimite comenzile când are tastatura).
  - **Deschiderea unui tabel** (dublu-click, ↵, „Deschide datele” / „Deschide structura”):
    pentru un fișier, vederea fișierului (aceeași filă), cu tabelul cerut (`want_table`, luat
    de vedere la `poll`); pentru o sursă configurată fără fișier, aceeași vedere de date după
    sursă și tabel (`OpenSource`, extensia virtuală `view-data-source`, comanda
    `database.open_source`), cu așezările grilei ținute după id-ul sursei. Deschiderea
    fișierelor a rămas neschimbată.
  - **Meniurile rândurilor** (`ToolPanel::menu_at` / `menu_command`, noi, rândurile întoarse
    ca `panel-menu:`): deschide datele / structura, modalurile 37f (tabel nou, redenumește,
    șterge, coloană, index, cheie externă; schimbă / șterge coloana, șterge indexul sau cheia
    — ce cere structura tabelului o citește întâi), copiază numele / numele complet,
    reîmprospătează, elimină sursa (datele rămân neatinse), păstrează un fișier deschis ca
    sursă.
  - **Meniul `+`** (`ToolPanelAction::Menu`, nou, sub buton): „Sursă de date ›” cu driverele
    din registru, cele recente sus, apoi toate (cu iconițe: fișier de bază, fișier text,
    server), și „Importă din fișier…” (driverul după fișier). Un driver pe fișier cere
    fișierul prin selectorul sistemului (`AppRequest::PickFile`, nou: răspunsul
    `picked-file:` merge întâi la panourile laterale); unul de server adaugă sursa
    neconfigurată, cu mesajul că conexiunea se setează în 37h. Același fișier adăugat a doua
    oară doar se arată.
  - **Capturi noi** (grupul `database`): `database-tree`, `database-add`, `database-menu`.
  - **Nefăcut, cu motiv:** tragerea unui fișier din arborele proiectului peste fereastră —
    arborele proiectului știe doar mutări în el însuși, nu există încă tragere între panouri.
    Eliminarea unei surse nu cere confirmare: se șterge doar configurarea, nu datele.
- **37h, gata.** Modalul „Data Sources and Drivers”, parolele securizate și PostgreSQL legat.
  - **Domeniul:** `fields` (`FieldSpec`, `FieldKind`, `Binding`, `FieldTab`,
    `ConnectionSpec`; doar date, deci și pentru driverele din 37i), valorile citite / scrise
    în sursă (parola niciodată), validarea (`FieldError`), URL-ul în ambele sensuri
    (`to_url` / `from_url`: `postgresql://user@host:port/db?opțiuni`, `sqlite:cale`, `jdbc:`
    înțeles; o parolă din URL e dată înapoi, nu păstrată). `Driver::connection()` (implicit:
    câmpurile unui server sau fișierul), `Session::dialect()` (dialectul serverului
    conectat). `tls::SslMode` (cele șase moduri libpq, din opțiunea `sslmode` sau din `Tls`,
    implicit `prefer`; o valoare greșită e eroare, nu o conexiune mai slabă) și
    `Verification`. `postgres`: `Feature` pe versiuni (jsonb 9.4, `ON CONFLICT` 9.5, `ADD
    COLUMN IF NOT EXISTS` 9.6, identity / partiții / `pg_sequence` 10, `prokind` / `INCLUDE`
    11, coloane generate / fără OIDS 12, `reltuples = -1` 14, generate virtuale 18),
    `estimated_rows`, `version_text` și dialectul `Postgres { version }`: literali la fel pe
    orice server (`E'…'` cu backslash, `decode('…', 'hex')` pentru bytea, `NaN`), `RETURNING`,
    `SERIAL` în loc de identity sub 10, coloanele generate refuzate sub 12 (virtuale sub 18).
    `DataSource` are `comment` și `schemas` (cele arătate), scrise de `TomlDataSources`.
  - **Parolele** (`credentials`, cerința: local, la nivel de proiect, securizat): după
    `SecretPolicy` — `ProjectVault` (implicit) e un seif per sursă în
    `<dosarul proiectului>/data-source-secrets/<uuid>`, criptat ChaCha20-Poly1305 de
    `secrets::ProjectVault` (același ca la remote), cheia în keychain (`<id>.database`,
    `vault:data-source/<uuid>`); `Keychain` direct în keychain; `Session` în memorie până se
    închide proiectul; `Never` cerută de fiecare dată (dată unei singure conectări). O sursă
    globală nu are proiect: „în proiect” devine keychain. Mutarea între proiect și global sau
    schimbarea politicii mută parola (întâi scrisă unde merge, apoi ștearsă de unde era);
    ștergerea sursei îi șterge seiful și cheia. Totul pe pool-ul `db` (keychain-ul poate
    aștepta), niciodată în loguri sau în fișierul surselor. `secrets` e acum și al lui
    `database-explorer` în `MODULE_OWNED_INFRA`.
  - **Modalul** (`sources`: `form` pur, modalul, desenul; `host.open_modal`, extensia
    `view-data-sources`, comanda `database.data_sources`): în stânga sursele proiectului și
    cele globale (iconițele driverelor, filtrul, `+` cu driverele, `−`, duplicare cu parola
    copiată, mutare proiect ↔ global cu butonul și mesajul despre unde ajunge parola), în
    dreapta Nume, Comentariu și tab-urile General (câmpurile driverului, gazdă și port pe
    un rând, parola mascată cu ochiul ei și unde se păstrează, URL-ul ținut în pas în ambele
    sensuri), Opțiuni (doar citire, opțiunile driverului, scriptul la pornire rulat la fiecare
    conectare), SSH/SSL (`sslmode`, fișierele CA / certificat / cheie cu „Alege…”), Scheme
    (bifate din cele găsite de test; arborele arată doar cele alese) și Avansat (opțiunile
    proprii, nume și valoare; una cu nume de parolă e refuzată). „Testează conexiunea” rulează
    în fundal, se poate opri, arată serverul cu versiunea și timpul sau motivul. Aplică / OK /
    Anulează; cu schimbări neaplicate, ×, Esc și Anulează întreabă întâi
    (`Viewer::may_close`, nou). Problemele (nume gol, port greșit) opresc aplicarea, cu
    sursa selectată.
  - **De unde:** `+` › un driver de server (o sursă nouă în modal; cele pe fișier aleg
    fișierul ca înainte), „Surse de date și drivere…” în `+` și în meniul butonului,
    „Proprietăți…” pe o sursă, dublu-clic pe nota unei surse neconfigurate sau pe eroarea
    ei (cu parola în focus). Arborele arată versiunea serverului după conectare.
  - **PostgreSQL** (`database-postgres`, id `database.postgres`, cu `db-postgres`):
    TLS după `sslmode` (rustls: fără verificare la `prefer` / `require`, lanțul la
    `verify-ca` sau cu CA dat, lanțul și numele la `verify-full`; `allow` încearcă întâi
    necriptat), certificat de client, `connect_timeout`, `application_name`. Tabelele (cu
    vederi materializate de la 9.3 și partiționate de la 10, fără partiții), structura
    (identity, generate, colaționări, cheia primară în ordine, indecși cu ordine și predicat,
    cheile externe) din catalog, cu interogări valabile pe 9.2 (fără `LATERAL`, `WITH
    ORDINALITY`, `array_position`) și coloanele noi doar unde există; paginile după cheia
    primară (`(k1, k2) > (…)`, fără `OFFSET` mare) sau `OFFSET` cu altă ordonare; bytea
    citit ca octeți; schimbările prin tranzacțiile domeniului în dialectul versiunii.
  - **Teste:** domeniul (câmpuri ↔ URL pe server și fișier, validare, `sslmode`, dialectul pe
    versiuni), `db-postgres` (încercările și verificarea TLS, fișierele de certificat greșite,
    interogările de catalog pe versiuni), `credentials` (seiful criptat pe disc, politici,
    mutări), modalul (câmpuri, URL, urmărirea schimbărilor, adăugare / duplicare / ștergere,
    filtru, probleme, scheme, testul în fundal cu oprire, aplicarea cu parola nicăieri în
    proiect, întrebarea la închidere). Integrarea pe servere reale e în
    `tables_structure_pages_and_changes_on_real_servers`, rulată doar cu
    `EDITOR_TEST_POSTGRES_URL` (unul sau mai multe URL-uri despărțite prin spații, de
    exemplu un 9.6 și un 17); nu a fost rulată aici.
  - **Capturi noi:** `database-sources`, `database-sources-ssl`, `database-sources-schemas`,
    `database-sources-ok`, `database-sources-failed`; `database-add` are acum PostgreSQL și
    „Surse de date și drivere…”.
  - **Nefăcut, cu motiv:** tunelul SSH (câmpurile arătate dezactivate, cu nota: nu există
    încă un client SSH pe care modulul să-l poată folosi); închiderea la inactivitate per
    sursă și modul auto-commit (schimbările trec mereu printr-o tranzacție; inactivitatea e
    setarea globală); un buton în antetul ferestrei (ar fi schimbat capturile ferestrei:
    comanda e în `+` și în meniul butonului); `Capabilities` pe versiune (aceleași de la 9.2:
    ce diferă refuză dialectul).
- **37i, gata.** Protocolul pentru drivere externe; contractul e descris în
  `03-sistem-module.md` §6 (handshake, mesaje, versiuni, securitate, ciclul de viață, cum
  scrii un driver).
  - **Organizarea:** `db-protocol` (`crates/infra/database/protocol`, nou): mesajele
    versionate (`dto`, `serde`), JSON-RPC 2.0 în cadre `Content-Length` ca la LSP (`wire`:
    metodele, codurile de eroare, base64 pentru octeți, `NaN` / `Infinity` ca text),
    conversiile cu modelul domeniului (`convert`, în ambele sensuri), capătul editorului
    (`client`), `ProtocolDriver` / `ProtocolSession` (`driver`), manifestele (`manifest`),
    capătul driverului pentru drivere în Rust (`server::serve`) și driverul de test în
    memorie (`toy`, feature `toy`). Modulul nou `database-external` (`database.external`)
    citește `<config>/drivers/<nume>/driver.toml` și înregistrează driverele găsite (fabrica,
    fișierele după extensie și antet, editorii de valori din manifest); `db-protocol` e al lui
    în `MODULE_OWNED_INFRA`. Abatere de la „`database::protocol`”: DTO-urile nu stau în
    domeniu, care rămâne fără `serde` ca restul domeniilor — sunt limbajul publicat al
    granței (se schimbă doar cu o versiune nouă a protocolului, modelul liber), iar conversia
    e a adaptorului; `serde_json` (fără crate nou, doar `raw_value`) și `toml` erau deja în
    workspace.
  - **Domeniul:** `DriverFactory` are `id` / `name` ca `Cow` (și din manifest), `create` ca
    funcție care poate ține date (`CreateDriver`), `external` (marcajul) și constructorul
    `DriverFactory::new` pentru cele incluse; `DataFiles` are extensii și potrivirea
    antetului ca date (`DataFiles::new`, `starting_with`, `none`). `drivers::register_discovery`
    (rulată o dată, la prima cerere a driverelor; cine întreabă între timp așteaptă) și
    `drivers::is_registered` (fără descoperire: un driver găsit nu ia locul unuia al
    aplicației). `DbError::Driver(DriverFailure)` nou: `NotStarted`, `Protocol { theirs, ours }`
    (`driver_too_old`), `Stopped`, `TimedOut`. `Session::plan_alter` nou (implicit `None`):
    o sursă fără dialectul editorului își scrie instrucțiunile pentru schimbările de structură;
    `Script::statements` și `transaction::run` sar instrucțiunile goale.
  - **Cine scrie SQL-ul:** driverul spune (`sql`): editorul, într-unul din dialectele lui
    (`generic`, `sqlite`, `postgres` cu versiunea dată la `connect`), rulat prin `execute` —
    previzualizarea e exact ce rulează, cu regulile editorului (ordinea, „exact un rând”,
    rollback); sau nimeni (`none`): schimbările de rânduri trimise ca atare (`apply`, toate
    sau niciuna, conflict la un rând schimbat), cele de structură fie făcute de driver
    (`alter`, descrise în cuvinte), fie scrise de el (`plan_alter` → `Script`), arătate,
    replanificate la „Aplică” (altfel conflict) și rulate de editor pas cu pas. Explorer-ul
    (`execute::plan` / `alter`) folosește `plan_alter` pentru sursele fără dialect; CSV-ul
    rămâne neschimbat (nu planifică nimic). `sql` din handshake trebuie să fie cel din
    manifest, capabilitățile din handshake țin sesiunea.
  - **Transportul:** `process::start` nou (programul ținut pornit, cu cele trei pipe-uri,
    propriul grup de procese, `CREATE_NO_WINDOW` pe Windows, oprit cu tot ce a pornit la
    `stop` / drop), fără shell; programul găsit lângă manifest (pe Windows și `.exe`,
    `.cmd`, `.bat`) sau în `PATH`. Un proces pe sesiune: pornit la prima cerere (pe pool-ul
    `db`), `initialize` (15 s), `connect`, cereri cu id, `cancel` (5 s, apoi oprit), timpul
    unei cereri din manifest (implicit 600 s, apoi oprit), `exit` la închidere (și la
    inactivitate, `idle_close`), oprit după 1 s pe un fir al lui. O cădere sau un mesaj în
    afara protocolului: `Stopped` cu codul de ieșire și ultima linie de eroare, sesiunea
    pierdută (worker-ul o închide ca pe o conexiune pierdută), următoarea cerere repornește
    programul. Mesajele utilizatorului în `locales` (`database.driver_too_old`,
    `driver_too_new`, `driver_not_started`, `driver_stopped`, `driver_timed_out`), prin
    `grid_source::message`.
  - **Versiunile:** editorul vorbește 1–1 (`db_protocol::VERSIONS`); fără versiune comună,
    refuzat cu ambele intervale — din manifest înainte să pornească ceva, sau la handshake —,
    cu mesajul „driverul e prea vechi” sau „editorul e prea vechi”.
  - **Parola:** doar în `connect`, pe pipe; niciodată în argumente, mediu (un `[env]` cu nume
    de parolă e refuzat) sau log; cadrele care o conțin șterse după scriere / citire; stderr-ul
    programului citit pe linii, cu parola înlocuită cu `••••`, ultimele 64 păstrate, fiecare
    în log la `debug`.
  - **UI:** meniul `+` arată driverele externe cu marcajul „extern” (în locul scurtăturii),
    lista de drivere a modalului „Surse de date și drivere” la fel. Fără driver instalat nu se
    vede nimic nou; capturile `about` și `settings-8` numără un modul în plus (57), cu numele și
    descrierea lui în `locales` (`module.database_external*`).
  - **Teste:** `db-protocol` (cadre, base64, erori în ambele sensuri, modelul prin JSON și
    înapoi, manifestele și descoperirea, stderr curățat, cereri anulate și răspunsuri în
    afara protocolului, driverul de test servit printr-un pipe în același proces: citiri
    identice cu cele directe, conflict, instrucțiunile proprii rulate, anulare, cădere și
    repornire, versiuni refuzate în ambele sensuri, parolă greșită, timeout), `process`
    (`start`), domeniul (descoperirea, `starting_with`, instrucțiunile goale), modulul
    `database-external` (instalare, id ocupat, manifest stricat, editori de valori) și
    capăt-la-capăt în `explorer/tests/protocol.rs`: binarul testului pornit ca program de
    driver (fără harness, ca stdout-ul să fie al protocolului) — worker-ul, repository-ul și
    grila peste el, anularea, căderea cu repornire, versiunea refuzată, parola doar prin pipe
    și scoasă din stderr, iar SQLite prin protocol dă exact ce dă direct (tabelele,
    structura, paginile cu cheile, instrucțiunile planificate ale unei reconstruiri,
    schimbările).
  - **Nefăcut, cu motiv:** gazda WASM (vine cu modulele dinamice, ADR-0006; mesajele sunt
    aceleași); previzualizarea schimbărilor de rânduri scrisă de driver (ar cere o cerere
    asincronă în previzualizarea grilei, azi sincronă din dialect: la `sql = "none"` se arată
    schimbările în cuvinte, exact ce se trimite); iconița din descriere (lista arată iconița
    felului, ca la driverele incluse: `ViewMenuEntry` are doar nume de iconițe statice);
    procesele driverelor nu apar încă în Task Manager; un rezultat `execute` e limitat la
    100 000 de rânduri (nimic din ce rulează azi nu se apropie; consola SQL va avea nevoie de
    rânduri în flux).
- **37j, prima parte (diagrama cu cheile reale), gata.**
  - **`ui_components::diagram`:** partea comună a diagramelor, mutată din `diagrams`:
    `Layout` / `Card` / `Edge` (cu felul liniei generic), `Viewport` (panoramare, zoom în
    jurul mouse-ului, potrivire cu limite, centrare, apăsare deosebită de tragere),
    `History`, desenul cardului (rama, antetul cu insigna, rândurile cu detaliul în dreapta,
    „încă n”), al liniilor (continue / punctate, cu săgeată goală, plină sau fără, și spre
    stânga acum), mostra din legendă, bara de sus și ←. Diagrama claselor PHP o folosește
    fără nicio schimbare: captura `diagram` e identică pixel cu pixel.
  - **Domeniul (`database::diagram`):** `SchemaGraph` din structurile tabelelor (sortat,
    deci determinist): legăturile din cheile externe, cu coloanele lor, cheile compuse,
    auto-referințele, tabelul țintă găsit după schemă și nume (oricum ar fi scris), altfel
    în schema tabelului, altfel singurul cu acel nume; unul necitit (altă schemă) apare ca
    tabel extern, cu coloanele numite. Cardinalitatea: 1–1 când coloanele cheii sunt cheia
    primară sau un index unic întreg, altfel 1–n. `neighborhood` (vecinii la 1–3 legături,
    în ambele sensuri), `matching` (filtrul după nume), `label`
    (`orders.user_id → users.id`).
  - **Granița dintre module:** portul `SchemaAccess` din domeniu (citirea structurii în
    fundal, cu progres și anulare; deschiderea datelor unui tabel ca `Opening`: fișierul
    sau o comandă) și `DiagramRequest` (portul, bus-ul proiectului, schema, tabelul de
    pornire), cu `VIEW_EXTENSION` comun. Explorer-ul, care are sesiunile, implementează
    portul (`diagram::SourceSchema`, peste worker-ul sursei: lista tabelelor, apoi toate
    structurile cerute deodată și luate pe măsură ce vin) și deschide vederea prin comanda
    `database.show_diagram`; modulul nou nu știe de explorer, nici invers. Am ales portul în
    domeniu (ca `ProjectQueries` pentru diagrama claselor) în locul unui modul în explorer:
    diagrama rămâne un modul de sine stătător, încărcat doar când se deschide, iar
    explorer-ul nu se recompilează când se schimbă desenul ei.
  - **Modulul `database-diagram`** (`crates/modules/database/diagram`, `database.diagram`):
    vederea `relations` (`view-data-diagram`): structura citită în fundal (pool-ul
    `db-diagram`), aranjarea calculată tot acolo, o dată pentru fiecare set de tabele arătat
    (tabelele care arată spre altele stau la dreapta lor, ordinea din coloană după vecini,
    coloanele prea înalte continuate alături, cele fără legături într-o grilă dedesubt;
    liniile ies din rândul coloanei și intră în rândul coloanei referite, răspândite în
    spațiul dintre coloane, buclă pe dreapta pentru auto-referințe); se desenează doar ce e
    în vedere, sub zoom 0,32 doar anteturile. Cardurile: cheia primară cu cheie, cheile
    externe cu „→ tabel”, tipurile în dreapta, cheile și coloanele legăturilor listate
    primele (10, 18 pentru tabelul de pornire). Click pe card: îl centrează (← înapoi);
    dublu-click: datele lui în grilă; meniul lui: deschide datele, arată vecinii, toate
    tabelele, copiază numele. Bara: filtrul după nume (⌘F, Enter centrează primul găsit),
    vecinii la 1 / 2 / 3 legături, „Toate tabelele”, legenda. Liniile tabelului selectat sau
    de sub mouse sunt colorate, cu eticheta și cu 1 / n la capete. Urmează `DataChanged` cu
    structură al sursei: citește din nou, păstrând tabelul de pornire.
  - **De unde se deschide:** meniul unei surse, scheme, grup de tabele sau tabel din
    fereastra „Database” („Arată diagrama”; pe tabel, cu vecinii lui) și meniul listei de
    tabele din vederea de date (în jurul tabelului de sub mouse), plus acțiunea
    `database.diagram` a vederii. Un fișier își păstrează sesiunea cât e deschisă diagrama.
  - **Capturi:** `relations` și `relations-focus` (un magazin de test, `store.sqlite`, făcut
    de captură); `database-menu` are acum rândul „Arată diagrama”; `about` și `settings-8`
    numără un modul în plus.
- **37j, gata.** A doua parte: relațiile deduse din nume, cheile virtuale și ignorarea lor.
  - **Domeniul (`database::relations`, pur, testat):** coloana se desparte în cuvinte
    (snake_case, camelCase, `ID` cu majuscule, cifre) și i se scoate cuvântul cheii (`id` la
    capăt sau la început, `fk` la capăt sau la început, ambele în `fk_user_id`); `userid`
    (un cuvânt terminat în `id`) contează doar dacă un tabel se numește așa — `paid`,
    `valid`, `guid` nu găsesc niciun tabel `pa`, `val`, `gu`. Tabelul: cuvintele lipite,
    fără majuscule, la singular sau plural (`-ies`, `-es` după `ss`, `us`, `x`, `z`, `ch`,
    `sh`, `o`, `-s`, neregulatele: `people`, `children`, `indices`…, ambele lecturi pentru
    `houses`), cu sau fără prefixul `tbl_` / `tb_` / `t_`. Cheia: `id`, altfel cheia
    primară de o coloană; tipurile compatibile (întreg cu întreg, text cu text, un tip
    necunoscut acceptat, dar mai puțin sigur; niciodată text cu întreg). Auto-referințele:
    `parent`, `ancestor`, `root`, `manager`, `supervisor`… spre același tabel, doar dacă nu
    există un tabel cu numele lor. Niciodată o coloană deja într-o cheie (reală sau
    acceptată); cheia proprie a unui tabel nu arată spre el însuși. Încrederea: sigură
    pentru `<tabel>_id` / `<tabel>Id` cu tabelul exact sau la plural (și `parent_id`),
    probabilă pentru `fk_`, `_fk`, `id_`, `userid`, tabel cu prefix sau alt nume de arbore,
    cu o treaptă mai puțin când cheia nu e `id` sau un tip e necunoscut. O singură trecere,
    cu o hartă a numelor (1 500 de tabele în mult sub două secunde, chiar în debug), deterministă. `RelationNotes`
    (cheile acceptate, cele ignorate, ce se arată: chei / deduse / ambele), cu `accept`,
    `ignore`, `forget`, `key_of`; `DataSource::relations`. `SchemaGraph::with_notes` pune
    cheile acceptate ca legături `Virtual` și pe cele deduse (neignorate) ca `Inferred`,
    `Origin::shown` le alege după comutator.
  - **Păstrarea:** `TomlDataSources` scrie `relations`, `[[source.virtual_key]]` și
    `[[source.ignored_relation]]` (tabel, coloane, țintă, referințe, schemele dacă sunt) în
    fișierul proiectului sau în cel global, după sursă; o cheie fără tabel face fișierul o
    eroare (nu se scrie peste el). Portul `SchemaAccess` are `notes` / `set_notes`; în
    explorer, o sursă configurată primește deciziile prin catalog; un fișier arătat singur
    devine sursa proiectului (cu id-ul sesiunii lui) abia când se acceptă sau se ignoră o
    relație — comutatorul singur rămâne în memorie.
  - **Diagrama:** cele deduse punctate, în culoarea lor, cu atât mai slabe cu cât sunt mai
    nesigure, cu eticheta „dedusă, sigură / probabilă / nesigură”; cele acceptate continue,
    în culoarea cheilor virtuale, cu săgeată goală și eticheta „cheie virtuală”; rândul
    coloanei arată ținta în aceeași culoare. Jos, în stânga: „Chei / Deduse / Ambele”
    (păstrat pe sursă) și butonul cu câte așteaptă o decizie, care deschide lista din
    dreapta: de verificat (Acceptă / Ignoră), acceptate (Anulează), ignorate (Arată din
    nou); un rând centrează tabelul lui, lângă listă. Deducerea rulează în fundal odată cu
    citirea; o decizie refăce doar legăturile.
  - **Grila:** „Mergi la rândul referit” în meniul celulei unei chei (reale sau acceptate):
    tabelul țintă, filtrat pe valoare (`id = 2`), cu filtrul pus când grila își știe
    coloanele. Meniul celulelor fără cheie e neschimbat (captura `data-menu` identică).
  - **Capturi noi:** `relations-inferred` (lista deschisă, `reviews` selectat) și
    `relations-decided` (una acceptată, una ignorată); deciziile capturii stau în memorie.
  - **Nefăcut, cu motiv / diferențe:** deducerea dă doar chei de o coloană (cheile
    virtuale compuse se citesc și se scriu, dar nu se deduc); „Mergi la rândul referit”
    filtrează după coloana celulei (la o cheie compusă, doar după ea); o cheie primară de o
    singură coloană care numește *alt* tabel (`profiles.user_id`, 1–1) e dedusă — regula
    „cheia proprie nu se leagă” se aplică doar spre același tabel.
- **37j, completare (diagrama relațiilor), gata.**
  - **Etichetele liniilor** se așază lângă segmentele liniei (întâi cele în care încap, cele
    lungi primele: deasupra / dedesubt pe orizontală, în dreapta / stânga pe verticală), în
    spațiul liber: niciodată peste un card sau peste altă etichetă; una fără loc liber nu se
    arată decât pentru linia de sub mouse sau selectată. Rămân în vedere (alt loc liber
    vizibil, altfel împinsă înăuntru), cresc și scad cu zoom-ul (cel puțin 9 px), sub zoom
    0,45 doar linia cerută. În repaus scurte (`user_id → users.id`), întregi pentru linia
    cerută. Locurile se calculează o dată pentru aranjare, liniile aprinse și zoom
    (`ui_components::diagram::{label_spots, place_labels}`), nu la fiecare cadru.
  - **Bara de jos și lista** din dreapta sunt margini ale zonei (`diagram::Insets`,
    `Viewport::visible`): potrivirea și centrarea folosesc ce rămâne, deci niciun card nu
    ajunge sub ele; bara nu mai lasă click-ul să treacă la cardurile de sub ea. Diagrama
    claselor PHP nu are margini: capturile `diagram*` identice.
  - **Iconița:** tab-ul și breadcrumb-ul diagramei au iconița `relations` (o regulă a
    modulului pentru extensia `view-data-diagram`; un tab virtual cu iconița dată de modulul
    lui o păstrează oricare ar fi setul de iconițe de fișiere).
  - **Click pe linie:** liniile au zonă de click (distanța până la linie, câțiva pixeli,
    crescând cu zoom-ul); sub mouse se îngroașă și își arată eticheta întreagă. Click:
    linia selectată și un popover (stilul popover-elor, la `POPOVER_GAP` de bara de jos) cu
    relația, felul și încrederea, Acceptă / Ignoră pentru una dedusă, Anulează decizia
    pentru una acceptată, „Mergi la” fiecare tabel; Esc îl închide (apoi lasă linia), un
    click în afară îl închide, ca la celelalte popover-e. Tab / ⇧Tab trec prin liniile
    tabelului selectat, Enter deschide popover-ul liniei.
  - **Legenda** arată și cheile virtuale și relațiile deduse, când sunt desenate.
  - **Capturi:** cele patru `relations*` refăcute, una nouă `relations-edge` (popover-ul
    liniei `reviews.userid`).
- **Butoanele, un singur stil (după 37j), gata.** `ds::button_font` (fontul dialogurilor:
  mărimea interfeței minus o jumătate de punct, greutate 500) e singura definiție a
  etichetei unui buton; dialogurile își fac stilul din el (aceiași pixeli), iar
  `ds::primary_button` / `primary_width` / `primary_button_with_key`, noile
  `ds::secondary_button` (cu chenar, transparent, nuanțat sub mouse) și `ds::danger_button`
  îl folosesc. Butoanele desenate de mână din modalele bazelor de date (surse, previzualizare,
  structură), editorul mare al grilei și banda exportului folosesc acum componentele;
  cele care își păstrează aspectul (alegătorul de culori, Theme Studio, asistentul și
  revizuirea proiectului la distanță) iau fontul comun. Capturile s-au schimbat doar în
  zonele butoanelor.
- **Foldere și culori pentru surse, gata.** Cerut: conexiunile de producție și de dezvoltare
  organizate pe foldere și deosebite prin culori.
  - **Domeniul** (`database::folders`, pur): `SourceFolder` (identitate UUID v4, nume,
    părinte, culoare, lista — proiectul sau toate proiectele), `SourceColor` (opt culori
    numite: roșu, portocaliu, galben, verde, turcoaz, albastru, mov, gri, salvate după nume,
    sau una aleasă `#rrggbb`), `DataSource.folder` / `DataSource.color`, `SourceList`
    (sursele și folderele unei liste). Regulile: culoarea unei surse e a ei, altfel a celui
    mai apropiat folder de deasupra care are una (`effective_color`); un folder nu intră în
    el însuși sau într-unul din el, nici în folderul altei liste; ștergerea unui folder nu
    șterge nicio sursă (ce conține urcă la părinte); o listă citită se repară (`repair`:
    bucle, părinți lipsă, folderul altei liste — totul ajunge sus, nimic pierdut).
    `same_connection`: o sursă doar mutată sau colorată își păstrează sesiunea deschisă.
  - **Stocarea:** `[[folder]]` (id, nume, părinte, culoare) în fișierul fiecărei liste,
    `folder` și `color` la surse; un fișier de dinainte se citește la fel, o culoare
    necunoscută face fișierul o eroare (nu se scrie peste el). Portul `DataSourceStore`
    citește și scrie `SourceList`.
  - **Catalogul:** `put_folder` (adăugat, redenumit, recolorat, mutat), `remove_folder`,
    `set_folders` (Apply-ul modalului), `folders`, `color_of`; aceeași cale de scriere și
    `DataSourcesChanged`; o generație (`generation`) pentru ce se arată din liste, cu
    `ColorCache` (culoarea unei file calculată o dată până se schimbă catalogul).
  - **Modalul:** lista arată folderele (după nume, înaintea surselor de pe același nivel,
    închise / deschise, cu numărul surselor și culoarea), sursele indentate cu un semn în
    culoarea lor; `+ Folder` în bara de jos; o sursă sau un folder se trage peste un folder
    (sau peste titlul unei liste) ca să-l muți (o sursă trasă în folderul celeilalte liste
    trece și ea acolo, cu parola după ea); meniul de click dreapta (nou: modalele modulelor
    au meniu contextual, nimic de sub ele nu primește click-ul dreapta): Folder nou,
    Redenumește, Culoare ›, Mută în ›, Șterge folderul; pe o sursă: Folder nou, Culoare ›
    (cu „Din folder”), Mută în ›. Un folder selectat arată în dreapta numele, culoarea
    („Fără culoare” / „Din folder” și cele opt) și câte surse are; o sursă are rândul
    „Culoare” la sfârșitul tab-ului General. Totul se aplică la Apply / OK, ca restul.
  - **Arborele „Baze de date”:** folderele deasupra surselor, cu numărul surselor, deschise
    implicit, iconița și un semn la începutul rândului în culoarea lor; filtrul găsește și
    numele folderelor (un folder găsit își arată tot conținutul); o sursă arătată din altă
    parte își deschide folderele; meniul unui folder: „Editează folderul…” (modalul, cu
    folderul selectat). O sursă doar mutată sau colorată nu se recitește.
  - **File:** `Viewer::accent` (nou în `ui-components`): grila / structura și diagrama unei
    surse cu culoare au un punct pe iconița tab-ului și o dungă subțire sus în vedere
    (desenate de aplicație). Un fișier deschis singur, fără sursă păstrată pentru el, nu are
    culoare. Culorile numite (`ui_components::tag_colors`) au câte o nuanță pentru temele
    întunecate și una pentru cele luminoase, cu contrast de cel puțin 3:1 față de fundal.
  - **Capturi noi:** `database-sources-folders` (modalul cu folderele prod roșu cu subfolderul
    eu, dev verde, prod selectat), `database-folders` (arborele), `database-folder-grid`
    (tabelul sursei din folderul roșu, cu dunga și punctul pe tab). Modalul surselor arată
    acum butonul de folder și rândul culorii (capturile `database-sources*`).
  - **Nefăcut:** alegerea unei culori oarecare (`#rrggbb`) din interfață — se citește și se
    scrie, dar modalul oferă doar cele opt numite, care se văd bine în ambele teme; mutarea
    unui folder întreg în cealaltă listă (doar sursele trec dintr-o listă în alta).
- **Grila îngustă, aceeași sursă prin legături, indicii, gata.**
  - **Bara de unelte a grilei** (`data_grid::toolbar`, pur): locul fiecărui control e calculat
    o dată pe lățime și stare (`kernel::Memo`), fără suprapuneri. Ce nu încape intră, în
    ordine, într-un meniu „…” la capătul din dreapta: întâi mărimea paginii, apoi
    Transpune, Caută, Reîmprospătează, „din N”, primul / ultimul, Adaugă, Șterge, la urmă
    ‹ ›; ce încape apoi la loc în spațiul rămas revine (întâi cel mai dorit). Intervalul
    (`1–12`) rămâne (tăiat cu „…” doar la extrem), la fel Trimite, Revert,
    oprirea și indicatorul de încărcare. Meniul (o suprafață `ds`, în grilă) are iconița,
    numele și tastele fiecărei acțiuni; cele ce nu se pot folosi acum sunt estompate;
    mărimea paginii din meniu deschide alegerile sub „…”. Tastele merg și pentru ce e în
    meniu (sunt comenzi). `WHERE` și `ORDER BY` stau unul sub altul când rândul e îngust
    (sub 440 de pixeli logici). În antetul vederii, numele tabelului se taie înainte de
    Date / Structură (numele întreg în indiciu); în structură, butonul „+ Coloană” rămâne
    doar „+” (cu numele în indiciu) când titlul n-ar încăpea lângă el.
  - **Identitatea fișierelor:** `database::identity` (pur: `.` / `..`, separatoarele; pe
    Windows fără majuscule și fără prefixele `\\?\` / `\\?\UNC\`) și
    `database_explorer::identity::FileIdentities` (legăturile rezolvate prin portul
    `workspace::FileSystem::canonical`, păstrate într-un `GenerationalCache` per proiect,
    golit la „Clear and Rebuild Caches”; un fișier care nu există e știut după cel mai
    apropiat folder existent, nepăstrat). Folosită peste tot unde sursele și fișierele
    deschise se potrivesc: `Catalog::source_of_file`, fișierele deschise de `Sessions`
    (după cheie, `FileLease` cu ea), arborele „Baze de date”, diagrama, culoarea tab-ului.
    Un fișier deschis printr-o legătură (`/var` și `/private/var` pe macOS) nu mai apare de
    două ori; ocolișul din captura `database-folder-grid` a fost scos.
  - **Indicii** (mecanismul comun, `Tip` / `tooltip_at`, cu tastele comenzii când are):
    butoanele grilei (și cele dezactivate), „…”, câmpurile `WHERE` / `ORDER BY` (exemple de
    sintaxă), semnele din antete (cheia, ordinea), lista tabelelor și Date / Structură;
    iconițele rândurilor din arborele „Baze de date” (cheie primară, cheie externă → țintă,
    index, tabel, view, sursă); modalul surselor; antetul NN / PK / AI / UQ din modalele de
    structură (scris întreg); controalele diagramei relațiilor și legenda.
  - **Capturi noi:** `database-folder-grid-more` (meniul „…” deschis), `database-folder-grid-tip`
    (indiciul butonului Șterge). `database-folder-grid` arată bara fără suprapuneri; în
    `data-csv-split*`, `data-edit` și `data-preview` mărimea paginii, care înainte dispărea
    fără urmă când nu încăpea, e acum în „…” (iar în `data-csv-split*` câmpurile stau unul
    sub altul).
  - **Nefăcut:** un buton „Coloane” în bară (coloanele se ascund din meniul antetului, ca
    înainte); butoane de zoom în diagramă (zoomul e cu rotița și `0`).
- **Matricea locală de baze de date și „PostgreSQL nu arată niciun tabel”, gata.** Cerut
  după 39k (imaginile 382–384: o sursă PostgreSQL 14.13 pe baza `postgres` se conecta, dar
  fila Schemas arăta doar `public`, iar arborele „No tables”; un alt client SQL vedea tabelele).
  - **Matricea** (`docker/databases/`, `scripts/test-databases.sh up|down|seed|urls|status`,
    README): serverele noastre de unică folosință, proiectul compose `editor-test-db`,
    containerele `editor-test-*`, porturile 127.0.0.1:55400–55415, parole de test, datele în
    containere (nimic altceva de pe mașină atins). PostgreSQL 9.2 (doar amd64: emulat pe
    arm64), 9.4, 9.6, 12, 14, 16, 17, 18; MySQL 5.7 (doar amd64: emulat), 8.0, 8.4; MariaDB
    10.11, 11.4, 11.8 — toate pornesc și se populează pe mașina de aici (arm64, OrbStack).
    Aceleași date peste tot (cu variantele dialectului): rolurile `editor` (utilizatorul
    testelor, fără superuser), `editor_owner` (deține tabele și o bază), `editor_reader`
    (doar citire); bazele `postgres` (goală — cazul raportat), `shop`, `analytics` (a lui
    `editor_owner`), `editor_private` (fără `CONNECT` pentru `editor`); în PostgreSQL
    schemele `public`, `sales`, `audit` (a altui rol), chei primare / străine (și compuse) /
    unice / `CHECK`, indecși pe expresii și parțiali, view, view materializat, enum, json /
    jsonb, identitate, tabel partiționat, coloană generată, comentarii, ~16k rânduri, un
    tabel al altui rol citibil și unul nu. MySQL / MariaDB sunt pregătite pentru driverul
    lor (39p).
  - **Testele de integrare** (`EDITOR_TEST_POSTGRES_URLS`, liste separate prin virgulă, pe
    baza `postgres`; `EDITOR_TEST_MYSQL_URLS` pentru 39p): `db-postgres/tests/matrix.rs`
    (bazele, schemele și tabelele oricui, structurile, paginile grilei, consola — cursor,
    explain, profil, statistici —, catalogul în bloc într-un index de completare, un
    utilizator doar-citire; eșecul fiecărui server spus la sfârșit, câte o linie) și
    `database-explorer/tests/postgres.rs` (fereastra „Database” pe server: bazele sub sursă,
    tabelele altei baze prin sesiunea ei). Rulate pe toate cele 8 servere PostgreSQL: înainte
    de reparații 5 din 7 treceau, după, toate.
  - **Cauza:** (a) arborele arăta doar baza din sursă (`postgres`, fără tabele: datele erau
    în celelalte baze), fără niciun nivel de baze de date; (b) interogările de catalog prin
    `information_schema` ascundeau ce nu e al utilizatorului: `schemata` (fila Schemas,
    comutatorul de schemă al consolei) lasă pe dinafară schemele altor roluri — înainte de
    10 orice schemă pe care nu o deține —, `tables` / `columns` (nodurile explorer-ului) pe
    cele fără privilegii și view-urile materializate; (c) lista bazelor includea și bazele
    la care utilizatorul nu se poate conecta, iar un astfel de refuz era o eroare SQL, nu o
    autentificare refuzată.
  - **Reparații:** arborele e acum sursă › baze de date › scheme › tabele
    pentru un server cu mai multe baze (`ConsoleCaps::databases`): toate bazele la care se
    poate conecta, cea a sursei marcată „implicită” (citită de sursa însăși, cu identitatea
    ei: consolele, grilele, completarea rămân ca înainte), fiecare altă bază printr-o sesiune
    a ei, deschisă în fundal doar când e deschis nodul (identitatea
    `DataSourceId::of_database`, derivată stabil — `kernel::id::derived_uuid`, UUID versiunea
    8 —, aceeași după repornire; `Catalog::database_id` / `database_of` / `get`: sursa pe acea
    bază, numele „sursă · bază”, parola, culoarea, marcajul de producție și relațiile ale
    sursei, nesalvată niciodată). Fila Schemas alege și bazele arătate (`DataSource::databases`,
    salvat ca `databases = […]`; „Toate bazele de date”, cele găsite de test), schemele
    rămânând alegerea lor (în fiecare bază). Consola deschisă dintr-o bază e a sursei, pe acea
    bază (`JumpRequest::database`; doar consolele acelei baze listate); închiderea sursei le
    închide și pe ale bazelor ei; reîmprospătarea sursei citește din nou bazele și tabelele
    celor deschise. În driver: schemele din `pg_namespace` (ale oricui), nodurile din
    `pg_class` / structură, bazele filtrate cu `has_database_privilege(…, 'CONNECT')`,
    refuzul 42501 la conectare ca autentificare refuzată.
  - **Capturi:** nouă `database-tree-databases` (un server inventat, în memorie: bazele sub
    sursă, `postgres` goală marcată, `shop` deschisă); de așteptat schimbată
    `database-sources-schemas` (fila Schemas cu bazele de date deasupra schemelor). Nerandate
    (modul rapid).
  - **Abateri / nefăcut:** schemele ascunse sunt aceleași pentru fiecare bază (nu tipare separate
    pe fiecare bază); consola deschisă pe altă bază deschide un tabel („Go to Definition”,
    rândul referit) tot în baza sursei (`ConsoleAccess::open_table` nu știe baza — ca
    înainte); testele MySQL vin cu driverul (39p).
- **Corecție: prima pagină a datelor nu mai așteaptă numărătorile (imaginea 387) — făcut:**
  - **Cauza:** lista de tabele a vederii de date cerea `COUNT(*)` pentru primele 200 de
    tabele (PostgreSQL, 103) pe aceeași sesiune, în aceeași coadă, înaintea paginii grilei:
    „Loading…” / „Counting the rows…” până se terminau toate.
  - **Lucrătorul** (`worker`): ce așteaptă utilizatorul merge primul — cererile în ordinea
    lor, apoi totalurile (`Request::Count`), apoi paginile citite înainte, apoi numărătorile
    listelor (`Request::RowCount`, nou). O sursă SQL numără pe o **bandă a ei**: o a doua
    sesiune a aceleiași surse, deschisă de prima numărătoare și închisă cu prima
    (`Request::Close`, `close_now`, inactivitatea), așa că un `COUNT(*)` lung nu ține pe
    loc nicio pagină; anulare / uitare pe ambele benzi. O sursă fără SQL (CSV: sesiunea îi
    ține indexul) numără pe sesiunea ei, după restul.
  - **Lista** (`view::counts`): estimările motorului, ieftine, venite cu lista — PostgreSQL
    `reltuples` / `relpages`, iar pentru un tabel neanalizat rândurile vii ale statisticilor
    (`pg_stat_get_live_tuples`, nou în `catalog::tables`); MySQL / MariaDB
    `information_schema.TABLES.TABLE_ROWS`; SQLite `sqlite_stat1` — arătate cu `~`
    („~29.6k”); exact doar când e ieftin (un fișier SQLite ≤ 32 MB: toate) sau cerut:
    tabelul de sub mouse (nu unul uriaș, > 1 M estimate, nici o vedere), cel arătat luând
    totalul grilei lui (fără filtru); cel mult 2 în zbor, restul în coadă; ce s-a cerut e
    anulat când lista se citește din nou (Refresh) și cu vederea (clientul ei). Etichetele
    se fac o dată, nu la fiecare cadru. Tooltipul spune dacă e estimare
    (`database.tip_table_rows_estimate`, ambele limbi). Fereastra de structură primește doar
    numărătorile exacte (avertismentele nu citează o estimare).
  - **Driverul PostgreSQL:** pregătirea unei instrucțiuni (pentru tipurile coloanelor)
    aștepta și ea lacătele tabelului fără să poată fi anulată; acum se anulează ca rularea.
  - **Teste:** worker (numărătorile unei surse SQL pe sesiunea lor, închise cu sursa; o
    singură sesiune: paginile înaintea totalurilor, numărătorile listei la urmă; anulate /
    uitate pe banda lor), `view::counts` (1000 de tabele: estimările, nimic cerut decât
    când e ieftin, apoi toate, niciodată mai mult de 2; tabelul cerut primul, nu unul
    uriaș / o vedere; relistat: anulate), vederea (1000 de tabele cu numărători ținute cât
    vrea testul, pe fire proprii: prima pagină vine cât timp totalul grilei și cele cerute
    de mouse stau; apoi vin; un SQLite mic numărat exact pe banda lui, un CSV din totalul
    grilei), db-postgres (interogarea listei), și pe matricea locală
    (`EDITOR_TEST_POSTGRES_URLS` / `EDITOR_TEST_MYSQL_URLS`): `tests/postgres.rs` și
    `tests/mysql.rs` — un tabel blocat de altă sesiune (`LOCK TABLE … ACCESS EXCLUSIVE` /
    `LOCK TABLES … WRITE`), totalul și numărătoarea lui ținute, pagina altui tabel vine
    imediat, numărătorile anulate cât lacătul încă ține, apoi numărat exact — pe toate cele
    8 servere PostgreSQL și 6 MySQL / MariaDB.
  - **Abateri / nefăcut:** numărătoarea exactă la cerere e la trecerea mouse-ului (fără
    buton separat); capturile nerandate (modul rapid) — se schimbă cele care arată lista
    vederii de date cu estimări (`~`); niciuna nouă.
