# Plan 55 — Resturi: scurtături, meniuri, diagnostice PHP, un test instabil, setări, indexul planurilor

## Context
Ce a rămas din planurile 47–54: conflictele de scurtături lăsate după 51, rândurile de meniu
amânate (Open Recent ›, Keyboard Shortcuts…, iconițe), rândurile meniului nativ macOS care se
actualizează târziu, diagnosticele PHP care nu văd schimbările din alte fișiere, un test al
serverelor de limbaj instabil sub încărcare, fundalul suprapunerilor care nu urmează setările
rescrise din afară, indexul planurilor.

## Stare
- [x] 55a — conflictele de scurtături rămase după 51
- [x] 55b — meniuri: Open Recent ›, Keyboard Shortcuts…, iconițe, rândurile native la zi
- [x] 55c — diagnosticele PHP peste fișiere
- [x] 55d — testul instabil `find_usages_are_the_servers_references_without_its_rename`
- [x] 55e — setările rescrise din afară: fundalul suprapunerilor
- [x] 55f — indexul planurilor (43–55)
- [x] 55g — suprapunerea de diagnostic: lizibilă, fără cadre proprii, cu ×
- [x] 55h — terminalul „stricat” la redimensionări cu un program care redesenează (urmare a 54b)
- [x] 55i — limba implicită: engleza
- [x] 55j — butonul Task Manager-ului jos, pe banda din dreapta
- [x] 55k — sticla panoului meniului (`[floating]`), Sticla implicită în toate temele
- [x] 55l — derularea terminalului la pixel
- [x] 55m — antetul tăiat după „frecarea” înălțimii panoului
- [x] 55n — numerele de pe butoanele ferestrelor-unealtă, compacte
- [x] 55o — stubs-urile PHP regenerate (G07, proprietățile claselor PHP)

## 55a — Conflictele de scurtături rămase după 51
Fiecare rezolvat ca în editorul original:
- **JetBrains ⌘⇧N** (New Window / Go to File): în keymap-ul original Go to File e ⌘⇧O pe macOS (⌘⇧N e
  New Scratch File, pe care nu-l avem: New Window îl păstrează acolo) și Ctrl+Shift+N pe
  Windows / Linux, unde New Window n-are tastă. Tot ca în keymap-ul original: Go to Line ⌘L / Ctrl+G,
  Find Next ⌘G / F3.
- **⌘G** (Find Next / Go to Line, în VS Code / Zed / Sublime pe Windows / Linux, unde ⌃G al
  macOS-ului devine Ctrl+G): Go to Line păstrează Ctrl+G, Find Next e ⌘G doar pe macOS și F3
  peste tot (ca în VS Code / Sublime pe Windows). Implicit (macOS): Find Next ⌘G, Go to Line
  ⌘L (ca Xcode), neschimbat.
- **Ctrl+Shift+−** (Navigate Forward / Zoom Out pe Windows): VS Code și Zed pe Windows Alt+← /
  Alt+→, Sublime Alt+− / Alt+Shift+−; pe macOS ⌃− / ⌃⇧− neschimbate.
- Testul (cu toate modulele încărcate, pe macOS / Windows / Linux, după meniurile fiecăruia):
  niciun preset nu dă aceeași combinație la două comenzi din bara de meniu, în afară de
  perechile care nu se întâlnesc niciodată, listate (`EXCLUSIVE`, acum goală). A găsit și
  două conflicte ale modulelor: Push al Git-ului (⌘⇧K, al setului JetBrains) față de Delete Line
  în VS Code / Zed / Sublime — acolo fără tastă (nici originalele n-au); Dependency Canvas
  (⌘⇧D) față de Duplicate în Zed / Sublime — acolo fără tastă.
- `keymaps.golden` (macOS): doar Go to File în JetBrains (⌘⇧O). Scurtăturile salvate de
  utilizator neatinse (alt strat).

