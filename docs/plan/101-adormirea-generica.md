# 101 — Adormirea generică a view-urilor grele

Cerut 2026-11-27. Adormirea sesiunilor de agenți (plan 100: 100c, 100g–100l) a mers; utilizatorul
vrea același mecanism refolosit în mai multe locuri, întâi la diagramele draw.io, apoi la orice
view greu. Gândit agnostic: o parte comună care doar măsoară, decide **când** și invocă; fiecare
modul decide **ce** face la adormire și la trezire (oprește ce e greu, își păstrează starea, o pune
înapoi). Comunicarea doar prin contracte: cereri (comenzi și întrebări), evenimente,
contribuții.

**Unde suntem:** 101a–101f făcute (mecanismul comun, terminalul mutat pe el, starea diagramei păstrată, diagrama adormită), cu teste, pe macOS; Windows compilat (clippy), nerulat. Urmează 101g (memoria măsurată).

## Ce vrem

1. **Un singur mecanism de adormire**, refolosit de orice view care ține ceva greu (un proces, o
   pagină web, o conexiune): aceeași regulă, același timer, același card „adormit”, aceleași
   meniuri, comenzi, indiciu, log și rând în Task Manager.
2. **Partea comună nu știe nimic de module.** Nu numește terminalul, diagrama sau agenții; nu umblă
   la starea lor. Le cunoaște doar prin ce ele declară (un „fel” de view adormibil) și prin
   contract.
3. **Modulul face tot ce ține de el.** La adormire: își salvează starea (zoom, poziție, pagina,
   selecția, istoricul de undo la diagramă; comanda de reluare la terminal), apoi eliberează. La
   trezire: pornește din nou și pune starea înapoi. Utilizatorul găsește tabul exact cum l-a lăsat.
4. **Opțiune prin setări, pe fiecare fel**: pornit / oprit și după cât timp, cu un indiciu
   (coach mark) care o oferă prima dată când deschizi un view de felul ăla cât e oprită, ca azi la
   sesiuni.
5. **Diagramele** primele după terminal; apoi oricine se înscrie (paginile web, alte view-uri
   grele), fără nicio schimbare în partea comună.

## Ce rămâne neschimbat pentru utilizator

- Adormirea e oprită implicit, pe fiecare fel: nimic nu e oprit fără acordul utilizatorului.
- Sesiunile de agenți se poartă exact ca acum (100l): regula, cardul, meniurile, panoul sesiunilor,
  indiciul, setarea lor (citită din cheile vechi, fără ca utilizatorul să o aleagă din nou).
- Nimic nu adoarme cât lucrează sau cât are ceva nesalvat; adormit, rămâne așa până îl trezește
  utilizatorul (o tastă, un clic, „Wake Up”), mutarea pe tab nu-l trezește.
- O diagramă trezită arată ce arăta: același desen, zoom, poziție, pagină, selecție, și undo-ul
  merge ca înainte.

## Hotărâri

### Cine ce face

| Parte | Unde | Ce face | Ce nu știe |
|---|---|---|---|
| **Regula** | `crates/domain/core/sleep` (nou, pur) | Faptele unui view → „poate adormi” sau motivul pentru care stă treaz; ordinea stărilor (`CanSleep`); felurile declarate | Threaduri, UI, module |
| **Contractele** | `contracts::sleep` (nou) | Cererile, întrebările și evenimentele dintre module (mai jos) | Implementările |
| **Partea din view** | `ui_components::sleep` (nou) | Slotul unui view (atins când, ordin primit, adormit, se trezește), registrul slot-urilor după cheie, primirea ordinului la `poll`, cardul „adormit” și voalul, rândurile din meniul tabului, notificarea de refuz | Ce ține view-ul, cum îl oprește |
| **Timerul** | modulul `modules/tools/sleep` (nou) | Firul care se uită o dată pe minut, cozile pe proiect, trezirea ferestrelor, setările pe fel, indiciul, comenzile, Task Manager, logul cu motivul | Ce fel e care; ce face un view |
| **Driverul** | fiecare modul (terminal, diagrame, …) | Implementează `Sleepable`: ce îl ține treaz, adormirea, trezirea, memoria | Timerul și regula |

