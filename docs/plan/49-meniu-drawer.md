# Plan 49 — Meniul aplicației într-un drawer (Windows / Linux)

## Context
Pe macOS meniul e cel nativ (bara de sus, `platform-winit/src/menu.rs`, muda). Pe Windows și
Linux nu există niciun meniu: `install_menu` nu face nimic („the menu bar will be drawn by the
app”), comenzile din File / Edit / Code / Refactor / Find / Navigate / Git / Window se ajung doar
din scurtături și paleta de comenzi. Cerința: un buton hamburger în stânga barei de titlu care
deschide un drawer din stânga, cu un overlay peste conținut, categorii cu iconițe care se
extind frumos la hover cu subcategoriile lor.

## Sursa meniului
Aceeași listă ca meniul nativ: `commands::menus()` (`Vec<Menu>`: titlu, itemi, separatori,
scurtături; modulele își pun comenzile unde le spune meniul lor, ex. Theme Studio). Nimic
duplicat: ce apare în meniul macOS apare și în drawer, în aceeași ordine. Iconițele itemilor din
`commands::menu_icon` (cele de la meniurile contextuale); fiecare meniu de top primește o
iconiță (File → folder, Edit → creion, Code → acolade, Refactor → refactor, Find → lupă,
Navigate → busolă / săgeți, Git → branch, Window → ferestre, Help → ?), date în spec-ul
meniului (`Menu.icon`, opțional, ignorat de muda).

## Design
- **Butonul**: hamburger (☰) în bara de titlu, primul din stânga, înaintea chip-ului de proiect;
  pe Windows după zona butoanelor sistemului doar dacă ele sunt în stânga (nu sunt: sunt în
  dreapta), deci primul element. Tooltip „Meniu” cu scurtătura (Alt; și F10, ca în Windows).
- **Drawer-ul**: un `OverlaySlot` nou (modal: nimic din spate nu primește input), panou lipit
  de stânga, toată înălțimea ferestrei — de sus până jos, peste bara de titlu —, lățime
  ~300 px (scalată), muchii drepte (fără colțuri rotunjite), fără umbră (o linie de margine);
  intră glisând din stânga (~160 ms, ease-out), iese la fel. Sus, pe înălțimea barei de titlu,
  un rând-antet cu butonul ☰ exact unde e în bară (pare că rămâne pe loc cât drawer-ul
  glisează sub el; un clic pe el îl închide); pe macOS antetul lasă loc butoanelor ferestrei
  (ca bara), pe Windows butoanele ferestrei sunt în dreapta, neacoperite și neîntunecate.
  Câmpul de filtrare și categoriile vin sub antet. Overlay-ul: toată fereastra din spate
  întunecată (bara de titlu inclusiv), după luminozitatea temei (~50–55 % negru în temele
  întunecate, ~25–30 % în cele luminoase), cu fade. Un click pe overlay îl închide (e un
  meniu, nu un modal cu lucrul userului: ca popover-ele) — Esc, hamburger-ul sau Alt îl
  închid și ele.
- **Conținut**: sus un câmp de filtrare („Caută în meniu…”, scrisul îl filtrează direct pe
  toate submeniurile, rezultatele cu calea lor „Refactor › Rename”). Apoi categoriile: rând cu
  iconiță + titlu + chevron. La hover (sau ← → / ↵ de la tastatură) categoria se deschide ca un
  al doilea panou alăturat (drawer-ul se lărgește lin cu ~300 px) cu itemii ei: iconiță,
  etichetă, scurtătura aliniată la dreapta (`display_shortcut`: Ctrl+Shift+F), separatori,
  itemii dezactivați estompați. Hover-ul are o mică întârziere și „triunghiul de intenție”
  (mutarea în diagonală spre submeniu nu schimbă categoria), ca în meniurile bune.
- **Tastatură**: ↑ ↓ prin categorii / itemi, → / ↵ intră în submeniu, ← iese, ↵ rulează, Esc
  închide (sau iese din filtru); litera subliniată nu — filtrul face asta mai bine. Subsolul cu
  scurtăturile (aceeași componentă ca la listele modale: ↑ ↓ navighează, ↵ deschide, esc închide).
- **Scroll**: categoriile și itemii derulează cu scrollbar-ul standard când fereastra e mică
  (aceeași abordare ca lista de proiecte).
- Rularea unei comenzi închide drawer-ul și trimite comanda pe aceeași cale ca meniul nativ
  (`UserEvent::Command`), deci modalele / tab-urile se deschid ca acum.
