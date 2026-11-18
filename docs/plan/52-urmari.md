# Plan 52 — Urmări: istoricul Git al unui fișier, terminalul

## Context
Văzute de user în aplicație, după planurile 49–50: în tab-ul „Istoric” al unui fișier
(fereastra Git) detaliile commit-ului nu derulează (un mesaj lung tăiat cu „…”, „Arată toate
fișierele din commit” și lista de fișiere de neatins), rândurile n-au meniu la clic dreapta;
terminalul n-are bară de derulare, iar derularea pare întârziată și sacadată.

## Stare
- [x] 52a — istoricul Git: detaliile commit-ului derulează (id-ul, data și autorul rămân
  sus; dedesubt mesajul întreg — fără limita de 16 rânduri și fără „…” — apoi rândul cu
  fișierele schimbate și lista lor întreagă, fără „încă N fișiere”), cu `ScrollState` și
  `EdgeScrollbar` pe banda lor din dreapta (conținutul îi lasă loc cât derulează; bara trasă
  sau clicul pe ea). Rotița peste detalii le derulează pe ele (nu lista commit-urilor),
  niciodată dincolo de capăt; alt commit ales: de sus. Doar rândurile vizibile sunt
  desenate și clicabile. Clic dreapta (vederile ferestrelor de jos primesc acum meniul lor:
  `tool_view_menu_at` / `run_tool_view_menu`, prefixul `tool-view-menu:`, cererile lor —
  copierea, deschiderea, comparațiile — ajung în aplicație): pe un commit (ales odată cu
  meniul) — „Arată toate fișierele din commit” / „Doar fișierul acesta” (doar în istoricul
  unui fișier), Copiază hash-ul, Copiază mesajul (întreg; rezumatul cât detaliile nu sunt
  citite), Compară cu revizia anterioară, Compară cu copia de lucru (fișierul istoricului,
  versiunea commit-ului în stânga, doar citire); pe un fișier din detalii — Deschide fișierul,
  Arată schimbarea din acest commit, Copiază calea (relativă), Arată în panoul proiectului,
  Arată în managerul de fișiere. Adnotarea la o revizie nu există în modul, deci nu e în
  meniu. Locale ro / en (cheia `git.more_files` scoasă). Teste: detaliile unui commit cu
  mesaj lung și 54 de fișiere (mesajul întreg, nimic tăiat, ultimul fișier atins după
  derulare, capătul oprește rotița, antetul fix), meniurile (rândurile și ce face fiecare).
  Capturi: un depozit de probă al lor în scriptul de capturi (`$work/history`, autor și date
  fixe: un commit cu 54 de fișiere și un mesaj lung pe `.phpactor.json`) — noi
  `git-file-history-scrolled` (toate fișierele, derulat la capăt), `-commit-menu`,
  `-file-menu`, dark și light; `git-annotated` schimbat cum trebuie (lista întreagă cu bara,
  în loc de „încă 9 fișiere”).
