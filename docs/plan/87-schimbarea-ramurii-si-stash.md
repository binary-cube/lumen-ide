# 87 — Schimbarea ramurii, stash și insignele fișierelor

## Ce vrem

1. Să schimbăm ramura cu **dublu click** pe ea și din **meniul de click dreapta**, oriunde
   apare (panoul de ramuri din jurnal, locurile din fereastra de commit). Înainte de schimbare
   aplicația își dă seama singură dacă se poate trece direct sau dacă modificările din lucru
   stau în cale, și atunci oferă variantele: stash, renunțare sau anulare.
2. **Stash** pentru ce e în lucru: tot, doar ce e în stage, sau doar fișierele alese. Stash-urile
   se aplică, se scot și se șterg din listă.
3. **Insigne colorate** pentru starea fișierelor, în loc de literele `M` / `U`: un pătrățel
   rotunjit, colorat după stare, cu un semn în el. Culorile vin din temă.
4. Ramura din **bara de sus** devine un buton: deschide o listă cu căutare și cu tastele afișate
   jos, din care schimbi ramura.

Ce avem azi:
- `Repository` nu are încă `switch`, nici `stash push / apply / pop / drop`. Are doar
  `stashes()`, care le citește.
- Ramurile se văd în jurnal (`log_view/branches.rs`): un click le alege, un dublu click
  filtrează jurnalul după ele. Se mai văd în locurile ferestrei de commit
  (`commit_view/places.rs`), unde apar și stash-urile.
- Meniul unei ramuri e `ref_menu.rs`: ramură nouă, redenumire, tag, push.
- Starea unui fișier se desenează prin `state_badge` (o literă colorată) și
  `ds::file_row` (`ui-components`).
- Ramura din bara de sus e doar afișată (`top_bar.rs`, „display only”).
- Lista cu căutare există deja: `ui_components::picker` (căutare sus, rânduri, bifă, `↵`, taste
  jos).

## 87a — Insignele stării (prima, mică și vizibilă)

Un singur component nou, `ds::change_badge` în `ui-components`. Îl folosesc toate listele de
fișiere modificate:
- fereastra de commit (stage și unstage);
- fișierele unui commit din jurnal;
- fișierele unui stash;
- vederea conflictelor.

Pătrățelul are ~16 px la scara 1, colțuri `radius::SM` și semnul centrat:

| Stare      | Semn | Pictogramă (pachetul nostru)        | Culoare (implicit)  |
|------------|------|-------------------------------------|---------------------|
| Modificat  | `…`  | `more-horizontal` (există)          | chihlimbar          |
| Adăugat    | `+`  | `plus` (există)                     | verde               |
| Șters      | `−`  | `minus` (există)                    | roșu                |
| Redenumit  | `→`  | `arrow-right` (există)              | albastru            |
| Neurmărit  | `?`  | glifă nouă `question` (fără cerc)   | violet              |
| În conflict| `!`  | glifă nouă `exclamation`            | portocaliu          |

- **Culorile sunt ale temei.** Câmpurile noi din `Palette` sunt `vcs_badge_modified`,
  `…_added`, `…_deleted`, `…_renamed`, `…_untracked` și `…_conflicted`. Cheile lor în temă sunt
  `vcs.badge.*`. Dacă o temă nu le are, se iau din `vcs.added` / `vcs.modified` /
  `vcs.deleted` / `danger` / `warning`, iar „neurmărit” are un violet propriu pentru fiecare
  temă, luminoasă sau întunecată.
- **Nu schimbăm `vcs_modified` & co.** Le folosesc arborele de fișiere și marginea editorului,
  iar ce vede utilizatorul acolo rămâne cum e.
- **Semnul** are culoarea de contrast a fundalului: alb pe fundal închis, aproape negru pe unul
  deschis (chihlimbarul din tema luminoasă). Alegerea se face după luminanța culorii, o singură
  dată pe temă.
- **La hover**, tooltip-ul spune starea în cuvinte („Modificat”, „Neurmărit”…), din locale.
- **Pictogramele noi** se desenează în stilul pachetului (24×24, linie de 2, capete rotunde)
  și intră în `crates/render/icons/svg`: `question`, `exclamation`, apoi cele de la 87b–87c:
  `arrow-left-right` (schimbare), `archive-down` (stash) și `archive-up` (aplicare).
