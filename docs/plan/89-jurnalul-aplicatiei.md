# 89 — Jurnalul aplicației: JSON pe linie, rotit, fără secrete

## Ce vrem

Un jurnal după care să putem afla ce s-a întâmplat la un utilizator, fără să-i cerem să
repete. De exemplu: de ce aplicația a rămas „Running in Background”, sau de ce autosave-ul
nu salvează pe Windows. Ca la un logger matur: fiecare eveniment cu nivelul, locul și
câmpurile lui, fișiere rotite și o limită de spațiu. Niciun secret nu ajunge pe disc.

- **Totul în JSON, câte un obiect pe linie** (JSON Lines, `.jsonl`): citit de om cu o
  unealtă, de mașină fără parsare fragilă, și de aplicație în propria fereastră de jurnal.
- **Rotație** după mărime și după zi, cu un număr maxim de fișiere și o limită totală.
  Nimic nu se mai pierde brusc, cum se întâmplă acum la golirea de la 5 MB.
- **Ascunderea datelor sensibile** înainte de scriere, după numele câmpului și după forma
  valorii.
- **Fără cost pe firul interfeței**: scrierea pe un fir al ei, cu o coadă mărginită; un disc
  plin sau lent nu blochează aplicația.
- Preia punctul mai vechi din TODO, „Logurile: debug pentru noi, liniște pentru public”.

## Ce e acum

- `crates/app/src/main.rs`: `tracing-subscriber` `fmt()`, text simplu. Fără consolă
  (Finder / Dock, Windows), merge în `<config>/<id>.log`, golit de tot peste 5 MB. Același
  filtru în debug și în release (`info,wgpu_core=warn,…`), schimbat doar prin `RUST_LOG`.
- 518 evenimente `tracing::*!` în 195 de fișiere, unele cu câmpuri structurate
  (`elapsed = ?d`), altele doar cu text formatat.
- Regula „secretele nu se loghează” e respectată de mână, fără nicio plasă.
- `hang.txt` (watchdog): stivele firelor la un blocaj, separat de jurnal.

## Forma unei linii

```json
{"ts":"2026-11-20T08:10:00.123Z","level":"INFO","target":"database_explorer::worker","thread":"db-explorer","run":"6f1c…","msg":"session opened","fields":{"source":"…","elapsed_ms":12.4}}
```

- `ts`: UTC, în milisecunde, RFC 3339.
- `level`: `ERROR`, `WARN`, `INFO`, `DEBUG`, `TRACE`.
- `target`: modulul (cel dat de `tracing`), după care se filtrează și se alege nivelul.
- `thread`: numele firului (`indexer`, `http-run`…), sau `ui` pentru firul principal.
- `run`: id-ul pornirii (UUID v4, scurtat). Deosebește două instanțe care scriu deodată.
- `msg`: mesajul, iar în `fields` câmpurile structurate, cu tipul lor (număr, bool, text).
  O durată devine număr de milisecunde, cu sufixul `_ms`.
- `span`: contextul, când există (proiectul, sursa de date), ca listă de nume cu câmpuri.
- La fiecare pornire, prima linie spune versiunea, sistemul, arhitectura, placa grafică și
  setările de jurnal. Restul liniilor nu le mai repetă.
- Un rând nu trece de 64 KB: textul lung se taie cu `…`, cu lungimea inițială notată.

## Fișierele

- Dosarul ales în setări (implicit `<config>/logs/`), fișierele `app-AAAA-LL-ZZ.jsonl`. Peste 10 MB, se continuă în
  `app-AAAA-LL-ZZ.1.jsonl`, `.2`… La o zi nouă, fișier nou.
- Se păstrează cel mult 14 zile și 100 MB în total; cele mai vechi pleacă primele. Curățenia
  se face la pornire, pe un fir de fundal.
- **Mai multe instanțe:** prima ține fișierul zilei blocat (`File::try_lock`). Una pornită
  peste ea scrie în `app-AAAA-LL-ZZ-<run>.jsonl`. Nicio linie nu se amestecă și nicio rotație
  nu se calcă.
- `lumen.log`-ul vechi se mută o dată în `logs/legacy.log` și pleacă la curățenie.
- `hang.txt` rămâne ce e, iar jurnalul notează unde s-a salvat.

## Ascunderea datelor sensibile

Totul se face într-un singur loc, pur, testat cu exemple din fiecare modul. Rulează pe
firul care loghează, înainte ca linia să intre în coadă: nimic nu ajunge întreg nici măcar
în memoria scriitorului.