## 55b — Meniuri: Open Recent ›, Keyboard Shortcuts…, iconițe, rândurile native la zi
- **Fișier › Deschide recent ›** (după Deschide proiect…): proiectele recente din grupul
  „Recente” (cele puse de utilizator în grupurile lui rămân în lista de proiecte), fără al
  ferestrei, doar cele cu folderul existent (ca meniul iconiței), cel mult 10, cu calea
  (`~` pentru acasă); apoi fișierele recente ale proiectului ferestrei (relative la el, cel
  mai nou primul, cel mult 10); la final „Golește lista recentelor” — uită proiectele din
  „Recente” (`RecentProjects::clear_recent`) și fișierele recente ale proiectului; stins când
  nu e nimic de golit. Submeniul rămâne submeniu și gol. Rândurile: `project.open_recent:<cale>`
  (aceeași comandă ca meniul iconiței) și `file.open_recent:<cale>`; iconițe `folder` /
  `file-text`. Lista meniului (`commands::RecentMenu`) e pusă de fereastra cu focus doar când
  se schimbă ce arată (generația proiectelor recente, proiectul, fișierele lui) — atunci
  meniul nativ e refăcut; nu la fiecare eveniment. În teste lista e a firului testului, ca
  depozitul recentelor (niciodată `~/.config/lumen/recent.toml`).
- **Ajutor › Scurtături de tastatură…** (`help.keyboard_shortcuts`, prima secțiune din
  Ajutor): setările deschise pe pagina Keymap.
- **Iconițe** pentru toate rândurile din bara de meniu (testul: fiecare rând, cu toate
  modulele, pe cele trei sisteme, are iconiță). Din setul existent: Delete Line / Clear
  Recent `trash`, Move Line ↑/↓ `arrow-up/down`, Move Statement `chevron-up/down`, Extend /
  Shrink Selection `maximize-2` / `minimize-2`, Add Selection / Unselect / Select All
  Occurrences `plus` / `minus` / `layers`, Add Caret `text-cursor`, Find Next / Previous
  `arrow-down/up`, Go to Line `rows`, Go to Declaration `arrow-up-right`, Collapse / Expand
  `chevron-right/down`, Next / Previous Tab `arrow-right/left`, Actual Size `rotate-ccw`,
  Close Notifications `x`, Keyboard Shortcuts `keyboard`, Theme Studio `droplet`. Noi (stil
  Lucide): `log-out` (Quit), `copy-plus` (Duplicate), `replace` (Replace).
- **Rândurile meniului nativ macOS la zi**: când bara începe să fie urmărită (clic pe ea, tastele
  ei: `NSMenuDidBeginTrackingNotification` pentru meniul principal, observat o dată),
  `PlatformEvent::MenuOpening` ajunge la fereastra cu focus, care citește din nou stările
  rândurilor (inclusiv `enabled` al modulelor: Revert Change după tastare) și le trimite;
  evenimentele buclei ajung și cât meniul e deschis (sursa ei e în modurile comune ale
  run loop-ului), iar starea unui rând se schimbă și pe meniul deschis. Nimic în plus la
  tastare. Meniurile contextuale nu declanșează.
- Teste: Open Recent (proiectele celelalte fără al ferestrei și fără cele grupate, fișierele
  relative, cel mai nou primul, rândurile din bară, iconițele, Golește la final), golirea în
  domeniu (grupurile păstrate), iconițele tuturor rândurilor. `menus.golden`,
  `keymaps.golden`, `context_menus.golden` (Go to Declaration cu iconiță) actualizate. O
  captură privită (Editare în sertar). Netestat: notificarea macOS (doar compilată; nu pot
  deschide bara nativă dintr-un test).

