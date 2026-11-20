# 83 — Procesele rămase după o cădere: știute, puse în fața utilizatorului

**Stare (2026-11-17): 83a, 83b, 83c făcute.**

Cerut 2026-11-17: „ar fi bine la nivel de proiect sau global să știm ce proiect are ce
procese deschise, dacă crapă ceva să nu rămânem cu ele orfane, și la deschiderea nouă ce se
poate rezolva cu acele procese orfane. Problema e că poate unele din ele încă fac ceva.”

## Ce am găsit (2026-11-17)

- În aplicația utilizatorului: un server PHP (`<data>/lsp/php/8.4.22/php`) rămas orfan de 14
  zile (părintele: PID 1), dintr-o rulare care s-a încheiat fără să-l oprească. Serverul
  primește `processId` la `initialize`, dar nu-l urmărește.
- Ce pornește aplicația: shell-urile terminalelor și ce rulează în ele (Claude Code, build-uri,
  `tail -f`), serverele de limbaj (și `node`-ul lor), agenții (ACP), task-urile, git. La o
  închidere normală se opresc; la o cădere, unele rămân (cele care ignoră închiderea pipe-ului
  sau SIGHUP-ul).

## Pași

### 83a — Serverele de limbaj rămase (făcut)

Ce rulează din folderul serverelor cu părintele dispărut e oprit la pornire, pe un worker
(`lsp_client::orphans::stop_left_behind`). Un server nu face nimic util fără aplicație și e
repornit la nevoie. O altă instanță a aplicației își păstrează serverele (părintele lor trăiește).

### 83b — Registrul (făcut)

- Fiecare program pornit de aplicație (prin `process::Run` și PTY-urile terminalelor) e notat
  într-un fișier al aplicației (`<data>/processes/<id-instanță>.toml`, scris pe un worker):
  PID, momentul pornirii (contra PID-urilor refolosite), proiectul (calea lui), felul (server,
  terminal, agent, task, git), numele programului (doar `argv[0]`, curățat: niciun argument,
  pot avea secrete). Scos când programul se termină; fișierul instanței șters la o ieșire
  normală.
- Fișiere mici, scrise rar (la pornire și la terminare, unite): nimic pe firul UI.

**Făcut.**

- `process::launched` (în crate-ul `process`, infra-shared): felul programului (`Kind`:
  server, terminal, agent, task, git, altul — din `Run::owner`: `lsp`/`driver` → server,
  `acp` → agent, `git` → git), numele lui (`program_name`: ultima parte a căii, fără `.exe`
  și `-`-ul unui login shell, fără caractere de control, cel mult 64 — niciodată argumentele)
  și un `Recorder` pus o dată de aplicație (fără el, în teste și unelte, nu se notează nimic).
  **Regula pentru cele scurte:** un `process::start` (server, agent, driver) e notat imediat;
  un `process::run` doar după ce a rulat `LONG_LIVED` = 2 s (comenzile git obișnuite, un
  formatter se termină înainte); sfârșitul lor e spus la drop (`Recorded`). Shell-ul unui
  terminal: un apel în modulul terminalului (`process::launched::shell`, cu PID-ul, programul,
  folderul de pornire, și dacă rulează un agent în locul shell-ului); sfârșitul lui îl vede
  registrul singur.
- Crate nou `process-registry` (`crates/infra/system/process-registry`, stratul infra; doar
  aplicația îl folosește): `Registry` ține `<data>/processes/<uuid-instanță>.toml` pe un fir al
  lui (`process-registry`): veștile vin pe un canal, sunt unite 300 ms (cel mult 2 s), apoi
  scrise atomic (`files::write_atomic`); momentul pornirii e citit acolo (sysinfo
  `start_time`), niciodată pe firul celui care pornește. Antetul: PID-ul aplicației și
  momentul ei de pornire. Cel mult 256 de programe, un fișier peste 256 KiB nu e citit. Ce nu-și
  spune sfârșitul (shell-urile, ce e preluat) e privit la 10 s, doar cât există. Nimic de
  notat → fișierul șters; la o ieșire normală (`left_behind::finish` după bucla de evenimente)
  fișierul pleacă (rămân în el doar programele preluate încă neîntrebate).

**Lăsat.**

- Proiectul unui program e folderul în care rulează (`Run::dir`, folderul de pornire al
  terminalului), nu identitatea proiectului: potrivit la deschidere cu „e în rădăcina
  proiectului”. Un terminal deschis într-un folder din afara oricărui proiect e întrebat doar
  la deschiderea acelui folder ca proiect.