Partea din view stă în `ui-components` pentru că orice modul își construiește view-ul de acolo
(regulă: modulele nu depind de `ui-workbench`, nici unul de altul). Timerul e un modul ca oricare:
scos, nimic nu mai adoarme și nimic altceva nu se strică (testul care scoate fiecare modul pe rând).

### Contractul driverului (`ui_components::sleep::Sleepable`)

Implementat de view-ul modulului; partea comună îl cheamă doar pe firul UI, la `poll`:

- `kind() -> &'static SleepKind`: felul lui (id stabil, nume i18n, icon, timpul implicit,
  textul indiciului), declarat o dată de modul.
- `awake_because(&self) -> Option<Veto>`: ce îl ține treaz acum, al lui (lucrează, ceva
  nesalvat, text tastat, un dialog deschis, nu știe cum se reia). `Veto` = un id static și cheia
  lui i18n: logul și meniurile îl spun fără să-l înțeleagă.
- `quiet_for(&self) -> Option<Duration>`: de cât timp n-a făcut nimic singur (terminalul: ieșire în
  PTY; diagrama: nicio scriere, nicio schimbare venită de pe disc).
- `fall_asleep(&mut self, why: SleepWhy) -> Result<Slept, Veto>`: își salvează starea, eliberează,
  spune ce a eliberat și ce imagine a ultimului ecran rămâne. Poate refuza în ultimul moment.
- `wake(&mut self)`: pornește din nou și își pune starea înapoi; slotul arată „se trezește” până
  când view-ul spune că s-a așezat (`settled()`).
- `memory(&self) -> ViewMemory`: ce ține acum (există deja pe `Viewer`).

„Atins” (100l) e al părții comune: view-ul îl spune prin slot (`slot.touch()`) din handler-ele lui
de input; o listă scurtă, aceeași pentru toți (tastă, text, paste, clic, rotiță, o comandă a
tabului, tabul ales).

### Comenzi, întrebări, evenimente (`contracts::sleep`)

Cheia unui view adormibil e cheia tabului lui (`ViewTab`/UUID, 100i), niciodată un titlu.

- **Comenzi (cereri care schimbă ceva):**
  - `PutToSleep { key }` → `bool`: adoarme acum (de mână); refuzul aduce motivul.
  - `WakeUp { key }` → `bool`: trezește (aduce tabul în față întâi).
  - `EnableSleep { kind }`: pornește setarea unui fel (butonul „Turn on” al indiciului).
- **Întrebări (cereri care doar citesc, din memorie):**
  - `SleepStateOf { key }` → `Option<SleepState>` (`asleep`, `running`, `can_sleep`, `memory`,
    motivul).
  - `SleepKinds` → felurile înscrise (pagina de setări, Task Manager).
- **Evenimente (pe busul proiectului, o dată pe schimbare, fără lacăt ținut):**
  - `SleepStateChanged { key, kind, state: Option<SleepState> }` (`None`: view-ul a dispărut).
  - setările își anunță singure schimbarea (`SettingChanged`, existent).

Contractele de azi ale terminalului (`SleepTerminal`, `WakeTerminal`, `TerminalUse`,
`TerminalState`, `TerminalStateChanged`, `CanSleep`) devin cele generice; panoul sesiunilor le
ascultă pe ele. `ResumeLaunch` rămâne al terminalului cu agenții (cum se reia un program e treaba
lor, nu a părții comune).

### Regula (aceeași pentru toți, 100l)

Un view adoarme când felul lui e pornit și, în același timp:
1. n-a fost atins de `idle` (pe ecran sau nu);
2. a stat liniștit de `idle` (`quiet_for`);
3. driverul nu are niciun veto;
4. nu e deja adormit, cu un ordin pe drum sau în trezire.

