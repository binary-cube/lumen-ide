# 20. Terminal colorat și configurabil

Cererea: ieșirea colorată din oficiu (ca în terminalele obișnuite), o zonă de configurare
(Setări → Terminal), să meargă la fel pe Windows (PowerShell / cmd / Git Bash) și, opțional,
prompt-ul și mesajele shell-ului colorate din tema aleasă. Notată în planul 11 („Terminal
colorat”).

## Azi (inventar, 2026-10-22)

- `infra/pty`: `alacritty_terminal` 0.26 — openpty pe Unix, ConPTY pe Windows (nu
  portable-pty, cum spun încă `pty/Cargo.toml` și planul 02). Mediul: `TERM=xterm-256color`,
  `COLORTERM=truecolor`, `TERM_PROGRAM`; **lipsesc** `CLICOLOR` / `LSCOLORS` / `LS_COLORS`
  (deci `ls` pe macOS e alb) și un `LANG` UTF-8 (pornită din Finder, aplicația poate să nu-l
  aibă).
- Shell-ul: niciun profil ales — cel implicit al lui alacritty: `$SHELL` (macOS: shell de
  login; Linux: nu de login), pe Windows `powershell`. Nicio detecție (pwsh, cmd, Git Bash,
  zsh, bash, fish).
- `modules/tools/terminal`: culorile din temă (`terminal.background / foreground / cursor`,
  `terminal.ansi.*` — cele 16; cubul 256 și truecolor calculate), OSC 4 respectat. Înălțimea
  rândului fixă (1,35), cursorul fără clipire, italicul nedesenat, cursorul ales doar de
  program.
- Setări: niciuna pentru terminal; rândurile contribuite au doar `Choices`, `Slider`, `Status`
  (fără comutator, fără câmp de text).

## Ținta

- **Colorat din oficiu**: `ls`, `grep`, `git`, prompt-ul — cu culorile temei (paleta ANSI vine
  deja din temă, deci arată bine în orice temă).
- **Setări → Terminal** (pagina modulului, după „Editor”), totul aplicat pe loc, în toate
  terminalele deschise unde se poate:
  - *Shell-ul*: cele găsite pe sistem (macOS / Linux: `$SHELL`, zsh, bash, fish; Windows:
    PowerShell 7, Windows PowerShell, cmd, Git Bash) + „Altul…” (calea și argumentele). Pentru
    terminalele noi.
  - *Culorile*: după temă (implicit) / clasice (xterm) / contrast mare — doar terminalul, nu
    restul aplicației.
  - *Prompt-ul colorat din temă* (comutator, oprit implicit): utilizator@gazdă, directorul,
    ramura git și simbolul în culorile temei; oprit → prompt-ul shell-ului, ca acum.
  - *Cursorul*: bloc / bară / subliniere, clipire da / nu (programele care își cer cursorul îl
    au pe al lor).
  - *Textul*: mărimea fontului, înălțimea rândului (slidere), aldin ca bright (opțional).
  - *Istoricul*: câte rânduri păstrează (1.000 … 100.000; implicit 10.000).
- **Windows**: aceleași culori și setări, ConPTY cu secvențele VT; Git Bash `--login -i`;
  cmd / PowerShell cu ieșirea colorată unde shell-ul o știe.

Reguli: totul în modulul `terminal` (setările lui în `src/settings.rs`, prin
`SettingsContribution`); domeniul (`terminal-session`) primește profilul shell-ului și paleta
aleasă, fără UI; nimic greu pe UI thread (detecția shell-urilor o dată, în fundal, ținută în
cache); nicio variabilă a mediului procesului schimbată (doar a shell-ului pornit).

## Pașii (un commit fiecare; `check.sh`; „go” între ei)