- **macOS**: meniul nativ rămâne, iar butonul ☰ și drawer-ul sunt și ele acolo, implicit.
  Setare „Arată meniul în bara ferestrei” (Interfață → Aspect), implicit pornită; doar pe
  macOS se poate opri (pe Windows / Linux e mereu pornită și nu apare în setări: altfel nu
  ar avea meniu).
- Teme: toate culorile din paletă (panou = popup, hover / selectat ca la listele modale),
  verificat pe toate temele în snapshot-uri. Efectele temei (Matrix, cyberpunk) nu intră peste
  drawer.

## Pași (câte un commit)
- **49a** — modelul: `Menu.icon`, iconițele meniurilor de top, starea drawer-ului (pur:
  filtru, navigare de tastatură, triunghiul de intenție) cu teste.
- **49b** — vizualul: butonul din bara de titlu, `OverlaySlot`-ul cu scrim și animație,
  cele două panouri, filtrul, subsolul, scroll; locale ro / en; setarea pentru macOS.
- **49c** — snapshot-uri (închis / deschis / cu submeniu / filtrat / fereastră mică; dark și
  light; o temă cu efecte), verificare clippy Windows, măsurat: zero cost cât e închis (nimic
  creat până la prima deschidere), animația doar cât durează.

## Stare
- [x] 49a — modelul: `Menu.icon` (`platform-api`, numele unei iconițe din setul aplicației;
  muda îl ignoră), iconițele meniurilor de top în `commands::MENU_ICONS` ({app} → info,
  File → folder, Edit → file-pen, Code → braces, Refactor → wand, Find → search, Navigate →
  compass, Git → git-branch, Window → app-window), `commands::drawer_menus()` (meniurile
  native, în ordinea lor, cu rândurile meniurilor contextuale: iconițe, scurtături ca pe
  sistem, separatori curățați). Iconițe noi în setul nostru (stil Lucide): `menu`,
  `compass`, `wand`. Starea drawer-ului, pură, în `ui-workbench/src/menu_nav.rs`:
  `DrawerNav` (categoria aleasă și submeniul ei, ↑ ↓ prin categorii / itemi / rezultate cu
  ocolire la capete, → / ↵ intră, ← iese, ↵ rulează, separatorii săriți; filtrul pe toate
  submeniurile, fără diacritice / majuscule, potrivit și pe titlul meniului, rezultatele cu
  calea lor) și `HoverIntent` (altă categorie după o pauză scurtă de 70 ms; spre submeniu, în
  triunghiul de intenție — `aims_at`, de la poziția anterioară la marginea apropiată a
  submeniului, ușor lărgit — amânată cât mouse-ul se mișcă spre el, 300 ms după ce se
  oprește; ajuns în submeniu, schimbarea renunțată). Itemi dezactivați nu există în model
  (`MenuItem` nu are stare; muda îi face pe toți activi), deci nimic de estompat deocamdată.
  Teste: navigarea de tastatură, filtrul cu căile, mouse-ul, triunghiul, întârzierile;
  meniurile drawer-ului = cele native (aceleași comenzi, în ordine), fiecare cu o iconiță
  existentă.
