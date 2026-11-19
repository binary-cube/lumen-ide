# 85 — Actualizarea pusă pe loc de aplicația însăși

Plan 84f pune actualizarea pe loc printr-un **ajutor**: aplicația, la închidere, își copiază
programul în folderul temporar și îl pornește; ajutorul așteaptă să se închidă aplicația,
mută versiunea nouă în locul celei vechi și pornește aplicația din nou.

## De ce schimbăm

- **macOS.** Ajutorul e programul scos din pachetul `.app`, singur în folderul temporar, deci
  semnătura lui nu mai e validă (ea acoperă tot pachetul). Ajutorul pornea aplicația nouă
  direct, iar macOS îl ținea pe el răspunzător pentru tot ce făcea aplicația. De aceea accesul
  la Documents era cerut iar și iar și nu se ținea minte pentru nimeni. Reparația din `beta.8`
  (`open -n` în loc de pornirea directă) scoate simptomul, dar ajutorul rămâne: un program
  fără identitate, care rulează din folderul temporar.
- **Peste tot.** Un al doilea program care rulează din folderul temporar e o piesă în plus:
  antivirusurile îl privesc cu suspiciune, copiile rămân în urmă (`clean_helpers`), iar o
  eroare a lui nu se vede nicăieri.
- **Ce permit sistemele.** Pe macOS și Linux, fișierele unei aplicații pornite se pot muta și
  înlocui: programul pornit e deja în memorie. Pe Windows, un `.exe` pornit **nu** se poate
  șterge sau suprascrie, dar **se poate redenumi** (pe același disc). Deci în niciun caz nu e
  nevoie de un al doilea program care să aștepte.

## Ținta

Aplicația pune singură versiunea nouă pe loc, **cât încă rulează**, apoi se închide. Pornirea
din nou o face sistemul (macOS), o comandă mică a shell-ului care așteaptă să se închidă
procesul vechi (Linux) sau programul nou, care așteaptă singur să se închidă cel vechi
(Windows). Nu mai există copie în folderul temporar și nici ajutor.

## 85a — Mutarea pe loc, în aplicație (toate sistemele) — făcut

- În `self-update`, `install::apply(dir, target)` rămâne cum e (pusă deoparte → mutată →
  pusă înapoi la eșec), dar e chemată **de aplicație**, la „Restart Now”, înainte de
  închidere, nu de ajutor.
  - Pe macOS: `Lumen.app` → `Lumen.app.old`, apoi `staged/Lumen.app` → `Lumen.app`.
  - Pe Windows: `lumen.exe` → `lumen.exe.old` (redenumirea e permisă cât rulează), apoi cel
    nou → `lumen.exe`.
  - Pe Linux: la fel, programul.
- Dacă mutarea eșuează, se pune înapoi ce era, aplicația **nu se închide** și „What's New”
  arată de ce (`download_failed` / un `Failed` nou: „Nu s-a putut pune pe loc: …”).
  Utilizatorul își păstrează lucrul deschis.
- Ce a ieșit se notează (`result.toml`) înainte de închidere, ca acum: aplicația pornită din
  nou spune „Actualizat la X”.
- `.old` se șterge la pornirea următoare (`clean_left` există deja). Pe Windows se încearcă
  de câteva ori, pentru că Explorer sau un antivirus îl pot ține deschis o clipă. Dacă tot nu
  merge, rămâne pentru data viitoare.
- Teste: mutarea și punerea înapoi într-un folder temporar (deja acoperite pentru `apply`),
  plus aplicația care nu se închide la eșec.

## 85b — Pornirea din nou — făcut

- **macOS**: la închidere, aplicația lansează
  `/bin/sh -c 'while kill -0 $0 2>/dev/null; do sleep 0.1; done; open "$1" --args "$@"'`
  cu PID-ul ei, calea pachetului și `--reopen-session <id>`. Pornirea o face sistemul
  (`open`), deci aplicația nouă e răspunzătoare de ea însăși și permisiunile date rămân.
  Fără `-n`: când rulează `open`, procesul vechi nu mai există.
- **Linux**: la fel, cu `exec "$1" "$@"` în loc de `open` (nicio regulă de identitate de
  ocolit).
- **Windows**: nu folosim shell. Aplicația pornește direct `lumen.exe` (cel nou, deja pus pe
  loc) cu `--wait-for <pid> --reopen-session <id>`. Programul nou, la început, înainte de
  orice fereastră, deschide procesul vechi (`OpenProcess(SYNCHRONIZE)`) și îl așteaptă
  (`WaitForSingleObject`, cel mult 30 s), apoi pornește normal. Așa nu se suprapun două
  instanțe peste aceeași sesiune.
