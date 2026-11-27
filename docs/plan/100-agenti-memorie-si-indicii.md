# 100 — Memoria sesiunilor de agenți și indiciile interactive

Cerut 2026-11-25, după o măsurătoare pe Mac-ul utilizatorului: pastila RAM arăta
„1.2 GB | 7.1 GB” (al doilea număr: aplicația plus programele pornite de ea). Aprobate patru
soluții și un sistem de indicii refolosibil.

**Unde suntem:** pașii 100a–100g sunt făcuți, cu teste, pe macOS; Windows compilat (clippy),
nerulat. Ce rămâne e la „Lăsat”, la fiecare pas.

## Context: ce s-a măsurat

- **13 sesiuni de agenți** (programul în linie de comandă al unui agent, rulat în taburile
  noastre de terminal) țineau ≈ **5,5 GB**: 220–650 MB fiecare, multe deschise de 2–4 zile;
  8 din 13 reluate (`--resume`), ceea ce încarcă toată conversația în memoria agentului.
- **Editorul însuși**: 1,3 GB, din care o parte e istoricul terminalelor (setarea
  `terminal.scrollback`, implicit 5000 de rânduri, o grilă pe terminal, ~19 terminale).
- Ce exista deja (83d): istoricul unui terminal nedesenat de 120 s e mutat într-o formă
  compactă în memorie — 5000×200 rânduri: 22,9 MB → 0,70 MB. Pasul 100d duce restul pe disc.

## Ce vrem

1. **Sesiuni adormite**: o sesiune de agent care nu lucrează (nicio tură în curs) și al cărei
   tab n-a fost privit de o vreme e oprită (procesul ei se termină); tabul rămâne (titlul,
   iconița, locul, ultimul ecran), iar când utilizatorul revine pe tab sesiunea e reluată
   singură, prin reluarea agentului. Setare: pornit / oprit (implicit **oprit**) și timpul de
   inactivitate (15 min, 30 min, 1 h, 2 h, 4 h; implicit 1 h).
2. **Reluarea leneșă la pornire**: după o repornire a aplicației, taburile agenților își
   pornesc procesul abia la prima afișare.
3. **Istoricul terminalelor ascunse, scos din memorie**: pe disc, criptat, adus înapoi la
   afișare; ce vede utilizatorul rămâne identic.
4. **Memoria fiecărei sesiuni, arătată** (procesul ei și copiii lui), în panoul sesiunilor și
   în Task Manager, ca utilizatorul să vadă ce sesiune merită închisă sau adormită.
5. **Un sistem de indicii (coach marks) generic și refolosibil**: primul folosit ca să spună,
   la deschiderea unei sesiuni de agent cât timp setarea e oprită, că sesiunile pot fi
   adormite.

## Ce rămâne neschimbat pentru utilizator

- Adormirea e oprită implicit: fără acordul utilizatorului nicio sesiune nu e oprită.
- Nicio sesiune nu e oprită în mijlocul unei ture, cât timp tabul ei e vizibil, cât timp are
  text tastat și netrimis, sau când nimeni nu știe cum s-o reia.
- Istoricul terminalului arată la fel (aceleași caractere, culori, atribute, poziția
  derulării); scoaterea pe disc e invizibilă.
- Taburile, titlurile, iconițele și locul lor rămân; o sesiune adormită spune asta în tab.

## Hotărâri

### Cine ce face (modulele nu se cunosc)

- **Terminalul** (`terminal-panel`) deține procesele: el adoarme un terminal și îl repornește.
  Setarea adormirii e a lui (`terminal.sleep_idle`, `terminal.sleep_after`), cu blocul ei în
  pagina Terminal (`SleepSettings`). Știe doar „un program pornit de alt modul, cunoscut
  după cheie” — niciodată „agent”.
- **Cum se reia** e întrebat printr-o cerere nouă din `contracts::terminal`:
  `ResumeLaunch { key, idle }` → `Option<agents::Launch>`. Răspunde cine urmărește sesiunile
  (`agent-sessions`, din `follow`: cheia → agentul, id-ul, folderul): `provider.resume(...)`.
  `None` (nimeni nu răspunde, sesiunea fără id încă, fișierul ei activ de curând): terminalul
  nu e adormit — prudent.
- **Memoria pe sesiune**: terminalul măsoară arborele de procese al fiecărui terminal cu cheie
  pe un fir al lui (`process-stats`, doar cât e cerută), răspunde la
  `TerminalUse { key }` → `Option<TerminalState { memory, asleep }>` din memorie; panoul
  sesiunilor o întreabă la poll-ul ferestrei (din `ProjectService::poll`) și o desenează pe
  rând. În Task Manager, rândul fiecărui proces al terminalelor poartă titlul tabului
  (`ResourceUse::process_titles`, nou) și detaliul părții numără sesiunile adormite.
- **Indiciile**: contractul în `workbench-model` (`hints`: `Hint`, `HintStep`, `HintAction`,
  `HintContribution`), desenul în `ui-components` (`hint_card`: cartonașul, săgeata, umbra,
  spotul), ancorele în `ui-components` (`anchors`: dreptunghiuri raportate la desen după un
  id), afișarea în aplicație (un `OverlaySlot` nou, `Overlay::Hint`), cererea de afișare prin
  gazdă (`host.show_hint(id)`), „văzut” în setările aplicației (`hints.seen`).

### Ce înseamnă „nu lucrează” (prudent)

Toate trebuie să fie adevărate, altfel terminalul rămâne treaz:

1. setarea pornită;
2. un program pornit de alt modul, cu cheie, încă în viață;
3. nedesenat de cel puțin timpul ales (un tab vizibil se desenează: nu doarme niciodată);
4. **nicio ieșire de la program** de cel puțin timpul ales (`SessionWatch::quiet_for`, nou:
   cititorul PTY notează momentul ultimei ieșiri; o tură în curs scrie mereu — rezultate,
   spinner);
5. niciun text tastat netrimis (vederea ține minte dacă s-a tastat ceva după ultimul ↵);
6. cine a pornit programul știe să-l reia (`ResumeLaunch`), iar fișierul sesiunii n-a fost
   scris în ultimul interval (al doilea semn că nu e o tură în curs).

### Cum doarme și cum se trezește

- Un fir al terminalului (`terminal-sleep`), pornit doar cât setarea e pornită și există
  terminale cu cheie, se uită o dată pe minut la starea lor (doar atomice / mutex-uri mici) și
  trezește fereastra proiectului când unul pare de adormit; `ProjectService::poll` întreabă
  `ResumeLaunch` și, cu răspuns, lasă ordinul în slotul terminalului.
- Vederea, la poll: renunță la sesiune (procesul primește SIGHUP, ca la închiderea tabului),
  păstrează ultimul ecran (grila copiată), ține `Launch`-ul de reluare și spune „adormit”.
- La prima desenare (tabul arătat), pornește `Launch`-ul de reluare; până vine ecranul nou,
  ultimul ecran rămâne desenat, cu o bandă „Se reia sesiunea…”. Tabul arată o lună în locul
  iconiței, iar rândul din panoul sesiunilor spune „Adormită — se reia când îi deschizi
  tabul”.

### Istoricul pe disc