- **După numele câmpului** (fără diferență între litere mari și mici, cu `-` sau `_`):
  `password`, `passwd`, `pwd`, `secret`, `token`, `access_token`, `refresh_token`,
  `api_key`, `apikey`, `authorization`, `cookie`, `set_cookie`, `private_key`,
  `client_secret`, `session_key`, `passphrase`. Valoarea devine `"***"`.
- **După forma valorii**, oriunde în text (și în `msg`):
  - `Bearer …`, `Basic …` (antete);
  - `schema://user:parolă@gazdă`: rămâne `schema://user:***@gazdă`;
  - `password=…` / `pwd=…` în șiruri de conexiune și query string;
  - chei private PEM (`-----BEGIN … PRIVATE KEY-----` … `END`);
  - JWT (`eyJ….eyJ….…`);
  - tokenurile cunoscute: GitHub (`ghp_`, `gho_`, `ghu_`, `ghs_`, `ghr_`,
    `github_pat_`), chei AWS (`AKIA…`), chei de API cu prefix (`sk-…`, `xox[abp]-…`).
- **Tipuri care nu se pot scrie întregi**: `Secret<T>`, cu `Debug` și `Display` care dau
  `***`. Se folosesc acolo unde trec parole și tokenuri (`database::Secret` există deja și
  se verifică).
- **Ce nu ascundem în jurnalul local**: căile. Arhiva pentru raportarea unui bug le
  înlocuiește: dosarul utilizatorului devine `~`, iar numele utilizatorului dispare.

## Nivelurile și setările (hotărât 2026-11-20)

- **Implicit `info`** pentru crate-urile noastre și `warn` pentru dependențe (`wgpu`, `naga`
  și celelalte), în debug și în release. Utilizatorul nu-l vede decât dacă deschide jurnalul.
- **Utilizatorul îl controlează din Settings → Advanced → „Jurnal”:**
  - **Nivel:** Oprit, Erori, Avertismente, Info (implicit), Debug. „Oprit” înseamnă că nu se
    scrie nimic pe disc și nu se deschide niciun fișier.
  - **Dosarul jurnalelor:** o cale aleasă cu selectorul de dosare, implicit
    `<config>/logs/`, cu un buton „Reveal”. La o schimbare, fișierul următor se deschide
    acolo. Jurnalele vechi rămân unde erau; nu se mută nimic fără să ceară utilizatorul.
  - **Excepții per modul**, pentru depanare, de exemplu `database=debug`.
  - `RUST_LOG` / `EDITOR_LOG` le suprascriu pe toate. Schimbările se aplică pe loc, fără
    repornire (filtru reîncărcabil).
- **`debug` rămâne compilat și în release**, ca un utilizator să-l poată porni pentru un
  raport de bug. Un eveniment oprit costă doar o verificare a nivelului, pe care `tracing` o
  ține în cache pe fiecare loc din cod. Se măsoară în 89c; dacă iese scump, rămâne doar `trace`
  scos la compilare.
- **Trecerea prin cele 518 evenimente:**
  - zgomotul de pe cadru sau de pe tastă iese;
  - ce ajută la depanare trece pe `debug`;
  - ce trebuie văzut (o eroare a utilizatorului, un serviciu căzut) trece pe `warn` /
    `error`;
  - textul formatat devine câmpuri (`path = %p`, nu `"… {p}"`), ca să poată fi filtrat;
  - `eprintln!` rămâne doar în xtask, teste și snapshot.
  - Regula se scrie în `.claude/rules/project-conventions.md`.

## Scrierea

- Un `Layer` al nostru (`tracing-subscriber`). Construiește linia JSON pe firul care
  loghează (câmpurile trebuie citite acolo) și o trimite pe un canal mărginit (8192 de
  linii) firului `log-writer`.
- **Coada plină:** linia nu se mai trimite, ci se numără. Următoarea linie scrisă spune câte
  s-au pierdut. Nimeni nu stă după disc.
- Firul scrie printr-un `BufWriter` și golește bufferul la cel mult 200 ms, imediat la
  `warn` / `error`, și la ieșire (`exiting`, panică, termenul de 5 s al watchdog-ului).
- **Discul care refuză** (plin, fără drepturi): se încearcă din nou la un minut, iar
  pierderea se notează o singură dată, la `stderr`.
- **Panicile** ajung în jurnal (hook): mesajul, locul, firul și stiva, apoi hook-ul de dinainte.
- **Consola** (aplicația pornită dintr-un terminal: `cargo run`, `./lumen`): primește aceleași
  linii JSON ca fișierul (hotărât 2026-11-20). Se citesc cu `jq`, de exemplu
  `… | jq -r '"\(.ts) \(.level) \(.target): \(.msg)"'`.