- Task-urile nu au încă un `owner` al lor (rulează în terminale): felul `task` există pentru
  când vor avea.

### 83c — La pornire (făcut)

- Fișierele instanțelor moarte (PID-ul lor nu mai e al aplicației) sunt citite pe un worker.
  Pentru fiecare program notat, încă viu, cu același moment de pornire (și, cu descendenții
  lui: ce a pornit el):
  - **servere, ajutoare** → oprite (ca la 83a);
  - **terminale, agenți, task-uri** → păstrate și **puse în fața utilizatorului** la
    deschiderea proiectului lor (ale niciunui proiect: în fereastra deschisă prima): o fereastră
    „Au rămas programe de la sesiunea care s-a închis” cu fiecare — ce e, proiectul, de cât
    rulează, CPU-ul de acum (lucrează sau stă) — și „Oprește” / „Lasă-l”. Ce e lăsat nu mai e
    întrebat (scos din registru).
- În Task Manager: rândurile lor, sub „Rămase”, cât trăiesc.

**Făcut.**

- `process_registry::survey`, pe firul `left-behind` la pornire: o privire la toate procesele
  (sysinfo); fișierele altor instanțe al căror PID nu mai e al aplicației (alt moment de
  pornire) sunt **luate** (redenumite `.taken`: două porniri nu le tratează de două ori; unul
  luat de o pornire căzută e reluat după un minut). Fiecare program încă viu cu același moment
  de pornire e găsit cu descendenții lui (copii porniți nu mai devreme decât părintele):
  servere, git, ajutoare → oprite pe loc (SIGTERM, apoi SIGKILL după 3 s, fiecare verificat
  după PID + moment de pornire; pe Windows, fără SIGTERM, oprit direct); terminale, agenți,
  task-uri → măsurat CPU-ul de acum (două priviri la 250 ms, cu descendenții; „lucrează” peste
  2%) și **preluate** în fișierul acestei instanțe (o nouă cădere înainte de răspuns întreabă
  iar), apoi fișierele luate șterse. PID-urile ≤ 1 și al aplicației nu sunt atinse niciodată.
- Întrebarea: dialogul aplicației (`DialogSpec`, fără UI nou), câte un program pe rând —
  „**claude**, un agent din **shop**, rulează de 3 ore și lucrează acum (42% CPU)” și că nu
  mai poate fi legat de un tab — cu „Oprește” / „Lasă-l” și, cât mai sunt, „Oprește toate” /
  „Lasă toate”; Esc / × = lasă-l. Pus în fereastra proiectului lui când e deschis (al
  niciunui proiect: în prima fereastră), doar când nu e deschis alt dialog sau modal; verificat
  la fiecare eveniment doar printr-un atomic. Ce primește un răspuns e scos din registru (nu
  mai e întrebat); oprirea rulează pe un fir (`left-behind-stop`).
- În Task Manager (făcut 2026-11-17, cerut după): un grup nou, „Rămase de la sesiunea
  trecută” (`resources::LEFT_BEHIND`), cu câte un rând pentru fiecare program rămas, cât
  rulează — întrebat sau nu, lăsat sau nu: numele și PID-ul, ce e, proiectul, de cât rulează;
  procesele lui (și ce a pornit) măsurate acolo (CPU, memorie); „Oprește” (întrebat întâi),
  care nu-l mai pune nici în dialog. Rândurile vin din memorie (raportorul aplicației,
  `left_behind::Reporter`, citit și pe firul UI); sfârșitul lor e văzut de firul
  `left-behind-watch` la 5 s, uitându-se doar la PID-urile lor
  (`process_registry::survey::still_running`), care trăiește doar cât sunt rânduri. Nu sunt
  „lucru” pe care o închidere îl oprește.
- Texte în `en` și `ro` (`[left_behind]`).
- Teste: `process` (numele, felul, cele lungi notate și cele scurte nu), `process-registry`
  (fișierul dus-întors și limitele, scris / scos / preluat / ieșirea normală cu un `sleep` al
  testului, PID refolosit respins, arborele, decizia, privirea de la pornire cu `sleep`-uri
  proprii: serverul și copilul lui opriți, terminalul păstrat, PID-ul cu alt moment lăsat,
  fișierul unei instanțe vii neatins), `app` (unde e întrebat, textul întrebării). Încercat
  și în aplicație (HOME izolat): serverul fals oprit, terminalul fals preluat în fișierul
  noii instanțe.

**Lăsat.**

- CPU-ul e cel de la pornire, nu cel de la deschiderea proiectului (dacă proiectul e deschis
  mult mai târziu, poate fi vechi); durata e calculată la afișare.