## 55c — Diagnosticele PHP peste fișiere
Diagnosticele care știu proiectul (`LanguageIntelligence::check`, pe firul de simboluri, 47c)
erau refăcute doar la editările documentului și la capătul unei indexări. Acum:
- **Indexerul** spune ce fișiere a actualizat unul câte unul (o salvare, o schimbare pe disc
  a altui program, o redenumire — `update`, nu un build): `ProjectIndexer::take_updated`
  (fiecare o dată; ale unui build nu, acela e `IndexingFinished`), prin `ProjectIndex`.
- **Pe magistrala proiectului**: `poll_indexer` (pe firul UI, la trezire) publică
  `workspace::events::IndexUpdated { index, paths }`; strânse de evenimentele proiectului
  deja de coada indexerului (`kernel::queue`, 300 ms de liniște). `ProjectData.index_updates`
  le ascultă (fiecare cale o dată, oricâte anunțuri).
- **Verificarea din nou**: documentele deschise (și în celelalte grupuri) al căror limbaj se
  verifică față de proiect (`LanguageIntelligence::checks_project`: PHP) sunt trimise din nou
  firului de simboluri (`SymbolWorker::recheck`, `kernel::queue::Latest` pe document: cereri
  repetate se contopesc), doar cu indexul complet; un document a cărui singură schimbare e
  propria reindexare (salvarea lui) nu. Nimic greu pe firul UI.
- Teste: indexerul (o schimbare listată o dată, un build nu); aplicația — `Lib.php` schimbat pe
  disc (anunțat ca de watcher, `EntriesChanged`): în `Probe.php` deschis „prea puține
  argumente” dispare și apare „metodă necunoscută”, fără nicio editare a lui (fără schimbare
  testul cade: verificat).

## 55d — Testul instabil `find_usages_are_the_servers_references_without_its_rename`
- **Cauza** (stare comună): testele care aleg serverul PHP își puneau alegerea cu
  `language_servers::settings::set` — o setare a procesului. De la 53b testele rulează în
  paralel, iar fiecare `crate::modules::load` (al oricărui test al aplicației, și al celor
  TypeScript de alături) aplică din nou setările salvate (`SettingsContribution::apply` la
  încărcare): motoarele redeveneau cele implicite (`[]`), serverele proiectelor erau
  reîmprospătate, iar serverul PHP al testului oprit la mijloc — Find Usages aștepta un
  răspuns care nu mai venea (sau planul cădea pe motorul nostru). Încuietoarea `PHP` ținea
  departe doar testele cu fișiere PHP, nu încărcările de module.
- **Reparația**: `language_servers::settings::pin(config) -> Pinned` — setările date rămân
  oricât sunt aplicate din nou cele salvate, până la renunțarea la gardă (care le pune la
  loc pe cele dinainte, și când testul cade); `apply` nu le atinge cât sunt fixate. Testele
  care aleg PHP-ul (`choose_php`, `choose_intelephense`) le fixează; nimic schimbat în aplicație
  (nefixate niciodată acolo).
- Verificat: un test nou (alegerea fixată supraviețuiește unei încărcări de module; cu `set`
  în loc — cum era — cade: motoarele `[]`); testele serverelor aplicației de 30 de ori la rând
  cu 10 nuclee ocupate de bucle: 30 / 30 verzi (înainte nu se reproducea în 12 rulări fără
  încărcare: o cursă rară, dovedită prin mecanism).

## 55e — Setările rescrise din afară: fundalul suprapunerilor
- `settings.toml` editat de alt program e citit din nou la revenirea în fereastră
  (`reload_settings`), iar blocurile de setări își aplică valorile (`SettingChanged { key: "*" }`):
  alegerea fundalului (`backdrop::OWN`) era la zi, dar paleta ferestrelor
  (`palette.backdrop`) era rezolvată din nou doar la următoarea schimbare de temă.