- **Rămâne `state_badge`** pentru textele simple (titluri, tooltip-uri), dar listele desenează
  pătrățelul.

## 87b — Schimbarea ramurii

### Unde

- **Panoul de ramuri din jurnal:**
  - dublu click pe o ramură o schimbă;
  - filtrarea jurnalului după ramură trece pe un click simplu, care azi doar o selectează. Așa,
    un singur click face un singur lucru.
  - Intrarea „Toate” și dosarele rămân cum sunt.
- **Locurile din fereastra de commit:** dublu click pe ramură o schimbă.
- **Meniul ramurii** (`ref_menu.rs`) are ca prim rând „Schimbă pe *nume*” (`arrow-left-right`),
  urmat de un separator și de rândurile de azi. Pe ramura curentă rândul nu apare.
- **Meniul unui commit din jurnal** primește „Checkout la acest commit…”, care duce la un HEAD
  detașat (vezi mai jos).
- **Bara de sus** primește lista de la 87d.

### Ce înseamnă „schimbă” pentru fiecare fel de referință

- **Ramură locală:** `git switch <nume>`.
- **Ramură la distanță** (`origin/feature`):
  - dacă există o ramură locală care o urmărește, se schimbă pe aceea. Dacă e în urmă, notificarea
    de după schimbare are un buton „Pull”;
  - dacă nu există, se deschide un dialog mic, cu numele local precompletat (`feature`) și
    „Urmărește `origin/feature`” bifat. Apoi rulează `git switch -c feature --track
    origin/feature`;
  - dacă numele local există deja, dar urmărește altceva, dialogul cere alt nume. Validarea e
    `check_ref_name`, plus verificarea că numele nu există.
- **Tag sau commit:** dialogul spune că HEAD va fi detașat, cu explicația pe scurt. Oferă
  „Creează ramură aici…” (dialogul de ramură nouă existent) sau „Checkout detașat”.
- **Ramura curentă:** nu face nimic, pentru că e deja marcată.

### Verificarea dinainte (pe un worker, niciodată pe firul interfeței)

Un caz de utilizare pur în `version-control` (domeniu): `switch_plan(dirty, differs, untracked_in_target) →
SwitchPlan`. Git-ul (infra) îi dă datele:

1. **Operație în curs** (merge, rebase, cherry-pick, `Repository::operation`): nu se schimbă.
   Dialogul spune ce operație e în curs și trimite la vederea conflictelor.
2. **Fișierele în lucru** (`staged_files`): dacă nu e niciunul, se schimbă direct.
3. **Ce diferă între HEAD și țintă**: `git diff --name-only HEAD <țintă>`, cu o limită de număr.
4. **Neurmăritele care ar fi suprascrise**: cele din lucru care există în arborele țintei
   (`git ls-tree --name-only <țintă> -- <căi>`, în loturi).
5. Rezultatul:
   - **liber**: nicio suprapunere. Se schimbă, iar modificările vin cu tine, pentru că git le
     poartă. Notificarea spune „Modificările tale au rămas în lucru”;
   - **blocat de fișiere**: cele care se suprapun. Se deschide dialogul de mai jos.
6. **Fișierele nesalvate din editor:** înainte de toate, aplicația le salvează pe cele din acest
   proiect. Dacă salvarea nu poate fi automată, întreabă: „Salvează și schimbă / Anulează”.
   Se face printr-o cerere neutră către gazdă (`host`), pentru că modulul nu știe de editor.

Verificarea e doar pentru un mesaj bun. `git switch` oricum își face verificarea lui: dacă
totuși refuză (o cursă cu discul), eroarea lui se arată în același dialog, nu ca un toast
criptic.

### Dialogul „Modificările stau în cale”

Un modal (nu se închide la click în afară):
- **Titlul**: „Nu se poate schimba pe *feature* fără să pierzi modificări”.
- **Lista fișierelor** care se suprapun, cu insigna lor de la 87a, derulabilă și cu căutare
  dacă sunt multe. Un click pe un fișier deschide diff-ul lui.
