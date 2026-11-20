# 88 — Iconițele bazelor de date: motorul sursei și fiecare fel de obiect

## Ce vrem

În fereastra „Database”, o sursă se recunoaște acum doar după nume și după detaliul gri
(„Postgr…”). Toate serverele au aceeași iconiță (`server`), iar toate fișierele aceeași
(`database` sau `file-text`). Vrem:

- **Iconița motorului** la fiecare sursă, recognoscibilă dintr-o privire (vezi „Făcut”: semne
  ale noastre, nu logourile motoarelor). Pentru un driver extern fără iconiță proprie
  rămâne `server`.
- **Starea conexiunii pe iconiță**: un punct verde mic în colțul de jos când sursa are o
  sesiune deschisă. Fără punct când nu e conectată, punct roșu după o eroare de conectare.
- **O iconiță proprie pentru fiecare fel de obiect**, ca arborele să se citească dintr-o
  privire: bază de date, schemă, grupul de tabele, tabel, vedere, coloană, cheie primară,
  cheie străină, index, index unic, cheie (constrângere).
- Aceleași iconițe oriunde apare sursa sau obiectul: meniul `+` (Data Source ›), dialogul
  surselor (lista de drivere și antetul), tab-ul unui tabel deschis, pastila consolei, arborele
  din „Services”, cardul de hover al unui tabel în consolă.

Ce se vede azi rămâne: culorile de folder ale utilizatorului, insigna PROD, numărătorile,
tooltip-urile iconițelor (`icon_tip`), ordinea și indentarea rândurilor.

## Cum arată

- **Desenate de noi**, în stilul pachetului nostru: 24×24, linie de 2, capete rotunde, aceeași
  greutate ca `table` și `folder`. Nu copiem fișiere de logo. Facem semne simplificate,
  recognoscibile la 16 px (silueta capului de elefant, delfinul, pana).
- **Înainte de livrare** verificăm politica de marcă a fiecărui motor pentru folosirea
  semnului său ca să numești motorul. Dacă una nu permite, motorul acela primește un semn
  generic al nostru (cilindru cu inițiala), fără să schimbe restul planului.
- **Culoarea**: o cheie de temă pentru fiecare motor (`database.engine.postgres`,
  `database.engine.mysql`, `database.engine.sqlite`, `database.engine.csv`), cu valori
  separate pentru tema luminoasă și cea întunecată, apropiate de culoarea cunoscută a
  motorului. Iconița rămâne monocromă, colorată la desen, ca să meargă în orice temă și cu
  orice pachet de iconițe ales. Un pachet care nu are desenul cade pe al nostru.
- **Obiectele** au culori puține și cu sens: tabelul și vederea în accent, cheia primară în
  avertisment (ca azi), cheia străină în accent, restul în `text_muted`. Grupurile („tables”,
  „views”, „columns”, „indexes”, „keys”) rămân foldere, colorate ca folderele.

## Cine aduce ce (modulele nu se cunosc)

- **Iconița motorului vine de la modulul driverului**, prin mecanismele existente:
  - `DriverFactory` primește `icon: Option<Cow<'static, str>>`: numele iconiței driverului;
  - desenul îl contribuie același modul, cu `languages::IconSvg`, cum face deja modulul de
    diagrame;
  - exploratorul nu numește niciun driver: `driver_icon` citește `icon` din registrul
    `database::drivers`. Fără iconiță, rămân regulile de azi (`server`, `database`,
    `file-text`).
- **Un driver extern** (manifestul de pe disc) poate da o iconiță SVG în manifest. O citim
  limitat (dimensiune maximă, doar forme, fără scripturi sau imagini legate) și o înregistrăm
  la descoperire. Fără ea, primește `server`.
- **Iconițele obiectelor** sunt ale aplicației (`crates/render/icons/svg/`), pentru că le
  folosesc și consola, „Services” și diagrama: `database`, `schema`, `table`, `view`,
  `column`, `key-primary`, `key-foreign`, `index`, `index-unique`, `constraint`.
- **Punctul de stare** îl desenează componenta de arbore (`ui-components`), ca semn peste
  iconiță: `RowLook` primește un punct cu ton (`ok`, `danger`). Exploratorul spune doar starea
  sesiunii, din ce are deja în memorie (`Sessions`), fără nicio citire la desen.

## Pași

### 88a — Iconițele obiectelor (fără logouri)

- Desenele noi în `icons/svg/`: `schema`, `view`, `key-primary`, `key-foreign`,
  `index-unique`, `constraint` (`table`, `column`, `index`, `database` există).