- [x] 49b — vizualul: butonul ☰ în bara de titlu, primul din stânga (după butoanele
  sistemului pe macOS; `TopBarModel.menu`, `TopBarHit::Menu`, ocupat pentru decorațiile
  temei), aprins cât drawer-ul e deschis, tooltip „Meniu” cu „Alt · F10” (pe macOS doar F10).
  `ui-workbench/src/menu_drawer.rs` (`MenuDrawer`): panoul categoriilor (300 px) lipit de
  stânga sub bară, colțurile din dreapta rotunjite (cele din stânga în afara ferestrei),
  umbra suprafețelor înalte, intră / iese glisând (160 ms, ease-out); scrim negru 35 % în
  temele întunecate / 20 % în cele luminoase, cu fade; submeniul ca al doilea panou (300 px)
  care se lărgește lin, cu titlul categoriei la nivelul filtrului, rândurile ca la meniurile
  contextuale (iconiță, etichetă, scurtătura la dreapta, separatori); la schimbarea
  categoriei rândurile noi apar cu un fade scurt (110 ms). Filtrul sus (`search_field`, are
  tastatura; lipit / tăiat / anulat urmat la fiecare cadru), rezultatele cu calea lor
  (`Fișier ›` estompat, apoi eticheta, scurtătura la dreapta), „Nicio comandă găsită”.
  Subsolul cu taste: aceeași bandă ca la modalele-listă (`modal::paint_key_footer`, scos din
  `Modal` — intrat în commit-ul d450424e al sesiunii principale), cu tastele care merg acum
  (categorii: ↑ ↓ / → / esc; submeniu: ↑ ↓ / ↵ / ← / esc; filtru: ↑ ↓ / ↵ / esc golește),
  tăiate de la coadă dacă nu încap. Scroll: `ScrollState` + `EdgeScrollbar` pe fiecare
  listă (rotița pe lista de sub mouse, bara trasă, rândul tastaturii ținut în vedere, doar
  rândurile vizibile clicabile; derulând, rândurile lasă loc barei). Culorile din paletă
  (panou = popup, ales = accent 12 %, sub mouse = text 5 %, ca la listele modale).
  În aplicație: `Overlay::MenuDrawer` (al doilea pe stivă, după notificări), `OverlaySlot`
  modal (nimic din spate nu primește taste / text / comenzi / scroll / hover), dar un clic
  în afară îl închide; pe bară închide și lasă butoanele ferestrei / mutarea ei să meargă.
  Esc golește filtrul, apoi închide; ☰, F10 (comanda `view.menu`, permisă cât e deschis) și
  Alt singur (Windows / Linux: `app/alt_tap.rs`, anulat de o tastă, un clic, un scroll, alt
  modificator — AltGr — sau o schimbare de focus, ca Alt + Tab) îl deschid / închid. O comandă
  aleasă (clic sau ↵) îl închide și pleacă pe calea meniului nativ (`MouseResult::MenuCommand`
  → `PlatformEvent::Command`). Nimic creat până la prima deschidere (`Option<Box<MenuDrawer>>`,
  meniurile citite la deschidere), aruncat după ce a ieșit; cadre doar cât glisează / se
  lărgește; schimbarea amânată a categoriei cere un singur cadru la termen (`hover_deadline`).
  Pe Windows, subclasa barei înghite `WM_SYSCOMMAND/SC_KEYMENU` fără tastă (Alt / F10
  singure ar porni bucla de meniu a sistemului, care fură următoarea tastă); Alt + Space
  rămâne al sistemului. Setarea macOS: `WINDOW_MENU` (`[appearance] window_menu`, implicit
  oprită), blocul „Meniu” la capătul paginii Aspect (doar pe macOS; pe Windows / Linux
  butonul e mereu acolo). Iconițe adăugate rândurilor meniului (Despre, Setări, Fișier nou,
  Fereastră nouă, Deschide…, Salvează, Anulează, Refă — `redo` nou —, Selectează tot, Caută,
  Înapoi / Înainte, Zoom, Temă, Notificări…). Locale ro / en (`[menu_drawer]`,
  `tooltip.menu`, `command.view_menu`, `settings.window_menu*`). Instantaneele: variabila
  `EDITOR_SNAPSHOT_MENU_DRAWER=closed|open|keys:<u d l r>|filter:<text>` (pornește setarea în
  memorie). Teste: bara (butonul primul, proiectul mutat după el), drawer-ul (categoria →
  submeniul alături și lărgirea, clic pe item, clic în afară, întârzierea hover-ului,
  filtrul, subsolul pe stări, fereastra scundă care derulează, ieșirea), aplicația (nimic
  înainte de deschidere, aruncat după, fără cadre după; fără buton nu se deschide, nici peste
  un modal; tastatura rulează pe calea meniului nativ, Esc golește apoi închide; clic în
  afară, clic pe item), Alt singur, setarea, tastele presetărilor (F10 în `keymaps.golden`).