- Peste parcarea din 83d: după `SPILL_AFTER` = 10 min nedesenat, istoricul deja compact e
  serializat (textul, rulările de culori, capetele rândurilor), comprimat (deflate,
  `miniz_oxide`, deja în workspace), criptat (ChaCha20-Poly1305 din `ring`, prin `secrets`) și
  scris într-un fișier; în memorie rămân doar numărătorile și rarele `CellExtra` (diacritice
  combinate, linkuri).
- **Cheia** e una pe rulare, ținută doar în memorie (`secrets::RunKey`, aleatoare): fișierele
  sunt inutile după ieșire, nu e nevoie de keychain și nu apare nicio cerere de parolă.
- **Unde**: `<proiect>/.lumen/terminal-spill/run-<uuid>/<uuid>.bin` (fără proiect: în
  folderul temporar), cu `.gitignore` = `*` în `.lumen/` și în `terminal-spill/`: nu poate
  ajunge într-un commit. Un fișier e șters când istoricul revine, când terminalul se închide;
  folderul rulării e ținut încuiat (`run.lock`); la prima scriere într-un proiect, folderele
  altor rulări neîncuiate (rămase de la o oprire bruscă) sunt șterse.
- **Portul** e în domeniu (`terminal_session::SpillStore`), adaptorul pe disc în modulul
  terminalului (`spill.rs`), `pty` primește doar portul (`TerminalSession::set_spill`) — `pty`
  nu depinde de `secrets` (infra nu folosește infra).
- **Înapoi**: tot pe workerul care pune la loc istoricul parcat (`terminal-park`): citit,
  decriptat, decomprimat, despachetat, pus la loc; ecranul e al grilei între timp.

### Indiciile (coach marks)

- **Contractul** (`workbench_model::hints`):
  - `Hint { id, steps }`, `HintStep { title, body, anchor, actions }` (chei i18n),
    `HintAction { label, command, kind, acknowledges }`;
  - `HintContribution` contribuit de module la pornire;
  - `host.show_hint(id)` (în `ViewHost`, implicit `false`): arată indiciul dacă nu e marcat
    văzut și nu s-a închis deja în rularea asta.
- **Ancorele** (`ui_components::anchors`): `anchors::report(id, rect)` la desen (un vector mic,
  fără alocări pe cadru după primul), `anchors::find(id)`; aplicația raportează tabul activ
  (`editor.active-tab`) și butoanele ferestrelor laterale (`tool-window.<id>`); orice vedere
  poate raporta ale ei.
- **Aspectul** (`ui_components::hint_card`): suprafața popover-elor (`ds::surface_with`,
  umbra „Medium”, colțuri `radius::LG`), o săgeată spre ancoră (dedesubt / deasupra, întoarsă
  când nu e loc), un inel accent în jurul ancorei (spotul), titlul și textul cu `ds::text`,
  butoanele cu `ds::button` în subsol (principalul accent cu `↵`), contorul de pași „2 / 4”,
  × în colț; culorile din paletă.
- **Comportament**: plutește (ca notificările): nu ia tastele editorului, nu blochează nimic,
  un clic în afară nu-l închide și nu-l confirmă. Un clic pe el îl focalizează: atunci ← → /
  Tab mută între butoane, ↵ apasă, Esc îl închide (fără confirmare). „Nu mai arăta” și
  acțiunile cu `acknowledges` îl marchează văzut pentru totdeauna (`hints.seen`, setările
  aplicației, nu ale proiectului). Mai mulți pași: Înapoi / Înainte / Sari peste.
- **Accesibilitate**: cartonașul se descrie prin `ui_kit::access` (un grup „dialog” cu titlul,
  butoanele ca butoane).
- Pagina din ghid: `docs/ui/components/hint-card.md`.

### Primul indiciu

La `RunInTerminal` în editor (o sesiune de agent deschisă) cât timp adormirea e oprită,
terminalul cere `host.show_hint("terminal.sleep")`: „Sesiunile pot adormi” — „Sesiunile
neatinse de o vreme se opresc și se reiau singure când revii la ele, ca să elibereze memoria.”
Butoane: „Nu mai arăta” (stânga), „Setări…” (deschide pagina Terminal), „Pornește ↵”
(pornește setarea). Ancora: tabul activ.

## Pașii

### 100a — Reluarea leneșă la pornire (verificat)

**Criterii.** Niciun proces de agent nu pornește la deschiderea aplicației; un tab de agent își
pornește programul abia la prima desenare.

**Făcut.**

- Verificat: taburile de terminal pornite de alt modul **nu sunt salvate în sesiune**
  (`Viewer::restorable` → `false` pentru un `launch`; tabul virtual al editorului e sărit de
  `current_session`), deci la repornire nu pornește niciun agent — utilizatorul le reia din
  panou, câte unul. Iar orice terminal își pornește programul abia la prima desenare
  (`TerminalView::start` din `paint`): un tab neafișat nu pornește nimic.
- Test nou (`terminal-panel`, `view.rs`): o vedere cu `TerminalStart` și `launch`, fără
  desenare, nu are sesiune și nu e restaurabilă; doar desenată pornește (sesiunea există).

**Lăsat.** Redeschiderea automată a taburilor de agent la pornire nu e făcută (ar fi o
schimbare de comportament, necerută); dacă va fi cerută, mecanismul de mai sus o face deja
leneșă.

### 100b — Memoria fiecărei sesiuni

**Criterii.** Panoul sesiunilor arată, pe rândul unei sesiuni cu tab deschis, memoria ei
(proces + descendenți), citită pe un fir, niciodată la desen; Task Manager-ul numește
procesele terminalelor după tab.

**Teste.** Răspunsul `TerminalUse` pentru o cheie cunoscută / necunoscută; sumarea pe arbore;
panoul păstrează valoarea primită; `process_titles` în raportul terminalului.

**Făcut.**

- `contracts::terminal::TerminalUse { key }` → `Option<TerminalState { memory, asleep }>`,
  răspuns de terminal din slotul tabului (`view::state`), niciodată citit la desen.
- Terminalul: firul `terminal-watch` (`sleep.rs`) măsoară la 10 s arborele de procese al
  fiecărui terminal cu cheie (`process_stats::ProcessTrees`, ca Task Manager-ul), **doar cât
  cineva a cerut în ultimele 30 s** (`memory_asked`); rezultatul stă în slot.
- `agent-sessions`: la poll-ul ferestrei, la 5 s și numai cât panoul a fost desenat în ultimele
  30 s (`follow::panel_shown`), întreabă `TerminalUse` pentru fiecare tab urmărit; rândul
  sesiunii desenează memoria (muted, `i18n::bytes`) sau o lună când doarme, cu tooltip
  (`agents.memory`, `agents.asleep`).
- Task Manager: `ResourceUse::process_titles` (nou, doar afișat, niciodată înregistrat de
  profiler) — rândul procesului începe cu titlul tabului („Fix the login form · agent
  (2.1.x) · proiect · pid…”); detaliul părții Terminal numără sesiunile adormite
  (`tasks.terminal_asleep`).
- Teste: `terminal-panel` (`a_keyed_terminal_tells_what_it_holds_and_its_title`),
  `agent-sessions` (`a_quiet_followed_session_says_how_it_resumes`: și starea ținută),
  `task-manager` (`a_program_s_row_says_its_tab_first`, titlurile în cadru).

**Lăsat.**

- Sesiunile mutate în terminal din fața grafică (`agent-chat`) nu sunt urmărite de panou: nu
  au memorie pe rând și nu adorm (nimeni nu răspunde la `ResumeLaunch` pentru ele) — prudent.

