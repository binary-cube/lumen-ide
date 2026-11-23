# TODO — lăsate pentru mai târziu

Lucruri hotărâte, dar amânate cu un motiv. Se iau de aici când le vine rândul, iar cele
făcute se șterg.

## Următorul (cerut)

- **Windows: decupajele urâte peste pagina motorului web** (raportat 2026-11-23, cu captură).
  Peste editorul de diagrame (o pagină a motorului web al sistemului), pastilele de resurse
  fixate în colț (CPU, RAM, GPU și pinul) apar înconjurate de pete albastre mari, rotunjite
  și deplasate. Forma lor trebuie să fie scoasă exact din pagină, ca să se vadă pastilele
  peste ea (`StatusPopover::shapes` și `resource_pills`: dreptunghiuri și colțuri, în pixeli
  fizici).
  - **De verificat:** cum se aplică forma pe fereastra motorului web pe Windows (regiunea
    ferestrei copil): unitățile (fizic față de logic, scara monitorului), colțurile (o
    elipsă în loc de un dreptunghi rotunjit), culoarea care umple ce e decupat (accentul în
    loc de transparent sau de fundalul pastilei), poziția față de fereastră.
  - Pe macOS de comparat: acolo arată corect?
  - **Făcut (2026-11-23):** codul de decupare recitit (poziția față de pagină, rândurile,
    colțurile, pixelii fizici) pare corect; fără Windows nu se poate vedea ce face sistemul.
    Jurnalul spune acum, la `debug`, zona paginii, fiecare gaură cerută (în pixelii paginii)
    și caseta regiunii puse (`GetRgnBox`). **De făcut pe Windows:** pornit cu
    `EDITOR_LOG=debug`, pastilele fixate peste editorul de diagrame, liniile „a page's hole
    cut” / „a page's region set” din jurnal arată unde e nepotrivirea.