- **Butoanele:**
  - **Stash și schimbă** (implicit, `↵`): rulează `git stash push -u -m "Înainte de schimbarea
    pe feature"`, apoi `switch`. Notificarea de după are butonul „Aplică stash-ul înapoi”.
  - **Renunță la modificări și schimbă** (periculos, roșu, cu confirmare): `git switch
    --discard-changes` și ștergerea neurmăritelor suprapuse.
  - **Fă commit întâi**: deschide fereastra de commit.
  - **Anulează**.

### După schimbare

- Tab-urile deschise se reîncarcă de pe disc prin urmărirea existentă a fișierelor, fără cod
  nou. Un tab al cărui fișier nu mai există pe ramura nouă rămâne deschis și e marcat ca șters,
  ca azi.
- Pe magistrala proiectului se publică un eveniment `BranchSwitched { from, to }`: jurnalul,
  bara de jos, marginea diff-ului și bara de sus se reîmprospătează din el, nu sunt chemate
  direct.
- Lucrul durează: rândul ramurii arată o rotiță cât rulează `switch`, iar un al doilea dublu
  click în timpul ăsta e ignorat.

## 87c — Stash

### Fă stash

Dialogul „Stash” (modal):
- **Mesaj**: opțional, precompletat cu „WIP pe *ramură*”.
- **Opțiuni** (bife):
  - **Păstrează ce e în stage** (`--keep-index`);
  - **Include fișierele neurmărite** (`-u`);
  - **Doar ce e în stage** (`--staged`). Cere git ≥ 2.35. Pe un git mai vechi rândul e gri, cu
    un tooltip care spune de ce.
  - „Doar ce e în stage” și „Păstrează ce e în stage” se exclud: bifând-o pe una, cealaltă
    devine gri.
- Butoanele sunt **Stash** (`↵`) și **Anulează**.

De unde se deschide:
- butonul din fereastra de commit (`archive-down`, lângă butoanele de deasupra listelor);
- comanda „Stash Changes…” din meniul Git, cu tastă;
- meniul secțiunii „Staged” („Stash ce e în stage…”, care deschide dialogul cu bifa pusă);
- meniul fișierelor alese („Stash fișierele alese…”: `git stash push -- <căi>`).

Butonul e activ doar când e ceva de pus deoparte.

### Stash-urile din listă (locurile ferestrei de commit)

- **Un click** pe un stash îi arată fișierele și diff-ul, doar pentru citire, cu aceleași
  insigne. Un stash e un commit, deci se refolosește vederea fișierelor unui commit.
- **Dublu click** deschide dialogul „Aplică stash-ul”, cu bifa „Șterge-l după aplicare” (pop).
- **Meniul** are:
  - **Aplică**;
  - **Aplică și șterge**;
  - **Ramură nouă din stash…** (`git stash branch`);
  - **Copiază mesajul**;
  - un separator, apoi **Șterge…** (roșu, cu confirmare: „Stash-ul nu se mai poate
    recupera”).
- **Conflictele** la aplicare duc în vederea conflictelor existentă. La `pop` cu conflict, git
  nu șterge stash-ul, iar mesajul spune asta.

### Domeniu și infra

- **`Repository`** primește:
  - `switch_to(target)`, `switch_new(name, start, track)` și `switch_detached(id)`;
  - `stash_push(StashOptions)`, `stash_apply(name, pop)`, `stash_drop(name)` și
    `stash_branch(name, branch)`;
  - pentru verificarea de la 87b: `changed_between(a, b)` și `present_in(rev, paths)`.

  Toate au implementări implicite care întorc „nu se poate” pentru alte sisteme de versiuni.
- **`StashOptions { message, keep_index, untracked, staged_only, paths }`** stă în domeniu.
  Domeniul validează combinațiile, iar argumentele git le construiește infra.
- Stash-urile se numesc după `stash@{n}`, dar acesta se schimbă când se adaugă unul nou. De
  aceea, înainte de apply sau drop, citim din nou lista și găsim intrarea după commit-ul ei
  (`%H` adăugat la `parse_stashes`), nu după poziție.

## 87d — Ramura din bara de sus, cu listă

Cine ce face:
- **Aplicația nu știe de git.** Bara de sus primește o contribuție neutră (`workbench-model`):
  „acțiunea etichetei de ramură”, adică un id de comandă. Dacă nicio contribuție nu e
  înregistrată, eticheta rămâne doar afișată (`presence`), deci nu e un buton care nu face
  nimic.