### 100c — Sesiunile adormite

**Criterii.** Cu setarea oprită nu se schimbă nimic. Pornită: un terminal cu cheie, ascuns,
tăcut, fără text netrimis, cu răspuns la `ResumeLaunch`, e oprit; tabul rămâne cu ultimul
ecran; arătat, pornește reluarea. Nu doarme: vizibil, cu ieșire recentă, cu text tastat, fără
răspuns, cu programul ieșit.

**Teste.** Decizia (pură, pe stări date) pentru fiecare condiție; ordinul luat de vedere (fără
sesiune după, `launch` înlocuit, ecranul păstrat); setările (implicit oprit, 1 h, valorile
valide); răspunsul `agent-sessions` (id cunoscut → `Launch` de reluare; activ de curând →
`None`).

**Făcut.**

- Setările (`terminal/src/settings.rs`): `SLEEP_IDLE` (implicit oprit), `SLEEP_AFTER`
  (minute: 15, 30, 60, 120, 240; implicit 60), blocul „Sesiuni adormite” (`SleepSettings`:
  comutatorul, apoi un `Dropdown` cu timpul cât e pornit), cuvintele de căutare în ambele
  limbi; `TerminalConfig::sleep_after`; comanda `terminal.sleep_on` (`enable_sleep`).
- Starea unui terminal cu cheie stă în slotul lui (`view/shells.rs::Asked`): proiectul, când a
  fost desenat ultima oară, text tastat netrimis (↵ / Ctrl+C îl golesc), `SessionWatch`,
  pid-ul, adormit, ordinul de adormire (cu momentul lui), memoria, titlul.
- `pty`: cititorul notează momentul ultimei ieșiri (`Shared::wrote`, un atomic la fiecare
  `Wakeup`); `SessionWatch::quiet_for` (nou, implicit `None`).
- `sleep.rs`: decizia pură `sleepy` (toate condițiile din „Ce înseamnă «nu lucrează»”), firul
  care trezește doar ferestrele proiectelor cu ceva de adormit (`DUE` pe proiect), `Sleeper`
  (un `ProjectService`) care întreabă `ResumeLaunch` și lasă ordinul; vederea îl ia la poll
  doar dacă n-a fost desenată de atunci (`fall_asleep`): sesiunea renunțată, ultimul ecran
  păstrat, luna în tab (`tab_icon`), `running` fals (redenumirile așteptate se scriu).
- Trezirea: prima desenare pornește `Launch`-ul de reluare; până se liniștește ieșirea, ultimul
  ecran rămâne desenat cu banda „Se reia sesiunea…” (`terminal.resuming`).
- `agent-sessions` răspunde `ResumeLaunch` din `follow` (id-ul știut, fișierul nescris de cel
  puțin timpul ales: `Followed::active`, citit de firul `agent-follow`), prin
  `AgentProvider::resume`.
- Teste: `sleep.rs` (fiecare condiție), `view.rs` (adormit cu ecranul păstrat; reluat la
  desen, cu un program real `/bin/sh` pe Unix; textul netrimis; ordinul vechi lăsat dacă tabul
  a fost arătat între timp), `settings.rs` (implicit oprit, timpii, valorile refuzate),
  `follow.rs` (răspunsul de reluare).

**După review (2026-11-25).**

- „Văzut” nu mai vine din momentul ultimei desenări (aplicația desenează doar la schimbări: un
  tab vizibil și tăcut părea nevăzut): `Viewer::set_shown`, spus de aplicație, ține
  `Asked::visible`; un terminal pe ecran nu doarme niciodată, iar timpul nevăzut curge de la
  ascundere. Un ordin dat cât tabul e vizibil e lăsat.
- La trezire, noua sesiune primește iar opțiunile (istoricul, cursorul) și culorile
  (`options` / `answered` golite la adormire și la fiecare sesiune nouă).
- ⇧↵ / ⌥↵ nu mai golesc „text netrimis” (doar adaugă un rând la ciornă); doar ↵ simplu și
  Ctrl+C.
- Banda „sesiunea s-a încheiat” și banda „se reia” au un singur desen (`paint_band`).
- Teste noi: `a_tab_on_screen_never_sleeps`, `sleepy` cu `visible`, ⇧↵ / ⌥↵, opțiunile goale
  după adormire și trimise iar la trezire.

**Lăsat.**

- Un tab adormit n-are tooltip propriu (contractul `Viewer` nu are încă unul pentru tab): se
  vede luna pe tab, iar panoul sesiunilor spune textul.
- Terminalele cu cheie din fereastra de jos nu sunt întrebate de vederi ascunse (nu sunt
  „poll”-uite): un ordin pentru ele așteaptă până sunt arătate și e lăsat atunci (nu adorm).

### 100d — Istoricul pe disc, criptat

**Criterii.** Un terminal nedesenat de 10 min cu istoric parcat îl are pe disc, criptat;
desenat, istoricul revine identic; fișierul e șters la revenire și la închidere; fără
proiect, folderul temporar; `.gitignore` scris.

**Teste.** Serializarea dus-întors (toate felurile de celule din `styled_output`); sigilarea cu
cheia rulării (alt AAD / octet schimbat → refuzat); parcare → scoatere pe disc → punere la loc
= identic; curățenia folderelor rămase; măsurătoarea (mai jos).

**Făcut.**

- Port în domeniu: `terminal_session::SpillStore` (`write` → `SpillToken`, `take`, `remove`)
  și `TerminalSession::set_spill` (implicit nimic); `pty` îl primește în `Parking`.
- `pty::parked`: `Parked::encode` / `decode` (format `PRK1`: textul, rulările cu culorile —
  numite printr-un tabel, RGB, indexate — și atributele, capetele rândurilor; în memorie rămân
  `Kept`: lățimea, rândurile lăsate, `CellExtra`-urile rare). Decodarea verifică totul
  (UTF-8, limitele, ordinea): ce nu se potrivește e refuzat, nu citit greșit.
- `pty::parking`: după `SPILL_AFTER` = 10 min nedesenat, nederulat înapoi și cu cel puțin 256
  de rânduri nou parcate, istoricul parcat pleacă în store; rândurile parcate mai târziu au
  fișierul lor (mai noi). Înapoi: pe workerul de la afișare (`restore(false)` refuză când e
  ceva pe disc — un cadru nu citește fișiere), în ordine; ștergerea (`ESC[3J`, `ESC c`, Clear
  Terminal), o limită mai mică, închiderea terminalului lasă fișierele (pe un worker).
- `secrets::RunKey`: cheie aleatoare doar în memorie, pe rulare (ChaCha20-Poly1305 din `ring`,
  nonce nou, AAD = numele fișierului).
- `terminal-panel::spill` (`DiskSpill`): deflate (`miniz_oxide`, nivel 1) → sigilat →
  `files::write_private` în `<proiect>/.lumen/terminal-spill/run-<uuid>/<uuid>.bin` (fără
  proiect: folderul temporar); `.gitignore` = `*` în `.lumen/` și în `terminal-spill/`;
  `run.lock` ținut încuiat; la prima scriere, folderele altor rulări neîncuiate sunt șterse.
  Rădăcina proiectului vine prin `TerminalStart::root` (nou).
- Teste: `pty` (dus-întors identic, mai noi în ordine, ștergere / uitare / închidere → fișiere
  lăsate, store care refuză → rămâne în memorie, nu cât e desenat sau derulat înapoi),
  `secrets` (cheia), `terminal-panel` (fișier ilizibil și comprimat, citit o dată, curățenie).

