# Plan 54 — Mici reparații: suprapunerea de diagnostic, istoricul terminalului, navigarea din setări, butonul de încărcare

## Context
Patru lucruri mici, văzute în aplicație: suprapunerea de diagnostic (F12) acoperă butoanele
din bara de sus și are culori fixe; programele care șterg istoricul terminalului la fiecare
redesenare (`ESC[2J ESC[3J ESC[H`, vezi 52c) îl pierd pe al utilizatorului; antetele
grupurilor din navigarea setărilor sunt simple; butonul „Încarcă tot” din fereastra Remote
e butonul mare al dialogurilor, pus într-un antet.

## Stare
- [x] 54a — suprapunerea de diagnostic
- [x] 54b — istoricul terminalului față de programele care cer ștergerea lui
- [x] 54c — antetele grupurilor din navigarea setărilor
- [x] 54d — butonul „Încarcă tot” din Remote, compact

## 54a — Suprapunerea de diagnostic (F12, Ajutor › Suprapunere de diagnostic)
În colțul din dreapta jos al zonei de lucru (`Workbench::overlay_area`: între benzile de
unelte, sub bara de sus, deasupra barei de stare, la `ds::POPOVER_GAP` de ele), niciodată
peste bara de titlu. Doar desenată: nu primește nimic, clicurile ajung la ce e dedesubt.
Culorile din paletă: fundalul și marginea popover-elor (`popup_background`,
`popup_border`), raza lor (`ds::radius::MD`), numele backend-ului în culoarea textului,
cifrele estompate (`text_muted`), tot monospace. Tot un singur dreptunghi, ca înainte (fără
straturile de umbră). Test: panoul stă în colțul zonei date.

## 54b — Istoricul terminalului față de programele care cer ștergerea lui
Programele bazate pe Ink trimit `ESC[2J ESC[3J ESC[H` la fiecare redesenare când ieșirea lor
e mai înaltă decât ecranul (52c): `ESC[3J` (ED 3) șterge istoricul. Decizia e după
comportamentul proceselor, niciodată după numele unui program:
- **Cine are terminalul** (`terminal_session::history::Foreground`), întrebat doar când
  sosește un `ESC[3J`: pe Unix grupul de procese din prim-plan al PTY-ului (`tcgetpgrp` pe
  capătul master) — al shell-ului (lider de sesiune: grupul lui e pid-ul lui) sau al unei
  comenzi, a cărei vârstă vine din momentul pornirii liderului (macOS `proc_pidinfo`, Linux
  `/proc/<pid>/stat` față de `/proc/uptime`; ținut minte pentru același grup). Pe Windows
  (ConPTY n-are grupuri de procese): copiii direcți ai shell-ului (Toolhelp) și vârsta celui
  mai tânăr (`GetProcessTimes`; niciunul: shell-ul însuși; un copil pornit înaintea
  shell-ului — pid refolosit — nu contează); verdictul ținut 500 ms (o privire la toate
  procesele costă câteva ms).
- **Regula** (`honors`): Standard — mereu respectat; Păstrat pentru programele care rulează
  (implicit) — respectat de la shell și de la o comandă pornită acum (< 1 s: `clear`,
  `reset`, `tput clear`), ignorat cât un program rulează de mai mult în prim-plan (un TUI,
  un agent, un REPL, un watcher); Păstrat mereu — niciodată. Neștiut (sistemul n-a spus):
  respectat, ca un terminal obișnuit. Ecranul e șters / redesenat oricum (`ESC[2J` rămâne),
  doar istoricul e păstrat.
- **Unde e interceptat**: bucla lui alacritty citește și parsează în ea însăși, fără cârlig
  între citire și parser, deci cititorul pe care îl primește e al nostru (`pty::filtered`:
  PTY-ul învelit, `EventedReadWrite` / `EventedPty` / `OnResize` date mai departe). Un
  automat (`EraseFilter`, în domeniu) găsește doar `ESC [ Ps J` cu Ps = 3 (fără marcaj
  privat, fără intermediari; primul parametru, ca emulatorul), și peste granițele citirilor:
  octeții unei secvențe neterminate sunt ținuți (cel mult 32), restul trece neschimbat și în
  ordine; fără `ESC` în ce s-a citit, nimic nu e copiat.