- **Windows: „Open with” deschide altă instanță a aplicației** (raportat 2026-11-23). Un
  fișier deschis cu „Open with” → aplicația, apoi alt fișier la fel: se deschide o
  aplicație nouă, în loc să ajungă la cea care rulează (`instance::claim`). Instanța e
  găsită după un hash FNV al drumului executabilului (`executable_tag`, după
  `canonicalize`), pe un pipe numit după el și după utilizator.
  - **Bănuiala:** pe Windows drumurile nu țin cont de literele mari și mici, dar hash-ul
    ține. „Open with” sau scurtătura de pe desktop pot porni programul cu alt drum: altă
    scriere a literelor, un drum scurt 8.3, `\\?\` în față. Atunci pipe-ul are alt nume.
  - **Făcut (2026-11-23):** drumul adus la o formă unică înainte de hash pe Windows
    (`instance::one_form`: fără `\\?\`, `\\?\UNC\`, `/` ca `\`, litere mici, fără separator
    la capăt; testat); același hash numește și canalul dintre instanțe (`peers`). Jurnalul
    spune canalul (`tag`) ascultat („listening for later starts”) și cel căutat („handed to
    the running instance”).
  - **Rămas:** confirmarea pe Windows (dacă tot pornește o instanță nouă: cele două
    `tag`-uri din jurnal spun dacă e alt nume sau altă cauză).
  - **De verificat:** dacă a doua pornire chiar încearcă (jurnalul „handed to the running
    instance”); dacă primul proces ascultă încă după ce și-a deschis fereastra.

- **Fișierele foarte lungi, analizate și sparte** (cerut 2026-11-20, după profilare). De
  găsit fișierele cele mai lungi din `crates/` (de numărat liniile, cele peste ~800 întâi),
  apoi pentru fiecare: ce face, dacă face optim (nimic pe firul interfeței, nimic refăcut pe
  cadru, fără copii inutile), și împărțit în fișiere mici, pe blocuri cu un rost fiecare
  (`code.md`: un dosar pentru o funcție mare). Ținta: cod de calitate, ușor de urmărit și de
  extins. Fără să se schimbe ce vede utilizatorul; testele rămân verzi după fiecare fișier.

- **Profilarea aplicației — prioritate mare** (cerut 2026-11-20; planul:
  `docs/plan/91-profilarea-aplicatiei.md`). Pornită cu acordul utilizatorului, anonimă,
  oprită singură după o durată aleasă, un fișier JSON mic de trimis, un tab cu
  înregistrările (axa de timp, flame graph). Primul folos: CPU-ul de ~60% în repaus de pe
  Ubuntu.

- **Terminalul: istoricul comenzilor pe proiect** — **făcut 2026-11-18**, cu setarea
  „Istoricul comenzilor pe proiect” (implicit pornită): fișierul în
  `<date>/terminal-history/<cheia proiectului>/` (cheia: hash-ul căii rădăcinii, `0600`);
  zsh prin `ZDOTDIR`-ul aplicației (`/etc/zshrc` de pe macOS își pune `HISTFILE`: al nostru
  e pus ultimul, după fișierele utilizatorului), bash prin `HISTFILE` (Git Bash: `/c/…`),
  fish prin `fish_history`, PowerShell prin `Set-PSReadLineOption -HistorySavePath` după
  profil, WSL prin `WSLENV` (`HISTFILE/p`), cmd neschimbat. Încercat cu zsh-ul real pe
  macOS (test `--ignored`). **Rămas:** de încercat de mână pe Windows (PowerShell, Git Bash,
  WSL) și pe Linux (bash), și un bash al cărui `.bashrc` își setează singur `HISTFILE`
  (câștigă el, dacă promptul colorat e oprit). Planul de la început, pentru context:
  Acum un shell deschis
  în editor folosește istoricul obișnuit al utilizatorului (`~/.zsh_history`,
  `~/.bash_history`…), amestecat cu ce a scris în alte terminale și în alte proiecte; ↑ și
  Ctrl+R aduc comenzi care n-au nimic de-a face cu proiectul. De făcut:
  - fiecare proiect cu istoricul lui, ținut în dosarul de date al aplicației (după
    identitatea proiectului, nu în `.lumen/` din repo: nimic personal ajuns din greșeală
    într-un commit), creat la prima comandă;
  - dat shell-ului la pornire, după felul lui: `HISTFILE` pentru zsh și bash (Oh My Zsh și
    configurările obișnuite îl păstrează dacă e deja setat), `fish_history` pentru fish (un
    nume de sesiune: `~/.local/share/fish/<nume>_history`), `-HistorySavePath` (PSReadLine)
    pentru PowerShell; un shell necunoscut rămâne cu al lui;
  - o setare a terminalului: „Istoric pe proiect” (implicit) / „Istoricul meu obișnuit”;
  - de verificat ce se întâmplă când `.zshrc` / `.bashrc` scriu `HISTFILE` necondiționat (îl
    suprascriu: de spus în setare, sau de pus și după rc, ca ultim cuvânt);
  - sesiunile agenților (Claude Code în terminal) nu se schimbă: nu sunt shell-uri;
  - **pe toate sistemele, cu shell-urile lor:**
    - macOS și Linux: zsh, bash, fish, ca mai sus; pe Linux bash e de obicei cel implicit, iar
      dosarul de date e `~/.local/share/<id>/` (XDG), cu fișierul de istoric doar al
      utilizatorului (`0600`, ca `~/.bash_history`);
    - Windows, PowerShell (5 și 7): istoricul e al PSReadLine
      (`%APPDATA%\Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt` de obicei),
      schimbat la pornire cu `Set-PSReadLineOption -HistorySavePath <fișier>` (dat prin
      `-NoExit -Command`, fără să ocolim profilul utilizatorului); căile cu spații citate;
    - Windows, `cmd.exe`: nu ține istoric între sesiuni (doskey e doar în memorie): nimic de
      făcut, de spus în setare;
    - Windows, Git Bash / MSYS2: bash, deci `HISTFILE`, dar cu calea în forma lor
      (`/c/Users/…`, nu `C:\Users\…`);
    - Windows, WSL (`wsl.exe`): variabila trece în Linux doar prin `WSLENV` (`HISTFILE/p`, care
      îi traduce și calea); de verificat că un `.bashrc` din distribuție n-o rescrie;
  - teste: variabila (sau argumentele) date fiecărui shell pe fiecare sistem, căile în forma
    lui (Windows, MSYS, WSL), dosarul unui proiect altul decât al altuia, setarea oprită lasă
    mediul neatins; încercat de mână pe macOS, Windows și Linux înainte de a fi gata.

- **Baze de date: crearea de obiecte din fereastra Database** (cerut 2026-11-16:
  meniul „New ›” cu Query Console, Database, Role, User, Virtual View… și
  dialogul „Create”, cu formularul sus și **Preview**-ul SQL dedesubt, actualizat cât
  scrii). Măcar **tabele**, apoi baze de date / scheme. **Totul după driver** (PostgreSQL,
  MySQL / MariaDB, SQLite, ce mai adăugăm), fiindcă sintaxa și opțiunile diferă:
  - **Database / schema**: numele, comentariul; PostgreSQL: owner, tablespace, template,
    allow connections; MySQL: charset, collation.
  - **Table**: numele, schema, comentariul; coloanele (nume, tip din lista driverului,
    nul / nu, implicit, auto-increment / identity / `serial`, comentariu); cheia primară;
    indecșii (unic, coloanele, ordinea); cheile străine (tabela, coloanele, `ON DELETE` /
    `ON UPDATE`); MySQL: engine, charset, collation; SQLite: `WITHOUT ROWID`, `STRICT`.
  - **Preview-ul** e SQL-ul exact care rulează (generat de un crate de domeniu pe driver,
    cu identificatorii citați corect: `"x"` / `` `x` ``), colorat ca în consolă, copiabil;
    OK îl rulează într-o tranzacție unde driverul o permite, iar eroarea serverului apare
    în dialog, fără să-l închidă.
  - Sursa marcată producție cere confirmare (ca la scrierile din consolă); după creare,
    arborele se reîmprospătează și noul obiect e selectat.
  - Mai târziu: „Modify Table…” (ALTER) cu același formular, diff-ul ca preview.
  Componentele: formularul nostru (`form`), lista de coloane ca grilă editabilă (cea din
  sursele de date), editorul read-only pentru preview.

- **Logurile: ce rămâne după planul 89** (cerut 2026-11-16; jurnalul JSON, rotația, ascunderea
  secretelor, nivelurile din setări și pe module, mesajele cu câmpuri, vizualizatorul: făcute în
  planul 89). Rămân:
  - **Arhiva pentru un raport de bug** (în `ROADMAP.md`).
  - Hotărâte 2026-11-20 (scrise în planul 89): `debug` / `trace` rămân în release, iar
    diagnosticele rămân cum sunt.

- (Făcute 2026-11-20) **Sesiunile agenților:** una pornită cu „+” și reluată din listă
  înainte să-i vadă firul de urmărire fișierul își găsește tab-ul nou pe loc
  (`follow::tab_of`), fără un al doilea; una a lui Claude cu primul rând peste 64 KB își are
  dosarul și ora pornirii căutate mai departe, în bucăți, cel mult 4 MB (`files::deeper`).

- ~~**Dosarele de grup în `crates/domain` și `crates/infra`**~~ — **făcut 2026-11-12**
  (varianta A, aleasă de utilizator).
  - Grupurile au numele celor de module. În `domain`: `core`, `code`, `database`, `vcs`,
    `project`, `tools`, `http`, `appearance`, `viewers`. În `infra`: `code`, `database`,
    `files`, `system`, `http`, `remote`, `vcs`, `media`.
  - Fiecare crate își păstrează dosarul cu același nume (fără redenumiri).
  - În `architecture.toml`, `crates/domain/*/*` și `crates/infra/*/*`.
  - Totul s-a mutat într-un singur commit, cu `git mv`.

  Ce scria înainte (cerut 2026-11-11; **de făcut după planul 77, cu o analiză întâi**). Azi ~28 de crate-uri stau direct în `domain/` și
  ~20 în `infra/`, fiecare cu dosarul lui; `crates/modules/` e deja pe grupuri (`languages/`,
  `tools/`, `database/`, `vcs/`…). Cerut: să le putem grupa și acolo sub dosare de grup.
  De analizat atunci: grupurile (după domeniu — baze de date, cod / limbaje, proiect, UI —
  sau după cum le grupează modulele, ca un crate de domeniu și adaptorul lui să fie ușor de
  găsit împreună); ce se schimbă: căile din `Cargo.toml` (membrii și dependențele
  workspace-ului), regulile din `architecture.toml` (`crates/domain/*` devine
  `crates/domain/*/*`, ca la module), scripturile și `xtask`-ul care numesc căi, planurile
  care le citează; un singur commit care mută tot (`git mv`, ca istoria să urmeze), cu
  gate-ul verde; numele crate-urilor rămân aceleași (nimic din cod nu se schimbă).
  Capturile utilizatorului (păstrate de el, în afara repo-ului): `crates/domain` și
  `crates/infra` așa cum sunt acum.

- **Modul pentru cereri HTTP, ca un client HTTP dedicat sau cel dintr-un IDE** (cerut
  2026-11-11; **planul: `docs/plan/77-cereri-http.md`**, deciziile la utilizator). „Super profesional și bine gândit”.
  **2026-11-12:** făcute și redenumirea / ștergerea mediilor, `SSLConfiguration` la
  WebSocket, JWT RS / PS / ES, OAuth Implicit, HTTP/2. **Rămân pe listă** (hotărât de
  utilizator): gRPC, GraphQL prin WebSocket, ES512, Revoke URL, cheia cu parolă.
  Ce a cerut utilizatorul:
  - **cereri organizate**: colecții, dosare; metoda, URL-ul, antetele, corpul (payload-ul);
  - **variabile** și **medii** (variabile la nivel de mediu), alese la rulare („Run with:
    No Environment”);
  - **salvate în proiect, ca fișiere care se pot comite**, ca toată echipa să aibă aceleași
    cereri; mediile cu `.local` (sau „private”) sunt doar ale utilizatorului și nu se comit
    (ca „Add Environment to Public File… / Private File…”);
  - **răspunsul**: codul, timpul, mărimea, antetele; corpul formatat după tip (JSON, YAML,
    XML…); HTML-ul randat (avem deja un randor de HTML care merge și pe macOS, și pe
    Windows; de verificat ce poate);
  - **cereri înlănțuite**: din răspunsul unei cereri se ia o valoare pentru următoarea, cu o
    cale ca `$.response.data.items[0].id`, **cu autocompletare** (din răspunsul primit
    ultima dată, din schema lui);
  - **lanțul văzut ca graf**, cu **play / stop**, ca să se vadă cum trec datele dintr-o
    cerere în alta;
  - **formatoare / transformări** pe date, unde e nevoie;
  - **fișierele `*.http`**: rulare din gutter (▷ lângă cerere), bara de sus
    (cerere nouă, istoric, „Run with” cu mediul, rulează tot), răspunsul salvat
    (`…/2026-11-11T121924.200.html`, „Show Preview”), cookie-urile păstrate între cereri;
  - de văzut și ce mai fac alți clienți HTTP (colecțiile ținute ca fișiere text în repo: model
    bun pentru partea „comisă”).

  Capturile trimise de utilizator (dintr-un alt client; păstrate de el, în afara repo-ului):
  - bara unui fișier `.http`;
  - rularea din gutter;
  - mediile, public sau privat;
  - răspunsul, în fereastra Services.

  De respectat la proiectare: un modul ca toate (`crates/modules/…`), fără să știe de
  altele (cereri / evenimente / gazdă); rețeaua, citirea și formatarea în fundal; nimic
  ținut în memorie cât nu e deschis; Windows.

- **Comentariile: navigarea doar unde e scris cod** — **făcut în mare** (2026-11-11, plan
  `docs/plan/75-comentarii-navigabile.md`, 75a–75e): PHP (PHPDoc, PHPStan, Psalm, adnotări),
  JavaScript / TypeScript (JSDoc, TSDoc), CSS / SCSS (SassDoc), HTML; definiția, hover-ul,
  sublinierea de la ⌘, rename-ul și „Find Usages”. **Rămase** (motivul în plan): `{# @var #}`
  în Twig și `{{-- @var --}}` în Blade (cer o cerere nouă între module), o schemă YAML dată
  prin cale relativă, colorarea tag-urilor în comentarii (de verificat pe ecran).
  **Făcute 2026-11-12:** `@var` în Twig / Blade (duce la clasa PHP), codul dintre
  backtick-uri (PHP, JS/TS), completarea tag-urilor la JSDoc / TSDoc și SassDoc, schema YAML
  prin cale relativă. Rămâne colorarea tag-urilor (de verificat pe ecran).

- **Windows: autosave-ul nu pare să salveze** (2026-11-10, raportat de utilizator; un singur
  fișier deschis, fără proiect). **Revine utilizatorul cu logul.** Pe macOS testul pentru
  exact acest caz trece (`autosave_writes_changed_files`), logica e aceeași pe toate
  sistemele, iar scrierea pe Windows reîncearcă și scrie pe loc la nevoie. Bănuiala:
  autosave-ul sare peste un fișier pe care îl crede schimbat pe disc, fiindcă mărimea sau data
  lui diferă de cele văzute la ultima salvare. Din `f0b990d7`, logul spune de ce nu scrie:
  - de cerut: `findstr /i "autosave" "%APPDATA%\lumen\lumen.log"` după un build nou (scris,
    apoi câteva secunde de pauză), plus ce se vede în aplicație (punctul de pe tab, o
    notificare „schimbat pe disc”);
  - „its stamp on disk differs” (cu `seen` / `now`): de reparat comparația pe Windows (data
    cu altă precizie după rename, sau citită înainte să fie scrisă de NTFS);
  - „the file changed on disk meanwhile”: watcher-ul a luat salvarea noastră drept a altcuiva;
  - nimic în log: autosave-ul nu se declanșează (termenul de redraw, focusul pe Windows).

- ~~**DE FĂCUT ACUM (aprobat de utilizator, 2026-11-06)**~~ — **făcut** (verificat
  2026-11-08: planurile 63, 64, 65 au toți pașii bifați):
  1. ~~`docs/plan/63-remote-sincronizare-manuala.md`~~ — **făcut** (63a–63c, 2026-11-06).
  2. ~~`docs/plan/64-remote-proprietati.md`~~ — **făcut** (64a–64d, 2026-11-06).
  3. ~~`docs/plan/65-framework-contributii.md`~~ — **făcut** (65a–65d, 2026-11-07).
  Fiecare pas (63a, 63b…) e un commit; „Stare” din plan se bifează după fiecare.
  Cele trei intrări mai lungi de mai jos (sincronizarea remote, drepturile, contribuțiile
  modulelor) sunt detaliate în planurile astea.

- ~~**Git: ramura schimbată din afară se vede și la noi**~~ — **făcut** (2026-11-08, cerut de
  utilizator):
  - starea Git (`HEAD`, ramura, o operație în curs) se recitește și când fereastra primește
    focusul (`Workbench::refresh_vcs`), nu doar când watcher-ul aude `.git`. Asta acoperă
    `.git`-ul din afara proiectului urmărit și evenimentele pierdute;
  - portul `Repository` spune `detached()` și `operation()` (rebase cu ramura și pasul, merge,
    cherry-pick, revert, bisect, `git am`), citite din fișierele lui git;
  - eticheta din bara de sus: `feature · rebase 3/7`, `main · merge în curs`,
    `detașat la 275df80`.

- ~~**Git: conflictele arătate și rezolvabile la noi**~~ — **făcut** (2026-11-08, cerut de
  utilizator), în modulul Git (`vcs-git`) prin portul `Repository`:
  - conflictele apărute (de la noi sau din SourceTree / terminal) sunt anunțate o dată, cu o
    notificare care duce la tabul „Conflicte”; butonul Git e în avertisment cât durează;
  - tabul „Conflicte” din fereastra Git arată operația (`Rebase · feature · 1/2 — 2
    conflicte`) cu pașii ei (Continuă, Sari peste, Anulează, cu confirmare) și fișierele în
    conflict. Meniul unui fișier: Rezolvă…, ia una dintre părți pentru tot fișierul (la
    rebase numite pe față: baza rebase-ului / commitul tău), Marchează rezolvat;
  - Rezolvă… deschide vederea în trei părți (a ta | rezultatul | a lor, în vederea Compare):
    ce a schimbat doar o parte e luat deja, fiecare conflict așteaptă o alegere (stânga,
    dreapta, ambele, niciuna). Salvarea rezultatului îl marchează rezolvat (`git add`);
  - pașii și meniul Git: Rezolvă conflictele…, Continuă, Sari peste commit, Anulează operația.
  - rezultatul din mijloc se poate scrie și de mână („Scriu manual”).

- ~~**Taburi care găzduiesc alte aplicații**~~ — **abandonat** (2026-11-08): vezi
  `docs/plan/68-taburi-gazduite.md` (de ce, și ce condiții ar trebui îndeplinite dacă revine).

- ~~**Calculatorul nu intră în sleep cât lucrez**~~ — **făcut**: plan 66 (`docs/plan/66-tinut-treaz.md`, 66a–66e, 2026-11-07). (cerut de utilizator, 2026-11-06).
  - Un **modul** al lui, cu buton în bara laterală din dreapta, **deasupra celui de
    „Mod concentrare”** (înregistrat ca fereastră de unelte / comandă, nu cod în aplicație):
    cât e pornit, calculatorul nu intră în sleep. Exemplu: pornesc un AI în terminal (sau
    altă treabă lungă: transferuri, indexare) și sistemul nu trebuie să adoarmă și să-i
    oprească treaba.
  - Doar sleep-ul sistemului e oprit; ecranul se poate stinge sau bloca în continuare.
  - **Ecranul protejat** (cerut, 2026-11-07), ca opțiuni ale modulului:
    - **fără burn-in**: cât mașina e ținută trează și nimeni nu atinge nimic, ceva se mișcă
      încet pe ecran (o scenă ca la Relax, sau conținutul deplasat câțiva pixeli din când
      în când), ca nimic să nu stea ore în același loc;
    - **dim după un timp**: după N minute fără activitate (setabil), fereastra se întunecă
      treptat (un văl peste tot); orice mișcare de mouse sau tastă o aduce înapoi;
    - fără să consume degeaba: câteva cadre pe secundă, nimic pe UI thread în rest, oprit
      când fereastra e ascunsă.
  - **Totul configurabil, dintr-o singură sursă** (cerut, 2026-11-07): ținut treaz (pornit /
    automat cât rulează ceva), fără burn-in (da / nu, ce se mișcă), dim (după câte minute,
    cât de întunecat). Declarate o dată în `src/settings.rs` al modulului (`Setting<T>`) și
    arătate în două locuri care folosesc **aceleași rânduri**:
    - în fereastra de setări (blocul modulului, `SettingsContribution`);
    - la clic pe butonul lui: un popover cu aceleași opțiuni, la îndemână.
    Fără cod dublat: popover-ul desenează rândurile blocului de setări (`SettingsRow`), cu
    aceleași componente (`form`), iar o alegere trece prin serviciul de setări în ambele
    locuri (se vede imediat și în celălalt). Dacă lipsește o astfel de componentă comună
    („rândurile unui bloc de setări într-un popover”), se face în `ui-components`, ca s-o
    poată folosi și alte module.
  - macOS: o aserțiune IOKit (`IOPMAssertionCreateWithName`,
    `PreventUserIdleSystemSleep`), cu motivul arătat de `pmset -g assertions`.
    Windows: `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)` (sau
    `PowerCreateRequest` / `PowerSetRequest`). Linux: `systemd-inhibit` / logind, unde există.
  - Ca modul, cu un port în domeniu și implementarea per sistem în infra. Eliberat la
    oprire, la închiderea aplicației și dacă aplicația cade (aserțiunea moare odată cu
    procesul). Starea arătată discret (iconiță în bara de jos) și ținută minte ca setare.
  - De gândit: varianta „automat”, care ține treaba trează doar cât rulează ceva (un
    terminal cu un proces activ, transferuri, indexare) și o lasă liberă după aceea.

- ~~**Baze de date și fișiere de date, ca într-un IDE complet**~~ — **făcut** (verificat 2026-11-08):
  planul 37, 37a–37j „gata”; modulele `database/*` există. **Nefăcute, cu motiv** (scrise în
  plan): tunelul SSH, tragerea unui fișier peste fereastră, gazda WASM pentru drivere.

- ~~**Module care contribuie la motorul intern de limbaj**~~ — **făcut** (verificat
  2026-11-08): plan 65 (65a–65d), `FrameworkSupport` în `code-intelligence`; modulele Symfony,
  Laravel, Yii, Vue, React, htmx și `.env` îl folosesc, cu generația proprie.

- ~~**Proiecte remote: sincronizarea automată, pornită sau oprită**~~ — **făcut** (verificat
  2026-11-08): plan 63 (63a–63c), `auto_upload`, lista „De urcat” (`pending_view`).

- ~~**Proiecte remote: proprietățile și drepturile, ca în WinSCP**~~ — **făcut** (verificat
  2026-11-08): plan 64 (64a–64d), `properties_view` cu Checksum, valori mixte, coloanele
  opționale (`[remote] show_permissions`).


## Rămase din planurile 43–55

- **LSP (43, 46):** CPU-ul în repaus al phpactor (timerele lui); phpactor pe un Windows real;
  ~~pull diagnostics~~ — **făcut** 2026-11-09 (`lsp_client::client::pull`); ~~Structura prin
  `documentSymbols`~~ — **era făcută** (46d, `servers::served_outline`).
  ~~Propriul nostru server~~ — **făcut** 2026-11-09 (43f): binarul `language-server`.
- ~~**Izolarea în teste (48)**~~ — **făcut** (verificat 2026-11-08): `dirs::config_dir` mai
  apare doar în `files::places`, unde se mută fișierele vechi.
- ~~**Un test pe ceas (48):**~~ — **reparat** 2026-11-10: cardul și parametrii arată că n-au
  așteptat timeout-ul prin întrebarea serverului încă în așteptare, nu prin ceas (limita de
  1,5 s cădea sub încărcare); 24 de rulări în paralel, nicio cădere.
- **Teste instabile sub `./scripts/check.sh`.** Reparate 2026-11-08: fișierele de licență din
  `language-servers` și închiderea consolei (un test odată), `intelligence` (așteptare mai
  lungă), numerotarea terminalelor, limita Mermaid. **De urmărit:**
  ~~`projects::tests::nothing_starts_without_a_document_of_a_served_language`~~ — **reparat**
  2026-11-10: număra pornirile tuturor proiectelor, iar un server lăsat pornind de testul
  dinainte putea veni târziu pe aceleași fakes; acum numără doar pentru proiectul lui
  (`Harness::made_for`, verificat și pe dos). Nereprodus în 40 de rulări în paralel.
- **Meniuri (49, 51):** Tools › Remote › și ⌃⌘F pentru Full Screen pe macOS sunt făcute
  (2026-11-08); meniul nativ macOS verificat doar prin construcție (55b: notificarea de deschidere neîncercată de un test).
  - Făcute: săgețile din scurtături (`⌥↑`, nu „⌥Up”: în 49d); rândurile condiționate
    (2026-11-08). Un rând al altui limbaj (`Requires::Language`, „Reformat SQL Statement” în
    afara SQL) e scos din drawer, iar separatorii și submeniurile rămase goale pleacă odată cu
    el (`tidy_menu`, cu teste în `commands.rs` și `context_menu.rs`, reverificat 2026-11-10);
    în meniul nativ rămâne dezactivat, cum vrea sistemul.
  - Rămas doar: notificarea de deschidere a meniului nativ macOS, fără test (cere un meniu
    de sistem real).
- **Terminalul (54):** pe Windows, dacă ConPTY trece `ESC[3J` mai departe (derularea la pixel:
  făcută în 55l).
- ~~**Setările din afară (55e)**~~ — **făcut** 2026-11-09: tema și limba din `settings.toml`, aplicate pe loc.

## Decuplarea modulelor

**Planul complet: `docs/plan/69-comunicare-module.md`** (patru mecanisme, cereri cu răspuns).

Făcut (2026-11-08): dependențele declarate între limbaje scoase (PHP, Blade, Twig nu se mai
opresc fără HTML), Blade cu propriile interogări, Services și blocul de setări al decorațiilor
pe contracte neutre, iar `cargo xtask check-deps` refuză `depends_on`, fișierele citite din
alt modul și id-urile de comenzi / vederi ale altui modul. `workbench_model::presence` spune
dacă cineva răspunde: butoanele fără partener sunt ascunse (Services, Effects, Compare, Task
Manager, motorul de limbaj, Remote, Theme Studio, Usages).

Rămâne:

- ~~**Limbajele numite în listele altor module**~~ — **hotărât: rămân** (2026-11-08, plan 69):
  sunt mai ales tipuri de fișiere, valabile și fără un modul al lor.
- ~~**Consola SQL fără Services**~~ — **făcut**: rezultatele într-un tab propriu.
- ~~**`PENDING` rămas**~~ — **făcut** 2026-11-09: `ViewerAction::OpenView` / `WithHost`, `Opening::View`;
  comenzile lor de legătură au dispărut.

## Performanță

- **Profilare și recenzii (2026-11-12)** — un proiect de probă cât unul Laravel real:
  3.200 de fișiere PHP proprii, 20.000 în `vendor/` (20 de pachete, cu `composer.json`),
  10.000 de fișiere JS în `node_modules/`, un depozit git; build-ul de release, macOS.
  - **Măsurat:** indexul de simboluri al celor 23.200 de fișiere PHP în 6,0 s (din cache
    0,4 s); `node_modules` lăsat afară; în repaus **0,0–0,2 % CPU**, toate firele dorm
    (`sample`: nimic treaz); amprenta fizică **151 MB** stabilă (heap ~76 MB scris: indexul
    de simboluri; suprafețele ferestrei 31 MB). La prima indexare RSS-ul urcă la 315 MB și
    coboară singur la 160 MB după 3 minute (indexul de text și memoria eliberată date
    înapoi).
  - **Reparate** (verificate în cod întâi, fiecare cu test):
    - editoarele vizuale HTTP (cererea, mediile) desenau cadre unul după altul după o pauză
      în scriere: termenul unei vederi cerea un cadru, dar salvarea se făcea doar la `Wake`.
      Acum vederile cu termenul trecut sunt întrebate înainte de cadru (general, pentru toate);
    - indicatoarele de lucru (Services cu o interogare, o cerere HTTP, lanțul de cereri,
      proprietățile remote) redesenau la frecvența ecranului: acum la ritmul unui loader
      (20 de cadre/s; `ToolPanel::is_loading`, nou);
    - **`git status` la fiecare scriere ignorată** (`storage/logs`, cache-ul framework-ului,
      un build în watch): o scriere din afară în care totul e ignorat de `.gitignore`-urile de
      pe drum (și `info/exclude`) nu mai cere o citire (`git::ignored`, regulile citite o dată,
      din nou doar la o dată schimbată);
    - scrierile interne (`.git/index` atins de propriul `git status`, `.lumen/`) nu mai
      trezesc bucla UI;
    - o instalare lungă (peste 4.096 de căi) cerea o reverificare completă la fiecare 4.096:
      acum una, după o secundă de liniște;
    - un fișier deschis, schimbat de alt program (un log), era citit întreg pe firul UI: acum
      pe un fir, aplicat la `poll` (pe loc doar fără trezitor: teste, snapshot-uri);
    - construirea indexului de text creștea cu pătratul fișierelor (fiecare fișier trecea
      prin tot indexul): acum în loturi de 512 (măsurat pe 3.400 de fișiere: 425 → 288 ms;
      la 20.000 de fișiere proprii, câteva secunde câștigate);
    - `ds::ellipsize*` (vreo 270 de locuri) forma textul de 8–12 ori pe etichetă la fiecare
      lățime nouă (tragerea unui splitter, redimensionarea): acum o dată, tăiat pe pozițiile
      glifelor, plus o verificare;
    - vederea răspunsului HTTP ținea răspunsul greu și după ce bugetul de memorie îl lăsase
      în fișier; are acum și `purge`. Un răspuns `# @no-log` peste 1 MB are o copie privată
      (`bodies/`, nu istoria), ca bugetul să-l poată lăsa și pe el;
    - ⌘ peste cuvinte: ce a spus discul despre fiecare cale candidată e ținut pe versiunea
      documentului.
  - **Lăsate, cu motiv:**
    - listările serverului la proiectele remote nu se golesc: modulul remote nu știe ce
      dosare arată arborele (alt modul); golite orbește, ar fi relistate de pe server;
    - caret-ul clipind redesenează fereastra (2 cadre/s, cu focus): măsurat 0,0–0,2 % CPU;
      o redesenare doar a lui ar cere o cale nouă în randare; oprirea clipirii ar schimba ce
      se vede;
    - bara de stare și titlurile taburilor refăcute la fiecare cadru întreg, `menu_states`
      la fiecare eveniment: zeci de alocări mici, doar în cadrele care se desenează oricum;
    - un `metadata` la clic în exploratorul de baze de date (local, microsecunde);
    - „monitorul de resurse păstrează trezitorul primei ferestre”: verificat, nu e un bug
      (`app_waker` trezește aplicația, nu o fereastră).

- **A treia iterație (2026-11-13)** — măsurat din nou pe același proiect de probă (118.000
  de fișiere, release, macOS, indexul din cache): amprenta fizică **123 MB** stabilă (151 MB
  pe 12.11), în repaus **0,0 % CPU**, firul principal în așteptarea evenimentelor. Două
  recenzii separate pe ce s-a scris pe 13.11 (remote / SFTP; git, dialoguri, parsare).
  - **Reparate** (fiecare cu test, unde se poate fără server real):
    - erorile de sintaxă: vecinii fiecărui nod vizitat căutați (tree-sitter îi găsește
      parcurgând copiii părintelui de la primul) — pătratic pe firul UI la o greșeală între mii
      de instrucțiuni; acum doar pentru nodurile raportate ale straturilor tăiate în bucăți,
      tăieturile căutate binar;
    - paste-ul din arbore: un singur trezitor pentru toate ferestrele (al ultimei atașate) și
      un proiect închis în timpul paste-ului lăsa paste-ul blocat până la repornire și
      upload-urile ținute; acum fiecare paste are trezitorul și magistrala proiectului său;
    - SFTP: proprietarul apoi modul (chown șterge setuid / setgid); refuzat proprietarul, se
      încearcă doar grupul (un fișier `0640 www-data` rămâne citibil de serverul web);
      `OwnedWrites` nu mai citește folderul pe FTP, ține minte un folder fără id-uri și după
      un refuz nu mai întreabă la fiecare fișier;
    - coada de upload: un upload cerut în timpul unui paste pleacă singur (nu mai ia cu el
      fișierele pe care paste-ul încă le scrie);
    - spinner-ul unui fișier retrimis cât urcă așteaptă al doilea upload; starea butoanelor
      din „To upload” citită o dată pe desen;
    - keychain: dacă reîncercarea eșuează, parola veche e pusă la loc;
    - Git Commit: capetele listei nu mai recitesc diff-ul la fiecare tastă; ancora seriei
      urmează fișierul arătat; testul de commit instabil era ciorna testului de amend (aceeași
      rădăcină falsă);
    - dialogurile: cuvântul lung format o dată și tăiat pe glife (nu un prefix nou pe
      caracter, la fiecare desen).
    - setuid / setgid: după ce „Fișierele aparțin” schimbă proprietarul fișierului abia
      scris, sesiunea SFTP îi pune modul la loc — îl știe din stat-ul de dinainte de scriere,
      deci o cerere în plus doar pentru fișierele cu acești biți;
    - butonul „Upload” din bandă își face loc pentru spinner animat (180 ms, ease-out):
      se lățește, spinner-ul crește și apare; la final se strâng înapoi.
  - **Lăsate, cu motiv:**
    - două intrări pentru apăsări în vederi (`pointer_down` în taburi, `press_with` în
      fereastra de jos), cu implicite diferite: unificarea ar schimba comportamentul
      vederilor din taburi care știu doar `press` (dublu-click); Commit, Log și „To upload”
      le tratează pe amândouă;
    - o eroare mare de sintaxă care conține o tăietură PHP e tăcută întreagă: neconfirmat
      că tree-sitter face un astfel de nod aici; restrângerea la margini ar putea readuce
      erorile false acoperite de test;
    - stat-ul de dinainte de scriere pe SFTP (o călătorie în plus): fără el fișierul
      suprascris ar pierde proprietarul și modul.

- **A patra iterație (2026-11-13, după-amiaza)** — release, macOS, proiectul de probă (118.000
  de fișiere, indexul din cache): fereastra gata în **247 ms**, primul cadru la 413 ms;
  amprenta fizică **130 MB** stabilă; în repaus **0,0 % CPU**. Trei recenzii separate pe ce
  nu fusese recenzat: clientul HTTP (planul 77), proiectul (arborele, indexarea, istoricul
  local, framework-urile), bazele de date și diagramele.
  - **Reparate** (fiecare cu test):
    - **salvările noastre păreau fișiere noi**: o salvare scrie un fișier temporar și îl
      redenumește peste fișier, iar supravegherea auzea „creat, redenumit, șters”. Fiecare
      salvare (și cea automată, la fiecare pauză în scriere) făcea indexul de căi vechi
      (parcurs din nou), muta generația structurii framework-urilor și recitea folderul în
      arbore pe firul UI. Acum fișierul temporar e ignorat, iar fișierul pus în locul lui
      însuși e doar „scris” (verificat cap-coadă, cu supravegherea reală);
    - **framework-urile pe firul UI**: după salvarea unui fișier urmărit, primul `route('`
      sau hover construia modelul pe firul UI (Laravel: toate rutele și config-ul citite)
      sau aștepta lacătul firului care îl construia. Acum firul UI întreabă cu generația
      ultimului model făcut în fundal (îl găsește gata; cel nou vine când e gata), iar
      memoriile sunt `kernel::cache::SharedMemo`, făcute în afara lacătului. React (refăcut la
      orice fișier) și Vue (la orice intrare) urmăresc acum doar fișierele lor;
    - **HTTP**: un tab ascuns cu o cerere care rulează (WebSocket, stream, autentificare
      OAuth) ținea fereastra la 20–25 de cadre/s toată durata ei — acum loader-ul doar cât se
      vede. Orice răspuns peste 100 ms trezea de 10 ori pe secundă toate vederile HTTP (barele,
      editoarele, Services, lanțurile, completarea): acum ce vine are numărătoarea lui, citită
      doar de vederea răspunsului. Tabul Headers nu mai copiază tot body-ul la fiecare cadru.
      Arborele de cereri nu mai copiază toate căile proiectului (doar fișierele `.http`);
    - **bazele de date**: indexul de scheme se reface în afara lacătului pe care completarea
      îl ia la fiecare tastă; cheile străine găsite după nume (nu printre toate tabelele), la
      fel coloanele legate; editările din grilă după hash-ul cheii (3.000 de rânduri șterse:
      pătratic la clic, apoi la fiecare cadru); diagrama de relații: legătura fiecărui rând și
      schemele calculate o dată pe layout, nu pe cadru; fereastra de baze de date: sortarea
      fără alocări la fiecare comparație, tabelele grupate pe scheme într-o trecere;
    - **canvas-ul**: un cadru doar când se schimbă ce e sub mouse; la tragere se refac doar
      muchiile cardului mutat;
    - **indexul de text** (32 MB aici) se scrie doar când s-a schimbat; fișierele temporare
      lăsate de o scriere întreruptă (unul de 21 MB în acest depozit) sunt șterse la pornire;
    - **istoricul local** rămâne în limita de versiuni pe fișier la fiecare versiune păstrată
      (înainte doar la prima a sesiunii: o sesiune lungă creștea fără limită);
    - arborele: sortarea fără alocări pe comparație, căile schimbate deduplicate înainte de
      căutare, listarea remote îmbinată după nume; lista de fișiere a framework-urilor
      (`retain` cu `contains` pe 5.000 de surse) într-o mulțime.
    - pe drum, cerut: **Delete pe Mac (⌫) în arbore** șterge selecția, cu același dialog.
    - apoi, cerut: **istoricul local fără citire pe firul UI** — fișierul cum l-a păstrat
      ultima oară (aceeași mărime și dată) nu mai e citit deloc; altfel, pe Unix, doar
      deschis și citit pe firul lui (salvarea pune un fișier nou în locul lui, cel deschis
      ține versiunea veche), lăsat dacă între timp a fost scris pe loc; pe Windows citit pe loc
      doar când l-a schimbat alt program;
    - **HTTP**: `{{` se uită doar la rândul caret-ului, variabilele și cererile fișierului
      citite o dată pe text; căutarea din arborele de cereri într-o trecere; o singură
      recitire a listei odată; bara `.http` citește fișierele peste 128 KB pe un fir;
      bodies fără fișier (`# @no-log` mici) își lasă JSON-ul peste buget, iar memoria spusă
      numără și JSON-ul;
    - **baze de date**: „Open extractors directory” în fundal; diagrama trezește aplicația cel
      mult la 50 ms cât citește structura; relațiile nedecise aflate o dată cu deciziile;
      canvas-ul: vecinii și metodele într-o trecere (nu fișiere × muchii la fiecare tastă).
  - **Lăsate, cu motiv:**
    - la pornire, indexul de căi (266 + 60 ms) și cel de simboluri (430 + 354 ms) se
      construiesc de două ori, iar framework-urile se pregătesc de două ori: plasa pentru ce
      s-ar fi schimbat între prima citire și pornirea supravegherii. Totul pe fire din fundal;
      evitarea ei ar cere ca fiecare urmăritor să-și înceapă citirea după supraveghere;
    - căutarea de pe ecranul gol (fișierele proiectului, la fiecare tastă) e pe firul UI,
      ținută pe interogare și generație;
    - HTTP: scripturile care copiază body-ul în programul JS (ar cere body-ul dat motorului
      ca valoare, nu în sursă); fișierele `.http` recitite întregi la o afișare a arborelui
      după o schimbare (pe un fir; una odată acum); câteva lucruri mici pe cadru (codul
      rândurilor vizibile din răspuns tokenizat, o rulare clonată pentru părțile arătate);
    - baze de date: `with_notes` care clonează graful la clic (graful e folosit pe loc după
      el; mutat în fundal, s-ar vedea altfel până vine), codul cardurilor canvas tokenizat pe
      cadru, `stat`-uri în vederea de date când vin tabelele (microsecunde pe disc local; un
      disc de rețea ar cere mărimea adusă de worker, o schimbare de protocol);
    - panourile primesc un `ProjectView` nou la fiecare generație de fișiere (arborele își
      golește atunci cache-ul de iconițe).

- **A șasea iterație (2026-11-13, noaptea)** — tot proiectul. Release, macOS, proiectul de
  probă: amprenta **125 MB**, **0,0 % CPU** în repaus. Măsurată și tastarea, cu un test păstrat
  (`cargo test -p app --release --bin lumen measure_typing -- --ignored --nocapture`, un
  workbench întreg, 2400×1500 @2x): un PHP obișnuit (429 de rânduri) — cadru 0,07 ms, o tastă
  0,19 ms, tasta cu cadrul ei 0,56 ms, `$this->` cu cadrul 0,85 ms; unul de 42.000 de rânduri —
  0,08 / 0,36 / 0,98 / 2,5 ms (cel mai rău 5,9 ms). `$x->` fără tip știut, cu 20.000 de fișiere
  în `vendor` (`measure_member_typing`): 9,4 ms prima literă, apoi 4–6 ms. Trei recenzii
  separate: PHP și inteligența codului, nucleul workbench-ului, kernel și componentele.
  - **Reparate** (fiecare cu test):
    - alocatorul care numără scria la fiecare alocare un indicator comun tuturor firelor
      (măsurat: 8 fire, 1,1 s în loc de 44 ms): acum scris o singură dată;
    - câmpurile de text găseau locul fiecărui caracter căutând de la primul glif (pătratic:
      39 ms pe cadru la 20.000 de caractere): acum într-o trecere;
    - coada: cheile forțate într-o mulțime (5.000 de upload-uri cerute deodată), privirile
      care nu schimbă nimic nu-i mai trezesc firul; `in_generated` fără șiruri făcute pe cale;
    - PHP: apelul în care se scriu argumente nu mai face un hover întreg (poate cu un fișier
      citit de pe disc) la fiecare tastă — ținut cât textul până la nume și indexul (care are
      acum o versiune) rămân aceleași; regex-ul importurilor făcut o dată;
    - bara de stare și tragerea unei selecții cer cadre doar când se schimbă ceva; căile
      actualizate ale indexului deduplicate într-o mulțime.
  - **Apoi, cerut (măsurat pe o copie a lui api.acme.test: Symfony, 17.700 de fișiere de
    bibliotecă, 22.800 PHP în total; fiecare tastă, pe firul UI):**
    - `$x->` fără tip: **100–145 → 10–25 ms**. Numele distincte ale membrilor bibliotecilor
      ținute o dată (refăcute doar când se schimbă o bibliotecă), notate, și doar fișierele
      celor mai bune dezarhivate (înainte: aproape toate, la fiecare tastă);
    - un nume (tipuri, funcții; început de instrucțiune, după `new`): **15–60 → 5–25 ms**. Fiecare
      potrivire devenea sugestie (șiruri copiate), calea ei verificată (dependență?) și toată
      lista sortată pentru 80; acum notate întâi, făcute doar cele mai bune, tipurile și
      funcțiile bibliotecilor (cu „dependență”) ținute cu indexul; `in_dependencies` pe octeți.
      Generic: motorul și indexul sunt ale tuturor limbajelor (`node_modules` ca `vendor`);
    - tab-urile ascunse care primesc date nu mai redesenează fereastra (doar dacă se schimbă
      semnul „nesalvat” din bara de tab-uri);
    - mouse-ul peste o vedere de tab: un cadru doar când se schimbă ce arată, pentru vederile
      care o spun (`Viewer::hover_tells_changes`, verificate una câte una: imaginea, binarul,
      datele și previzualizarea tabelelor, diagrama relațiilor, diagramele, canvas-ul);
    - cache-ul formării textului crește (până la 4×) cât se desenează mai multe rânduri decât
      încap, și scade înapoi;
    - restrângerea: cât cuvântul doar crește (`ge` → `get`), bibliotecile sunt notate doar
      printre ce a potrivit tasta dinainte (potrivirea fuzzy e pe subsecvență: ce potrivește
      `get` a potrivit și `ge`); ținut pe firul care completează, legat de versiunea
      bibliotecilor și de limbaj; un test arată că literă cu literă iese exact ce iese dintr-o
      dată. Măsurat (mediane pe 3–4 rulări, înainte → după, ms): început de instrucțiune
      s/t/r 14,8/14,6/14,7 → 19,8/15,2/10,3; `$x->` g/e/t 23,8/10,4/10,6 → 24,8/9,7/9,9;
      `new ` R/e/q 19,8/7,9/4,2 → 23,3/9,0/5,3 (prima tastă și `new` în zgomot: ±5 ms între
      rulări). Câștig doar de la a treia tastă: restul timpului nu e în notarea bibliotecilor.
  - **Lăsate, cu motiv:**
    - completarea: ~10–20 ms pe tastă rămân și după restrângere, deci nu în notarea
      bibliotecilor: de profilat (fișierele proiectului notate toate la fiecare tastă,
      sugestiile făcute, contextul, cadrul);
    - celelalte vederi de tab (Markdown, răspunsul HTTP…) răspund încă „clicabil” la hover: un
      cadru la fiecare mișcare peste ele, până sunt trecute la „s-a schimbat”;
    - `by_name` dezarhivează și ține fișierele de bibliotecă cu un membru de acel nume,
      rechecking-ul tuturor tab-urilor PHP la fiecare salvare, hint-urile care copiază indexul,
      citirile blocante ale indexului pe firul UI (completarea, breadcrumbs);
    - fiecare eveniment marchează cadrul următor „murdar”; un fișier deschis, schimbat pe disc,
      comparat pe firul UI (un log mare); pickerul formând toate rândurile la deschidere; fire
      fără un buget comun; `glob` lent pe modele patologice din `.gitignore`.

- **A cincea iterație (2026-11-13, seara)** — release, macOS, proiectul de probă: fereastra
  gata în **232 ms**, primul cadru la 382 ms, amprenta **130 MB** stabilă, **0,0 % CPU** în
  repaus. Trei recenzii separate pe ce nu fusese văzut deloc: limbajele și editarea, uneltele
  și aspectul, ecranele aplicației și vizualizatoarele.
  - **Reparate** (fiecare cu test):
    - **diff-ul** (Myers) ținea o copie a tabloului la fiecare pas: două fișiere de 10.000 de
      rânduri foarte schimbate luau **2,08 GB** (măsurat). Acum în spațiu liniar (middle snake),
      tot cel mai scurt (verificat față de cea mai lungă subsecvență comună pe 400 de perechi),
      0,14 s; Compare refăcea diff-ul pe firul UI la salvare (acum pe firul lui) și lumina
      diferența din rândurile cu tab-uri decalat;
    - **Find in Files** căuta în tot indexul de text (și vectorii semantici) pe firul UI la
      fiecare tastă, iar înainte de indexul de căi parcurgea proiectul: acum pe un fir, cea mai
      nouă câștigă; modelul semantic nu mai e verificat pe disc la fiecare tastă și o
      descărcare eșuată nu mai e reîncercată la fiecare tastă;
    - **setările** se scriau cu sincronizare pe disc pe firul UI la fiecare alegere (5,2 ms
      măsurat), la fiecare pas al unui slider tras: acum setările utilizatorului se scriu în
      spate, pe firul lor (cea mai nouă câștigă), așteptate la ieșire; `project.toml` (scris și
      de nivele, de framework-uri) rămâne scris pe loc;
    - **Git**: o copie a referințelor unui depozit pentru toate vederile (`git::refs_store`),
      recitită după push / fetch / pull, o mutare a `HEAD`, o ramură / un tag făcut: după push,
      Log și Commit arată la fel, singure (înainte „↗1” / „1↑” rămâneau); meniul de clic-dreapta
      al ramurilor e unul (`git::ref_menu`), și pe locurile din Commit;
    - CSS: simbolurile unei foi de stil fără căutări pătratice; JavaScript: definiția sub mouse
      nu mai reparsează fișierul la fiecare cuvânt; „deprecated” găsite într-o mulțime;
    - Rename: conflictele numelui nou pe un fir (citeau indexul blocând și fișiere întregi);
      vederile de refactorizare nu mai cer cadre cât caută; Emmet refuză expansiunile uriașe
      (`div*1000>div*1000>div*1000`, `lorem999999999`);
    - Markdown: imaginile diagramelor nu mai reconstruiesc tot documentul, cuvintele numărate o
      dată pe versiune; proiectele recente descrise în fundal (citeau fișierele fiecăruia pe
      firul UI); căutarea din setări nu mai reface intrările la fiecare cadru;
    - ecranul estompat de „Ține treaz” redesenat la 5 s, pe pixeli întregi (nu la 0,5 s toată
      noaptea).
  - **Lăsate, cu motiv:**
    - Code quality caută programele în `PATH` pe firul UI (la comandă și în pagina de setări:
      sub o milisecundă pe macOS; de mutat pe fir cu pagina ei);
    - `.env` „chei nefolosite” parcurge proiectul la fiecare salvare (în fundal, oprit implicit);
    - completarea: contextul calculat de 2–3 ori pe tastă, clasele CSS căutate printre toate
      simbolurile proiectului, Change Signature refăcut pe firul UI la fiecare tastă,
      Move Statement reparsând fișierul;
    - Compare: rândurile copiază textul la fiecare alegere, celulele lucrează pe tot rândul,
      banda din dreapta un dreptunghi pe diferență pe cadru (doar la comparații uriașe);
    - Markdown: cinci treceri peste elemente pe cadru (documente de zeci de mii de rânduri);
      imaginile: o decodare depășită nu e oprită;
    - decorațiunile rare (fulgere, CRT) cer cadre între evenimente; Task Manager-ul
      reîmprospătează toate procesele sistemului la fiecare secundă cât e deschis; Theme Studio
      la 20 de cadre/s cât se mișcă previzualizarea; Relax la 60 de cadre/s nesupravegheat
      (toate pornite de utilizator, oprite la inactivitate sau închidere).

- **A doua iterație de recenzii (2026-11-12)** — randarea pe GPU și textul, nucleul
  editorului, terminalul, bazele de date, serverele de limbaj. Măsurat: un PHP de 43.000 de
  rânduri (1,4 MB) și un log de 38 MB deschise: amprenta **155 MB**, în repaus 0,0–0,8 % CPU.
  - **Reparate** (fiecare cu test):
    - **rândurile care amestecă litere latine și CJK** în fontul de cod: pe macOS fontul
      `GB18030Bitmap` se dă drept monospațiat cu spațiul de lățime 0, iar cosmic-text dădea
      glifelor lui de rezervă o mărime infinită (panică în debug, glife pierdute în
      release). O față care face asta iese din fonturi, o dată, și rândul e format din nou
      (celelalte fețe își păstrează id-urile);
    - Windows: redimensionarea făcea la fiecare pas o textură cât fereastra (cadrul păstrat);
      acum, 300 ms după ultimul pas, cadrele merg direct în suprafață, ca pe macOS;
    - bold / italic în editor: fiecare rând cu o față parcurgea toate glifele de până la el
      (pătratic pe ecran); acum doar pe ale lui;
    - mărimile de font prin care a trecut un zoom își lasă rândurile formate după ~1M de
      căutări fără ele;
    - un caret pe rândul lung al unui fișier minificat forma tot rândul de două ori pe cadru:
      acum până la un pas de 1.024 de coloane după el;
    - căutarea din fișier cu bara deschisă căuta în tot documentul la fiecare tastă: acum doar
      pe rândurile atinse, celelalte potriviri mutate cu textul (verificat față de căutarea
      întreagă pe sute de editări la întâmplare);
    - „Collapse All” pe mii de regiuni: fiecare parsare compara fiecare ancoră cu fiecare
      regiune; acum o mulțime;
    - Output-ul consolei: o instrucțiune înfășurată era tokenizată o dată pe rând vizibil, la
      fiecare cadru (un `INSERT` de 1 MB: zeci de MB pe cadru); acum o dată, ținută pentru
      linia ei. Și o limită de 32 MB pe log, pe lângă cea de linii (un dump trimis bucată cu
      bucată);
    - grila: mulțimea tuturor rândurilor selectate făcută la fiecare cadru (⌘A pe o pagină
      „toate”), acum o întrebare pe rândul vizibil; textul unei celule copiat întreg (până la
      64 KB) ca să fie tăiat la 240 de caractere, acum doar partea desenată;
    - istoricul consolei: cheia fiecărei intrări refăcută (alocând) la fiecare rulare: acum
      comparată cuvânt cu cuvânt;
    - terminalul: o tastă sau o lipire copia tot ecranul și aștepta cititorul (până la 40 ms
      cu un `ESC[2J` ținut) doar ca să afle două moduri; acum le citește fără să aștepte (cele
      ale ultimei copii, dacă e ocupat);
    - jurnalul serverelor de limbaj copia toate cele 2.000 de linii la fiecare desen: acum doar
      pe cele din fereastră.
  - **Făcute apoi (aceeași zi, „toate cele găsite”):**
    - **bulina limbii tastaturii („US”) pe macOS** apărea la fiecare mișcare a caret-ului
      (scris, clic, un câmp care ia tastatura, Relax peste editor): fiecare loc spus
      sistemului o arăta din nou. Acum locul e ținut și spus doar într-o compoziție (taste
      moarte, CJK: candidații urmează caret-ul) și când se schimbă limba tastaturii
      (`NSTextInputContextKeyboardSelectionDidChangeNotification`): o dată, la caret
      (`platform-winit::ime_caret`). Windows / Linux: ca înainte. **De verificat pe ecran.**
    - **completarea PHP pe fișiere mari:** extragerea simbolurilor era pătratică (`visibility`
      căuta la părintele fiecărei clase — tot fișierul — iar `children()` cerea fiecare copil
      cu `child(i)`, O(i) în tree-sitter): 39.000 de rânduri, release: **368 → 30 ms** pe
      tastă. Toate buclele `0..child_count()` din PHP și JS merg acum cu un cursor
      (`syntax_ts::children`, `push_children`), aceeași ordine;
    - evidențierea pe rânduri foarte lungi (un fișier minificat): doar partea desenată a
      fiecărui rând lung (`EditorView::long_line_windows`, `Highlighter::highlight_ranges`),
      aceleași culori octet cu octet;
    - căutarea din bara Find, la un document peste 1 MB: căutarea întreagă a unei interogări
      noi pe un fir (`find_search`), selecția venind când e gata; navigarea și înlocuirea
      nu o așteaptă;
    - cel mai lung rând: după o editare se măsoară doar rândurile schimbate (verificat față de
      numărarea întreagă pe sute de editări la întâmplare); întreg doar când cel mai lung a
      fost scurtat;
    - cursoare multiple: selecțiile mutate cu căutări binare (`Transaction::offsets`), pliurile
      și zonele parsării cu `ChangeMap` (peste 16 schimbări), zonele schimbate ale parserului
      pe rulări, regiunile de pliere căutate într-o mulțime — nimic pătratic; verificate față
      de mutarea schimbare cu schimbare;
    - Select Next / All Occurrences caută pe bucățile textului (nu-l mai copiază întreg);
    - jurnalul schimbărilor unui document: și o limită de mărime (1M schimbări), nu doar 256
      de versiuni;
    - atlasul de glife: multe glife noi (≥ 16) încărcate dintr-un singur buffer, copiate de
      encoderul cadrului (pixel cu pixel ca una câte una, verificat pe GPU); crescut, se face
      mai mic când fereastra pierde focusul, dacă ce ține încape;
    - texturile imaginilor: fiecare fereastră își știe imaginile de pe ecran (nu mai sunt
      lăsate cât sunt văzute, de nicio fereastră); cele de pe niciun ecran sunt lăsate după
      20 s și fără cadre (aplicația în fundal);
    - suprafața: două imagini în repaus pe toate platformele, a treia doar cât se
      redimensionează (Windows: o imagine cât fereastra mai puțin, 15–33 MB).
      **De verificat pe Windows: derularea fluidă, redimensionarea.**
    - desenele SVG ale iconițelor lăsate când fereastra pierde focusul (citite iar la nevoie);
    - terminalul: rotița nu mai așteaptă cititorul (liniile la cadrul următor);
    - Output-ul consolei: ora și fontul prefixului făcute o dată pe cadru, nu pe rând;
    - exploratorul de baze de date: mărimea fișierului e citită când vin tabelele, nu la clic.
  - **Lăsate, cu motiv (verificat):**
    - terminalul la rafale: glife așezate pe celule pentru ASCII ar pierde ligaturile
      fonturilor de cod (`->`, `=>`) — s-ar vedea altfel;
    - listările serverului la proiectele remote: `known()` e cerut de arbore la fiecare
      reîmprospătare; o listare golită ar arăta câteva clipe un dosar deschis fără intrările
      de pe server;
    - bara de stare / `menu_states`: câteva zeci de șiruri scurte doar în cadrele desenate;
      un cache ar cere cheia tuturor intrărilor (riscul unei stări vechi) pentru microsecunde.

- **A treia profilare (2026-11-12, seara)** — release, macOS, același proiect de probă
  (23.200 de fișiere PHP) și un PHP de 50.000 de rânduri.
  - **Indexul de simboluri de la zero: 7,5 → 3,8 s.** Era pe un singur fir; acum fișierele
    sunt citite și parsate în loturi de 256 pe cel mult 4 fire (jumătate din nuclee), partea
    comună (textele împărțite ale indexului) pe firul lui, în ordine; CPU-ul ajutoarelor
    numărat cu al lui. Amprenta după indexare: 138 MB (151 înainte).
  - **Analizele unui document după o editare** (pe firul lor), fișierul de 50.000 de rânduri:
    `analyse` **566 → 276 ms** — parsarea lui de la zero (180 ms) înlocuită cu arborele
    incremental deja ținut; „nefolosite” fără o copie a fiecărui nume, cu clasa și
    instrucțiunea știute din parcurgere (nu prin `parent()`, care coboară de la rădăcină),
    o parcurgere a fișierului mai puțin, un `include` căutat întâi în text.
  - **Lăsate, cu motiv:**
    - `check` (inferența) ~340 ms pe acel fișier: aproape tot e `parent()` urcând din fiecare
      variabilă (tree-sitter coboară de la rădăcină, scump doar la mii de declarații pe
      primul nivel — un fișier generat; unul obișnuit are 5–20) și căutarea legării în
      corpul funcției; de schimbat doar cu o hartă a părinților pentru 21 de locuri;
    - pornirea GPU-ului (280–600 ms, în paralel cu restul pornirii): driverul Metal;
    - după prima construire a indexului, o a doua (toate din cache, ~340 ms) cerută de
      privitorul de fișiere când pornește: plasa pentru ce s-ar fi schimbat între timp.

- **A patra profilare (2026-11-12): cadrele și tastarea pe firul UI** — un `Workbench`
  întreg în release (2400×1500 @2x), măsurat pas cu pas:
  - un fișier PHP obișnuit (551 de rânduri, o clasă de 60 de metode): cadru fără schimbări
    0,05 ms, la derulare 0,1 ms, o tastă 0,18 ms (cel mult 0,7), cadrul după ea 0,36 ms;
  - fișierul de 43.000 de rânduri: cadrele la fel; o tastă într-o variabilă ~31 ms, `(` după
    o funcție 133 ms — de fiecare dată simbolurile fișierului extrase din nou pentru textul
    nou (completarea, apoi ajutorul parametrilor pe textul „reparat”), ~4,5 µs pe simbol.
  - **Reparat:** comentariul de documentație al fiecărei declarații notat de parcurgere
    (`prev_named_sibling` număra de la primul frate: la mii de declarații pe primul nivel,
    20.000 de funcții: 72 → 60 ms).
  - **Rămâne de ales:** pe fișierele uriașe, simbolurile textului de dinainte refolosite când
    editarea e în corpul unei funcții (fără acolade, ghilimele, comentarii) — ar trebui
    mutate pozițiile de după editare și ar putea arăta un fișier stricat de o ghilimea
    deschisă altfel decât acum; sau completarea calculată pe un fir.

- **Memoria fără proiect (2026-11-12, macOS, release, fereastra 1280×800 @2x)** — măsurată
  cu `footprint`, `vmmap` și `malloc_history` (fiecare alocare cu stiva ei):
  - **75 → 52 MB în repaus.** Cursorul care clipea (câmpul de pe ecranul de start) desena un
    cadru la 0,5 s, iar stratul ferestrei ținea astfel două imagini (2 × 15,6 MB). Acum,
    după 15 s fără nimic (ales de utilizator), cursorul se oprește aprins; nu mai e niciun
    cadru, imaginea de rezervă se dă înapoi; o tastă, un clic, mouse-ul îl trezesc
    (`ui_kit::caret::REST`, `wake`).
  - **Traducerile: heap 29,4 → 26,2 MB.** Ambele cataloage erau parsate întregi la pornire
    (33.000 de alocări); acum un mesaj e citit la prima cerere, textul împrumutat din
    binar.
  - **Lăsate, cu motiv:** ColorSync ~11 MB (tabelele de conversie ale culorilor straturilor
    AppKit: fundalul translucid al barei de titlu, cadrul ferestrei — ale sistemului;
    doar renunțând la bara unificată); driverul GPU ~13 MB (bufferele de comenzi Metal);
    stivele firelor sunt rezervate, nu atinse (0,4 MB în amprentă).

- **Mai multe proiecte deschise (2026-11-12, macOS, release)** — 1, 2, 3 ferestre (proiectul
  de 23.200 de fișiere PHP și altele mici), cu `footprint`, `heap`, `sample`:
  - o fereastră în plus: heap +1–2,4 MB, ~8 fire (stive aproape neatinse), CPU în repaus 0
    (10 s eșantionate: niciun fir treaz); imaginea ei cât fereastra cât timp se vede (macOS o
    compune din ea; acoperită de tot: 1×1);
  - indexul de simboluri al proiectului mare: ~34 MB pentru 254.000 de simboluri (≈1,3 KB pe
    fișier: fișierele proiectului 7,2 MB, cele 20.000 de biblioteci împachetate 16 MB,
    tabelele 5,5 MB) — nimic risipit; după 4 minute heap-ul viu e 90 MB, aproape tot real;
  - un proiect lăsat în fundal își oprește serverele de limbaj (phpactor: 30–46 MB fiecare)
    și își odihnește indexurile — mecanismele existente;
  - **reparat pe drum:** o cerere făcută cu „+” în rădăcina proiectului nu apărea în
    arborele HTTP (lista proiectului încă veche).

- **Consumul de memorie a crescut și nu pare să scadă** (observat de utilizator,
  2026-11-08). **Remăsurat 2026-11-12** (heap-ul, cu alocatorul care numără, release):
  - pornirea: fonturile 4,4 MB, temele 0,4 MB, un proiect deschis 2,1 MB; în total 7,4 MB,
    neschimbați după 10 s de lucru în fundal;
  - zece runde cu două proiecte deschise pe rând, un fișier deschis și închis în fiecare:
    6,4 MB la fiecare rundă, adică **nimic nu rămâne în urmă**.

  Rămâne de văzut ce măsoară heap-ul nu: memoria GPU (texturile, capturile ferestrelor) și
  RSS-ul întreg, pe o sesiune reală lungă. De măsurat cu Task Manager-ul aplicației (RSS,
  GPU pe fereastră) la pornire, după o oră de lucru și după închiderea proiectelor. A apărut după schimbările din 2026-11-06 / 08: planurile 65, 66 și 67, plus
  68, scos între timp. **Trecut prin cod (2026-11-08):**
  - **Capturile ferestrei (67c): reparate**, ele erau suspectul principal:
    - o singură captură odată pe fereastră (wgpu și software);
    - micșorarea se face direct din buffer-ul mapat. Dispare copia întreagă a cadrului,
      adică o alocare de zeci de MB la fiecare captură;
    - la pierderea focusului, cel mult o captură la 10 s pe fereastră. Cea cerută de
      comutator se face mereu.
    Rămâne buffer-ul de citire cât fereastra, făcut pe moment și eliberat după.
  - **Diagnosticele framework-urilor: reparate.** Se trimit doar când se schimbă. Înainte, la
    fiecare tastă într-un proiect cu framework, o listă neschimbată refăcea marcajele
    editorului. Ele se înlocuiau oricum: nu se adunau.
  - **Verificate, fără creștere:**
    - pozele păstrate: o poză nouă o înlocuiește pe cea veche; imaginile din comutator sunt
      eliberate la înlocuire și la închiderea listei; poza unui proiect închis pleacă cu el;
    - modelele Symfony: un `Memo` ține o singură valoare, pe cea veche o aruncă înainte s-o
      calculeze pe cea nouă. Risipă de CPU, nu de memorie: o salvare în `src/` recitește și
      containerul compilat;
    - firele: pool-urile se închid după 30 s fără lucru. Excepție, cu intenție: „Ține
      treaz” în modul automat, care verifică la 5 s.
  - **De măsurat**, ca să se vadă dacă a fost destul sau dacă e altceva:
    - memoria procesului în Task Manager / bara de jos, la pornire, după 10 minute de lucru,
      după 20 de comutări între ferestre, după deschiderea și închiderea unui proiect
      Symfony;
    - comparat cu build-ul de la `d9ce197f` (dinainte de 66a, primul pas din seria asta);
    - pe macOS și `footprint` / Instruments (Allocations) pe procesul aplicației, ca să se
      vadă cine ține memoria, nu doar cât.
  - **Chiar și pe un proiect gol ocupă mult** (spus de utilizator, 2026-11-08): un proiect cu
    un singur `index.php` ia **160–200 MB la pornire**. Deci nu e (doar) ce adună lucrul în
    timp: costul de bază al pornirii e mare. De despărțit în ce se încarcă la start
    indiferent de proiect: fonturile și atlasul de glife, iconițele, gramaticile
    tree-sitter, stub-urile PHP (`php.stubs`) și datele CSS / HTML / JS, tema, modulele
    activate (fiecare cu ce face la `activate`), firele pornite. Fiecare modul trebuie să se
    încarce leneș și să nu țină nimic până nu e folosit: de verificat cine nu respectă asta.
    Măsurat pe pornire goală (fără proiect), apoi cu `index.php`, apoi după prima deschidere
    a fișierului, cu contorul de heap pe fir.
  - **Măsurat (2026-11-08): heap-ul pornirii e mic, ~7 MB.** Unealta:
    `cargo test -p app startup_memory -- --ignored --nocapture` (alocatorul de test numără
    octeții vii, `kernel::memory::Counting`). Pe macOS, debug:
    - fonturile 4,2 MB, limba și tastele 0, modulele 0,2 MB, temele 0,6 MB, workbench-ul
      0,1 MB, proiectul cu `index.php` deschis 2,1 MB, fișierul deschis 0,1 MB, 10 s de
      treziri (indexare, sintaxă, diagnostice) 0;
    - **deci cei 160–200 MB nu sunt date ale aplicației.** Din ce se compun (estimat):
      - **GPU-ul ferestrei**, pe Apple Silicon socotit ca memorie a aplicației: o fereastră
        Retina mare (3024×1964) are ~24 MB pe imagine. Sunt 2 imagini ale suprafeței
        (latența 1 pe macOS), plus cadrul păstrat (încă una), plus blur-urile și atlasul
        (~5 MB). **~75–80 MB, cu mărimea ferestrei**: punctul „Memoria video crește cu
        mărimea ferestrei” e cel mai mare consumator;
      - **codul**: binarul release are 76 MB (debug 213 MB). Paginile lui atinse stau în
        memorie;
      - **framework-urile sistemului** încărcate (AppKit, Metal, CoreText): zeci de MB
        partajați, numărați în RSS.
    - **Cifra arătată e RSS** (`sysinfo` `process.memory()` în `resources.rs`). Ea numără și
      paginile partajate ale sistemului și ale binarului. Activity Monitor arată
      `phys_footprint` (mai mic, mai cinstit). De făcut:
      - pe macOS arătat `phys_footprint`, pe Windows „private bytes”;
      - în Task Manager despărțit: heap-ul viu (contorul), GPU-ul (estimarea
        `gpu_memory_bytes`), restul (cod, sistem).
    - **Făcut (2026-11-08), cei 4 pași:**
      1. **Cifra corectă:** bara de jos și Task Manager-ul arată amprenta procesului
         (`process_stats::footprint`: `phys_footprint` pe macOS, ca Activity Monitor; private
         bytes pe Windows; paginile anonime pe Linux), nu RSS. Contorul de heap
         (`kernel::memory::Counting`, câte un contor pe fir, să nu se calce firele) e pus în
         aplicație, pe Windows în jurul lui mimalloc. Task Manager-ul împarte ce nu raportează
         nimeni în „Cod, GPU și sistem” (amprenta minus heap-ul, cu GPU-ul estimat) și
         „Memorie neatribuită” (heap-ul pe care nu-l raportează nicio parte);
      2. **Cadrul păstrat:** o fereastră care pierde focusul dă înapoi cadrul păstrat și
         blur-urile (`Renderer::release_idle`, după captura ei), ~24 MB pe o fereastră Retina
         mare; primul cadru de după se desenează întreg;
      3. **Binarul:** codul are 37,6 MB, datele constante 23,8 MB (tabelele gramaticilor;
         fișierele incluse ~4 MB: stub-uri, MDN, splash, fontul STIX). Sunt pagini curate,
         citite din fișierul binarului: amprenta nu le numără, deci nu le mai vedem în
         cifră. `opt-level = "s"` ar încetini parsarea și indexarea fără câștig de
         amprentă, așa că profilul rămâne cum e;
      4. **Creșterea în timp, fără desenare:**
         `cargo test -p app memory_over_rounds -- --ignored --nocapture` deschide pe rând două
         proiecte, cu un fișier deschis și închis în fiecare, 10 runde: heap-ul stă la
         6,1 MB, deci proiectele și taburile nu lasă nimic în urmă. Pe drumul desenării, cache-ul
         de linii are limită (LRU), imaginile glifelor nu stau pe CPU, iar atlasul de pe GPU
         are limită (4096²) și se golește la purjare.
    - **Măsurat în aplicația pornită (2026-11-08, release, `footprint`, macOS):**
      - proiect cu `index.php`, profil curat, fereastra implicită, după 40 s: `d9ce197f`
        93 și 86 MB, versiunea curentă 94 și 87 MB. **Fără regresie la pornire** între ele.
        Din ei: IOSurface 31, grafică ~26, heap ~21 MB. Stă stabil la mers în gol (2 min);
      - o copie a acestui repo (2607 fișiere): 120 MB după 90 s, vârf 168 MB, heap 50 MB;
      - aplicația de lucru a utilizatorului, la 8 minute de la pornire: **339 MB, vârf
        463 MB**, heap 149 MB, IOSurface 109 MB, grafică ~59 MB. Serverele de limbaj sunt
        separat. De aflat ce are în plus: heap-ul de ~3× față de repo-ul copiat (ce proiect,
        câte taburi, ce module) și IOSurface de 3,5× față de fereastra implicită (fereastră
        mare sau mai multe ferestre: neconfirmat). Vârful arată o alocare trecătoare mare
        (probabil indexarea: de confirmat).
    - **Profilat 2026-11-09** (`footprint`, `vmmap`, `malloc_history`, copie a unui proiect mic):
      fereastra mare a utilizatorului, 201 MB: ~78 MB cele două imagini ale suprafeței, ~40 MB
      cadrul păstrat, heap ~33 MB. Heap-ul e mic și împrăștiat (21 MB, ~9 MB ai sistemului); un
      proiect în plus adaugă ~3 MB de heap. **Făcut:** o fereastră care stă (2 s fără cadre
      apropiate; clipirea cursorului nu contează) dă înapoi cadrul păstrat și imaginea de
      rezervă a suprafeței; desenează întreg, direct. Fereastra implicită: 94 → 56 MB;
      două proiecte: 76 MB. Nevăzut încă pe ecran de mine (fără drept de captură): de probat.
    - **Mai multe proiecte (2026-11-09):** pipeline-urile GPU comune pe format; serverele unui
      proiect aflat în spatele ferestrei altuia se opresc după timpul de inactivitate și
      repornesc când vine în față (altă aplicație în față nu contează). **Nefăcut:** cursorul
      într-un strat separat (~1 imagine a ferestrei, doar la fereastra activă cu fișier):
      atinge desenarea cursorului și ce îl acoperă, cere verificare pe ecran.
    - **Măsurat 2026-11-09, sesiunea reală cu 3 proiecte, 667 MB după 34 min:** heap 415 MB în
      ~3 milioane de obiecte mici (medie 142 B), IOSurface 109 MB. O copie a proiectului cel mai
      mare (14.952 fișiere PHP), după 75 s: heap 85 MB (indexul simbolurilor + al căutării),
      deci ~6 KB pe fișier. **Atribuit:** cele 3 proiecte au ~70.000 de fișiere PHP (vendor),
      429 MB de heap la 77 s de la pornire: indexul lor. **Făcut:** un proiect aflat 2 minute
      în spatele altuia își lasă indexul pe disc și îl reîncarcă la revenire (două copii:
      242 → 167 MB). Rămâne de redus indexul proiectului activ.
    - **Indexul proiectului activ, redus (2026-11-11, planurile 73–74, branch-ul
      `index-members-on-demand`):** fișierele bibliotecilor (`vendor`, `node_modules`,
      `vendor-dir` din `composer.json`…) își țin membrii claselor comprimați în RAM, decodați
      o dată, la prima cerere (mediana 6,5 µs pe fișier, cel mai rău 0,45 ms); tabelele au o
      intrare pe clasă, nu pe membru. Pe copia Laravel (8.251 de fișiere), heap-ul proiectului
      indexat **33,7 → 23,6 MB (−30%)**, cache-ul de pe disc 11,6 → 4,2 MB. Completarea,
      hover-ul, definițiile, verificările și rename-ul dau aceleași rezultate (test). De
      măsurat la utilizator, în sesiunea reală, după unirea branch-ului.
    - **Rămâne de măsurat în aplicație** (cu cifra nouă și rândurile din Task Manager):
      - dacă amprenta tot crește în timp, rândul care crește spune unde e: heap-ul
        neatribuit (date) sau „Cod, GPU și sistem” (texturi, atlas, imagini). Texturile
        imaginilor nefolosite sunt eliberate (`state.textures.retain`, trecut prin cod);
      - serverele de limbaj sunt procese separate: Task Manager-ul le arată pe rândurile lor,
        nu în amprenta aplicației.
  - **Încă nu a scăzut** (spus de utilizator, 2026-11-08). Ce s-a cerut atunci, trecut prin
    cod și commituri (2026-11-11):
    - ~~**contorul de heap**, arătat în Task Manager lângă amprentă~~ — **făcut** 2026-11-08
      (`c8d270d9`): `kernel::memory::Counting` e alocatorul aplicației (pe fir, însumat la
      cerere), `ResourceSnapshot::heap`, rândurile „Cod, GPU și sistem” și „Memorie
      neatribuită”;
    - ~~**cine o ține** (fiecare modul își raportează memoria)~~ — **făcut**: `ResourceReporter`
      la modulele care țin date (~20), diferența arătată ca „Memorie neatribuită”;
    - ~~**un contor de alocări pentru diagnostic**~~ — **făcut** ca unelte de test, nu în
      aplicație: `startup_memory`, `memory_over_rounds`, `project_index_memory` numără octeții
      vii pas cu pas (alocatorul de test). Pe etichetă nu: n-a mai fost nevoie, măsurătorile
      au găsit vinovatul;
    - ~~**căutat pe commituri** (bisect) față de `d9ce197f`~~ — **nu mai e nevoie**: pornirea
      măsurată pe amândouă (2026-11-08) e la fel (93/86 față de 94/87 MB), iar heap-ul mare al
      sesiunii reale e atribuit (2026-11-09): **indexul celor ~70.000 de fișiere PHP**;
    - ~~**suspecții din 2026-11-08**~~ — trecuți prin cod, niciunul nu crește (mai sus);
    - ~~**alocatorul**~~ — încercat: pe Windows mimalloc față de cel al sistemului (măsurat, în
      `app/src/main.rs`), pe macOS al sistemului, cu paginile libere date înapoi după ce se
      eliberează ceva mare (`kernel::memory::release_free`);
    - **„Clear and Rebuild Caches” ca test** — **rămas**, la utilizator: o cifră înainte și
      după, în sesiunea reală (golește și reconstruiește indexul, deci arată cât e al lui);
    - **ce rămâne de făcut**, după unirea branch-ului `index-members-on-demand`: măsurătoarea
      în sesiunea reală (amprenta, heap-ul, „Memorie neatribuită”, la pornire și după 30 de
      minute), comparată cu cei 667 MB / heap 415 MB din 2026-11-09.

- ~~**Nodurile șterse din arborele proiectului rămân în arenă.**~~ — **făcut** 2026-11-10:
  `NodeId` cu generație, locurile celor dispăruți (cu tot ce e sub ei) eliberate și luate de
  cele noi; un id vechi nu mai e al arborelui (`contains`), iar calea lui e goală. Refolosirea lor cere ca
  selecția, intrarea nouă și restul să nu mai țină `NodeId` vechi, de exemplu cu un
  `NodeId` cu generație.
  - Contează doar după multe mii de ștergeri. (review 29)

- ~~**Clona indexului ținută de canvas cât construiește graful.**~~ — **făcut** 2026-11-10:
  tabelele indexului în 64 de bucăți (`Sharded`), o schimbare copiază doar bucata ei. Măsurat
  pe 5000 de fișiere: `set_file` cu o copie ținută 1,33 ms → 0,70 ms (cel mai rău 2,72 →
  1,25 ms), fără copie 66 → 74 µs. O scriere în index în timpul
  ăsta copiază toate tabelele (`Arc::make_mut`), uneori pe firul UI.
  - Apare rar: doar dacă scrii exact cât se construiește graful.
  - Soluția: tabele pe bucăți (copiate doar cele atinse) sau o structură persistentă.
    (review 32)

- ~~**Memoria video crește cu mărimea ferestrei**~~ — **făcut pe macOS** (verificat
  2026-11-10 în `render-wgpu`): un singur cadru în așteptare (al doilea doar cât se
  redimensionează); copia pentru redesenarea parțială și imaginea de rezervă se dau înapoi
  după ce fereastra stă (`kept_frame_release_at`), la ascundere și la minimizare; blur-ul la
  rezoluție redusă, eliberat când nu mai e folosit. O imagine cât fereastra la 2×: 13 MB
  (1200×700), 33 MB (1920×1080), 51 MB (2560×1440); în repaus rămâne cam una.
  - **Rămas pe Windows:** 3 cadre gata mereu, nimic dat înapoi în repaus. Ar cere refăcut
    swapchain-ul când fereastra se liniștește; de încercat doar pe un Windows real, cu
    măsurătoarea din Task Manager înainte / după și fără vreo sacadare la primul cadru.

## Insigna cu numărul de pe butoane (cerut 2026-11-12) — **făcut**

- **Făcut (2026-11-12):**
  - pe un buton aprins, conturul insignei are culoarea butonului (nu a ferestrei), iar
    insigna stă cu totul în buton; una lată („99+”) crește tot spre dreapta;
  - după fiecare operație a Commit-ului (commit, stage, discard), numărul se citește din
    nou pe loc (`CommitView::tell_status`), fără să aștepte un eveniment.
  - **Rămas:** cauza exactă a lui „1” n-am putut-o reproduce (ambele părți citesc același
    `git status`). Dacă mai apare, de văzut ce fișier numără: tooltip-ul butonului
    (`git.changed_files`) ar putea să-l numească.
- **Ce s-a cerut — butonul Git arată „1” deși Commit nu are nimic** (Staged 0, Unstaged 0). Probabil
  numără ce Commit ascunde: dosarul de date al aplicației (`.lumen/`, cu fișiere urmărite
  de git în unele proiecte) sau un fișier nou ignorat în listă (`.codegraph/`). Regula:
  insigna numără exact ce arată lista de Commit, nimic în plus. De făcut cu un test care
  pica înainte: un proiect cu o schimbare doar în `.lumen/` înseamnă insigna goală.
- **Insigna arată rău:** un cerc alb care iese din colțul butonului. De făcut o singură
  componentă în `ui-components`, folosită de toate butoanele cu număr (Git, fișierele de
  urcat ale proiectului remote și celelalte): pastilă în culoarea temei, textul centrat,
  lățimea după număr („9+”/„99+”), un contur în culoarea fundalului ca să se despartă de
  iconiță, poziția fixă în colțul butonului, la fel la orice scară.

## Barele de derulare care se ascund (cerut 2026-11-12) — **făcut**

- **Făcut (2026-11-12):**
  - **Cauza barei rămase vizibile:** doar panoul de sub mouse afla de mișcare. Ieșit
    direct de pe banda barei, arborele nu afla că mouse-ul a plecat, iar bara rămânea
    ținută. Acum panoul părăsit primește un ultim hover, cu punctul de afară
    (`Workbench::leave_side_panel`, cu test).
  - **Regula de ascundere e o singură piesă**, `ui_components::AutoHide`: apare la
    derulare, rămâne dacă mouse-ul e pe ea după ce s-a arătat, apoi se stinge.
    `EdgeScrollbar` o folosește.
  - **Bara editorului de cod** folosește aceeași piesă: e ascunsă în repaus, apare la
    derulare și se stinge după. Marcajele de pe banda din dreapta (erori, modificări) rămân
    vizibile. Un clic pe bandă sare acolo ca înainte. Cadrele pentru stingere le cere
    aplicația (`EditorView::scrollbar_deadline`).

Ce scria înainte:

- **Arborele de proiect și listele** (`EdgeScrollbar` cu `auto_hiding`, schimbat în
  `bf1ae01d`): apar doar la derulare, dispar după 0,6 s. Utilizatorul a văzut bara
  arborelui rămasă vizibilă când a trecut cu mouse-ul în altă zonă. De verificat pe un
  build de după acel commit:
  - dacă `observe` vede o derulare și fără una reală (offset-ul se schimbă la layout);
  - dacă mouse-ul trecut pe deasupra benzii ei o ține vizibilă (`kept`);
  - dacă se cere cadrul în care se stinge (`next_frame_at`), ca ascunderea să fie
    desenată și când nu se mai întâmplă nimic altceva.
- **Bara editorului de cod** (a lui `ui-workbench`, alta decât `EdgeScrollbar`) e mereu
  vizibilă. De adus la aceeași regulă: ascunsă până la o derulare, cu harta modificărilor
  din dreapta (cele marcate pe ea) păstrată vizibilă, dacă rămâne pe bandă.

## Sesiunea de dinainte, de redeschis (cerut 2026-11-12) — **făcut**

- **Făcut (2026-11-12, plan 78):** pagina de start arată mereu „Sesiuni anterioare”, fiecare
  un arbore sesiune → ferestre → fișiere, cu „închisă neașteptat” la cele căzute și
  „Redeschide”. Ferestrele se redeschid pe rând (după primul cadru al celei dinainte + 300 ms),
  cu progresul și „Stop” în bara de jos; limita și numărul de sesiuni păstrate sunt setări.
  Nimic nu se redeschide singur.

Ce scria înainte:

- **Ce s-a cerut:** după ce aplicația a fost închisă forțat (force quit, o cădere), cu mai
  multe proiecte deschise, pagina de start arată „sesiunea de dinainte”: data și ora,
  proiectele și fișierele care erau deschise. Un buton le redeschide pe toate.
- **Condiție:** redeschiderea trebuie să fie prietenoasă cu sistemul, nu toate deodată.
- **De analizat înainte de orice cod:**
  - ce se păstrează azi pe proiect (`.lumen/session.toml`: tab-uri, grupuri) și ce
    lipsește: lista ferestrelor/proiectelor deschise în aceeași sesiune a aplicației,
    scrisă des (nu doar la ieșire: la o cădere nu mai apuci) și atomic;
  - cum se recunoaște o ieșire nefirească: un marker șters la ieșirea normală;
  - redeschiderea pe rând: o fereastră după alta, fiecare proiect cu indexarea lui pornită
    doar după ce precedentul s-a liniștit (sau doar cel din față), tab-urile citite leneș
    (cum se face deja la o singură fereastră), o limită și un „Stop” vizibil;
  - ce arată pagina de start: o carte „Sesiunea de dinainte” (data și ora, proiectele cu
    numărul de fișiere deschise), „Redeschide tot” / alegerea câtorva / „Ignoră”;
  - testele: o sesiune scrisă, o „cădere” simulată, redeschiderea în ordinea și ritmul
    stabilite.

## Căutarea în proiect (cerut 2026-11-12) — **făcut**

- **Făcut (2026-11-12):** cauza era indexul de căi, care sărea peste tot ce lasă afară
  `.gitignore`, deși arborele arată acele fișiere.
  - Fișierele ignorate din dosarele parcurse oricum intră acum în `PathIndex::ignored` și
    sunt găsite după nume (Go to File, fișierele după nume din căutare).
  - Nu sunt căutate în conținut și nici listate ca fișierele proiectului.
  - Un dosar ignorat (build, cache) tot nu e parcurs.
  - Testul: `.env.local` și `build/` din `.gitignore`.

Ce scria înainte:

- **`.env.local` nu apare la căutare**, deși e în proiect (îl arată arborele). Căutarea
  textului (semantic + lexical) a dat `README.md`, `.gitlab-ci.yml`, `.env` și altele, care
  conțin textul „.env.local”, dar nu fișierul însuși. De verificat:
  - dacă fișierele ignorate de git (`.env.local` de obicei e) sau cele cu nume care
    încep cu punct sunt lăsate afară din index, deși arborele le arată;
  - dacă un rezultat după numele fișierului (o potrivire exactă a numelui) intră în
    listă înaintea celor după conținut, cum face căutarea de fișiere din arbore;
  - un test care pica înainte: un proiect cu `.env.local` ignorat de git, căutat după
    nume.

## Timpul de build (cerut 2026-11-12)

- **Făcut 2026-11-12** (măsurat pe Mac-ul utilizatorului, Apple Silicon):

  | profil | build complet | doar aplicația schimbată | binarul |
  |---|---|---|---|
  | `release` (LTO fat, 1 codegen unit) | 6 m 48 s | 5 m 39 s | 84,0 MB |
  | `dist-fast` (LTO thin, 16 codegen units) | 3 m 55 s | 2 m 02 s | 115,8 MB |

  - **`./scripts/build.sh mac --fast`** construiește cu `dist-fast`: pentru build-urile de
    încercat, de ~2,8× mai repede când s-a schimbat doar aplicația. Ce se distribuie rămâne
    `release`: aplicația livrată nu se schimbă.
  - **Iconița** se generează doar când se schimbă `assets/app-icon.png` sau
    `xtask/src/app_icon.rs`. Altfel `xtask` (cu SQLite din C, bibliotecile de imagini) nu se
    mai compilează deloc, iar asta era așteptarea lungă de la „the app icon”.
    `xtask` e compilat optimizat: iconița iese în 1,4 s în loc de 6,8 s.
- **Făcut tot 2026-11-12:**
  - `scripts/build.ps1 -Fast` (Windows nativ) construiește cu `dist-fast`, ca
    `build.sh --fast` (sintaxa verificată cu parserul PowerShell; build-ul îl rulează
    utilizatorul).
  - **LTO fat cu 16 codegen units, măsurat și respins** (macOS, Apple Silicon):

    | profil | build complet | doar aplicația | binarul |
    |---|---|---|---|
    | `release` (fat, 1 unit) | — | 6 m 00 s | 84,9 MB |
    | fat, 16 units | 8 m 47 s | 7 m 24 s | 88,9 MB |

    Mai lent și mai mare: pasul LTO fat unește tot oricum, iar unitățile în plus doar se
    adaugă după el. `release` rămâne cum e; pentru încercări, `--fast` / `-Fast`.
  - **`sccache`**: nu e instalat aici și nu l-am instalat. Ajută doar la build-uri curate
    (alt dosar `target`, CI, după `cargo clean`); între build-uri obișnuite, cache-ul
    incremental al cargo face deja treaba asta, iar pasul lung (LTO, legarea) nu se poate
    pune în cache. De încercat doar dacă build-urile curate devin dese
    (`brew install sccache`, `RUSTC_WRAPPER=sccache`).

## DDD

- ~~**Restul din planul 36f**~~ — **făcut** (verificat 2026-11-08): 36f2–36f4 „gata”;
  `refactor::applying`, `Ports` în `Workbench`, `FileWatcher` în `fs-local`, capabilitățile
  `WorkbenchHost`.

- **Setările quality** rămân secțiuni dinamice per limbaj (hotărât 2026-11-08): se scriu
  și se citesc într-un singur loc (`quality/src/settings.rs`), iar cheile compuse și scrierile
  dintr-o bucată nu încap într-un `Setting<T>`.
- **Ce rămâne generic, cu motiv** (review 32d, planul 33):
  - scope-ul `variable.php` e captura gramaticii PHP, pe care o folosesc temele;
  - separatorii din `fuzzy` sunt punctuație la marginea cuvintelor;
  - `signature_plainly` e doar rezervă, pentru un limbaj pe care proiectul nu-l cunoaște.
- **Lista de extensii a căutării text e fixă.** Decizie acceptată în planul 28; se
  redeschide dacă apar limbaje noi fără extensie în listă.

## De păstrat (nu se schimbă)

- **`BUNDLE_ID`** rămâne parte din brand (`brand.toml`). (review 29)