- **Hover**: eticheta capătă fundalul butoanelor din bară și un chevron. Tooltip-ul e
  „Schimbă ramura” plus tasta.
- **Click**: comanda modulului git deschide un overlay ancorat sub etichetă
  (`host.open_overlay`, popover), cu `ui_components::picker`.

Lista:
- **Sus**: căutarea, care filtrează după nume, după remote și după ultimul mesaj de commit, ca
  detaliu.
- **Rândurile, pe secțiuni:**
  - **Recente**: ultimele 5 ramuri pe care ai fost, după reflog;
  - **Locale**;
  - **La distanță** (doar cele fără o ramură locală care le urmărește).

  Fiecare rând are pictograma, numele, `↑2 ↓1` față de ce urmărește (aliniat la dreapta) și
  bifa pe ramura curentă.
- **Primul rând**: „Ramură nouă…”, care deschide dialogul existent, pornit din HEAD.
- **Jos, tastele**: `↑↓` navighează, `↵` schimbă, `⌘↵` face ramură nouă din cea aleasă, `Esc`
  închide.
- **Alegerea** trece prin exact aceeași verificare de la 87b. Dacă e nevoie de dialog, lista se
  închide și se deschide dialogul.
- **Datele** vin din `refs_store`, care sunt deja în memorie. Lista nu citește git la
  deschidere: doar reflog-ul pentru „Recente”, citit pe un worker cu `Deferred`, iar secțiunea
  apare când sosește.

### Câmpul de căutare (aici și în filtrul de ramuri din jurnal)

Se poartă ca orice câmp de text din aplicație:
- **`⌘A`** selectează tot ce e scris;
- **`⌘C` / `⌘X` / `⌘V`** copiază, taie și lipesc, ca text simplu, pe un singur rând (rândurile
  noi lipite se fac spații);
- **`⌘Z` / `⌘⇧Z`** anulează și refac;
- **`×`** la capătul câmpului golește căutarea, cât timp e ceva scris. Cursorul de deasupra lui
  e o mână, iar tooltip-ul spune „Golește”;
- **dublu click** selectează un cuvânt;
- **tragerea** cu mouse-ul selectează.

`picker` are deja `select_all`, `×` și `selected_text`. Verificăm că overlay-ul îi trimite
toate comenzile de editare (`select_all`, `copy`, `cut`, `paste`, `undo`, `redo`) și că
`⌘C` copiază din căutare, nu rândul evidențiat. Aceleași teste se scriu și pentru filtrul de
ramuri din jurnal.

## Teste (în `tests/unit/`, cum e regula)

- **`switch_plan`, pur**: fără modificări; cu modificări care nu se suprapun; cu suprapuneri; cu
  neurmărite care ar fi suprascrise; cu o operație în curs.
- **Infra, pe un depozit git din scratchpad** (niciodată unul al utilizatorului):
  - schimbare liberă, iar modificarea rămâne;
  - schimbare blocată, apoi stash și schimbare, apoi aplicare înapoi;
  - o ramură la distanță din care se face una locală care o urmărește;
  - checkout detașat;
  - `stash --staged`, `--keep-index` și `-u`;
  - drop după commit, nu după poziție;
  - `stash branch`.
- **`StashOptions`**: combinațiile care se exclud sunt refuzate.
- **Insignele**: semnul și culoarea pentru fiecare stare. Contrastul semnului e lizibil pe
  temele luminoasă și întunecată, iar tema fără cheile noi ia culorile de rezervă.
- **Lista din bara de sus**:
  - secțiunile și ordinea lor;
  - căutarea;
  - `↵` și `⌘↵`;
  - `⌘A`, `×`, copiere și lipire în căutare;
  - fără contribuția git, eticheta nu e buton.
- **Meniul ramurii**: „Schimbă pe” e primul rând și lipsește pe ramura curentă.
- **Modulele scoase pe rând**: fără git, bara de sus și restul nu se schimbă.

## Ordinea