- Procesul pornit e detașat (`detach`, ca acum): un Ctrl-C în terminalul din care a pornit
  aplicația nu-l oprește.
- Teste: comanda compusă pe fiecare sistem (argumentele, citarea căilor cu spații), plus
  `--wait-for` cu un PID care nu există (pornește imediat).

### Cum a ieșit (85a + 85b)

- `self_update::install::put_in_place(dir)` mută versiunea nouă pe loc; o face modulul de
  actualizări la „Restart Now”, pe loc, înainte de închidere. La eșec spune de ce
  (`updates.cant_place`), iar aplicația nu se închide. Merge o singură dată pe același
  update (`state::is_placed`).
- „Later” din „What's New” o pune și ea pe loc, fără să spună nimic, deci pornirea următoare
  e versiunea nouă. Dacă renunți la închidere (Cancel la fișierele nesalvate), versiunea nouă
  rămâne pe loc pentru pornirea următoare.
- `install::place()` află locul aplicației o dată, la pornire. După mutare, calea propriului
  program poate arăta spre `.old` (Linux, Windows).
- Pornirea din nou: `install::restart(session)` → `restart_command`. Pe macOS e
  `open -n <app> --args --wait-for <pid> --reopen-session <id>`, iar în rest programul,
  direct, cu aceleași argumente. Așteptarea o face programul nou, oriunde, în `main`, înainte
  de verificarea de instanță unică (`install::wait_for`, prin `sysinfo`, cel mult 30 s), nu
  un script de shell.
- `.old` rămâne până la pornirea următoare (`clean_left`), pentru că programul vechi încă
  rulează din el.

## 85c — Windows: instalatorul, când aplicația a venit din el

Instalată prin `lumen-<v>-windows-x64-setup.exe`, aplicația are o intrare în Settings › Apps
cu versiunea ei. Dacă înlocuim doar `.exe`-ul, intrarea rămâne cu versiunea veche.

- **Cum știm**: lângă program e `unins000.exe` (pus de instalator), sau cheia de dezinstalare
  cu `AppId`-ul nostru.
- **Atunci**: se descarcă `…-setup.exe` (o a doua intrare pe Windows în `latest.json`, cu
  SHA-256 și semnătura manifestului, ca arhiva), în loc de `.zip`. La „Restart Now”, aplicația
  pornește instalatorul `/VERYSILENT /SUPPRESSMSGBOXES /NORESTART /MERGETASKS=!desktopicon
  /REOPEN=<id>` și se închide. Instalatorul are deja `CloseApplications=yes`, deci așteaptă
  închiderea. La final, o intrare `[Run]` cu `Check: WizardSilent` pornește aplicația cu
  `--reopen-session` din parametrul `/REOPEN`.
- Dacă instalatorul eșuează, se vede la pornirea următoare: instalatorul scrie `result.toml`
  doar la succes, iar aplicația care pornește fără el și cu versiunea veche spune
  „Actualizarea nu s-a terminat”.
- Fără instalator (din `.zip`, mutată oriunde): 85a + 85b.
- Teste: alegerea arhivei sau a instalatorului din manifest după cum e instalată aplicația,
  plus argumentele instalatorului.

## 85d — Ce dispare

- `start_helper`, `apply_main`, `HELPER_ARG`, `HELD` (pipe-ul), copia din folderul temporar.
- `clean_helpers` mai rămâne **o versiune**, ca să șteargă copiile lăsate de versiunile
  vechi, apoi pleacă și el.
- Compatibilitate: o versiune veche care se actualizează la una nouă folosește încă ajutorul
  ei (copia programului vechi). Acesta pune pe loc versiunea nouă și o pornește cu
  `--reopen-session`. Versiunea nouă nu are nevoie să înțeleagă `--apply-update`.

## 85e — Verificarea

- Unitare: 85a–85c, plus `./scripts/check.sh` (clippy și pe Windows).
- Pe bune, cu două versiuni de test publicate ca pre-release:
  - macOS, din `/Applications`: update, repornire, ferestrele redeschise, **nicio** cerere
    pentru Documents după ce a fost dată o dată, nimic rămas în folderul temporar;
  - Windows din `.zip`: update, repornire, `.old` șters la pornirea a doua;
  - Windows din instalator: Settings › Apps arată versiunea nouă, scurtăturile merg;
  - eșecul: un folder fără drept de scriere, deci aplicația rămâne deschisă și spune de ce.

## Ordinea

85a + 85b (macOS / Linux / Windows din zip) → o lansare → verificarea → 85c (instalatorul) →
o lansare → 85d.