- Un program al unui proiect care nu mai e deschis niciodată nu e întrebat (rămâne preluat de
  la o instanță la alta cât trăiește).
- Windows: oprirea (fără SIGTERM, direct) și citirea proceselor sunt prin sysinfo, verificate
  doar cu clippy pentru Windows, nu rulate.

### 83d — Istoricul terminalelor ascunse, mutat compact (făcut)

Cerut 2026-11-17 (din măsurarea heap-ului: ~16–26 MB per terminal, 7 terminale Claude).

**Făcut.**

- `pty::parking` / `pty::parked`: un terminal nedesenat de `PARK_AFTER` = 120 s, cu cel puțin
  256 de rânduri de istoric, își mută istoricul (nu ecranul) într-o formă compactă: textul
  UTF-8 al rândurilor, rulări de culori și atribute, lungimi; ce are `CellExtra` (diacritice
  combinate, linkuri OSC 8) păstrat ca atare. Sub lacătul emulatorului doar se mută rândurile
  (`mem::take`, `clear_history`, `truncate`: pleacă și cele ~1000 făcute dinainte), împachetarea
  e în afara lui, pe firul `terminal-park` (unul pentru toate, oprit fără sesiuni). Ce vine cât
  e ascuns e mutat iar la `PARK_AGAIN` = 15 s; nimic cât rulează un program pe ecranul
  alternativ.
- Înapoi, identic (`PartialEq` pe rânduri): la afișare, pe un worker (bara de derulare
  numără rândurile mutate între timp); la derulare, redimensionare, căutare imediat. Peste
  limita istoricului, cele mai vechi pleacă; poziția derulării e păstrată; ce a fost mutat la
  altă lățime e reașezat. Un `ESC[3J`, un `ESC c` sau „Clear Terminal” le șterge și pe cele mutate.
- Măsurat (release, macOS): 5000×200 rânduri — 22,9 MB → 0,70 MB, lacătul ținut 26–40 µs la
  mutare, 2,2–4 ms la întoarcere; un terminal ascuns cu 4046 de rânduri: 23,5 MB → 1,0 MB
  (heap-ul procesului 51,4 → 29,1 MB); primul cadru la reafișare ~0,5 ms.

**Lăsat.**

- Un program care tot scrie cât e ascuns umple iar grila între mutări: economia e parțială
  cât scrie, întreagă când se oprește.
- Windows: compilat (clippy), nerulat; cu mimalloc, rândurile eliberate pe alt fir se pot
  întoarce la sistem abia când firul cititorului alocă iar.
- Neîncercat în aplicație cu două tab-uri (snapshot-ul nu poate ascunde un tab de terminal):
  numerele sunt dintr-un harness cu sesiuni `pty::spawn` reale.

## Cadrele barelor de resurse

**Făcut (2026-11-17).** Barele contorului din bara de stare alunecă (220 ms, cadre la 16 ms)
doar în fereastra din față; în celelalte sar la noua valoare (`StatusBar::set_still`): înainte,
fiecare eșantion (la 2 s) cerea ~14 cadre în fiecare fereastră vizibilă.

## De hotărât

- Un terminal rămas nu poate fi „reatașat” (PTY-ul lui a murit cu aplicația): doar oprit sau
  lăsat. De spus în fereastră.

## La ieșire, pe macOS: ce a pornit aplicația se oprește (2026-11-20)

**Ce s-a văzut.**
- După închiderea ultimei ferestre, Dock-ul arăta aplicația „Running in Background”. Cam 20 s
  mai târziu, macOS o închidea singur și întreba dacă are voie să ruleze în fundal.
- Jurnalul arăta aplicația terminată în câteva milisecunde.
- O urmărire din afară a arătat ce mai rula: ajutorul unui agent, pornit dintr-un terminal al
  aplicației. Se desprinsese în propriul grup de procese și aștepta o vreme după ce părintele
  lui se încheia. macOS socotește tot ce pornește din aplicație ca fiind al ei, deci și pe el.

**Făcut.**
- `process::descendants`: tot ce a pornit un proces și ce au pornit acelea, citit din tabelul
  de părinți al sistemului (sysinfo).
- Un copil e socotit copil doar dacă a pornit după părintele lui. Așa un id refolosit nu
  adoptă un program mai vechi.
- Oprirea: fiecare program e numit după id și ora pornirii. Întâi i se cere să se încheie
  (`SIGTERM`). După 500 ms, cele rămase sunt oprite forțat (`SIGKILL`), doar dacă sunt încă
  același program. Semnalul merge doar la procesul găsit, niciodată la grupul lui.