- **Golește terminalul** (`terminal.clear`): ⌘K pe macOS, Ctrl+Shift+K pe Windows / Linux
  (Ctrl+K e al shell-ului: taie până la capătul rândului) — istoricul gol mereu, rândul
  cursorului (promptul) mutat sus, restul ecranului gol (pe ecranul propriu al unui program
  full-screen doar istoricul). Comanda e a vederii terminalului (`ViewAction`): vederea care
  ia toată tastatura are acum locul ei în keymap (`keyed_panel_view`), tastele ei ajung la
  aplicație doar ca scurtături ⌘ / Ctrl+Shift.
- **Setări → Terminal**: „Istoricul când un program cere ștergerea lui” (`history_clear`:
  `standard` / `keep-running` / `keep-always`), ro / en; ajunge la sesiunile deschise prin
  `TermOptions` (un atomic citit de firul PTY-ului).
- Teste: automatul (secvența tăiată la fiecare octet, celelalte secvențe neatinse, primul
  parametru, una prea lungă), regulile; cititorul filtrat cu un PTY fals (un program lung
  care redesenează păstrează istoricul, de la shell / o comandă scurtă e golit, fiecare
  valoare a setării, ieșirea fără secvență neschimbată); un shell interactiv real (Unix, cu
  job control): un program care rulează de 1,3 s și cere ștergerea — păstrat; o comandă
  scurtă — golit; Golește terminalul — golit; ⌘K / Ctrl+Shift+K în terminal; setarea.
- Netestat pe Windows (doar compilat și clippy): dacă ConPTY trece `ESC[3J` mai departe
  așa cum îl trimite programul.

## 54c — Antetele grupurilor din navigarea setărilor
`form::nav_caption`: numele grupului (majuscule mici, acum aldine) urmat pe același rând de
o linie care începe imediat după text, mai tare lângă el și stingându-se spre marginea din
dreapta (`ds::divider_faded_right`: pași de pixeli întregi, culoarea dată la început,
transparent la capăt — render-api n-are gradiente). Grupul paginii deschise: numele și
începutul liniei în accent, discret. Fără bară verticală înainte. Mai mult loc (8 px) deasupra
fiecărui grup în afară de primul (și în înălțimea listei, pentru derulare). Așezarea îngustă
(două rânduri de taburi) neschimbată. Sertarul meniurilor n-are antete de grup (titlul
panoului și separatoare), deci nimic de schimbat acolo. Teste: linia (pixeli întregi, de la
început la capăt, tot mai slabă), antetul (linia după nume, pe rândul lui, accentul doar
pentru grupul deschis).

## 54d — Butonul „Încarcă tot” din Remote, compact
Butoanele din antetul ferestrei Git (Commit / Reîmprospătează / Fetch / Pull / Push) devin
o componentă: `ds::toolbar_button` (`ToolbarButton`: eticheta, iconița, accentul, hover;
`toolbar_button_width`, `TOOLBAR_BUTTON_H` = 30 px, fontul `toolbar_button_font`) —
bordat, iconița apoi eticheta, cel principal colorat discret în accent (nu umplut). Jurnalul
Git o folosește (aceleași dimensiuni ca înainte). Schimbate la ea, cu accentul (acțiunea
principală):
- Remote › „N fișiere de încărcat”: „Încarcă tot” (iconița `upload`) — era butonul mare al
  dialogurilor (34 px, umplut).
- Git › Modificări: „Commit…” din antet (iconița `git-commit`) — același caz.
Căutate în toate modulele celelalte butoane principale: restul sunt în subsolul dialogurilor
și al cardurilor (unde rămân), bara serverelor de limbaj (o bandă de notificare, deja
compactă, 26 px), „Adaugă efect” din Theme Studio (un rând de listă), butonul binarului (în
mijlocul stării goale) și cel de commit de lângă câmpul mesajului — lăsate așa.
Test: butonul de antet (mai jos decât al dialogurilor, bordat, accentul colorat, nu umplut).