**După review (2026-11-25).**

- Fără I/O sub lacăt: la scriere, rândurile sunt scoase sub lacăt, codate și scrise fără el,
  apoi tokenul pus la loc; la citire la fel. `Store::moving` le numără cât sunt pe drum,
  `Store::generation` spune ce s-a lăsat între timp (șters, uitat: ce sosește e lăsat).
- Peste limită nu se mai citește nimic: `trim` lasă cele mai vechi după numărul lor — fișiere
  întregi șterse (pe worker), cel mai vechi rămas tăiat la citire (`Spilled::skip`).
- `set_limit` (spus la desen) doar notează limita și taie dacă istoricul e liber
  (`try_lock`), altfel firul de parcare taie la următoarea privire; `forget` doar mută
  rândurile sub lacăt, fișierele se șterg pe worker. Nimic de pe firul UI nu citește fișiere.
- La parcare, lungimea numărată include rândurile de pe disc și pe drum (`counting`).
- Folderul: unul pe store (`run-<uuid>` per rădăcină și deschidere), șters când store-ul e
  lăsat (proiectul închis: `spill::release` din `Subscription::on_end` al `Sleeper`, apoi
  `Drop` după ce terminalele lui se închid). Curățenia șterge doar folderele cu `run.lock`
  existent, liber și mai vechi de `LEFT_AFTER` = 60 s (unul abia făcut de altă instanță nu e
  atins); un folder care n-a putut fi făcut e reîncercat după `RETRY_AFTER` = 60 s.
- Teste noi: `over_the_limit_the_oldest_rows_go_unread` (nicio citire, cele mai noi 1000 în
  ordine), `the_ui_thread_never_reads_rows_out_of_memory`,
  `the_count_while_parking_includes_rows_out_of_memory`, curățenia (ținut / abia făcut / fără
  lacăt rămân), reîncercarea și ștergerea la `Drop`, `release`.
- Măsurat din nou: aceleași cifre (citit și pus la loc ~7,5 ms).

**Lăsat.**

- Windows: compilat (clippy), nerulat.

### 100e — Sistemul de indicii

**Criterii.** Un modul contribuie un indiciu și cere afișarea lui; aplicația îl arată lângă
ancoră (întors când nu e loc), cu butoanele lui; „văzut” persistă; un clic în afară nu-l
închide; Esc doar când e focalizat; mai mulți pași cu Înapoi / Înainte.

**Teste.** Așezarea (dedesubt / deasupra / fără ancoră, în fereastră); starea (pași, văzut,
închis în rulare); slotul aplicației (nu ia tastele nefocalizat, Esc focalizat, clic în afară);
persistența prin setare.

**Făcut.**

- `workbench_model::hints`: `Hint`, `HintStep`, `HintAction` (`primary` / `secondary`,
  `acknowledges`), `HintContribution`, `HintTour` (pasul, butoanele în ordine: „Nu mai arăta”,
  Înapoi, acțiunile, Înainte / Gata; focusul tastelor; ↵ pe cel principal), `HintBook` (văzute
  / închise în rulare), `ANCHOR_ACTIVE_TAB`; `ViewHost::show_hint` (implicit `false`).
- `ui_components::anchors` (`begin` / `report` / `find`, o listă pe firul de desen, refolosită)
  și `ui_components::hint_card` (`place` pur, `HintCard`: suprafața popover-elor, crestătura,
  inelul în accent pe ancoră, titlul, textul împachetat o dată, contorul, ×, subsolul cu
  butoanele `ds::button`, inelul de focus; `Dialog` pentru cititoarele de ecran). Paginile
  `docs/ui/components/hint-card.md` și `anchors.md`, legate din index; rândul din
  `.claude/rules/ui-components.md`.
- Aplicația: `Overlay::Hint` (sub notificări, peste fereastra barei de stare), `HintSlot`:
  plutește cât nu e focalizat; clic pe el → tastele (← → / Tab, ↵, Esc); apăsare în altă
  parte → tastele înapoi, indiciul rămâne; × / Esc → închis pentru rulare; acțiune / „Nu mai
  arăta” → `[hints] seen` (`settings::HINTS_SEEN`, setările aplicației). Contribuțiile în
  `LanguageSupport::hints` (ascunse cu modulul). Tabul activ raportat ca ancoră la fiecare
  cadru.
- Teste: `workbench-model` (butoane, tur, carte), `ui-components` (așezare în patru cazuri,
  ancore, lovituri după desen), `app` (plutește sub tab, clic în afară, focus, Esc pentru
  rulare; ↵ rulează comanda și ține minte; „Nu mai arăta”; ordinea și felul în stivă).

**Reproiectat (2026-11-25, aprobat).** Antet cu iconița într-un cerc în accent (nou:
`HintStep::icon`; terminalul: luna), o etichetă mică în accent („SFAT”, `HintStep::eyebrow`,
implicit `hints.tip`), titlul sub ea; textul scurtat, aliniat sub titlu; locul din setări pe o
linie a lui, cu o rotiță (`HintStep::place`: „Setări › Terminal › Sesiuni adormite”); un
separator; un singur rând de subsol — „Nu mai arăta” în stânga, butoanele în dreapta, compacte
(`ButtonSpec::compact`, nou în `ds::button`: cât eticheta, fără minimul dialogurilor); lățimea
312, spațieri din `ds::space`. Cardul adormit ia același cerc pentru lună. Teste: un rând de
subsol când încape, butoane compacte, linia locului, cele două rânduri doar când nu încap.

**Lăsat.**

- Ancore raportate azi: doar tabul activ. Butoanele ferestrelor laterale și ale barei de sus
  le pot raporta la fel (`anchors::report`) când un tur va avea nevoie.
- Un al doilea indiciu cerut cât unul e arătat e lăsat (nu e pus la coadă).
- Indiciul primește tastatura doar printr-un clic pe el (nicio comandă cu scurtătură încă).
- Neverificat vizual cu `--snapshot` (regula: doar la cerere).

### 100f — Primul indiciu: adormirea sesiunilor

**Criterii.** Deschiderea unei sesiuni de agent cu adormirea oprită cere indiciul; „Pornește”
pornește setarea și îl marchează văzut; „Setări…” deschide pagina Terminal; cu setarea pornită
nu se mai cere.

**Făcut.**

- Terminalul contribuie `terminal.sleep` (ancora: tabul activ; „Setări…” → `terminal.settings`,
  „Pornește ↵” → `terminal.sleep_on`, ambele îl marchează văzut) și îl cere la `RunInTerminal`
  în editor cât `sleep_after` e `None`. Textele în `[terminal]` și `[hints]`, ro și en.
- Test `app` (`a_program_runs_in_a_terminal_tab_of_the_editor`): deschiderea sesiunii arată
  indiciul; testele de indicii folosesc chiar acest indiciu (↵ → `terminal.sleep_on`).

**Lăsat.** Nimic.

### 100g — Adormită de mână, trezită de mână (cerut 2026-11-25)

Cerut: „click dreapta sau ceva ca să pun eu tabul în modul ăla de suspend”; apoi: ce se vede
pe un tab adormit (cum îl trezești) și „Trezește” în locul lui „Adoarme” în aceleași meniuri.