- [x] **20a. Colorat din oficiu + shell-urile găsite.** Mediul shell-ului: `CLICOLOR=1`,
      `LSCOLORS` / `LS_COLORS` (pe culorile ANSI, deci ale temei), `LANG` UTF-8 dacă lipsește,
      `TERM_PROGRAM_VERSION`; shell de login și pe Linux. Detecția shell-urilor (domeniu: lista
      și alegerea implicită; infra: căutarea pe disc / în PATH), Git Bash `--login -i` pe
      Windows. Teste (mediul, detecția cu un sistem fals), captură cu `ls` colorat.
      *Făcut:* `pty::shell_env` — `CLICOLOR=1`, `LS_COLORS` / `LSCOLORS` (folderele albastre
      aldine, link-urile cyan, programele verzi: indici ANSI, deci culorile temei),
      `COLORTERM`, `TERM_PROGRAM_VERSION`, `LANG=en_US.UTF-8` pe Unix — fiecare doar dacă
      nici profilul, nici mediul utilizatorului nu-l au; `TERM` / `TERM_PROGRAM` ale noastre.
      Domeniul `terminal_session::shells` (pur, cu portul `ShellProbe`): Unix — shell-ul
      utilizatorului primul, apoi zsh / bash / fish (și din Homebrew, și din `PATH`), toate de
      login (`-l`, deci și pe Linux); Windows — PowerShell 7, Windows PowerShell, cmd, Git
      Bash (`--login -i`, lângă `git.exe` sau în locurile obișnuite); implicit primul; ales
      după id (`zsh`, `pwsh`, `git-bash`…), unul dispărut → implicitul. `pty::shells()` (o dată,
      în cache), `pty::profile(id, cwd)`. Teste: detecția pe sisteme false (macOS, Linux,
      Windows), mediul, un shell real care îl vede. Captură: `ls -la` colorat, diacritice.
- [x] **20b. Rândurile noi din setări + pagina Terminal.** `SettingsRow::Toggle` și
      `SettingsRow::Text` (un câmp, ca al căutării în setări) în contract, desenate de
      fereastra de setări pentru orice modul; pagina „Terminal” a modulului cu shell-ul,
      culorile, cursorul, textul, istoricul; valorile ajung la terminal prin `apply` (ca la
      Local History). Teste, capturi (temă închisă / deschisă).
      *Făcut:* contractul (`workbench-model`) are `SettingsRow::Toggle` (comutator → `choose`
      cu 1 / 0) și `SettingsRow::Text` (câmp → `set_text`), desenate de fereastra de setări
      pentru orice modul; câmpul primește tastele cât e scris (nu căutarea), Enter / un clic
      în altă parte îl salvează, Esc renunță (fereastra rămâne deschisă). Pagina „Terminal”
      (după „Editor”) a modulului: blocul **Shell** (cele găsite + „Altul…” cu comanda, o
      cale cu spații între ghilimele) și **Afișare** (culorile: ale temei / clasice /
      contrast mare; cursorul: bloc / bară / subliniere; clipirea; mărimea fontului — ca
      editorul sau 11…18; înălțimea rândului 1,00…1,70; aldin luminos; istoricul 1k…50k).
      Setările `[terminal] …` în `terminal-panel/src/settings.rs`, ajunse prin `apply` la
      `config::configure`; taburile noi pornesc shell-ul ales. Implicitele = terminalul de
      până acum. Teste: salvarea fiecărei alegeri, shell-ul după id și „Altul…”, comanda
      despărțită, câmpul de text (scris / salvat / renunțat). Capturi: pagina, câmpul.

- [x] **20c. Desenarea după setări.** Înălțimea rândului și mărimea fontului din setări,
      cursorul implicit și clipirea (`ui_kit::caret`, aceeași ca în editor: cadre doar la
      schimbare), italicul, aldin ca bright, paletele clasică și contrast mare, istoricul
      (`scrolling_history`). Teste pe culori, capturi.
      *Făcut:* fiecare terminal citește setările la desenare (`config::current`, un `Arc`):
      mărimea fontului (ca editorul sau a lui) și înălțimea rândului; culorile (`colors.rs`:
      ale temei / xterm clasice pe fundalul temei / contrast mare — fiecare culoare împinsă
      spre text până la 4,5:1 pe fundal, textul alb / negru), calculate o dată pe temă și
      alegere; italicul (4 fețe: normal, aldin, italic, aldin italic); aldin luminos (primele
      8 culori → cele luminoase); cursorul implicit și istoricul trimise sesiunii doar la
      schimbare (`TermOptions` în port, `Term::set_options` în `pty`) — programele care își cer
      cursorul îl au pe al lor; clipirea cu `ui_kit::caret` (cadre doar la schimbare, solid la
      tastare). Capturile prind caret-urile aprinse (determinist). Teste: paletele (contrastul
      în temă închisă și deschisă), aldin luminos / inversul, opțiunile pe o sesiune reală.
      Capturi: tema, clasice + bară + rând 1,5, contrast mare + aldin luminos + font 15.