Ordinul e lăsat dacă view-ul a fost atins între timp; unul neluat în 20 s e lăsat (100g). De mână
adoarme oricum, cu excepția vetourilor pe care driverul le marchează „și de mână” (text netrimis,
ceva nesalvat).

### Setări pe fel

- Un bloc „Adormire” în setări, generat din felurile înscrise: pentru fiecare, un switch și timpul
  (15 min … 4 h, implicit al felului). Căutarea îl găsește după cuvintele fiecărui fel.
- Cheile: `sleep.<kind>.on`, `sleep.<kind>.after`. Pentru terminal, cele vechi
  (`terminal.sleep_idle`, `terminal.sleep_after`) sunt citite ca alias (regula: un id salvat care
  se schimbă merge în continuare).
- Indiciul: unul per fel (`sleep.<kind>`), cerut de driver la prima deschidere cât e oprit;
  „Turn on” → `EnableSleep`, „Settings…” → pagina. Cel de azi (`terminal.sleep`) rămâne seen
  pentru cine l-a închis (alias).

## Pașii

### 101a — Regula și contractele (pur)

- `crates/domain/core/sleep`: `SleepKind`, `Veto`, `SleepFacts` (atins, liniștit, veto, adormit,
  ordin, trezire), `awake_because(facts, idle, now)`, `CanSleep` și ordinea lui, `SleepWhy`
  (de mână / după N min). Strat `domain` în `architecture.toml`.
- `contracts::sleep`: cererile, întrebările, evenimentul de mai sus, doar date (`Send`, fără
  tipuri de module).
- Teste: regula pe fiecare caz (cele din 100l mutate aici), ordinea `CanSleep`.

**Făcut:** crate-ul pur `sleep` (`crates/domain/core/sleep`): `SleepKind` (id, viewer, indiciu,
textele lui: fiecare fel cu cuvintele lui), `Veto` (id-ul din log, rândul din meniu, refuzul;
`by_hand`: ține și împotriva ordinului utilizatorului; `never`: rândul ascuns), `CanSleep` (`No`,
`NotNow(veto)`, `Sleeping`, `Waking`, `Yes`), `SleepFacts`, `awake_because`, `can_sleep`,
`order_holds`, `ORDER_FRESH`. `contracts::sleep`: `PutToSleep`, `WakeUp`, `SleepStateOf`,
`SleepState`, `SleepStateChanged`. `EnableSleep` n-a mai fost nevoie: „Pornește” din indiciu e
comanda modulului (setarea e a lui). Teste: regula pe fiecare motiv, ordinea `CanSleep`, ordinul.

### 101b — Partea din view (`ui_components::sleep`)

- `SleepSlot` (mutat din `Asked`: cheia, proiectul, atins, ordinul, adormit, de mână, trezire,
  waker-ul ferestrei, memoria, ultimul motiv spus) și registrul slot-urilor (slabe, după cheie);
  `announce` o dată pe schimbare.
- `SleepPart`: ce pune un view în `poll` (ia ordinul, verifică „atins de atunci”, cheamă
  `fall_asleep` / `wake`), în `tab_icon` (luna), `tab_menu` (Adoarme / Wake Up, cu motivul în
  etichetă), `frozen()` (nimic nu se mișcă pe ecranul adormit).
- Cardul „adormit” mutat din terminal în `ds` (`ds::asleep_card`: luna, titlul, de ce, cât a
  eliberat, „Wake Up ↵”, „sau apasă orice tastă”) și voalul peste ultimul ecran; pagină în
  `docs/ui/components/`.
- Teste: cele din `terminal/tests/unit/view.rs` care țin de ordine și trezire, pe un view de probă.