**Criterii.** „Adoarme” în meniul tabului unei sesiuni care poate fi reluată și în meniul
rândului din panoul sesiunilor, ca setarea să fie pornită sau oprită; dezactivat, cu motivul,
cât lucrează sau are text netrimis; ascuns când nimeni nu știe s-o reia. Adormită pe ecran,
rămâne adormită până la o tastă, un clic sau revenirea pe tab. Adormită: un card cu luna, „Sesiune
adormită”, de ce și cât a eliberat, butonul „Trezește ↵”; „Trezește” în meniuri și ca
comandă.

**Făcut.**

- Contracte (`contracts::terminal`): `SleepTerminal { key }` → `bool`, `WakeTerminal { key }`
  → `bool` (trezește și aduce tabul în față: `open_view` pe cheia lui, ca `RunInTerminal`),
  `CanSleep` (`No`, `Working`, `Typed`, `Yes`) în `TerminalState`.
- `Viewer::tab_menu` (nou, `ui-components`): rândurile pe care o vedere le adaugă meniului
  tabului ei, citite din memorie; aplicația le pune în meniul tabului
  (`Workbench::tab_view_menu`, prefixul `tab-view-menu:<doc>:`) și le trimite înapoi acelei
  vederi, oricare tab e activ (`run_tab_view_menu`).
- Terminalul: slotul ține `resume` (cum se reia, întrebat la 5 s de `Sleeper::poll` prin
  `ResumeLaunch` cu `idle` zero — meniurile citesc doar asta), `can_sleep` (lucrează = a scris
  în ultimele 3 s — scos apoi, vezi mai jos), `by_hand`, `wake`, `in_editor`. Rândul tabului:
  „Adoarme” (dezactivat cu motivul în etichetă), sau „Trezește” cât doarme. Comenzile
  `terminal.sleep_now` („Adoarme sesiunea”) și `terminal.wake_now` („Trezește sesiunea”), active
  doar după aceeași regulă, pentru terminalul cu tastatura.
- Adormită de mână (`held`): nu pornește la desenare; o tastă (și ↵), un clic, butonul cardului
  sau tabul arătat din nou după ce a fost părăsit o trezesc. Adormirea automată rămâne cum era
  (se reia la prima afișare).
- Cardul (`view/asleep.rs`): peste ultimul ecran, din tokenii temei (`ds::surface`, accentul,
  textele muted), luna, „Sesiune adormită”, de ce („Adormită de mână” / „după N min nevăzută
  și inactivă”) și „circa X eliberați” (memoria măsurată înainte), butonul `ds::button`
  principal „Trezește ↵”, „sau apasă orice tastă ori dă clic în ea”; descris ca grup. La
  reluarea unei sesiuni adormite automat, același card spune „Se reia sesiunea…”, fără buton
  (înlocuiește banda de dinainte).
- Panoul sesiunilor: rândul „Adoarme” / „Trezește” după starea spusă de terminal, ascuns fără
  cine să răspundă (`presence`); cererile prin `ViewerAction::Ask` (fără răspuns: nimic).
- Texte în ro și en (`[terminal]`, `[agents]`, `[command]`).
- Teste: `terminal-panel` (rândul după stare: ascuns / dezactivat cu motiv / activ / Trezește;
  adormită pe ecran rămâne după desenări, o tastă / un clic / revenirea o trezesc; cererea din
  panou; cardul cu butonul și memoria eliberată, butonul și ↵ trezesc), `agent-sessions`
  (rândul din panou după stare), `app` (fără cine să reia: niciun rând, comanda oprită, un rând
  cerut oricum e refuzat).

**După review 2 (2026-11-25).**

- `pty`: firul UI (redimensionare, derulare, „la capăt”) nu mai citește niciodată fișiere:
  `Parking::restore_now` pune la loc ce e în memorie, iar ce e pe disc vine pe worker, după
  (mai vechi: deasupra). O punere la loc așteaptă rândurile aflate pe drum (scriere în curs),
  ca să nu ajungă mai vechi peste mai noi; o limită mai mică spusă între timp le taie pe cele
  de pe drum (`moving_skip`), nu pe cele mai noi.
- Vederile ferestrei de jos primesc `set_shown` ca taburile (cea activă a fiecărui grup al
  ferestrei arătate): un terminal de sesiune vizibil acolo nu mai adoarme.
- Comenzile Adoarme / Trezește urmează tastatura: un shell simplu cu tastatura, sau
  terminalul desenat fără ea, golește ținta.
- Cum se reia e întrebat pe ceas propriu pentru fiecare proiect (altă fereastră nu-l mai
  amână).
- O reluare care nu pornește (programul lipsă) nu mai lasă tabul „adormit”: arată eroarea.
- Teste noi: ordinea în jurul unei scrieri în zbor, limita în timpul ei, firul UI fără citiri,
  vederile de jos spuse, ținta tastaturii, două proiecte, reluarea eșuată.

**După review 3 (2026-11-25).**

- Un store care n-a putut scrie nu mai primește istoricul codat la fiecare 15 s:
  `SpillStore::ready()` (ieftin, întrebat înainte de codare; `DiskSpill`: nu cât folderul a
  eșuat de curând) și o pauză în parcare după o scriere eșuată (`SPILL_RETRY` = 60 s).
- O scriere și o citire nu se mai pot încrucișa: amândouă hotărăsc sub lacătul istoricului
  (nimic cât `moving` > 0).
- `Overlay::Hint` stă sub meniuri, liste, dialoguri și modale (deasupra cardurilor
  editorului); ascuns cât e deschisă o modală, revine după; ce e deasupra lui ia clicul.
- Trezirea unei sesiuni din fereastra de jos îi arată tabul acolo (`show_in_panel` pe numele
  ei) ca să fie desenată și pornită; dacă nu poate fi arătat, răspunsul e `false`.
- `Viewer::set_active` (nou): tabul ales, spus separat de `set_shown` (fereastra în fundal,
  o modală deasupra). O sesiune adormită de mână se trezește doar când tabul ei e ales din
  nou, nu când fereastra revine din fundal sau se închide o modală.
- Indiciul arată spre tabul sesiunii (ancore pe nume: `anchors::report_keyed` /
  `find_keyed`, `ANCHOR_TAB`, `HintAnchor::Tab`, `host.show_hint_by`), nu spre tabul activ.
- Mărunțișuri: `sync_shown` nu mai copiază nimic pe cadru; textul cardului nu mai e alocat
  la fiecare desenare; valorile cardului adormit și ale rândului din panou sunt constante
  numite; exemplul din 100b fără numele unui program anume.
- Teste noi: store eșuat lăsat în pace, scriere/citire neîncrucișate (ambele sensuri), indiciul
  sub meniu și modală, trezirea arată tabul jos / în editor, ales vs. văzut (app și terminal),
  ancorele pe nume, indiciul pe tabul propriu după schimbarea tabului.
- Snapshot-urile n-au mai fost refăcute: nimic vizibil nu s-a schimbat (indiciul din ele
  folosește ancora tabului activ, cardurile au aceleași valori).

**Cardul adormit transparent (văzut în aplicație, 2026-11-25).** Un strat își desenează
dreptunghiurile înaintea glifelor: cardul, desenat în stratul grilei, lăsa textul
terminalului să se vadă prin el. Acum cardul (și banda „s-a încheiat”) sunt într-un strat al
lor, peste textul grilei, cu suprafața popover-elor opacă și un văl (fundalul terminalului,
`SCRIM_ALPHA` = 0,78) peste ultimul ecran; cardul indiciului are și el suprafața opacă. Test:
stratul cardului vine după cel cu textul grilei. Snapshot-ul `SESSION_TAB` are acum o
transcriere care umple ecranul.