- Lista se ia când pleacă ultima fereastră, înainte să cadă terminalele ei. Atunci arborele e
  încă legat; mai târziu, ajutorul desprins ar fi adoptat de sistem și n-ar mai putea fi
  recunoscut. La Quit, lista se ia la sfârșitul buclei.
- Oprirea are loc înainte ca aplicația să repornească după o actualizare, deci noua instanță
  nu e printre ele. Jurnalul spune câte au fost și câte au trebuit oprite forțat.
- Doar pe macOS. Pe Linux, un browser deschis din aplicație poate fi copilul ei, iar pe
  Windows nu există problema.
- Teste: arborele sub o rădăcină; un id refolosit; un ciclu; un shell real cu două programe
  pornite de el, găsite și oprite.

**Fereastra de oprire (cerută după, 2026-11-20).**
- Când pleacă ultima fereastră sau dai Quit din ea, aplicația nu mai iese pe loc dacă a pornit
  programe care încă rulează. Fereastra întrebării „Ieși din aplicație?” (a modulului
  `running-work`) se deschide cu titlul „Se închide aplicația” și arată:
  - o bară de progres, „3 din 5”, cu procentul;
  - lista programelor într-o casetă ca a unui câmp de formular: starea fiecăruia (un loader
    cât se închide, ✓ închis, ⚡ oprit forțat), numele, pid-ul aliniat pe coloană. Peste 8
    rânduri, lista se derulează cu rotița.
- Cât lucrează, nimic nu închide fereastra: nu e niciun buton, `esc` nu e desenat, × nu face
  nimic, tastele sunt luate fără efect. Cmd+Q și butonul de închidere al ferestrei sunt
  ignorate. Fereastra se închide singură (sau aplicația iese) după ce s-au încheiat toate:
  rămâne cel puțin 0,8 s și încă 0,45 s după ultimul, ca să se vadă cum se termină.
- Oprirea rulează pe firul `app-ending`. Fiecare program primește 2 s ca să se închidă singur,
  apoi e oprit forțat. Firul anunță fereastra după fiecare program încheiat.
- Contractul: `RunningPrompt.stopping` (`Stopping`, `StoppingProgram`, `ProgramState`).
  `Viewer::modal_closable()` și `ModalSpec.closable` ascund `esc` și opresc ×.

**Ce nu atinge niciodată (grija cerută pentru mai multe proiecte).**
- Altă instanță a aplicației sau alt program: se iau doar descendenții acestui proces.
- Alt proiect deschis în aceeași aplicație: oprirea se face doar când pleacă **ultima**
  fereastră. Închiderea unei ferestre dintre mai multe nu oprește nimic în plus, iar
  programele ei se închid ca înainte, cu terminalele ei. La Quit cu mai multe ferestre se
  închid oricum toate proiectele; oprirea se face atunci la ieșire, fără fereastră.
- Testele și capturile: fără fereastră reală nu se oprește nimic. Altfel ar putea fi oprite
  programele altor teste, care rulează în paralel.

**Lăsat.**
- (Făcut 2026-11-20) **Oprirea ajutoarelor desprinse ale unui singur proiect**, când i se
  închide fereastra și altele rămân deschise.
  - La închidere, registrul (`Registry::programs`, din memoria firului lui) dă programele
    pornite de această rulare și dosarul fiecăruia.
  - Ale proiectului sunt cele din dosarul lui sau dintr-un subdosar, dacă niciun alt proiect
    deschis nu e mai aproape de ele: un proiect aflat în dosarul altuia își păstrează
    programele.
  - Descendenții lor (`Descendants::under`, o singură citire a tabelului de procese) sunt
    luați înainte să cadă terminalele ferestrei, fără rădăcini: acestea se închid cu
    fereastra, ca înainte. Fiecare rădăcină e verificată după pid și ora pornirii.
  - După ce fereastra a plecat, cele încă pornite sunt oprite pe firul `project-leaving`:
    `SIGTERM`, apoi `SIGKILL` după 2 s, doar dacă sunt încă același program.
  - Nimic nu se oprește dacă același proiect e deschis în altă fereastră. Fereastră fără
    proiect: nimic. Doar pe macOS, ca la ultima fereastră.
  - Teste: regula proiectului cel mai apropiat (`owned_by`), pădurea sub mai multe rădăcini
    (`forest`: rădăcinile lăsate afară, un id refolosit sărit).
- Pe Linux și Windows: fără fereastră de oprire (vezi mai sus de ce).