- Acum citirea din afară e numărată (`config::refresh_from_outside`, `outside_changes`), iar
  fiecare fereastră, în `sync_shared`, rezolvă din nou ce a derivat din setări când numărul se
  mișcă: tema cu setările ei proprii (care se citeau și ele doar la o schimbare de temă) și
  fundalul — paleta întreagă —, apoi keymap-ul (presetul și scurtăturile proprii ale
  utilizatorului din `[shortcuts]`, citite până acum doar la pornire). Restul blocurilor sunt
  citite la folosire (notificările, butonul meniului, saltul de rând…): neschimbate. Tema
  aleasă (`[appearance] theme`) și limba schimbate din afară rămân pentru pornirea următoare
  (alegeri ale catalogului, nu valori derivate).
- Test: fișierul rescris (Glass, 40%): citit din nou — secțiunea schimbată, numărul crescut o
  dată, alegerea aplicată, o paletă rezolvată acum are fundalul nou; citit din nou neschimbat:
  nimic. Urmare: testul a picat o dată în `check.sh` — alegerea utilizatorului (`OWN`) era a
  procesului, iar alte teste care încarcă modulele își aplicau setările (ale firului lor)
  peste ea; în teste e acum a firului, ca setările (`config::settings`). Ramura din `sync_shared` (fereastra) neacoperită de un test (are nevoie de o fereastră
  reală).

## 55f — Indexul planurilor
`README.md`: rândurile 42–55 în cuprins (42 lipsea și el), în formatul celor de dinainte (titlul cu
legătura, ce conține pe scurt). `TODO.md`: o secțiune „Rămase din planurile 43–55” cu ce au lăsat
deschis (LSP, stubs-urile PHP, izolarea câtorva module în teste, un test pe ceas, meniurile,
terminalul, setările schimbate din afară).

## 55g — Suprapunerea de diagnostic (F12): lizibilă, fără cadre proprii, cu ×
- **Cauza CPU-ului mare**: cât era arătată, aplicația cerea cadre fără oprire ca s-o măsoare
  (`request_redraw` la fiecare cadru) — o buclă care se măsura pe ea însăși (~8 ms / cadru,
  ~30% CPU desenând aproape nimic). Acum nu cere niciun cadru pentru ea: arată ce au măsurat
  cadrele desenate oricum, iar tabelul e refăcut cel mult o dată la 500 ms; în repaus rămân
  ultimele valori, fără redesenare. Singurul cadru cerut e al tooltip-ului ×-ului
  (`frame_deadline`). Cadrele doar ale decorațiunilor merg și cu ea arătată.
- **Lizibilă**: un tabel pe două coloane — eticheta estompată în stânga, valoarea aliniată la
  dreapta, în monospace (cifrele nu sar) —, etichete ro / en (`[debug]`): Backend, Cadru (CPU) ·
  medie / max, Intrare → ecran · p95, Desenat în cadru · dreptunghiuri / glife / apeluri, Cache
  de glife, Zonă redesenată. Culorile după buget (verde / avertisment / eroare din paletă):
  cadrul > 8 ms avertisment, > 16 ms rău; intrarea p95 > 16 / > 33 ms. Dedesubt un sparkline
  al ultimelor 60 de cadre (câte un quad, 16 ms sus, colorat la fel).
- **×** în colțul din dreapta sus (`ds::clear_button`, hover, cursorul mână, tooltip „Închide
  (F12)”): o ascunde ca F12. Doar el primește clicuri; restul panoului le lasă mai departe.
- Teste: un flux de 125 de cadre în 2 s — tabelul se schimbă de cel mult 4 ori (≤ 2 / s), niciun
  cadru cerut; culorile după buget; ×-ul (sus dreapta, ascunde, restul nu ia clicul, tooltip-ul
  singurul care cere un cadru). O captură privită (`EDITOR_SNAPSHOT_DEBUG_OVERLAY=1`, cadre
  inventate).