**După review 4 (2026-11-25).**

- Ordinele către un terminal nevăzut (adormire, trezire) nu mai așteaptă desenarea lui:
  `Viewer::wants_poll` (nou) — aplicația îi face „poll” și unei vederi nevăzute a ferestrei de
  jos care are ceva de luat. Un ordin mai vechi de `SLEEP_ORDER_FRESH` (20 s) e lăsat, niciodată
  aplicat târziu.
- Parcarea ține minte locul derulării și când rânduri mai vechi sunt pe disc.
- Ferestrele fără proiect sunt trezite și ele pentru terminalele fără proiect
  (`ProjectService::window_opened`).
- Ștergerea folderului unui store, pe un worker; folderul făcut fără lacătul `ready` ținut;
  ștergerea fișierelor pornită după ce lacătul istoricului e eliberat (`Store::gone`).
- Firul `terminal-watch` rulează doar cât e nevoie (adormirea pornită sau memoria cerută de
  curând), pornit iar la o schimbare a setării sau la o cerere.
- Eticheta memoriei din panou, formatată și formată o dată pe valoare; `MIN_WIDTH` numit în card.
- Teste noi: ordin vechi lăsat + `wants_poll`, vedere nevăzută cu ordin „poll”-uită (app), locul
  derulării cu rânduri pe disc, fereastră fără proiect trezită, firul doar cât e nevoie.

**„Trebuie să apăs de mai multe ori pe Adoarme” (văzut în aplicație, 2026-11-25).** Cauze:
(1) regula „lucrează” (a scris în ultimele 3 s): programele agenților își redesenează des un
rând de stare, deci la clic ordinul era refuzat în tăcere, deși meniul îl arătase activ;
(2) un ordin dat din panou doar se nota în slot, fără să trezească fereastra — aștepta un poll
oarecare; (3) cât prima ieșire a unei sesiuni se „așeza”, poll-ul vederii ieșea înainte să ia
ordinul. Acum: de mână, doar textul tastat și netrimis oprește (`CanSleep::Working` scos;
utilizatorul a ales); un refuz spune de ce (notificare, `terminal.cant_sleep*`); slotul ține
waker-ul ferestrei (de la desenare), iar un ordin din afară o trezește; ordinele se iau
înaintea „așezării”. Un clic = adormită în același cadru.

**Ecranul adormit stă pe loc.** Cât e adormită sau se reia, rotița nu derulează și nu ajunge la
program, nu se selectează nimic, nu sunt linkuri, bara de derulare nu se arată, cursorul e
săgeata (mâna pe butonul „Trezește”); trezită, pornește de la capătul de jos și totul merge
iar. Teste: un clic pe o sesiune care tot scrie o adoarme, refuzul spune de ce, cererea din
afară trezește fereastra (și cât se așază), ecranul adormit stă pe loc și apoi derulează iar.

**După review 5 (2026-11-25).**

- „Trezește” din meniul unui tab din spate îl aduce în față (`Viewer::tab_row_activates`,
  generic: aplicația activează tabul rândului dacă vederea o cere), apoi îl trezește.
- Un tab din spate al cărui icon se schimbă la poll (adormit → lună) e redesenat.
- „Tastat și netrimis” e o estimare: ⌫ scade, Esc / Ctrl+U / Ctrl+C / ↵ golesc, un paste care se
  termină cu rând nou e trimis.
- Trezirea din afară arată întâi tabul, apoi trezește (nearătat: lăsat cum era).
- La închiderea unui proiect nu rămâne nimic al lui în cozile terminalului.
- `.gitignore`-ul folderului de date scris prin `files::ignore::let_through` (rândurile `!`
  ale altor module păstrate); timpul implicit numit (`DEFAULT_SLEEP_AFTER`).
- Tabul unui indiciu raportat doar cât indiciul e arătat, căutat în toate grupurile.
- Teste noi pentru fiecare.

**Văzut (snapshot-uri, 2026-11-25).** Cârlige noi: `HINT=<id>` (+ `HINT_FOCUS=1`) și
`SESSION_TAB=<titlu>` cu `VIEW_COMMAND=snapshot:asleep` (adormit de mână, 412 MB „eliberați”
inventați). Reparat după ce s-au văzut: subsolul indiciului se suprapunea („Nu mai arăta”
peste „Setări…”) — acum trece pe un rând al lui când nu încap; crestătura are marginea
cardului (se vedea doar pe temă închisă); cardul adormit pune „sau apasă orice tastă…” sub
buton; ultimul ecran al unei sesiuni adormite nu mai arată cursorul.

**Lăsat.**

- „Lucrează” se citește din ieșirea programului în ultimele 3 s: o unealtă tăcută în mijlocul
  unei ture ar putea părea liberă (adormirea de mână e a utilizatorului; cea automată cere și
  fișierul sesiunii nescris).
- Comenzile acționează asupra terminalului cu cheie desenat ultima oară cu tastatura.

### 100h — Memoria chiar eliberată, meniurile la zi (cerut 2026-11-25)

Cerut: „chiar se eliberează spațiul? am pus pe sleep o sesiune și tot 518 văd”; apoi: „până
intră pe sleep, la click dreapta îmi zice tot să pun pe sleep… mai sunt momente când dispar
de tot opțiunile… să fie tot timpul, dar dezactivate cât e deja un proces” — decuplat, cu
evenimente.

**Făcut.**

- Găsit: programul agentului nu ieșea la închiderea PTY-ului (prinde `SIGHUP` și continuă);
  cititorul terminalului aștepta (`wait4`) la nesfârșit, PTY-ul și ~280 MB rămâneau.
  `pty::ending` (Unix): la renunțarea la o sesiune ia programul și arborele lui, îi dă 2 s să
  iasă singur, apoi `SIGTERM` și, după 3 s, `SIGKILL` — pe un fir al lui. Nu atinge un program
  ieșit deja sau un id refolosit (pornirea trebuie să cadă între cele două citiri ale ceasului
  din jurul lansării). Pe Windows închiderea consolei oprește ce e atașat.
- `CanSleep` are `Sleeping` și `Waking`: rândurile „Adoarme” / „Trezește” rămân în meniuri,
  dezactivate („Adoarme…”, „Se trezește…”) cât ține drumul, apoi cea potrivită activă.
- Evenimentul `contracts::terminal::TerminalStateChanged { key, state }` pe magistrala
  proiectului (`None`: tabul s-a închis): terminalul îl publică la fiecare schimbare (ordin,
  adormit, pornit, liniștit, eșuat, măsurat, ordin expirat, cerere refuzată), o dată pe
  schimbare, fără lock ținut; panoul sesiunilor îl ascultă și se redesenează (fiecare proiect
  al lui). Modulele rămân fără să se știe.
- Ecranul de start: cardul mai mare (640×720 cel mult); implicit 6 sesiuni păstrate și arătate
  (setarea rămâne a utilizatorului). Între memoria unui rând și steaua lui, 6 px.
- Teste pentru fiecare (programul care ignoră hangup-ul oprit, id refolosit neatins, meniuri
  dezactivate pe drum, evenimentele publicate și ascultate per proiect).

