# 32 — Review: securitate, Windows, corectitudine, performanță, DDD

Review-ul a avut cinci direcții:
- bug-uri din ultimele commit-uri;
- securitate;
- performanță;
- arhitectură;
- UX și Windows.

Bug-ul grav găsit în popover-ul de versiuni (Esc sau un click în afară nu-l închideau și
blocau aplicația) e deja reparat în `54d4c44`. Ce rămâne e mai jos, pe etape, în ordinea
priorității.

## 32a — Securitate (un proiect descărcat nu trebuie să poată face nimic singur)

1. **`.lumen/remote.toml` din proiect pornește singur o sesiune remote** la deschidere
   (`remote-projects/config.rs:174`, `session/service.rs:41`).
   - Fiecare salvare se urcă pe serverul scris în fișier.
   - `trusted` pre-acceptă certificatul.
   - `local` poate trimite fișierele în afara proiectului.
   - De făcut:
     - o conexiune venită din proiect cere aprobare o singură dată, ca la quality, iar
       aprobarea se ține în afara proiectului;
     - pentru `SavedIn::Project`, `local` e forțat la rădăcina proiectului;
     - `trusted` din fișier e ignorat.
2. **`git status` automat poate rula cod** prin `core.fsmonitor` din `.git/config` (un
   proiect dintr-o arhivă).
   - Comenzile git automate primesc `-c core.fsmonitor=false`,
     `-c core.hooksPath=<gol>`, `-c protocol.ext.allow=never`.
3. **Numele excluse se compară cu majuscule și minuscule ca fiind diferite**
   (`domain/remote/lib.rs:196, 280`). Pe macOS și Windows, `.GIT/config` sau
   `.LUMEN/remote.toml` venite de pe server ajung în `.git` / `.lumen`.
   - Compararea devine insensibilă la majuscule.
   - Pe Windows se refuză și numele terminate în `.` sau spațiu, numele rezervate și
     formele `~N`.
4. **Seiful își poate face o cheie nouă și pierde toate parolele** când keychain-ul e
   blocat sau indisponibil (`secrets/lib.rs:21, 71`).
   - Lipsa intrării (`NoEntry`) se separă de celelalte erori.
   - Cheia se face doar dacă seiful e gol.
5. **Imaginile din Markdown** se citesc pe firul UI, fără limită de mărime și din orice
   cale (de exemplu `/dev/zero` sau un FIFO), iar decodarea nu are limite.
   - Se acceptă doar fișiere obișnuite, cu limită de mărime.
   - Decodarea se face pe un fir separat, cu `image::Limits`.
6. **Descărcarea modelului semantic** (`embed/lib.rs:60`) folosește o ramură mobilă, nu
   are SHA-256 și nu verifică lungimea, iar un header `s > e` produce panică.
   - Commit fixat, SHA-256 pe fișier, verificarea lungimii și a lui `s <= e`.
7. **Oglindirea copiei locale urmează symlink-urile** (`fs-local/remote_copy.rs:40`), deci
   poate urca `~/.ssh` printr-un link.
   - Link-urile se sar sau trebuie să rămână în proiect.
8. **Fișierul temporar al unei salvări** e creat cu umask-ul implicit (citibil de alții),
   fără `create_new` (`fs-local/lib.rs:69`).
   - Se creează cu modul originalului din start.
9. **Ieșirea proceselor nu are limită**, iar la timeout se omoară doar `sh`.
   - Limită de câțiva MB pe ieșire.
   - Grup de procese: `process_group` pe Unix, Job Object pe Windows.

## 32b — Corectitudine și Windows

1. **AltGr pe Windows** (Ctrl+Alt): `{ [ @ \ | ș ą` nu se scriu, iar `{` / `}` fac
   fold / unfold (`app/input.rs:176`, `app/mod.rs:560`). Textul tipărit cu Ctrl+Alt pe
   Windows e tratat ca text.
2. **Redenumirea doar a majusculelor** (`user.php` → `User.php`) e refuzată pe macOS și
   Windows, cu mesajul „există deja” (`files.rs:504`, `refactorings.rs:479`,
   `fs-local/lib.rs:167`).
   - Se verifică dacă ținta e chiar același fișier.
   - Textul „nume invalid” din cod trece în locale.
