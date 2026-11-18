# Baze de date (modul `database`)

Ce face, pe scurt: o fereastră „Database” de IDE — conexiuni la baze de date diferite,
arborele lor (baze, scheme, tabele, view-uri, rutine), tabelele afișate ca grilă (cu filtrare,
sortare, editare), informațiile despre tabele (coloane, indexuri, chei, DDL), editarea
structurii și o consolă SQL cu completare din schema reală.

**Cerință fermă: merge identic pe macOS, Linux și Windows** (vezi „Windows” mai jos).

## Principii

- **Un modul, încărcat leneș.** Nimic din `database` nu pornește până nu deschizi panoul sau o
  conexiune; runtime-ul asincron, conexiunile și cache-urile se opresc / se eliberează când nu
  mai sunt folosite (ca terminalul).
- **Driverele sunt module separate** (`db-sqlite`, `db-mysql`, `db-postgres`, `db-sqlserver`,
  `db-mongodb`, `db-redis`…), contribuite prin punctul de extensie `DatabaseDriver`; doar
  driverele folosite de conexiunile tale se încarcă.
- **Doar Rust pur** pentru drivere (fără biblioteci de sistem de instalat): utilizatorul nu
  are nevoie de client MySQL, de `libpq` sau de ODBC.
- **Nimic blocant pe firul UI**: conectarea, interogările și introspecția rulează în fundal,
  cu anulare; UI-ul primește rezultate pe bucăți.
- **Siguranță înaintea comodității**: nimic distructiv fără previzualizarea SQL-ului generat și
  confirmare; conexiuni marcate „producție” și „doar citire”.

## Arhitectură (pe straturile din `02-arhitectura-ddd.md`)

### Domeniu: `crates/domain/database/database` (pur, fără I/O)

- Modelul: `DataSource` (o conexiune configurată: driver, gazdă, port, utilizator, bază,
  opțiuni SSL / tunel SSH, „doar citire”, „producție”, culoare), `Catalog` → `Schema` →
  `Table` / `View` / `Routine` / `Sequence`, `Column` (tip, nul, implicit, auto-increment,
  comentariu), `Index`, `ForeignKey`, `Check`, `Trigger`.
- Rezultatele: `ResultSet` (coloane + rânduri pe pagini; valori tipizate: text, număr, dată,
  binar, JSON, NULL), `RowChange` (inserare / modificare / ștergere, cu cheia rândului).
- Portul `DatabaseDriver`: `connect`, `introspect` (pe niveluri, la cerere: bazele, apoi
  schemele, apoi tabelele unei scheme), `query` (cu limită și anulare), `explain`,
  `apply_changes` (într-o tranzacție), `capabilities()` (ce știe driverul: tranzacții,
  scheme, secvențe, `RETURNING`, editare de structură…).
- Portul `Dialect`: cum se scriu identificatorii și literalele, paginarea, DDL-ul (`CREATE /
  ALTER TABLE`, indexuri, chei) — fiecare driver își aduce dialectul.
- Reguli pure, testate fără bază: generarea SQL-ului pentru modificările din grilă și pentru
  modificările de structură (diferența dintre structura veche și cea nouă → `ALTER`-uri),
  detectarea instrucțiunilor periculoase (`DROP`, `TRUNCATE`, `DELETE` / `UPDATE` fără `WHERE`).

### Aplicație (use case-uri)

`Connect`, `BrowseCatalog`, `OpenTable` (pagină + filtru + sortare), `RunStatement` (cel de la
cursor / selecția / tot scriptul), `EditRows` (modificările din grilă, trimise împreună),
`AlterTable` (previzualizare DDL → aplicare), `ExportResult` (CSV, JSON, `INSERT`-uri, Markdown).

### Infra

- Drivere: SQLite (`rusqlite` cu SQLite inclus în build), MySQL / MariaDB și PostgreSQL
  (`sqlx`, TLS prin `rustls`), SQL Server (`tiberius`, `rustls`), MongoDB (driverul oficial
  Rust), Redis (`redis`). Oracle doar opțional, mai târziu (are nevoie de clientul Oracle).
- Secretele (parole, chei) în keychain-ul sistemului (crate-ul `keyring`: Keychain pe macOS,
  Credential Manager pe Windows, Secret Service pe Linux) — niciodată în fișiere.
- Tunel SSH: refolosește modulul SSH / SFTP din „Proiecte la distanță”.

### UI (modulul `database`)

- **Conexiunile — ale proiectului sau globale** (cerere nouă): la creare alegi dacă o
  conexiune e doar a proiectului (salvată în proiect) sau globală (vizibilă în toate
  proiectele, salvată în configurarea utilizatorului); se poate muta oricând dintr-o parte în
  alta. Credențialele rămân în keychain sau criptate (ca la remote), oricare ar fi locul.
- **Nume și etichete**: fiecare conexiune are un nume dat de utilizator (implicit
  `user@host/baza`) și etichete — `prod`, `dev`, `staging` sau oricare alta — fiecare cu
  culoarea bulinei ei (aleasă din paletă), arătată în arbore, pe tabul consolei / grilei și în
  bara de stare; eticheta `prod` (sau orice etichetă marcată „producție”) pornește
  confirmările pentru instrucțiunile distructive. Filtrare după etichetă în panou.