## În aplicație

- **Help → „Show Log”** deschide jurnalul zilei într-o vedere a unui modul (`app-log`), cu
  componenta de jurnal pe care o avem (`log_view`):
  - pe fiecare rând: ora, nivelul colorat, modulul și mesajul, cu câmpurile la cerere;
  - filtre după nivel, după modul și după text;
  - urmărește fișierul cât e deschis; citește în fundal; un fișier mare se citește de la
    coadă.
- **„Reveal Log Folder”**: dosarul în Finder sau în Explorer.
- **„Collect Logs for a Bug Report…”**: o arhivă `.zip` cu ultimele zile, căile anonimizate,
  `hang.txt` dacă există și un `about.json` cu versiunea, sistemul și placa grafică. Arată
  ce intră în ea înainte să fie salvată. Nimic nu se trimite singur.

## Pași

### 89a — Formatul și scrierea
Crate-ul `crates/infra/system/logging`: linia JSON, firul `log-writer`, coada mărginită,
rotația după zi și mărime, curățenia, blocarea fișierului pentru o a doua instanță, prima
linie a pornirii, panica. Legat în `main.rs`, în locul lui `fmt()`. Consola rămâne text.

### 89b — Ascunderea datelor sensibile
Modulul pur `redact`, cu regulile de mai sus, aplicat pe câmpuri și pe `msg`, plus tipul
`Secret<T>`. Teste cu exemple adevărate din baze de date, HTTP, remote, agenți și git.

### 89c — Nivelurile și setările
Filtrul reîncărcabil, setarea „Jurnal” (nivel cu „Oprit”, dosar, excepții), `EDITOR_LOG`,
măsurătoarea costului lui `debug` oprit în release. Apoi
trecerea prin cele 518 evenimente, după reguli (zgomot, `debug`, `warn`, câmpuri), și
regula scrisă în `project-conventions.md`.

### 89d — Fereastra de jurnal și arhiva
Modulul `app-log`: „Show Log”, filtrele, urmărirea fișierului, „Reveal Log Folder” și
„Collect Logs for a Bug Report…”.

## Teste (în `tests/unit/`)

- Linia: câmpuri de toate tipurile, `_ms` pentru durate, ghilimele și caractere de control
  scăpate, linia de peste 64 KB tăiată, UTF-8 tăiat doar la margine de caracter.
- Rotația: o zi nouă; trecerea de 10 MB; păstrarea a 14 zile și 100 MB; o a doua instanță
  în fișierul ei.
- Coada plină: nimic nu se blochează, iar pierderea e numărată și spusă.
- Ascunderea: fiecare regulă, cu exemple adevărate, plus ce nu trebuie atins (un `token` de
  limbaj, un `key` din config, o cale).
- Panica ajunge în fișier. La ieșire, nimic rămas în buffer nu se pierde.
- Fereastra: citirea de la coadă, filtrele, urmărirea fișierului.

## Hotărât (2026-11-20)

- `info` implicit, cu nivelul la alegerea utilizatorului (inclusiv „Oprit”) și dosarul ales
  de el.
- Mărimile: 10 MB pe fișier, 14 zile, 100 MB în total.
- JSON pe linie peste tot, consola inclusiv.

## Făcut

**89a — Formatul și scrierea** (2026-11-20).
- Crate-ul nou `crates/infra/system/logging` are patru părți:
  - `line`: linia JSON. Cheile apar în ordinea `ts`, `level`, `target`, `thread`, `run`,
    `msg`, `fields`, `span`; harta din serde le-ar fi sortat alfabetic, așa că sunt scrise de
    mână. Ora e în UTC, la milisecundă, calculată fără vreo dependență nouă. O durată devine
    `<nume>_ms`. Un text e tăiat la 16 KB, iar linia la 64 KB.
  - `layer`: stratul pentru `tracing`. Construiește linia pe firul care loghează, cu
    câmpurile span-urilor păstrate în extensiile lor. O trimite printr-un canal de 8192 de
    linii. Când canalul e plin, linia se numără, iar următoarea spune câte s-au pierdut.
  - `files`: fișierul zilei și rotația după mărime. Un fișier deja blocat de altă instanță o
    face pe aceasta să-și scrie propriul fișier. Curățenia ține 14 zile și 100 MB și nu
    atinge niciodată un fișier în uz sau unul care nu e al jurnalului.
  - `writer`: firul `log-writer`. Golește bufferul la 200 ms, imediat la `warn`/`error` și
    la `flush()`. Un disc care refuză scrierea e încercat din nou peste un minut, iar
    pierderea se spune o singură dată.