**Făcut:** `ui_components::sleep`: `Slot` (registrul după cheie, slab), `SleepPart` (ce ține
view-ul: `touch`, `started`, `ended`, `set_veto`, `take_order`, `take_wake`, `fell_asleep`,
`waking`, `tab_menu`, `tab_icon`, `refusal`, `paint_card`), `Quiet` (liniștea măsurată de
view, citită de pe alt fir), `SleepPlace` (unde e tabul, ca să fie adus în față), cardul mutat
din terminal (`card.rs`), `configure` / `idle_for` (timpul fiecărui fel, spus de modulul lui din
setările lui), `want_memory` / `memory_wanted`, view-ul cu tastatura (`focused`), anunțurile (o
dată pe schimbare, fără lacăt ținut; plecat: `None`). Pagina `docs/ui/components/sleep.md`,
rândul din regulile componentelor. Teste: registrul, ordinele (atins de atunci, prea vechi, de
mână), meniul pe stări, trezirea, anunțurile, timpul pe fel, tastatura.

### 101c — Timerul: modulul `sleep`

- Firul (`sleep-watch`), cozile pe proiect, trezirea ferestrelor, logul „de ce stă treaz” (100l),
  `ProjectService`-ul care la `poll` dă ordinele — toate mutate din `terminal/src/sleep.rs`, fără
  nimic al terminalului.
- Setările pe fel (blocul generat, cheile, aliasurile vechi), indiciul pe fel, comenzile („Put to
  Sleep”, „Wake Up”, „Put Idle … to Sleep” pe fel), rândul din Task Manager (câte dorm, pe fel).
- Înregistrat în `crates/app/src/modules.rs` (nivel de log propriu).
- Teste: firul rulează doar cât trebuie, un proiect închis nu lasă nimic, ferestrele fără proiect,
  setările vechi citite.

**Făcut:** modulul `sleep-timer` (`modules/tools/sleep`, id `sleep`): firul `sleep-watch` (o
dată pe minut, cât trăiește un view și un fel e pornit), ordinul pus direct în slot și fereastra
view-ului trezită (fără coada pe proiect de la 100c), logul „a view's sleep” (`debug`, doar la
schimbare, cu felul), răspunsurile la `PutToSleep` / `WakeUp` (tabul adus în față după
`SleepPlace`) / `SleepStateOf`, busul fiecărui proiect, indiciul fiecărui fel (cerut o dată pe
view, la poll-ul ferestrei proiectului lui), comenzile „Put Tab to Sleep” / „Wake Up Tab”
(`sleep.now`, `sleep.wake`; cele vechi ale terminalului scoase). Setările rămân ale modulelor
(regula proiectului): terminalul își păstrează cheile, deci nicio migrare. Teste: o privire
ordonă doar ce a fost lăsat în pace la un fel pornit, ordinul neluat e lăsat, trezirea aduce
tabul unde e, indiciul o dată și doar în fereastra proiectului.

### 101d — Terminalul pe mecanismul comun (fără nicio schimbare vizibilă)

- `TerminalView` implementează `Sleepable`: vetourile lui (text netrimis, nu știe cum se reia,
  pornește), `quiet_for` din PTY, adormirea (oprește procesul, păstrează ecranul, comanda de reluare),
  trezirea (pornește cu comanda, „se reia” până se așază ieșirea).
- Panoul sesiunilor ascultă contractele generice.
- Toate testele de azi (terminal, agent-sessions, app, hints) trec neschimbate ca sens.
- Verificat pe o instanță izolată, ca în 100l (agent de probă, adormire scurtă doar în test).

**Făcut:** `SESSIONS` (felul `agent-session`, cu textele de azi ale terminalului: nimic nu se
schimbă la vedere). Motivele terminalului ca `Veto`: programul s-a terminat, nimeni nu știe cum
se reia (sau abia pornește), ceva tastat netrimis, proprietarul lucrează (`ResumeLaunch` întrebat
și cu timpul de inactivitate). `Asked` păstrează doar ce e al terminalului, plus legătura cu
slotul; ordinea, adormirea, trezirea, meniul și cardul trec prin `SleepPart`. Firul terminalului
doar măsoară memoria (`terminal-measure`, cât a întrebat cineva) și urmărește sesiunile care
pornesc (`terminal-start`; trecută vremea, spune singur că nu mai „pornește”). Panoul sesiunilor
ascultă contractele generice (rândurile cu motiv își iau textul din veto). Toate testele de azi
trec, rescrise pe slot.