## 55h — Terminalul „stricat” când panoul e redimensionat cu un program care redesenează
Văzut de utilizator (Claude Code rulând, înălțimea panoului schimbată de mai multe ori):
istoricul arăta bucăți din cadre vechi — antetul programului și bara lui de intrare (o bandă
inversată pe toată lățimea) în mijlocul istoricului, urmate de ieșire mai veche.
- **Cauza** (urmare a 54b): astfel de programe redesenează cu `ESC[2J ESC[3J ESC[H` și tot
  ecranul. `ESC[2J` al lui alacritty împinge ecranul vizibil în istoric (ca kitty / Ghostty;
  al lui xterm nu); `ESC[3J`-ul de după îl ștergea, iar de la 54b e păstrat pentru programele
  care rulează — deci fiecare redesenare (una la fiecare redimensionare) lăsa în istoric o
  copie a cadrului de dinainte. În plus, redimensionarea însăși: micșorarea împinge în istoric
  rândurile de deasupra cursorului (bucăți din cadru), mărirea aduce rânduri din istoric pe
  ecran (pe care redesenarea le-ar fi șters).
- **Filtrul** (`EraseFilter`): `ESC[2J`-ul de lângă un `ESC[3J` ignorat — imediat înainte, sau
  imediat după cu doar mutări ale cursorului între (ordinea lui `clear`: `3J H 2J`) — devine
  `ESC[H ESC[J` (`QUIET_ERASE`: ecranul șters de la colț, nimic împins în istoric). Un
  `ESC[2J` e ținut până se vede ce urmează (și peste granițele citirilor); orice alt octet îl
  lasă să treacă neschimbat, iar „nimic de citit acum” (`WouldBlock`, și pe Windows: cititorul
  lui alacritty e neblocant) îl lasă la fel (`flush`). Un `ESC[2J` singur (fără `3J`) și
  perechea onorată (`clear` din shell) rămân ca înainte.
- **Redimensionarea** (`resize_keeping_history`): cât programul care redesenează (`3J`
  ignorat; reținut cu momentul pornirii lui, `filtered::Redrawing`) e încă cel din prim-plan
  (același început, la o secundă — întrebat la redimensionare, înainte de a bloca terminalul),
  ecranul lui e golit înainte (nimic din el împins în istoric de micșorare sau de reflow), iar
  ce a adus mărirea din istoric e pus la loc. Programul își redesenează ecranul după. Altfel
  (shell-ul, alt program) redimensionarea lui alacritty neschimbată. Un `clear` onorat uită
  programul.
- Teste: filtrul (perechile în ambele ordini, tăiate la fiecare octet, onorată, singur, cu ieșire
  între, ținut la capăt și lăsat de `flush`); prin cititorul filtrat și redimensionarea
  sesiunii: 100 de rânduri, apoi 20 de redimensionări (80 × 24 ↔ 60 × 18) cu câte o redesenare
  `2J 3J H` + 10 rânduri cu bara inversată pe toată lățimea — în istoric doar cele 100 de rânduri
  de dinainte, fiecare o dată și în ordine, nicio bucată de cadru; `clear` din shell îl golește;
  un `2J` singur împinge ecranul ca înainte; istoricul după cele 20 de redesenări din 54b e
  exact cel de dinainte (477 de rânduri), nu crescut cu cadre.
- Limită: un program care trimite `2J` și `3J` în scrieri separate, cu o pauză între care
  cititorul golește tot, lasă încă o copie (rar: Ink le scrie împreună).

## 55i — Limba implicită: engleza
Pentru cine pornește aplicația prima oară (nicio limbă salvată) interfața e în engleză — nu
limba sistemului și nu româna. Cine a ales o limbă o păstrează; româna rămâne întreagă în
Setări → Limbă. Schimbate doar implicitele: `language::load` („en” fără alegere salvată),
`settings::LANGUAGE` (implicit „en”) și limba de pornire a registrului `i18n` (înainte „ro”).
Nicio detectare a limbii sistemului nu exista. Teste: `menus.golden` și
`context_menus.golden` refăcute în engleză (aceleași rânduri); testul îngust al vederii de date
(`database-explorer`) își alege explicit româna (lățimile lui sunt ale etichetelor românești) și
revine la engleză. `ui-snapshots.sh` dă `EDITOR_LANG=ro`, ca referințele.