- `main.rs` leagă stratul în locul lui `fmt()`. La pornire scrie o linie cu versiunea,
  sistemul, arhitectura și dosarul jurnalului. `log_panics` notează panicile, cu locul și
  stiva. `flush()` se face la sfârșitul lui `main` și la termenul de ieșire al watchdog-ului.
  `lumen.log`-ul vechi se mută o dată în `logs/legacy.log`.
- Verificat pe o rulare reală, în `HOME` izolat: toate liniile sunt JSON valid, iar o a doua
  pornire scrie în același fișier al zilei, cu `run`-ul ei.

**89b — Ascunderea datelor sensibile** (2026-11-20).
- `redact`, pur, aplicat la citirea fiecărui câmp și a mesajului:
  - un nume de secret ascunde valoarea întreagă;
  - 11 forme de valori: o cheie privată, antetele de autorizare, parola dintr-un URL,
    `password=` / `pwd=` / `api_key=`, JWT, tokenurile GitHub, AWS, `sk-` și Slack.
- Schema `token …` se ascunde doar pentru o valoare de cel puțin 16 caractere, ca „the token
  stream” să rămână neatins.
- `kernel::Secret` exista deja, cu un `Debug` care nu-l arată; nu mai trebuie un tip nou.
- Testele: fiecare formă, ce nu trebuie atins (căi, URL-uri fără parolă, cod), o linie
  întreagă prin strat, și compilarea tuturor formelor.

**89c — Nivelurile, setările și trecerea prin cod** (2026-11-20).
- **Setările:** pagina Settings → Application → „Jurnal” (`crates/app/src/logs.rs`), cu:
  - nivelul: Oprit, Erori, Avertismente, Info, Debug;
  - dosarul: câmp de text, plus butoanele „Alege dosarul…” (`log.choose_folder`, cu
    selectorul de dosare al sistemului) și „Arată”;
  - nivelurile pe module (`database_explorer=debug`): o parte neînțeleasă e lăsată deoparte
    și spusă sub câmp.
- Toate se aplică pe loc. `logging::install` pune un filtru reîncărcabil (`set_filter`), iar
  scriitorul își schimbă dosarul (`set_dir`, `Msg::Dir`). „Oprit” înseamnă filtru `off` și
  niciun fișier deschis. `EDITOR_LOG` / `RUST_LOG` au întâietate, iar pagina spune asta.
- Help → „Reveal Log Folder” (`help.reveal_log_folder`) arată dosarul, făcut întâi dacă nu
  există.
- **`debug` rămâne compilat în release.** Măsurat în release, cu nivelul Info: un `debug!`
  oprit costă 0,3 ns (10 milioane în 3 ms), pentru că `tracing` ține interesul fiecărui loc
  în cache. Testul e `#[ignore]`, în `tests/unit/layer.rs`.
- **Trecerea prin cod:** 525 de apeluri.
  - 329 lipeau valori în mesaj. 286 au fost trecute în câmpuri de două scripturi
    (`error = %e`, `path = %path.display()`, numele câmpului luat din expresie). Scripturile
    nu ating mesajele unde rezultatul ar ieși ciudat (o valoare după o prepoziție). Încă 38
    au fost rescrise de mână, cu mesaje scurte și câmpuri numite. Au rămas cinci, în
    `snapshot.rs` (unealtă de dezvoltare), lăsate intenționat.
  - 15 `info` care se repetă la fiecare acțiune obișnuită au trecut pe `debug`: indexarea la
    fiecare salvare, limbajul detectat, răspunsul unui dialog, pipeta, blocarea unui tab,
    înlocuirile și altele.
  - `eprintln!` / `println!` rămân doar în driverul-jucărie de test (stdout e protocolul),
    în serverul de limbaj (unealtă de linie de comandă) și în modul ajutător MCP al aplicației
    (stdout e protocolul lui).
  - Nimic secret nu se loghează prin cod: niciun câmp cu nume de secret, niciun conținut de
    fișier. Plasa `redact` prinde acum și parolele din SQL (`IDENTIFIED BY '…'`,
    `WITH PASSWORD '…'`).
- `legacy.log` pleacă la curățenie după cele 14 zile, ca restul.
- Regula se află în `.claude/rules/project-conventions.md` (secțiunea „The log”): mesaj
  scurt și valori în câmpuri, ce nivel pentru ce, nimic secret.