1. **87a, insignele**: mică, se vede imediat și nu atinge git-ul.
2. **87b, schimbarea**: verificarea, dialogul, dublu click și meniul.
3. **87c, stash**: dialogul, lista și meniul stash-urilor. 87b îl folosește deja pentru „Stash
   și schimbă”, deci cele două `stash_push` se fac împreună.
4. **87d, bara de sus**: lista și câmpul de căutare.

Fiecare pas trece de `./scripts/check.sh` și se verifică cu un snapshot pe un HOME izolat.
Commit-urile le facem doar când spune utilizatorul.

## Lăsat pentru mai târziu

- Stash parțial pe bucăți dintr-un fișier (hunk-uri).
- Schimbarea ramurii unui submodul din listă.
- Tragerea unei ramuri peste alta pentru merge sau rebase.

## Făcut

**87a — Insignele.**
- `ds::change_badge` (`ui-components`) desenează un pătrățel colorat cu semnul stării, din
  `Palette::vcs_badges`. Culorile vin din cheile de temă `vcs.badge.*`, cu valori implicite
  separate pentru tema luminoasă și cea întunecată. Semnul e alb sau aproape negru, după
  luminanța culorii de fundal (`sign_on`).
- Pictogramele noi, în stilul pachetului (linie de 3, ca să se citească la 11 px):
  `mark-modified`, `mark-added`, `mark-deleted`, `mark-renamed`, `mark-untracked`,
  `mark-conflicted`. Plus `arrow-left-right`, `archive-down` și `archive-up`.
- `ChangeState` are acum și `Untracked`.
- Insignele se văd în arborele de fișiere modificate (jurnalul, conflictele, încărcările unui
  proiect la distanță), în listele Staged / Unstaged și în dialogul de schimbare. În ultimele
  două, la hover apare și starea în cuvinte.
- Culorile arborelui și ale marginii editorului rămân cele de dinainte.

**87b — Schimbarea ramurii.**
- **Domeniu** (`version_control::switching`):
  - `switch_plan` întoarce liber / blocat de fișiere / operație în curs;
  - `remote_switch` găsește ramura locală care urmărește una la distanță, una nouă, sau un nume
    deja luat;
  - `recent_checkouts` citește ramurile recente din reflog.
- **Infra** (`git::switching`):
  - verificarea se face doar pe fișierele în lucru: `git diff --name-only HEAD <țintă> --
    <căi>`, apoi `ls-tree` pentru neurmărite, în loturi de 200;
  - `git switch` / `switch -c --track` / `switch --detach` / `--discard-changes`;
  - o revizie care începe cu `-` e refuzată.
- **Modul** (`switch.rs`, `switch_dialog.rs`):
  - dacă sunt fișiere nesalvate, întreabă întâi (`host.unsaved_files`, capabilitate nouă);
  - un tag sau un commit cere confirmare pentru HEAD detașat;
  - verificarea rulează pe un worker. Dacă e liberă, schimbarea rulează imediat, cu notificarea
    „modificările au rămas în lucru”. Dacă e blocată, apare dialogul cu lista de fișiere (cu
    insigne și derulare) și butoanele: Stash și schimbă (`↵`), Renunță și schimbă (cere a doua
    apăsare, roșu), Fă commit întâi, Anulează;
  - „Stash și schimbă” pune la loc lucrul dacă schimbarea totuși eșuează;
  - notificarea de după are „Adu-l înapoi” (`git.pop_switch_stash`), care aplică exact stash-ul
    acela, ținut minte pentru fiecare depozit, după commit.
- **Unde:**
  - dublu click în panoul de ramuri din jurnal (un click simplu filtrează acum jurnalul) și în
    locurile ferestrei de commit;
  - „Schimbă pe *nume*” e primul rând în meniul ramurii, fără ramura curentă și fără HEAD;
  - „Checkout la acest commit…” în meniul unui commit;
  - „Switch Branch…” în meniul Git.
- Reîmprospătarea după schimbare vine din urmărirea existentă (`HeadChanged`,
  `refs_store::moved`), deci n-a fost nevoie de un eveniment nou.

**87c — Stash.**
- **Dialogul** (`stash_dialog.rs`) are mesaj, „Păstrează ce e în stage”, „Include
  neurmăritele” și „Doar ce e în stage”:
  - opțiunile care nu merg împreună devin gri, cu tooltip;
  - „Doar ce e în stage” e gri pe un git mai vechi de 2.35 (versiunea se citește o dată).