**Lăsat:** comenzile din paleta de comenzi se numesc acum „Adoarme tabul” / „Trezește tabul”
(erau „… sesiunea”): sunt ale tuturor felurilor.

### 101e — Diagrama: starea ei, salvată și pusă înapoi

Azi un tab draw.io ascuns 60 s își pierde fără să spună pagina web (`HIDDEN_KEPT`): undo, zoom,
poziție, pagină, selecție. Pasul ăsta repară asta și pregătește adormirea.

- `host.js` (același origin cu iframe-ul draw.io) citește starea editorului: scara și translația
  grafului, pagina curentă, selecția (id-urile celulelor), panourile deschise; și spune dacă e un
  text în editare sau un dialog deschis. Mesaje noi în protocol: `hostState` (spre noi) și
  `restoreState` (spre pagină, după `load`); `Session` le tratează (azi le aruncă).
- Undo peste o pagină refăcută: fiecare schimbare ne vine deja ca XML (`autosave`); păstrăm o stivă
  limitată de stări anterioare (în memorie, cu buget, cea mai veche lăsată prima). După refacere,
  când stiva proprie a draw.io e goală, `host.js` cere starea anterioară și o încarcă; redo la fel.
- `DiagramWebView` păstrează starea primită și o trimite la fiecare pagină refăcută (temă schimbată,
  tab ascuns 60 s, adormire).
- Teste: protocolul (mesajele noi, starea păstrată, trimisă după `load`), stiva (buget, ordine).

**Făcut:** draw.io nu-și expune editorul, dar în embed îl face pe `createLoadMessage` (verificat
în pachetul fixat, v32.6.10): `host.js` îl prinde acolo (învelit după `init`, prins la primul
`load`; un `configure` înseamnă un draw.io nou: prins din nou). Spune (`hostState`, la 300 ms
după o schimbare: zoom, translație, scroll, pagina, selecția, editarea unei celule, un dialog)
unde e utilizatorul; `crate::state::ViewState` îl citește verificat (numere finite, în limite,
cel mult 1000 de celule, id-uri scurte). După fiecare `load`, view-ul trimite `hostRestore`;
`host.js` îl ține și îl aplică după ce diagrama s-a încărcat (pagina aleasă „quiet”, ca să nu
intre în undo-ul draw.io; un timer, nu un cadru: o pagină neafișată nu desenează cadre). Undo
peste o pagină refăcută: `state::History` ține ce a fost diagrama înainte de fiecare schimbare
(cel mult 100 de stări și 16 MB, cele mai vechi lăsate întâi); `canUndo` / `undo` ale editorului
învelite: când istoria lui nu mai are nimic, întreabă view-ul (`hostUndo` / `hostRedo`), care
încarcă starea de dinainte și o scrie. Nimic din acestea nu ajunge în cadrul draw.io.

**Verificat** pe draw.io-ul real (pachetul fixat, servit local, Chrome fără interfață cu un profil
al lui, `host.js` exact din sursă, aplicația simulată de o pagină de test): o diagramă cu două
pagini, a doua aleasă, zoom 1,5, o celulă selectată → spus; pagina refăcută → pusă înapoi exact
(pagina, zoom, translație, selecția); undo oferit și cerut view-ului.

**Teste:** citirea verificată (în afara limitelor, prea multe, prea lungi), istoria (doar ce a fost
înainte de pagină, redo, o schimbare nouă taie redo-ul, limitele), protocolul (pusă înapoi după
pagina refăcută, `busy`, undo / redo care încarcă și scriu, pagina reîncărcată singură).

**Lăsat:** fără încărcătorul arătat când pagina e refăcută după 60 s ascunsă (același id): la
101f, odată cu trezirea.

### 101f — Diagrama adormită