**Lăsat.** Memoria din card („circa N eliberați”) e cea măsurată înainte de adormire, nu una
citită după.

### 100i — Adormită până o trezești, tab-uri cu identitate, fără flash (cerut 2026-11-26)

Cerut: schimbatul tab-urilor nu trezește sesiunea; „Adoarme” mereu în meniu, dezactivat cât
nu se poate; un indicator de sesiuni deschise / adormite în panou; tabul unui indiciu colorat
(nu încadrat), animat discret; tooltip-urile niciodată sub cursor; copierea din terminal cu tot
istoricul; un flash negru la revenirea în aplicație; „să aibă tabul un uuid în spate”.

**Făcut.**

- Adormită (de mână sau după inactivitate) rămâne așa până la o tastă, un clic în ea sau
  „Trezește”; un clic pe sesiunea ei în panou o aduce în față fără s-o trezească (datele de
  pornire nu se mai reaplică unui terminal cu cheie).
- `CanSleep::Starting`: „Adoarme (sesiunea abia pornește)”, dezactivat, cât cineva răspunde
  la `ResumeLaunch` și cel mult 10 s de la pornire; atunci terminalul întreabă la 1 s, altfel
  la 5 s. `TerminalState::running`: punctul verde din panou (luna, adormită).
- Fiecare tab are un UUID v4 (`Tab::id`): nuanța și indiciul îl țin după el; modulele numesc
  tabul cum l-au deschis (`ViewTab { title, extension }`), aplicația îl rezolvă o dată.
  `WorkbenchHost::tint_tab` colorează tabul în tonul temei, cu `anim::Attention` (apare,
  pulsează de două ori, stă fără cadre, dispare); indiciul se ascunde cât tabul nu se vede și
  se renunță la el (nenumărat ca închis) când tabul se închide.
- Tooltip-urile: săgeata la stânga cursorului, cardul sub corpul lui sau deasupra elementului;
  ce se decupează peste pagini e forma desenată.
- Copierea din terminal: din tot istoricul (rândurile împărțite de terminal, unite; mărcile
  combinate cu litera lor; rândurile venite între timp, găsite unde au ajuns).
- Flash-ul negru: stratul Metal era lăsat la 1×1 după ce își dădea înapoi imaginile de rezervă
  (fereastra nefolosită), iar macOS îl arăta așa la activare. Acum e readus la mărimea
  ferestrei pe loc: rezerva tot se golește. Măsurat (instanță izolată, `footprint`):
  IOSurface 31 MB → 16 MB după plecarea din aplicație, ca înainte; fără eliberare rămânea 31 MB.
- Ecranul de start: secțiunile „Sesiuni anterioare” și „Toate proiectele” se pliază din titlu.

**Lăsat.** Cele trei rămase aici sunt făcute la 100j.

### 100j — Ce rămăsese de la 100i (cerut 2026-11-26)

**Făcut.**

- **Copierea din terminal cât emulatorul e ocupat**: o selecție care iese din ecran își are
  textul întreg făcut în aceeași privire la emulator ca instantaneul ecranului
  (`TerminalSession::try_snapshot_selecting`, o singură încercare, niciodată o așteptare;
  `view::selection::ReadyCopy`), deci e chiar textul evidențiat. Fiecare instantaneu are o
  amprentă (`GridSnapshot::fingerprint`: caracterele vizibile, lungimea istoricului,
  derularea, mărimea; ieșirea care mută istoricul mută și ecranul, chiar cu istoricul plin):
  cu aceeași amprentă textul nu se mai face (o redesenare fără nimic nou nu costă), iar la
  copiere e folosit doar dacă amprenta ecranului arătat e a lui. Nu se face cât selecția e
  trasă cu mouse-ul, pentru una aflată toată pe ecran (ecranul o are întreagă) sau pentru
  una de peste 1000 de rânduri (`COPY_READY_MOST`: ar fi construită la fiecare cadru cât
  programul scrie). Copierea încearcă întâi textul proaspăt (~20 ms), apoi pe cel pregătit,
  abia apoi partea de pe ecran.
- **Plierea secțiunilor ecranului de start**: ținută în setarea `[home] folded` (`sessions`,
  `projects`), aceeași în ambele liste (ecranul de start și fereastra din bara de sus), citită
  o dată la primul desen. La scriere se recitește întâi lista salvată, ca să rămână ce au pliat
  alte ferestre. Titlurile secțiunilor se pot alege cu ↑ ↓ (lista pornește tot pe un rând,
  nu pe un titlu, iar selecția rămâne pe un titlu doar dacă tastele l-au ales, niciodată
  pentru că rândurile s-au mutat sub ea): ↵ pliază sau deschide secțiunea, ← o pliază,
  → o deschide. Cititorul de ecran le aude ca butoane, deschise sau pliate.
- **Ritmul de 1 s pentru o sesiune care pornește**: un fir mic (`terminal-start`) trezește
  fereastra proiectului la fiecare secundă cât terminalul e „pornind”. Se oprește singur după
  cel mult 10 s (`STARTING_WITHIN`), când se află cum se reia sesiunea sau când tabul s-a
  închis. Fereastra nu mai depinde de alte treziri. Fiindcă sondajul ferestrei vine puțin
  după trezire, o întrebare cu până la 200 ms mai devreme de secundă e primită
  (`RESUMES_SLACK`), altfel una din două ar fi sărită.

**Teste.** Copierea cu emulatorul ocupat ia selecția întreagă (61 de rânduri, nu cele 11 de pe
ecran); o selecție nepregătită ia ecranul; una toată pe ecran sau trasă încă nu păstrează
nimic. Tastele ajung la titlul unei secțiuni (↑ din primul ei rând, ↵ o pliază, cheile
salvate). Terminalele „pornind” sunt urmărite doar cât pornesc: cele aflate, expirate sau
închise sunt lăsate, iar proiectul e numit o singură dată.

**Revizie** (agent separat): textul pregătit era legat de lungimea istoricului, care nu mai
crește când istoricul e plin (rămânea vechi): legat acum de instantaneu. Banda titlului ales
intra sub rândul următor: oprită unde începe el. Selecția putea ajunge singură pe un titlu:
doar tastele o duc acolo.

**A doua revizie** (alt agent): textul pregătit se refăcea la fiecare desenare și era citit
într-o a doua privire la emulator (putea fi decalat față de ce e evidențiat): acum în aceeași
privire, păstrat după amprentă. Pasul de 1 s putea deveni 2 s: toleranța de mai sus. Titlurile
spuse cititorului de ecran; alegerea unui titlu uitată odată cu selecția.

**Lăsat.** Setarea pliată nu se sincronizează în timp real între ferestre deja deschise
(fiecare o citește o dată; o pliere într-una nu o strică pe a celeilalte la salvare). O
selecție de peste 1000 de rânduri copiată exact când emulatorul e ocupat ia tot doar ecranul.
↵ / ← / → pe un titlu nu sunt testate prin `Workbench` (doar lista însăși), nici citirea și
scrierea setării. Amprenta nu vede ieșirea care mută istoricul plin cu rânduri identice celor
de pe ecran (același text arătat): textul pregătit rămâne cel de dinainte.

### 100k — Terminalele închise chiar se închid (cerut 2026-11-27)

O instanță pornită de 5 zile ținea ~970 MB după ce proiectele ei fuseseră închise: 22 de
terminale rămăseseră deschise (cititorul fiecăruia oprit în `wait4`), cu agenții lor vii sau
blocați „la ieșire” (starea `E`, fără memorie), plus două shell-uri la fel.