- **Panoul „Baze de date”**: conexiunile → baze → scheme → tabele / view-uri / rutine, încărcate
  la deschiderea nodului; căutare în arbore; acțiuni contextuale (deschide, consolă nouă,
  copiază DDL, reîmprospătează, golește, șterge — cu confirmare).
- **Grila de date** (tab): desenată pe GPU ca restul editorului, virtualizată (doar rândurile
  vizibile), pagini încărcate pe măsură ce derulezi; bară de filtru (`WHERE …` cu completare din
  coloane) și sortare pe click pe antet; celulele editabile (editor potrivit tipului: text,
  număr, dată, JSON, NULL); modificările rămân în așteptare, colorate, până la „Trimite” (cu
  previzualizarea SQL-ului) sau „Anulează”; copiere ca TSV / JSON / `INSERT`.
- **Informații despre tabel**: coloanele (tip, nul, implicit, comentariu), indexurile, cheile
  externe (cu salt la tabelul referit), numărul de rânduri, mărimea, DDL-ul.
- **Editorul de structură**: adaugă / redenumește / șterge coloane, schimbă tipuri, indexuri,
  chei; arată mereu `ALTER`-urile care vor rula; aplicarea cere confirmare (dublă pe „producție”).
- **Consola SQL** (tab cu limbajul SQL): completare de tabele / coloane / funcții din schema
  conexiunii, rulează instrucțiunea de la cursor (⌘↵) sau selecția, rezultatele în grilă sub
  editor, istoricul consolelor, `EXPLAIN` vizual.
- **Bara de stare**: conexiunea activă, tranzacție deschisă, timpul ultimei interogări.

## Integrarea cu editorul

- Limbajul **SQL** ca modul (`lang-sql`, gramatică tree-sitter), cu dialectul conexiunii.
- SQL-ul din șirurile PHP (`$pdo->query("SELECT …")`, Doctrine DQL mai târziu) colorat prin
  injecție și completat cu tabelele / coloanele conexiunii asociate proiectului.
- ⌘+click pe un tabel / o coloană din SQL → informațiile despre tabel.
- Proiectul își poate asocia conexiuni: `.lumen/database.toml` (fără secrete), propuse automat
  din `.env` (Laravel `DB_*`, Symfony `DATABASE_URL`) — doar sugestie, nu se conectează singur.

## Performanță și resurse

- Un runtime asincron (`tokio`, câteva fire) pornit la prima conexiune, oprit când nu mai e
  nicio conexiune deschisă.
- Introspecție la cerere (nivel cu nivel), schema în cache în
  `~/.config/lumen/database/<conexiune>/schema.bin`, reîmprospătată la cerere sau la DDL rulat
  din Lumen.
- Rezultatele cu limită (implicit 500 de rânduri pe pagină), streaming, anulare la închiderea
  tabului; celulele mari (BLOB, JSON lung) încărcate doar când sunt deschise.
- Conexiunile inactive se închid după un timp (configurabil).

## Siguranță

- „Doar citire” pe conexiune (instrucțiunile care scriu sunt refuzate înainte de a pleca).
- „Producție”: chenar colorat pe tot ce ține de conexiune + confirmare pentru orice scriere.
- Instrucțiunile periculoase (vezi domeniul) cer confirmare cu SQL-ul afișat.
- Tranzacții: automat (implicit) sau manual (commit / rollback din bara de stare).
- Nicio parolă în loguri, în sesiuni sau în fișierele proiectului.

## Windows

- Toate driverele alese sunt Rust pur, cu TLS prin `rustls` (fără OpenSSL de instalat).
- Secretele în Windows Credential Manager (`keyring`).
- Căile de date prin `config_dir()` (`%APPDATA%\lumen\…`), fără presupuneri Unix.
- CI: testele domeniului și ale driverelor (SQLite în memorie; MySQL / PostgreSQL în
  containere pe Linux) rulează și pe Windows (`cargo test` pe `windows-latest`).

## Etape

1. **E1 — citire**: SQLite, MySQL / MariaDB, PostgreSQL; managerul de conexiuni; arborele;
   grila doar-citire cu paginare, filtru, sortare; informațiile despre tabel; consola SQL.
2. **E2 — editarea datelor**: modificări în grilă, trimise într-o tranzacție, cu
   previzualizare; export.
3. **E3 — editarea structurii**: editorul de structură cu DDL generat și aplicat.
4. **E4 — mai multe baze**: SQL Server, MongoDB (documente ca arbore / JSON), Redis (chei,
   tipuri, TTL).
5. **E5 — conexiuni avansate**: tunel SSH, SSL cu certificate, `EXPLAIN` vizual, SQL în PHP.
6. **E6 — drivere ale altora**: API stabil pentru drivere contribuite de module.

## Riscuri

- Diferențele dintre dialecte (DDL-ul mai ales) → teste pe fiecare dialect, generare doar din
  model, nimic concatenat de mână.
- Rezultate uriașe → limite și streaming de la început.
- Editarea unei tabele fără cheie primară → editare refuzată (doar citire), explicat în UI.