## 55j — Butonul Task Manager-ului jos, pe banda din dreapta
Butonul trece în colțul din dreapta jos (deasupra barei de stare), tot pe banda din dreapta,
prin înregistrarea lui, nu prin aplicație: `ToolWindowProvider::strip_end` (implicit nu;
`ToolWindow.strip_end`) — „butonul în grupul de jos al benzii lui”; grupul de jos al benzii din
stânga rămâne al lui `Dock::Bottom` (alt loc al ferestrei), cel din dreapta e doar al butonului:
fereastra se arată unde se arăta. Aplicația așază banda din dreapta ca pe cea din stânga (același
buton, tooltip, antet, meniul de clic dreapta după id). View › Tool Windows: ordonat după (dock,
grup, ordine), cu un separator și între grupurile unei benzi — Task Manager după un separator la
capătul grupului din dreapta. Ce se salvează din ferestre e după id: neschimbat. Test: butonul e
jos pe bandă, sub celelalte, și deschide tot Task Manager-ul; `menus.golden`.

## 55k — Sticla panoului meniului; Sticla implicită în temele incluse
- **Un stil generic de „suprafață de sticlă”** pentru panourile plutitoare (folosit acum doar de
  panoul drawer-ului): secțiunea `[floating]` a temelor — `style` (simple / glass), `color`
  (tenta, opțională: altfel culoarea panoului), `opacity` (a panoului, 0,6–1; mai puțin nu se
  citește: refuzat), `blur` (0–12 px) —, citită ca `[backdrop]` (`theming::backdrop::SurfaceSpec`,
  același tip), moștenită prin `extends`, rezolvată o dată în paletă (`palette.floating`:
  `ui_components::backdrop::SurfaceStyle` — `fill`, `blur_radius`, `with_user`, `without_blur`).
  Ce nu spune nimeni: opac, ca înainte.
- **Drawer-ul**: la Sticlă panoul are culoarea lui la ~90% peste o copie încețoșată a ce e sub el
  și o margine interioară mai deschisă de 1 px. Ieftin: când fundalul de sub drawer e și el Sticlă
  (cu cel puțin aceeași rază) încețoșarea lui e deja sub panou — nimic în plus; altfel panoul își
  face propriul blur doar după ce a intrat (o dată, în locul final, păstrat cât nu se schimbă
  nimic dedesubt; cât alunecă, opac). Simplu, sau un motor care nu încețoșează: opac.
- **Setări → Interfață → Aspect**, blocul „Panoul meniului” lângă cel al fundalului
  (`app/src/floating.rs`, `[appearance] floating` / `floating_opacity` / `floating_blur`, -1 =
  din temă): Stilul (Din temă / Simplu / Sticlă), Opacitatea panoului (60–100%), Încețoșarea
  (0–12 px), valorile temei marcate „· temă”, văzute pe loc (ca fundalul). **Theme Studio**, în
  Interfață, secțiunea „Panoul meniului” după „Fundal overlay”: aceleași trei rânduri, cu „↺”,
  găsite de căutare, salvate ca `[floating]`; dialogul previzualizării își arată panoul ca
  panourile plutitoare ale ciornei.
- **Sticla implicită** (cerută de utilizator): toate cele 30 de teme incluse spun `[backdrop]
  style = "glass"` (raza blândă de 4 px, tenta și opacitatea fiecăreia, neschimbate) și
  `[floating] style = "glass"`; „Din temă” rămâne astfel cu sens. Cine a ales Simplu îl păstrează;
  un motor care nu încețoșează cade pe Simplu.