- **De unde se deschide:**
  - „Stash…” în antetul ferestrei de commit;
  - meniul Git;
  - meniul fișierelor: unul sau mai multe fișiere, plus „ce e în stage”, pe lista Staged.
- **Stash-urile din locuri:**
  - dublu click deschide „Aplică”, cu bifa „Șterge-l după”;
  - meniul are Aplică, Aplică și șterge, Ramură nouă din stash… (dialogul de ramură, cu
    `git stash branch`), Copiază mesajul, Șterge… (cu confirmare).
- **Aplicarea** încearcă întâi `--index`. Dacă indexul nu poate fi pus la loc, se aplică fără
  el. La conflicte, notificarea duce la Rezolvă conflictele.
- **Identificarea**: un stash e găsit după commit-ul lui (`%H` în listă), nu după
  `stash@{n}`.

**87d — Bara de sus.**
- Contribuția neutră `TopBarLabelAction` (`workbench-model`) face din eticheta ramurii un
  buton: fundal la hover, chevron, tooltip. Fără contribuție, eticheta rămâne doar afișată.
- `host.anchor(Anchor::TopBarLabel)` spune unde e butonul.
- Lista (`branch_picker.rs`, peste `ui_components::picker`):
  - căutare mereu vizibilă, care are tastatura;
  - „Ramură nouă…” primul, apoi ramura curentă (bifată), recentele (din reflog), celelalte
    locale și cele la distanță pe care nu le urmărește nicio ramură locală;
  - fiecare rând arată `↑2 · ↓1 · origin/main`;
  - tastele sunt afișate jos.
- `picker` a primit:
  - căutare mereu vizibilă;
  - dublu click pe un cuvânt și selecție prin tragere;
  - lipire pe un rând.
  - `⌘A`, `×`, copiere, tăiere, undo și redo existau deja; le folosește și lista.

**După încercare**:
- Butoanele dialogurilor de ramură, stash și schimbare stau în subsol (`modal_buttons`), iar
  dialogurile sunt mai înguste (560; cel de schimbare 620).
- Rândurile din panoul de ramuri au 2 px între ele, ca selecția și hover-ul să nu se lipească.
- Derularea peste lista din bara de sus derulează lista (`Viewer::takes_scroll`), nu ce e sub ea.
- Lista închisă cu Esc sau cu click în afară cere închiderea; aplicația închide și orice
  fereastră a unui modul care spune că s-a terminat fără să ceară nimic. Înainte, lista
  rămânea deschisă, invizibilă, și lua toate click-urile și tastele.

**Teste**:
- domeniu: planul, ramura la distanță, opțiunile de stash, căutarea după commit, reflog-ul;
- infra, pe depozite de test în dosarul temporar: schimbare liberă și blocată, stash și înapoi,
  renunțare, ramură nouă care urmărește, HEAD detașat, opțiune refuzată, `--keep-index`,
  `--staged`, ștergere după commit, versiunea git;
- modul: verificarea pe depozitul fals, dialogurile (stash și schimbă, renunțare în doi pași,
  bife care se exclud, pop), lista din bara de sus (ordine, căutare, editare, Esc), meniurile
  actualizate.

## Lăsat

- **`⌘↵`** („ramură nouă din cea aleasă”) în listă: subsolul `picker` e comun și are doar
  `↑↓`, `↵`, `esc`. Ar cere ca subsolul să poată primi taste în plus.
- **Antete de secțiune** în listă (Recente / Locale / La distanță): `picker` nu are antete.
  Deocamdată detaliul rândului spune „recentă” / „la distanță”.
- **Un click simplu pe un stash** nu-i arată încă fișierele și diff-ul. Deocamdată merg dublu
  click (aplică) și meniul.
- **Rotița pe rândul ramurii** cât rulează schimbarea, și ignorarea unui al doilea dublu click
  în timpul ei.
- **Fișierele nesalvate** nu se salvează automat. Aplicația întreabă, iar modificările rămân în
  editor: la salvare, cine a schimbat discul e întrebat, ca azi. Salvarea automată ar fi pornit
  în fundal și n-ar fi terminat sigur înainte de `switch`.