- `icon_of` (`explorer/src/window/paint.rs`) folosește iconițele noi: vederea nu mai e `eye`,
  schema nu mai e `layers`, cheia străină nu mai e `link`, indexul unic se deosebește de cel
  simplu.
- Aceleași nume în cardul de hover al consolei, în diagrama schemei și în „Services”, unde
  apar obiecte.
- Tooltip-urile rămân (`datasources.tip_*`); adăugăm `tip_unique_index` în ambele limbi.

### 88b — Iconița motorului

- `DriverFactory.icon` și `driver_icon` citit din registru.
- Desenele `engine-postgres`, `engine-mysql`, `engine-sqlite`, `engine-csv`, contribuite
  fiecare de modulul driverului său (`modules/database/{postgres,mysql,sqlite,csv}`).
- Cheile de temă `database.engine.*` și culoarea lor la desen. `RowKind::Source` se colorează
  după motor, nu după accent.
- Folosite în meniul `+`, în lista de drivere și antetul dialogului surselor, în tab-ul unui
  tabel și al unei console, și în pastila consolei.

### 88c — Starea conexiunii

- Semnul peste iconiță în componenta de arbore (`RowLook`), desenat în colțul de jos-dreapta,
  cu un inel în culoarea fundalului rândului, ca să se vadă și peste iconiță.
- Starea vine din `Sessions` (deschisă / eroare la ultima încercare), cu o generație: rândurile
  se refac doar când se schimbă, nu la fiecare cadru.
- Tooltip-ul iconiței sursei spune și starea („PostgreSQL · conectat”).

### 88d — Driverele externe

- Câmpul `icon` din manifest, citit limitat, înregistrat o singură dată.
- Testul cu un manifest de probă: cu iconiță, fără iconiță, cu un SVG prea mare sau cu
  `<script>` (refuzat, cade pe `server`).

## Teste (în `tests/unit/`, cum e regula)

- `icon_of` pentru fiecare `RowKind`: numele și culoarea așteptate.
- `driver_icon`: un driver cu iconiță, unul fără (server / fișier / text), unul necunoscut.
- Fiecare modul de driver contribuie iconița pe care o numește (un test care scoate modulul
  arată că exploratorul cade pe `server`, fără eroare).
- Semnul de stare: desenat doar cu sesiune deschisă, roșu după eroare, nimic altfel.
- Fiecare desen nou se citește (`Icons::builtin` / înregistrarea contribuției) și are
  `viewBox="0 0 24 24"`.
- Snapshot-urile `DATABASE=tree|databases|mysql|folders` refăcute și verificate în ambele
  teme.

## Ordinea

1. 88a: iconițele obiectelor (doar ale aplicației, fără întrebări de marcă).
2. 88b: iconița motorului, după verificarea politicii de marcă a fiecăruia.
3. 88c: punctul de stare.
4. 88d: driverele externe.

## Lăsat pentru mai târziu

- Iconițe pentru obiectele pe care arborele nu le listează încă (rutine, secvențe,
  declanșatoare, tipuri). Vin odată cu listarea lor.
- Iconițe colorate în mai multe culori pentru motoare: cer `Tint::Own` și ar ignora tema.

## Făcut