- **Eticheta tăiată** din blocul fundalului („16% · the them…”): „16% · theme” / „16% · temă”.
- Teste: blocul setărilor (din temă / ale mele / înapoi, opac fără blur, eticheta valorii temei),
  `[floating]` citit și refuzat sub 0,6, toate temele incluse Sticlă, ciorna studio-ului citită /
  compusă, rândurile studio-ului (opacitatea aleasă în temă, „↺”, reset), drawer-ul (sticlă
  peste fundal de sticlă: ~90%, fără blur propriu; peste fundal simplu: blur propriu odată intrat,
  opac cât alunecă; Simplu: opac). Capturi privite: drawer-ul în default-dark și default-light
  (codul din spate abia ghicit, textul limpede), Setări → Aspect (blocul nou, etichetele întregi).

## 55l — Derularea terminalului la pixel
Înainte istoricul se derula doar pe rânduri întregi (offset-ul lui alacritty). Acum:
- **Poziția** (`terminal_session::scroll::SmoothScroll`, pur): deltele rotiței / trackpad-ului
  se adună într-o poziție în rânduri de la capăt, cu fracțiune; alacritty ține partea întreagă
  (`display_offset`), vederea desenează grila coborâtă cu fracțiunea unui rând, iar în golul de
  sus rândul de istoric de deasupra ecranului (`GridSnapshot.above`, cerut prin `want_above`
  doar cât e o fracțiune: un rând în plus copiat). Clipul stratului grilei taie ce iese. La
  capăt e exact acolo (urmărind ieșirea: nimic decalat, nimic cerut deasupra), la începutul
  istoricului la fel; tastarea / scrollbar-ul / un program pe tot ecranul aduc înapoi la un
  rând întreg. Restul rămâne la capătul gestului (ca Ghostty): impulsul trackpad-ului continuă
  lin, o treaptă a rotiței mișcă ~3 rânduri.
- Selecția, linkul subliniat, cursorul și celula de sub mouse urmează decalajul; bara arată și
  fracțiunea. Ecranul alternativ (TUI-urile) neschimbat: rotița merge la program, pe rânduri.
- Cost: nimic în plus fără derulare; cât e o fracțiune, glifele unui rând. Măsurat (release,
  200 × 50, 500 de cadre, cu Docker-ul stubs-urilor rulând alături): pe rând întreg ~81 µs /
  cadru, cu jumătate de rând ~79 µs — la fel.
- Teste: matematica (pașii mici se adună fără pierderi, o treaptă = rânduri întregi, capetele
  exacte, setarea la capăt), vederea (o fracțiune: emulatorul pe rândul lui, grila coborâtă,
  rândul de deasupra în gol, celula mouse-ului după decalaj; tastarea: exact la capăt, nimic
  cerut deasupra), fiecare pas mic cere un cadru.

## 55m — Antetul unui program tăiat după redimensionări repetate
Văzut de utilizator (și înainte de 55l): după ce schimbă de multe ori înălțimea panoului cu
Claude Code rulând, logo-ul din blocuri (jumătăți ▀▄█ cu fundal) din antetul programului își
arăta primul rând ca o bandă neagră / parțial lipsă.
- **Cauza principală**: fiecare pixel al tragerii era o redimensionare spusă programului
  (`TIOCSWINSZ` → `SIGWINCH`), iar fiecare îl face să redeseneze — redesenări relative la cursor
  (urcă N rânduri, șterge) calculate pentru o lățime / înălțime deja trecute, în cursă cu
  următoarele dimensiuni: rândurile de sus rămâneau desenate dintr-o stare veche. Acum grila
  urmează pe loc (ce se vede), iar programului i se spune dimensiunea o singură dată, ultima,
  după 80 ms de liniște (`ResizeDebounce`, firul `pty-resize` al sesiunii, adormit între; dacă
  firul nu pornește, spusă imediat).
- **A doua**: grila începea pe o fracțiune de pixel (originea panoului fracționară, mai ales
  la scale fracționare): fundalurile rândurilor cădeau între pixeli și se amestecau la margini.
  Acum zona grilei e pe pixeli întregi, rândurile de înălțime întreagă: niciunul nu
  încalecă pixelul altuia.