- [x] 49c — după ce userul a văzut drawer-ul: pe macOS butonul ☰ și drawer-ul sunt implicit
  și ele (setarea `WINDOW_MENU` implicit pornită, se poate opri doar pe macOS; meniul nativ
  rămâne). Drawer-ul pe toată înălțimea ferestrei, peste bara de titlu: sus un rând-antet cât
  bara, cu butonul ☰ redesenat exact unde e în bară (rămâne pe loc cât drawer-ul glisează sub
  el; clic pe el îl închide), pe macOS loc lăsat butoanelor ferestrei ca în bară; filtrul și
  categoriile sub antet. Scrim pe toată fereastra, după luminozitatea temei (0.55 − 0.3 ×
  luminanța fundalului, între 25 % și 55 %: ~52 % în dark, ~27 % în light); butoanele
  ferestrei desenate de noi (Windows) lăsate neîntunecate (`TopBar::window_controls`), iar pe
  bară în afara drawer-ului clicul le ajunge în continuare (mutarea ferestrei, min / max /
  închide); pe Windows lovirea barei rămâne a aplicației (`DragWindow`), subclasa
  `caption_windows` nu se schimbă în afară de SC_KEYMENU. Muchii drepte (fără colțuri), fără
  umbră: o linie în culoarea `border` pe muchia din dreapta. Separatorul dintre panouri se
  stinge spre capete (`ds::divider_vertical_faded`, reutilizabil: 24 de trepte la pixeli
  întregi, sinus², fără gradient în renderer — nu există). Lustruire: toate rândurile 34 px,
  iconițele 16 px la 12 px și etichetele la 40 px de marginea rândului în ambele panouri,
  titlul submeniului aliniat cu etichetele; categoria deschisă cât tastatura / mouse-ul e în
  submeniu se citește „deschisă” (umplere neutră + semn accent la stânga, iconița în accent),
  rândul ales din submeniu în accent; ☰ din antet aprins neutru. Capturi noi (dark și light):
  `menu-button` (bara ca pe Windows, butonul primul), `menu-drawer`, `menu-drawer-submenu`
  (Fișier, al treilea rând ales), `menu-drawer-filter` („sal”), `menu-drawer-short`
  (1200 × 440: categoriile și Cod derulează), `menu-drawer-matrix` (ploaia Matrix rămâne în
  spate). Toate celelalte capturi: diferă doar în bara de sus (butonul ☰ implicit, proiectul
  mutat la dreapta — verificat automat: nicio diferență sub primii 80 px fizici), plus
  `settings-0`, `-12`, `-short`, `-theme-options` (doar bara de derulare: conținut mai lung —
  blocul „Meniu” la capătul paginii Aspect, comanda „Meniu · F10” în Scurtături) și `task-manager*` (memoria vie,
  zgomot cunoscut). Măsurat (release, fereastră 1200 × 800 cu un fișier): închis nu există
  nimic (`menu_drawer: None`), un cadru ~72 µs la fel ca înainte; deschiderea (meniurile
  citite, drawer-ul făcut) 0,30 ms, primul cadru cu el 0,25 ms, un cadru deschis ~60 µs;
  `MenuDrawer` 1 KB plus meniurile; după închidere și ieșire, aruncat și nicio cerere de
  cadre (`animating()` fals). Clippy Windows (`x86_64-pc-windows-gnu`) verde; pe un Windows
  real neîncercat: Alt singur (subclasa care înghite SC_KEYMENU), F10, hit-test-ul barei.
  Rămas: scurtăturile cu săgeți se arată ca „⌥Up” (`display_shortcut` nu traduce Up / Down;
  schimbarea ar atinge și Setări → Scurtături).

### Urmări (2026-11-03)
- [x] 49d — pâlpâirea submeniului (văzută de user în aplicație): mișcând mouse-ul peste
  itemii submeniului, categoria deschisă trecea între aspectul „ales” și cel „deschis”, iar
  uneori submeniul se strângea și revenea. Două cauze: (1) între rânduri, pe titlul
  submeniului sau pe un separator `hover_item(None)` scotea tastatura din submeniu (categoria
  redevenea „aleasă”); acum mouse-ul oriunde în submeniu ține tastatura acolo, rândul de sub
  el ales (între rânduri rămâne ultimul), iar categoria rămâne „deschisă”; înapoi pe categoria
  deschisă, tastatura revine pe categorii. (2) cât submeniul încă se lărgea (panoul lui nu
  avea încă lățime), `HoverIntent` îl socotea „nimic arătat” și trecea imediat pe categoria
  vecină atinsă în drum (submeniul schimbat, conținutul refăcut cu fade); acum o categorie
  arătată, chiar nedesfăcută încă, cere pauza scurtă. Teste: o succesiune de mișcări peste
  submeniu (rânduri, goluri, separator) cu cadre pictate între ele, cât se lărgește: categoria
  rămâne aceeași, tastatura în submeniu, lățimea doar crește până la cea plină. Tot aici:
  `display_shortcut` arată săgețile (Up / Down / Left / Right → ↑ ↓ ← →) pe orice sistem
  (`⌘⇧↑`, `Ctrl+Shift+↑`); capturi refăcute: `menu-drawer-short`, `menu-drawer-matrix`.