3. **Salvarea pe Windows:**
   - un fișier read-only lasă `.saving-*.tmp` în proiect;
   - un fișier blocat (antivirus, Dropbox) eșuează fără nicio reîncercare.

   Bitul read-only nu se mai copiază pe temp; se reîncearcă de câteva ori, apoi se scrie
   pe loc.
4. **Setările în fundal (31a):**
   - un `set` concurent poate fi suprascris în cache: e nevoie de un contor seqlock
     (înainte și după scriere);
   - un `settings.toml` stricat temporar resetează toate setările: se păstrează cache-ul
     dacă fișierul nu se poate parsa;
   - fișierul se citește o singură dată, nu o dată pe secțiune.
5. **Symfony „console ”** se declanșează pe orice linie care conține cuvântul, inclusiv
   comentarii și Markdown. Se cere `bin/console` / `php …console` sau tipul de fișier
   potrivit.
6. **Argumentele cu nume:**
   - parametrul activ se ghicește după prefix (`user` vs `userId`);
   - virgulele și `:` din stringuri sunt numărate;
   - argumentele cu nume sunt oferite și într-o declarație `function f(…`.
7. **Docblock-ul** se pierde după `#[Route('/x')] // notă`: revine verificarea pe o
   singură linie, ca rezervă.
8. **`#[AsCommand('app:x', 'Import: users')]`** își pierde descrierea.
9. **Câmpul de parolă:**
   - `Clone` nu păstrează capacitatea rezervată;
   - un `insert` lung (lipirea unei chei) crește bufferul și lasă copii.