- Verificat și exclus: (a) fracțiunea de rând a lui 55l (utilizatorul a văzut bug-ul și înainte;
  oricum, la capăt fracțiunea se anulează acum la 200 ms după ultimul pas al rotiței și la orice
  redimensionare); (b) ștergerea fără împingere a lui 55h (`ESC[H ESC[J` șterge cu același fundal
  curent ca `ESC[2J`).
- Teste: o tragere (30 de dimensiuni la 10 ms) spune programului o singură dimensiune, ultima,
  după 80 ms; un logo din jumătăți de bloc cu fundal în primele rânduri, panoul redimensionat de
  30 de ori (crescând și scăzând, la scala 1,25 și origine fracționară) — de fiecare dată logo-ul
  e primul rând, sus, fundalurile rândurilor pe pixeli întregi, niciunul peste altul.

## 55n — Numerele de pe butoanele ferestrelor-unealtă, compacte
„99+”-ul de pe Remote (și numărul Git-ului) era o pastilă mare centrată pe colțul butonului,
peste jumătate din iconiță. Acum o singură componentă, `ds::count_badge` (`ui-components`),
folosită de fiecare buton al benzilor (stânga, dreapta, jos): pastilă compactă de 14 px la 1×
(scalată cu zoom-ul, pe pixeli întregi), un cerc pentru o cifră, mai lată spre dreapta pentru
mai multe — „99+” cu litere mai mici și mai strâns, rămânând peste sfertul din dreapta sus al
iconiței și în interiorul benzii (`max_x`) —, cifrele de ~9,5 px semibold, un inel de 2 px în
culoarea fundalului benzii (pare decupată din iconiță). Culoarea după sens, aleasă de modul:
`workbench_model::BadgeKind` (`Count` — accentul: schimbări, încărcări; `Warning` — un conflict
Git; `Error` — încărcări eșuate în Remote), în locul vechiului `attention: bool`. Statică (nimic
animat). Test: colțul iconiței (un cerc la o cifră, „99+” doar în sfertul de sus-dreapta, în
bandă cu inel), culoarea după sens. Un cârlig de capturi (`EDITOR_SNAPSHOT_TOOL_BADGES=
<unealtă>:<număr>[:warning|error],…`), privit în default-dark și default-light.

## 55o — Stubs-urile PHP regenerate
`cargo xtask php-stubs --update` (Docker / OrbStack, cele 12 versiuni din
`docker/php/compose.yaml`, 5.6 – 8.5; imaginile extinse `php-stubs:<versiune>[-pecl]` construite din
`docker/php/Dockerfile`, cu extensiile PECL la 8.4 și 8.5; containerele pornite cu `--rm`, niciunul
rămas; nicio altă imagine sau container atins). Nicio versiune nouă de PHP pe Docker Hub. Rezultatul:
16 364 de înregistrări (înainte 15 524) — numai adăugate: 671 `p` (proprietățile claselor PHP), 120
`df` (funcții depreciate, de ex. `utf8_encode` / `utf8_decode` din 8.2) și 49 `dm` (metode /
constante depreciate); cele existente (`m`, `k`, `cc`, `f`, `c`) neschimbate ca număr și conținut.
Imaginile construite rămân: ~5 GB în total (`php-stubs:8.5-pecl` 1,17 GB, `8.4-pecl` 1,15 GB, restul
150–390 MB). Teste: stubs-urile incluse știu deprecierea lui `utf8_encode` (8.2) și
`Exception::$message` (protejată); G07 — un apel `utf8_encode()` e semnalat ca depreciat de motorul
nostru. Harness-ul de sonde din 43 (`scratchpad/lsp-compare`) nu mai există (era în scratchpad-ul unei
sesiuni vechi): scorul nu a putut fi măsurat din nou; G07 e acoperit de testul de mai sus.