- `DiagramWebView` implementează `Sleepable`: veto cât e ceva nescris (`dirty`), un export în
  așteptare, un text în editare, un dialog deschis; `quiet_for` de la ultima schimbare.
- Adormirea: cere starea (101e), scrie ce e nescris, ia o imagine a ultimului ecran (pagina
  motorului: îi cerem pixelii, `Texture::Pixels`; pagina sistemului pe macOS: `snapshot`; altfel
  cardul fără imagine), închide pagina.
- **Imaginea nu se întinde niciodată** (cerut 2026-11-27): desenată la mărimea ei în pixeli
  (1:1, ancorată sus-stânga, ca pagina vie când fereastra se micșorează); o zonă mai mică o taie
  la margine, una mai mare e umplută cu fundalul temei. O singură funcție în
  `ui_components::sleep` (orice view adormit cu imagine o folosește), cu un test la două mărimi.
  Cardul rămâne centrat peste ea.
- Trezirea: pagină nouă (`new_page`, încărcătorul arătat cât se reface), apoi starea pusă înapoi;
  primul clic sau tastă trezește, nu ajunge la o pagină care nu mai există; focusul trece la pagina
  nouă când e gata.
- Motorul căzut la trezire: starea „fără motor”, niciodată o imagine care pare vie.
- Felul `diagram` cu indiciul lui; setarea oprită implicit.

**Făcut:**
- **Poza paginii, în aplicație:** `WebPage::freeze` (view-ul adoarme) → `WebEvent::Picture`, o
  singură dată. Pagina desenată de motorul nostru: o copie a cadrului ei pe GPU
  (`ImageData::copy_of`; renderer-ul o face la următorul lui cadru, desenată sau nu — cu
  `COPY_SRC` pe texturile paginilor — și o ține cât n-o eliberează proprietarul, `held`).
  Una ascunsă destul cât să-și fi pierdut cadrul: motorul e rugat s-o deseneze din nou (spus
  „văzută”, fără să apară pe ecran), așteptată cel mult 2 s. Pagina sistemului: snapshot-ul
  lui (doar macOS). Altfel `None`: cardul singur.
- **Poza nu se întinde:** `sleep::paint_picture` o pune la mărimea ei în pixeli, din colțul de
  sus-stânga, tăiată de o zonă mai mică, fundalul temei dincolo de ea, voalul peste; test la
  două mărimi.
- **Diagrama:** `DIAGRAMS` (felul `diagram`), motivele ei (`NO_PAGE`, `UNSAVED`, `EDITING`,
  `EXPORTING`), liniștea de la ultima schimbare (a utilizatorului sau de pe disc), folosirea
  spusă de `host.js` (`hostTouched`: o apăsare, o tastă, rotița, cel mult o dată la 2 s) și de
  tabul ales. Adormirea: poza cerută, pagina ținută doar până e copiată, apoi lăsată; luna pe
  tab, poza și cardul; o tastă, un clic sau „Wake Up” o trezesc: pagină nouă, starea pusă
  înapoi (101e), poza și „se redeschide” până e desenată. O temă schimbată cât adoarme o lasă
  trează. Pagina refăcută singură după 60 s ascunsă arată acum încărcătorul (`configure` peste
  o pagină desenată).
- **Setări și indiciu:** Setări › Instrumente › Diagrame (pornit / oprit, implicit oprit;
  după cât), indiciul „Diagramele neatinse pot adormi” cu „Pornește” și „Setări…”, comenzile
  lui. Indiciul e marcat arătat doar când chiar apare (un tab de diagramă nu-și știe
  proiectul: fereastra cu tabul lui îl arată).

**Teste:** copia pe GPU (făcută fără să fie desenată, neschimbată de cadrele de după, ținută
până la eliberare; GPU real), poza dată o dată (copie, cadru pierdut desenat din nou, prea
târziu: niciuna, pagină inexistentă), diagrama (adoarme pe poză, pagina lăsată după copie, poza
nu se întinde, trezită ca pagină nouă, poza eliberată; nimic nesalvat / editat / un export nu
adoarme, spunând de ce; fără poză; tema schimbată), folosirea (pagina, tabul), setările.