10. **Mărunțișuri:**
    - **Linkuri:**
      - căile cu `$` în limbaje fără sigil (Blade, JS, shell) se văd ca linkuri:
        `$` revine ca exclus implicit;
      - `~\` din terminal pe Windows nu e recunoscut.
    - **Afișare:**
      - Structure scurtează `module.exports` la `exports`: se ține cont de separatorul
        limbajului;
      - tooltip-ul de eroare apare prin popover-e (`links.rs:205`, `overlay_open`).
    - **Windows:** „Reveal in Explorer” pe căi cu spații, cu `raw_arg`.
    - **Texte:** mesajul din watchdog e în română.

## 32c — Performanță

1. **Indexer-ul rescrie tot `symbols.bin` + `search.bin`** (zeci de MB, cu vectori) la
   fiecare 10 s cât timp editezi.
   - Salvare la inactivitate sau la ieșire.
   - Vectorii într-un fișier separat, scris doar când se schimbă.
2. **Cu Task Manager-ul deschis, fiecare wake re-măsoară tot pe firul UI** (inclusiv
   `SymbolIndex::memory()` sub lock). Throttling efectiv, iar memoria indexului devine o
   valoare ținută de indexer.
3. **Feed-ul de indexare** clasifică fiecare cale și face `exists()` pe firul UI
   (`npm install` înseamnă mii de căi). Se mută pe firul `index-queue`.
4. **Canvas-ul ține o clonă a indexului** cât timp construiește graful, iar orice scriere
   dublează tabelele (uneori pe firul UI).
5. **Rutele, șabloanele și comenzile Symfony** se recitesc de pe disc, pe firul UI, după
   fiecare salvare. Se invalidează doar la căi relevante.
6. **`files_generation` crește la schimbări în `var/cache` / `storage`.** Folderele
   dependențelor și cache-urilor se ignoră.
7. **⌘-hover:**
   - folosește `read()` blocant: trece pe `try_read`;
   - citește fișiere fără limită: se pune o limită;
   - copiază bufferul tabului: folosește textul din cache.
8. **Ferestrele remote** se redesenează la 60–120 fps minute întregi: trec pe `is_loading`.
9. **Completarea** alocă pentru fiecare candidat (sute de mii cu vendor). Fără alocări
   până la potrivire.
10. **Ștergerea unui folder indexat** rescanează tot proiectul: se scot doar cheile lui.
11. **Mărunțișuri:**
    - **Pe fiecare frame:**
      - căutarea în setări se reconstruiește;
      - log-ul git calculează lățimea grafului;
      - avertismentele se sortează.
    - **La cursor și tastare:**
      - Structure se recalculează la fiecare mișcare a cursorului;
      - `exists()` la fiecare tastă la o intrare nouă în arbore;
      - `is_file()` la fiecare mișcare a mouse-ului în terminal.
    - **Fișiere și căutare:**
      - utilizările numără liniile pătratic;
      - un fișier schimbat e citit și hash-uit de două ori.

## 32d — DDD și convenții

1. **Comenzi fără etichetă în locale**, deci afișate în engleză în UI-ul românesc:
   - `edit.select_all_occurrences` și `edit.unselect_occurrence`;
   - patru comenzi `quality.*`.

   Se adaugă etichetele, plus un test că orice comandă are cheia ei.
2. **Lista elementelor HTML fără tag de închidere există de 3 ori**, iar `markup.rs` ține
   cunoștințe HTML în domeniul `language`. Devine un hook al lui lang-html, refolosit de
   PHP, Blade, Twig și JSX, iar emmet folosește aceeași listă.
3. **CSS încă în motorul generic:**
   - hover-ul cu regula CSS (`engine.rs:678, 759`);
   - `is_css` duplicat în `usages.rs`;
   - `css: bool` în `rename.rs`.

   Totul devine hook-uri, implementate de lang-css.
4. **`docs.rs` are sintaxa docblock-urilor PHP scrisă direct în cod.** Vine din limbaj
   (opener, closer, atribute).
5. **`ui-components/code_tokens.rs` are cuvinte cheie PHP/JS**, iar scope-ul
   `variable.php` e scris direct în paletă și studio. Cuvintele cheie vin din limbaj, iar
   scope-ul primește un nume generic.
6. **Alte particularități PHP/JS în generic:**
   - cuvintele `class|interface|trait|enum` din `usages.rs`;
   - semnătura implicită din `engine.rs`;
   - `\` și `$` din `fuzzy.rs`.
7. **`snapshot.rs` folosește constante interne ale modulelor.** Trec pe id-uri de comenzi
   și view-uri.
8. **`When::Terminal` în modelul de comenzi** devine `When::View("terminal")`.
9. **Numerotarea terminalelor** e pe calea proiectului: trece pe `ProjectId`.
10. **Locale:**
    - cheia `[git] refresh` e duplicată, deci TOML invalid;
    - cheia `remote.connect_tab` nu e folosită.
11. **Helperi duplicați:**
    - `attribute_name_at` (fw-vue / fw-htmx);
    - numărarea liniilor până la un offset, de vreo 10 ori: trece în `kernel`;
    - `flow` din `code_hover` față de `ds::wrap`.

## Ordinea

32a → 32b → 32c → 32d, câte un commit pe etapă, fiecare după `check.sh` și capturi.

## Stare

- Popover-ul de versiuni: reparat (`54d4c44`).
- **32a, gata.**
  - Conexiunea venită cu proiectul cere aprobare: amprenta e în
    `~/.config/<app>/remote-approved`, iar `local` e forțat la rădăcină.
  - git rulează cu `core.fsmonitor=false` și `protocol.ext.allow=never`.
  - `.git` și folderul proiectului sunt excluse mereu, fără să conteze majusculele; pe
    Windows se refuză numele care ar fi citite drept altele.
  - Seiful nu-și înlocuiește cheia cât timp ține alte parole.
  - Imaginile:
    - se citesc doar din fișiere obișnuite, de cel mult 256 MB;
    - se decodează cu laturi de cel mult 16384;
    - în Markdown, decodarea se face pe firul `markdown-image`.
  - Modelul semantic e fixat la un commit, cu SHA-256, mărime verificată și header
    verificat.
  - Copia locală nu urmează link-urile.
  - Salvarea:
    - fișierul temporar e nou, cu permisiunile originalului;
    - pe Windows reîncearcă, apoi scrie pe loc;
    - nu lasă `.tmp` în urmă.
  - Procesele:
    - ieșirea e limitată la 16 MB;
    - rulează în grupul lor, care e oprit întreg (`taskkill /T` pe Windows).
- **32b, gata.**
  - AltGr (Ctrl+Alt cu text) e text, nu scurtătură.
  - `fs_local::is_taken`: redenumirea doar a majusculelor merge.
  - Setările:
    - contorul de scrieri e de tip seqlock;
    - un fișier invalid păstrează ce s-a citit;
    - fișierul se citește o singură dată.
  - Symfony cere `…/console`.
  - Argumentele cu nume:
    - întregul nume urmat de `:`;
    - stringurile sărite;
    - nimic oferit într-o declarație.
  - Docblock-ul are rezervă pentru atributele pe o linie.
  - `AsCommand` își păstrează descrierea pozițională.
  - Parola:
    - `Clone` păstrează capacitatea;
    - un `insert` lung mută textul și îl șterge pe cel vechi.
  - `$` e exclus mereu din căi.
  - `~\` merge pe Windows.
  - Tooltip-ul de eroare nu mai apare prin popover-e.
  - Explorer cu `raw_arg`.
  - Mesajul din watchdog e în engleză.
  - Fără schimbare: `short_name` în Structure. Containerul e mereu FQN-ul unui tip, iar
    numele scurt e partea de după separator, în orice limbaj.
- **32c, gata.**
  - Indexul se salvează după 30 s de liniște, cel mult o dată la 5 minute, și la închidere.
  - Task Manager-ul:
    - se reîmprospătează doar la o probă nouă, în ritmul lui;
    - memoria indexului e măsurată o dată la 5 s, cu `try_read`.
  - Feed-ul de indexare clasifică pe firul cozii; un folder șters scoate doar fișierele
    lui.
  - `entries_generation` ține listele de fișiere ale framework-urilor (șabloanele Symfony,
    `src`, view-urile Laravel, componentele Vue), care nu se mai refac la fiecare
    salvare.
  - Definiția unui șablon vine din listă.
  - `files_generation` ignoră folderele generate (`kernel::paths::in_generated`).
  - ⌘-hover:
    - folosește `try_read` (go to definition așteaptă în continuare);
    - fișierele de pe disc sunt limitate la `MAX_SOURCE_FILE`;
    - tab-urile deschise vin din cache-ul textului.
  - Ferestrele remote merg în ritmul `is_loading`.
  - Completarea:
    - query-ul e coborât la litere mici o singură dată;
    - fără copie pentru candidații care nu se potrivesc.
  - Utilizările numără liniile liniar.
  - Structure folosește `try_read`.
  - Lăsate, cu motiv:
    - **costă foarte puțin:**
      - lățimea grafului git, cu O(rânduri) întregi;
      - sortarea avertismentelor, câteva sute;
      - `is_file` în terminal, doar cu ⌘ apăsat;
      - `exists()` la o intrare nouă, unul pe tastă;
      - căutarea în setări, doar cât tastezi;
    - **puțini fișiere:** citirea dublă a unui fișier schimbat (simboluri + căutare);
    - **în TODO:** clona indexului din canvas.
- **32d, gata (parțial, restul în TODO cu motiv).**
  - Locale:
    - `[git] refresh` dublat scos;
    - `remote.connect_tab` nefolosit scos;
    - un test cere fiecare cheie o singură dată, aceleași chei în ro și en.
  - Cele 6 comenzi au etichete, iar un test cere etichete pentru toate comenzile, ale
    app-ului și ale modulelor.
  - `When::Terminal` a devenit `When::View("terminal")`; tastele panoului se caută prin
    `key_place()`.
  - Terminalele se numerotează după `ProjectId`, iar `Restored.id` e nou.
  - Helperi comuni:
    - `code_intelligence::attribute_name_at` (fw-vue, fw-htmx);
    - `kernel::text::line_of`, în loc de 7 copii.
  - `snapshot.rs` rulează comenzile după id.
  - Crate-ul `infra/markup`:
    - lista `VOID` e una singură (lang-html, emmet);
    - `unclosed_tag` a ieșit din domeniu;
    - descriptorul primește funcția limbajului (`closing_tags(markup::unclosed_tag)`).
  - `SourceRules::declares_type` înlocuiește `DECLARES_TYPE`, iar dublura `is_css` din
    `usages.rs` a dispărut.
  - Lăsate, cu motiv:
    - `flow` din `code_hover`: are lățimi pentru primul rând și restul, nu taie textul și
      nu limitează rândurile, spre deosebire de `ds::wrap`;
    - `HistoryData` în `snapshot.rs`: istoricul se citește dintr-un fixture, nu are id de
      comandă;
    - **în TODO:** hover-ul CSS, `docs.rs`, `code_tokens`, semnătura implicită și `fuzzy`.