**Verificarea mărcilor (înainte de 88b).**
- Elefantul PostgreSQL e marcă înregistrată. Politica cere aprobare pentru orice modificare a
  lui, iar folosirea în produse poate cere licență
  (https://www.postgresql.org/about/policies/trademarks).
- Delfinul MySQL se folosește doar în condițiile Oracle pentru mărci, nu pentru a-ți prezenta
  propriul produs (https://mysql.com/about/legal/trademark.html).
- Pentru pana SQLite n-am găsit o regulă oficială.
- Am ales deci varianta de rezervă din plan pentru toate: semne ale noastre, niciun logo și
  nicio siluetă după logo. Douăsprezece direcții au fost arătate înainte (pagina de propuneri);
  aleasă a fost „H”: o plăcuță în nuanța motorului, cu monograma desenată ca linii subțiri.
  După încercare, monogramele au fost scoase (vezi 88b).

**88a — Obiectele.**
- Desene noi în `icons/svg/`: `schema` (trei cutii legate), `view` (un tabel cu un ochi),
  `key-foreign` (inelul cheii cu o săgeată spre ce arată), `index-unique` (indexul cu un „1”).
- Cheia primară folosește `key`, cea de azi, nu un desen nou. Rândurile din grupul „keys” sunt
  chei străine (așa spunea și tooltip-ul lor), deci au `key-foreign`; `constraint` n-a mai
  fost nevoie.
- Folosite în arbore (`icon_of`), în antetul și tab-ul vederii de date (o vedere nu mai e
  `eye`), în diagrama schemei (coloana cu cheie străină) și în vederea „Tabele” a profilului
  (indexul unic).
- Pachetele de iconițe care n-au aceste nume cad pe desenele noastre, ca la `archive-down`.

**88b — Motorul.**
- `DriverFactory.icon: Option<DriverIcon>` (`name`, `tone`) și `with_icon(name, tone)`.
  `IconTone` e una din culorile de terminal ale temei (roșu, verde, galben, albastru, magenta,
  turcoaz) sau neutru. Așa fiecare dintre cele 30 de teme o are deja potrivită; `icon.<nume>`
  dintr-o temă o înlocuiește.
- `explorer::engine_icon` e locul unic: `of(id, file_based)` dă iconița și tonul (cea a
  driverului, altfel `server` / `database` / `file-text`, ca înainte), `color(...)` dă
  culoarea. Înainte regula era scrisă de două ori (arbore și dialogul surselor).
- **Monogramele au fost scoase după încercare.** Varianta H (o plăcuță în nuanța motorului cu
  „Pg”, „My”, „Ma”, „Sq” și o grilă pentru CSV) arăta prost la 16 px, și cu literele mărite.
  Toate sursele, server sau fișier (și CSV), au acum aceeași iconiță: `database`.
  `engine_icon::of(id)` dă iconița numită de driver (doar driverele externe pot numi una,
  88d), altfel `database`. Modulele driverelor nu mai aduc desene și nu mai depind de
  `languages`.
- Stilul „sketch” păstra plin orice formă umplută. O nuanță (o umplere transparentă) își
  păstrează acum transparența, iar un punct rămâne întreg. Reparația a rămas: e corectă pentru
  orice pachet de iconițe.
- Colorate în arbore, în lista și antetul dialogului surselor (când sursa nu are culoarea ei
  de etichetă). Meniul `+`, tab-urile și pastila consolei arată iconița motorului în culoarea
  lor obișnuită.

**88c — Starea conexiunii.**
- Evenimentul nou `database::events::ConnectionChanged { source, open, failed }`, publicat de
  worker după ce își eliberează lacătul, doar când starea chiar se schimbă (deschis / închis)
  sau când deschiderea a eșuat (server de negăsit, autentificare refuzată; o interogare
  refuzată nu contează).
- Fereastra „Database” îl ascultă: ține sursele care au eșuat și refă rândurile. Nimic nu se
  citește la desen. `TreeSource.link` / `RowMarks.link`: `Closed`, `Open`, `Failed`.
- Punctul (verde deschis, roșu după eșec) e în colțul de jos-dreapta al iconiței, cu un inel în
  culoarea rândului (și a selecției sau a hover-ului). Arborele e desenat de explorator, nu de
  componenta comună, deci punctul e desenat tot acolo, nu prin `RowLook`.
- Tooltip-ul iconiței: „Sursă de date PostgreSQL · conectată” / „· nu s-a putut conecta”.

**88d — Driverele externe.**
- Manifestul poate numi `icon` dintr-o listă fixă a aplicației (`manifest::ICONS`: `database`,
  `server`, `hard-drive`, `file-text`, `table`, `cloud`, `globe`, `layers`) și `icon_tone`.
  Câmpul `icon` exista deja în protocol, nefolosit.
- Un desen propriu al driverului nu se citește. Driverele externe sunt găsite abia când e
  nevoie de ele, după ce setul de iconițe e făcut, iar un SVG dintr-un dosar străin ar fi
  intrare ostilă. Un nume sau un ton necunoscut e ignorat: sursa rămâne cu iconița felului ei.

**Teste**:
- iconița și culoarea fiecărui `RowKind`, punctul pentru fiecare stare (`window.rs`);
- `engine_icon`: iconița numită de driver, altfel `database`; culoarea din temă înaintea tonului;
- worker-ul: deschis, închis și refuzat spuse o dată fiecare pe magistrala proiectului;
- manifestul: un nume din listă cu tonul lui, un ton necunoscut, o cale spre un SVG și
  iconița altui driver refuzate.

## Lăsat

- **Iconițe proprii motoarelor**: nici logouri (doar cu acordul fiecărui proprietar de marcă),
  nici monograme (n-au arătat bine la 16 px). Dacă vine un desen bun, un modul de driver îl
  contribuie cu `IconSvg` și `with_icon`: mecanismul a rămas.
- **Punctul în alte locuri** (pastila consolei, „Services”): acolo starea se vede deja în alt
  fel (pastila, `TX`).
- **Iconițe pentru rutine, secvențe, declanșatoare**: odată cu listarea lor în arbore.