- [x] **20d. Prompt-ul colorat din temă (opțional).** zsh / bash / fish: prompt-ul dat
      shell-ului la pornire (fără să atingem fișierele utilizatorului: un `ZDOTDIR` /
      `--rcfile` temporar care încarcă întâi configurația lui, apoi doar prompt-ul nostru);
      PowerShell: funcția `prompt` la pornire. Culorile ca indici ANSI → ale temei. Oprit →
      neschimbat. Teste (scripturile generate), captură.
      *Făcut:* comutatorul „Prompt-ul în culorile temei” (blocul Shell, oprit implicit, pentru
      taburile noi). Domeniul `terminal_session::prompt` (pur): zsh — `ZDOTDIR` spre fișierele
      noastre (`.zshenv`, `.zprofile`, `.zshrc`), fiecare citind întâi pe al utilizatorului
      (și un `ZDOTDIR` pe care și-l setează el), apoi prompt-ul (`vcs_info` pentru ramură),
      la final `ZDOTDIR` înapoi; bash / Git Bash — `--rcfile` care citește fișierele de login
      (`/etc/profile`, `.bash_profile` / `.bash_login` / `.profile`), apoi `PS1`; fish — `-C`;
      PowerShell — `-NoExit -Command` cu `prompt`; cmd — `PROMPT` (directorul și semnul:
      cmd nu arată variabile acolo); alte shell-uri / comanda proprie neschimbate. Culorile:
      indici ANSI → ale temei. `pty::themed_prompt` scrie fișierele în `terminal-prompt` din
      folderul temporar. Teste: ce primește fiecare shell; zsh și bash reale (configurația
      lor citită, prompt-ul nostru, `ZDOTDIR` înapoi). Captură: prompt-ul cu ramura git.
      Mesajele shell-ului („Last login…”) — nu mai apar: shell-ul pornește direct ca shell de
      login (`-l`), nu prin `login(1)`.

- [x] **20e. Windows și lista de verificare.** Profilele Windows (pwsh / powershell / cmd /
      Git Bash), culorile prin ConPTY, clippy Windows; capturi; planul 11 bifat; documentația
      (`pty` — alacritty, nu portable-pty).
      *Făcut:* profilele Windows (PowerShell 7 / Windows PowerShell / cmd / Git Bash) din 20a,
      prin ConPTY (alacritty); prompt-ul PowerShell trimis ca `-EncodedCommand` (UTF-16LE în
      base64: fără ghilimele pe care Windows PowerShell 5.1 le-ar citi altfel), Git Bash cu
      calea `--rcfile` cu `/`; clippy pentru Windows. Capturi: tema închisă și cea deschisă
      (terminalul cu `ls` și prompt-ul, pagina Terminal). Planul 11 bifat; documentația:
      `pty` = alacritty_terminal (planul 02, `Cargo.toml`), cheile `terminal.*` din `[ui]`
      (planul 07). **Nerulat încă pe Windows.**

## Hotărât (2026-10-22)

1. Prompt-ul colorat: oprit implicit. 2. Shell-ul implicit: al utilizatorului (pe Windows
   PowerShell 7 dacă e instalat). 3. Setările: globale.

## De hotărât (răspuns mai sus)

1. Prompt-ul colorat: **oprit implicit** (unii își au deja prompt-ul lor, ex. oh-my-zsh,
   starship) — propun așa.
2. Shell-ul implicit: cel al utilizatorului (`$SHELL`; pe Windows PowerShell 7 dacă e instalat,
   altfel Windows PowerShell).
3. Setările: globale (ca tema), nu per proiect.