**89d — Vizualizatorul** (2026-11-20).
- **Un vizualizator de fișier, nu o fereastră.** Modulul `log-viewer` recunoaște numele
  jurnalului (`app-<zi>[-<run>][.<parte>].jsonl`) și îl deschide în tab-ul lui de editor ca
  grilă de linii (`ViewerKind::Replace`), ca o imagine. Alt fișier `.jsonl` rămâne text.
  Help → „Show Log” (`log.show_log`) deschide fișierul de azi. Numele lui se calculează din
  dată, deci clicul nu citește nimic de pe disc.
- **Fișierul nu e niciodată întreg în memorie.** Fiecare vedere are firul ei de citire
  (`log-reader`, dintr-un pool de 16) care:
  - parcurge fișierul în bucăți de 64 KB și ține doar începutul fiecărei linii (8 octeți pe
    linie); o linie încă în scriere așteaptă până se termină;
  - citește rândurile cerute bloc cu bloc (câte 200), linie cu linie, prin `seek`, și le
    parsează acolo;
  - filtrează tot acolo, în tranșe de 20.000 de linii, spunând numărul pe parcurs.
  Firul interfeței ține doar blocurile afișate, cel mult 32 (6.400 de rânduri); cele mai puțin
  văzute pleacă primele. Nimic de pe firul interfeței nu atinge discul.
- **Live:** la 500 ms se verifică mărimea fișierului; doar ce s-a adăugat e parcurs și
  filtrat, apoi e spus noul număr. Un fișier micșorat sau înlocuit se citește de la capăt.
  Cele mai noi linii sunt primele: rândul `r` e linia `count - 1 - r` din cele afișate, deci
  un bloc rămâne aceleași linii oricât ar mai veni.
- **Pauza e doar vizuală:** numărul afișat rămâne (rândurile nu se mișcă), iar cititorul
  merge mai departe. Bara spune „N linii, în pauză · M noi”; „Reia” arată ce a venit. Un bloc
  citit pentru un număr care nu mai e afișat e lăsat deoparte.
- **Grila e cea a aplicației** (`DataGrid`), cu coloanele `ts` (ora locală, la milisecundă),
  `level`, `target`, `thread`, `msg` și `fields` (`nume=valoare`, plus span-urile).
  Filtrarea se face prin WHERE-ul ei (`level = 'ERROR'`, `msg LIKE '%save%'`). Merg și
  copierea, căutarea și exportul.
- **„Mai vechi” / „Mai nou”** deschid fișierul de dinainte și de după. Le caută cititorul,
  pe firul lui.
- Testele: linia citită și filtrul; cititorul (linii neterminate, creștere, înlocuire, un
  fișier mult mai mare decât o bucată, blocuri, filtrul pe bucăți, vecinii); sursa (fiecare
  bloc cerut o singură dată, Pauza, un filtru nou, limita de blocuri); numele fișierelor; o
  vedere reală, cu fișier și fir adevărate (live, pauză, reluare).

- **Detaliile unei linii** (cerute după): un rând ales, cu un clic sau cu săgețile, își
  arată detaliile sub grilă, pe 40% din înălțime:
  - ca titlu, mesajul întreg;
  - ce e linia: nivelul, ora, modulul, firul, run-ul;
  - fiecare câmp pe rândul lui;
  - span-urile, cu câmpurile lor.
  Textul se selectează, se copiază și se derulează. × sau Esc îl închid până se alege alt
  rând. În grilă, o celulă se taie la 300 de caractere (pe firul de citire, cu „…”); linia
  rămâne întreagă pentru detalii, iar filtrul lucrează pe textul întreg. Nivelul e prima
  coloană. Coloanele se redimensionează trăgând de marginea lor (din grilă).

## Lăsat

- **Arhiva pentru un raport de bug** („Collect Logs for a Bug Report…”, cu căile
  anonimizate): încă de făcut.
- (Făcut după) **Nivelul cu iconița lui** și **exemplul WHERE al jurnalului**. Grila are în
  `GridSource` două metode noi, cu implementare implicită:
  - `cell_icon`: o iconiță în tonul ei, înaintea textului unei celule, pusă la socoteală
    când se măsoară coloana;
  - `filter_hint`: exemplul propriu al câmpului WHERE.
  Nivelurile au iconițele lor: eroare `octagon-alert` (roșu), avertisment `alert`, info
  `info` (accentul), debug și trace `wrench` (estompat). Exemplul WHERE e
  `level = 'ERROR' · target LIKE '%database%' · msg LIKE '%save%'`. Celelalte surse ale grilei
  nu se schimbă.