- [x] 52b — terminalul. **Derularea** — măsurat înainte (release, o grilă 200 × 50 cu 5 000
  de rânduri de istoric, un `FakeSession`): din 240 de pași mici de trackpad (3 px fizici,
  sub o jumătate de rând fiecare) **niciunul** nu muta istoricul — `scroll` rotunjea fiecare
  pas la 0 rânduri și cerea niciun cadru (720 px pierduți, ~21 de rânduri), deci derularea
  stătea și sărea doar la un pas mai mare: „întârziată” și „sacadată”. Desenul nu era cauza:
  un pas cu cadru ~0,06 ms (p50), un pas de rotiță cu 3–4 rânduri noi de format ~0,6 ms (p50;
  p95 0,67 ms, max 0,69 ms). Acum pașii se adună (rămășița sub un rând păstrată): toți cei 20
  de rânduri ajung, câte un cadru pe rând (p50 0,06 ms, p95 0,19 ms); vederea citește grila
  din nou și se repictează imediat, fără trezirea în plus de la PTY (`scroll` nu mai marchează
  „dirty”). Derulare la pixel (sub-rând) nu există: grila are doar rânduri întregi.
  Neverificat pe aplicația reală: contenția lacătului emulatorului cu firul PTY la ieșire
  multă (FairMutex-ul alacritty, care cedează rândul). **Ecranul alternativ** (vim, htop, un
  TUI): fără istoric acolo, deci fără bară — iar rotița nu ajungea nicăieri (`scroll_display`
  nu face nimic acolo); acum merge la program pe loc: ↑ / ↓ (în modul aplicației cum cere)
  dacă programul ține „alternate scroll” (implicit), raportul rotiței (SGR `ESC[<64;x;yM` sau
  clasicul `ESC[M…`, la celula de sub mouse) dacă a cerut mouse-ul — și pe ecranul normal
  (tmux, less cu mouse); `terminal_session::encode_wheel`. **Bara de derulare**:
  `EdgeScrollbar` pe banda ei din dreapta grilei (păstrată mereu: coloanele nu se schimbă când
  apare istoricul, nu acoperă textul), arătată doar cu istoric și nu pe ecranul alternativ,
  locul ecranului în istoric (jos cât urmărește ieșirea), trasă sau apăsată mută istoricul;
  `GridSnapshot` știe acum `history`, `alt_screen`, `mouse`, `alternate_scroll`. **Tastele**:
  ⌘V lipea deja (Paste ajungea la terminal); pe Windows / Linux Ctrl+Shift+V și Shift+Insert
  lipesc acum (Ctrl+V rămâne al programului; `NamedKey::Insert` nou); lipirea înfășurată
  (`ESC[200~ … ESC[201~`) când programul a cerut-o, rândurile noi ca `\r`, iar în lipirea
  înfășurată nu mai rămâne niciun ESC (nu doar `ESC[201~`): textul lipit nu poate trimite
  taste. ⌘C copiază selecția; fără selecție, cât terminalul are tastatura, nimic (nu mai cade
  pe selecția editorului); Ctrl+C rămâne SIGINT. Shift+Enter: `ESC CR` (meta-Enter, rând nou
  fără a trimite linia — readline, Claude Code, cum leagă terminalele editoarelor prin `/terminal-setup`,
  iTerm2, WezTerm când programul n-a cerut protocolul de taste kitty; emulatorul nostru nu-l
  pornește — `kitty_keyboard` oprit în alacritty_terminal — și nici modifyOtherKeys nu e
  urmărit, deci `CSI 13;2u` / `CSI 27;2;13~` nu se trimit); Alt / ⌥+Enter: tot `ESC CR`;
  Enter simplu neschimbat. Tastele scrise în Setări → Terminal → Shell (notele blocului, pe
  sistemul fiecăruia; ro / en). Teste: pașii mici adunați (fiecare rând un cadru, jos fără să
  treacă), bara (banda ei, jos, apăsarea mută istoricul, fără istoric fără bară), ecranul
  alternativ (fără bară, săgețile trimise) și mouse-ul cerut (raportul SGR), codările rotiței,
  Shift / Alt+Enter, lipirea fără ESC. Captură nouă `terminal-scrollback` (dark și light).
- [x] 52c — istoricul terminalului „dispărut” (userul, cu o conversație lungă Claude Code pe
  ecranul principal: bara sus, ieșirea de la început nu mai era, nici după redimensionare).
  Verificat pe emulator (`alacritty_terminal`, fără PTY): 5 000 de rânduri, apoi o regiune de
  derulare cu marginea de sus pe primul rând (DECSTBM 1;20) — ce iese din ea intră în istoric,
  ca la xterm / Ghostty —, ED 2 (`ESC[2J`: ecranul împins în istoric, nu pierdut), redesenări
  în stilul Ink (cursorul sus, ștergere dedesubt) și două redimensionări (reflow-ul nu pierde
  rânduri): primul rând scris rămâne în vârful istoricului, iar intervalul derulării vederii e
  lungimea istoricului (`GridSnapshot::history` = `history_size`, bara sus = cel mai vechi
  rând). Singurul lucru care golește istoricul e `ESC[3J` (ED 3), cerut explicit de program —
  iar Claude Code îl trimite singur: Ink-ul lui (`ink/clearTerminal.ts`) scrie
  `ESC[2J ESC[3J ESC[H` (în afară de consolele vechi Windows) ori de câte ori ieșirea lui e
  mai înaltă decât ecranul, la fiecare redesenare în starea aceea (raportat în issue-urile
  lui; pe Windows Terminal îi strică chiar și IME-ul). E alegerea programului: terminalele
  care respectă ED 3 (xterm, Ghostty, alte terminale pe `alacritty_terminal`, Windows Terminal) fac
  la fel; n-am un mod de a-l ignora fără a filtra fluxul PTY înaintea parserului alacritty
  (bucla lui citește direct), deci nu l-am schimbat. Plafonul rămâne 5 000 de rânduri implicit
  (Setări → Terminal: până la 50 000): un rând ține celulele pe toată lățimea (~24 B / celulă),
  10 000 de rânduri × 200 de coloane ≈ 48 MB pe un terminal plin — n-a fost cauza aici. Test:
  `the_history_survives_regions_erases_redraws_and_resizes` (prin aceeași copiere a grilei pe
  care o folosește vederea, `snapshot_of`, scoasă din `PtySession::snapshot`). Derularea la
  pixel nu: grila alacritty și `display_offset` sunt pe rânduri întregi; ar cere un rând în
  plus copiat și desenat decalat — nu e ieftin, lăsat.