**Cauza:** la eliberarea PTY-ului, biblioteca emulatorului trimite hangup programului și îl
așteaptă, cu PTY-ul încă deschis și necitit. Un program care avea de scris (un shell la
hangup, un agent oprit de 100i) nu poate ieși până nu i se citește ce a scris: așteptarea nu
se mai termină, terminalul și memoria lui rămân. Oprirea forțată (`ending`) nu ajută: un
proces omorât rămâne și el blocat la ieșire.

**Făcut:** `drain` — la pornire se ia o a doua copie a PTY-ului; când terminalul e eliberat
(abia atunci: unul viu nu pierde nimic), un fir `pty-drain` citește și aruncă ce a rămas cât
timp biblioteca așteaptă programul, apoi se oprește (sau când PTY-ul nu mai dă nimic).
Windows are aceeași așteptare altfel: închiderea consolei pseudo așteaptă să i se citească ce
mai scrie, iar cititorul bibliotecii are un tampon fix (1 MB) pe care nu-l mai golește nimeni.
Acolo, pe firul care închide: programul e oprit întâi (închiderea consolei l-ar opri oricum),
apoi tamponul e golit până nu mai vine nimic (100 ms de liniște, cel mult 2 s; `drain_until_quiet`),
abia apoi e închisă consola: loc pentru ultimul ecran pe care îl scrie. Doar cât programul
încă rulează (sfârșitul lui nevăzut): un `exit` obișnuit nu pornește nimic. O copie a PTY-ului
care nu se poate lua (prea multe fișiere deschise) e spusă în jurnal.

Review (agent separat): `#[cfg(unix)]` ajunsese pe `drain` în loc de `ending` (Windows nu s-ar
fi compilat); firul putea învârti procesorul pe un hangup fără date (citește acum doar cu
`POLLIN`); testul firului putea rămâne agățat (scrie acum pe un fir, cu termen); intervalul,
o `Duration` cu motivul ei. Copia închisă cu cel mult 100 ms după PTY: scris în doc.
Al doilea review: pe Windows golirea o singură dată nu ajungea (programul încă scria și umplea
tamponul la loc): oprit întâi, golit până tace; firul Unix nu mai învârte procesorul pe o citire
goală după `POLLIN` (pauză de 10 ms); niciun fir pentru un program deja ieșit; testele: cititorul
fals spune „gol” ca cel adevărat (`Ok(0)`), o aserțiune care nu putea pica scoasă, programul
recunoscut după id și ora pornirii (un id refolosit nu-l mai e).
Testul vectorilor din setări cerea cadre (animația de deschidere, pe care o încărcare mare o
termina înainte): cere acum ca vectorii să fie pe drum (îi aduce waker-ul, nu cadrele).

**Teste:** un shell care scrie fără oprire, închis: dispare din sistem (cu hangup tratat ca
ieșire, și ignorat — oprit forțat); amândouă picau înainte (7 s, rămas „la ieșire”), trec
acum (2 s, cât e grația hangup-ului). Firul: citește până la capăt, se oprește la cerere,
fără copie nu face nimic.

**Lăsat:** procesele deja blocate într-o instanță veche se eliberează doar la închiderea ei.

### 100l — Adormită și pe ecran, când n-o mai atingi (cerut 2026-11-27)

O sesiune de agent lăsată pe ecran nu adormea niciodată: regula era „nevăzută atâta timp”, iar
un tab ales într-o fereastră lăsată deschisă rămânea „văzut” la nesfârșit. Nici nu se vedea de
ce rămâne trează una ascunsă: jurnalul spunea doar când adoarme.

**Făcut:** o singură regulă, aceeași pentru orice agent (terminalul nu știe care e):
neatinsă atâta timp, pe ecran sau nu, programul tăcut atâta timp (nimic scris în PTY) și
sesiunea agentului nescrisă atâta timp (`ResumeLaunch`, din fișierele lui, prin
`AgentProvider::sessions`: fiecare agent își citește formatul). „Atinsă” (`Asked::touched`)
= o tastă, text, un paste, un clic, rotița, o comandă a tabului (Clear, Wake Up), tabul ales,
programul pornit. O fereastră acoperită și descoperită de alta, un cadru desenat, mouse-ul
trecut peste nu ating nimic (ar fi ținut-o trează la nesfârșit). Urmărirea „văzută / pe ecran”
(`shown`, `visible`) scoasă. Un ordin dat e lăsat dacă tabul a fost atins între timp.
Jurnalul spune la `debug`, doar când se schimbă, de ce rămâne trează fiecare sesiune
(`a session's sleep`: `used lately`, `output lately`, `text typed, not sent`, `never started`,
`its owner can't resume it now`…; `no: may sleep`), scris cu lacătul slotului lăsat.
Textele setărilor și ale cardului („neatinsă și inactivă”) spun regula nouă.

**Verificat** pe o instanță izolată (un `HOME` al ei, un agent de probă care scrie un fișier
de sesiune ca cel adevărat, apoi tace; adormirea la 3 min): ascunsă, a adormit; pe ecran,
neatinsă, a adormit. Pe macOS, cu fereastra în spatele altora, adormirea a venit cu ~1–2 min
după termen (sistemul încetinește aplicațiile din fundal): la o oră, neînsemnat.

**Teste:** regula și motivul spus pentru fiecare caz; tastă, clic, rotiță, text, paste,
comandă ating; tabul lăsat, mouse-ul trecut peste, un cadru nu; un ordin urmat de o tastă e
lăsat, unul neatins e luat; motivul e ținut ca ultimul spus.

**Lăsat:** dacă un agent adevărat (nu cel de probă) scrie ceva în terminal cât stă (un
indicator, o linie de stare), nu adoarme: jurnalul spune atunci `output lately` și se vede.

## Riscuri

- Un program care nu scrie nimic în timpul unei ture lungi (o unealtă tăcută) ar putea părea
  inactiv: de aceea și fișierul sesiunii (scris la fiecare pas) trebuie să fie vechi, iar
  intervalul minim e 15 min.
- O previzualizare a tabului care l-ar desena l-ar trezi: niciuna nu desenează terminalele
  azi.
- Fișierele de pe disc: criptate cu o cheie care moare cu procesul; o oprire bruscă lasă
  fișiere ilizibile, șterse la următoarea pornire.
- Windows: căile și încuierea folderului verificate cu clippy pentru Windows, nerulate.

## Măsurători

Release, macOS (`cargo test --release -p pty measure -- --ignored --nocapture`), 5000 de
rânduri de istoric × 200 de coloane (liniile colorate ale unei transcrieri de agent):

| Unde stă istoricul | Memorie |
|---|---|
| În grila emulatorului | 22,9 MB |
| Parcat compact (83d, după 2 min) | 0,70 MB |
| Pe disc (100d, după 10 min) | **40 B** în memorie; fișierul: 0,63 MB codat → **57 KB** comprimat (+28 B sigilat) |

- Codarea: 0,35 ms; comprimarea: 0,62 ms; citit înapoi și pus la loc: ~10 ms (pe worker).
- La ~19 terminale cu istoricul plin: încă ~13 MB scoși din memorie peste 83d. Câștigul mare
  rămâne adormirea sesiunilor (220–650 MB fiecare, procesul agentului).