**Verificat** pe o instanță izolată (draw.io instalat în ea, un `.drawio` deschis, adormirea la
3 min, nimic atins). Am găsit și reparat trei probleme pe care testele nu le prindeau:
- diagrama nu-și primea măsura de liniște: tabul ales o atinge înainte ca pagina să se
  deseneze, iar startul cerea „neatinsă” (acum: oricând sleeping-ul nu știe cât de liniștită e,
  deci și după o trezire);
- nu dădea slotului trezitorul ferestrei: ordinul nu era luat la timp și expira;
- o fereastră acoperită nu desenează cadre, deci paginile nu se urmăreau: acum, la trezirea
  ei, aplicația face ce cere adormirea (poza, copia pe GPU la cerere, pagina lăsată), fără să
  deseneze și fără să atingă celelalte pagini ascunse; snapshot-ul sistemului venit trezește
  fereastra; o diagramă adormită cere cadre până i se face copia.
Apoi: adormită la ordin (70 ms). Instanța de test n-avea motorul nostru (pagina sistemului,
fereastra acoperită: fără poză); copia pe GPU a paginii motorului e testată pe GPU real.

**Lăsat:** memoria eliberată nu e spusă pe card (paginile stau în același proces al motorului;
de măsurat la 101g). Pe Windows și Linux, o pagină a sistemului n-are poză (cardul singur).
Poza cu motorul nostru, într-o instanță reală, nevăzută încă.

### 101g — Memoria arătată și măsurată

- Procesul motorului web (`web-host`) în Task Manager, cu copiii lui (azi nu apare nicăieri).
- Cât a eliberat un tab de diagramă: diferența arborelui de procese înainte și după, spusă pe card
  ca la terminal („circa X eliberați”).
- **Măsurat înainte de 101f:** cât ține un tab draw.io deschis (1, 5, 10 taburi; mic și mare), cât
  scade la închiderea paginii lui, cât după ce motorul rămâne fără pagini (se oprește singur după
  5 min). Cifrele scrise aici; dacă un tab ține puțin, adormirea diagramei rămâne doar pentru
  starea păstrată (101e), iar memoria câștigată se spune cinstit.

### 101h — Ce mai rămâne de unificat (după măsurători)

- Cele 60 s de ascundere ale paginilor web (`HIDDEN_KEPT`): păstrate ca economie, dar acum fără
  pierdere (starea pusă înapoi, 101e); sau înlocuite de adormire pentru felurile înscrise.
- Alți candidați (de măsurat, nu promiși): previzualizările din clientul HTTP (ieftine, refăcute
  din HTML), consolele bazei de date (au deja închiderea conexiunii la inactivitate: ar putea doar
  să-și spună starea prin aceleași contracte), view-urile native cu cache-uri mari (au deja
  `purge`).

## Riscuri

- **Interiorul draw.io nu e un API stabil**: starea citită din `host.js` (scară, translație,
  pagină, selecție, stiva de undo) se poate schimba la o versiune nouă de draw.io. Versiunea e
  fixată (`package.rs`); un test pe pagina reală la fiecare actualizare a lui.
- **Imaginea ultimului ecran**: pagina motorului nu are pixeli pe CPU; cererea lor trebuie făcută
  cât pagina mai e vie. Pe Windows / Linux, pagina sistemului nu are `snapshot`: cardul fără
  imagine.
- **Memoria câștigată la diagrame poate fi mică**: toate paginile draw.io stau în același proces al
  motorului (cel mult 2 procese de randare). Măsurăm întâi (101g).
- **Mutarea terminalului** atinge o parte care merge: 101d nu schimbă nimic vizibil, verificat cu
  toate testele de azi și pe o instanță izolată.
- **Aliasurile setărilor**: cine a pornit adormirea sesiunilor o găsește pornită.
- **Windows**: compilat (clippy pentru Windows), nerulat.

## Măsurători

De făcut la 101g.
