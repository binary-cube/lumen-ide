# 22. Limbaje și editor (formatarea întâi)

Cererea: punctele „limbaje și editor” din roadmap, cu **formatarea externă** prima — un
formatter cunoscut (php-cs-fixer, pint, prettier) sau **o comandă / un script al proiectului**
(ex. `./scripts/linux/shell.sh composer cs:fix`, rulat în Docker); și „Go to Line” după setul
de scurtături.

## Azi (inventar, 2026-10-22)

Făcute, deși roadmap-ul (planul 10) le are nebifate: Twig (`lang-twig`: evidențiere, HTML
injectat, plierea blocurilor), link-urile `fișier:linie` și selecția în terminal, sesiunea, git
(gutter, blame), căutarea în proiect, inteligența PHP nativă — roadmap-ul se actualizează la
final.

Formatarea — **nu există**, dar piesele există:
- procese externe: două copii ale aceluiași tipar (`infra/git/src/process.rs` — doar `git`;
  `lang-php/src/interpreter.rs` — doar versiunea PHP), fără stdin;
- joburi în fundal: `WorkbenchHost::spawn(Job)`, încheierea pe UI thread;
- **fișierele scrise de altcineva se reîncarcă deja** (`fs_watch` → `FilesWritten` →
  `reload_written`: taburile curate cu `Document::reload` — un pas de undo, cursoarele
  păstrate; cele modificate primesc avertismentul) — deci și un script rulat în Docker;
- setări per proiect: `.lumen/project.toml` doar pentru niveluri și framework-uri (porturi
  înguste); un modul își poate **arăta** valorile pe proiect, dar nu le poate **salva**
  (`choose` / `set_text` primesc doar setările globale).

Restul: TypeScript / TSX — nicio gramatică; Vue — `<script>` mereu JS, `<style>` mereu CSS
(`lang=` ignorat), expresiile din template neinjectate; htmx — completare și hover, dar
`hx-on:*` fără JS injectat; italic / bold din temă — ignorate în editor (doar culoarea);
indentarea — rândul anterior + bloc după `{` / `(` / `[`, fără `indents.scm`, fără de-indentare
la `}`; pachete — zip-uri (macOS semnat ad-hoc), fără `.dmg` / `.msi` / AppImage.
„Go to Line”: ⌘L / Ctrl+L implicit și în setul JetBrains (și pe Windows), Ctrl+G în VS Code /
Zed / Sublime. Seturile de acum rămân exact cum sunt; cine vrea altceva le schimbă din
Scurtături (22g).

## Pașii (un commit fiecare; `check.sh`; „go” între ei)

- [x] **22a. Rularea proceselor, comună.** `infra/process`: orice program, în folderul dat,
      cu stdin opțional, timp limită (oprit la depășire), ieșirea și erorile citite pe fire
      separate, fără fereastră de consolă pe Windows; git-ul și interpretorul PHP trec pe el
      (comportament neschimbat). Teste (un program real: `sh` / `cmd`, timpul limită).
      *Făcut:* crate-ul `process` (`infra/process`): `Run` (programul, argumentele,
      folderul, mediul, stdin, timpul limită, oprirea la cerere — o rulare nouă peste una în
      curs), `Run::shell` pentru comenzile scrise de utilizator (`sh -c` / `cmd /C`), `run` →
      `Output` (codul de ieșire, ieșirile, ultimul rând util; o ieșire cu eroare e un rezultat,
      nu o eroare — un verificator raportează așa), `RunError` (negăsit, timp depășit, oprit),
      `which` (cu extensiile Windows). Git-ul și detecția PHP trec pe el (aceleași argumente,
      același timp limită). Strat nou în `check-deps`: „infrastructură comună” (`process`,
      doar kernel), folosibil de orice adaptor și de module. Teste (Unix): folderul, codul,
      stdin, timpul limită, oprirea, programul lipsă.

- [x] **22b. Setări per proiect pentru module.** `SettingsScope` primește rădăcina proiectului;
      `choose` / `set_text` primesc scope-ul; un port pentru tabelele modulelor din
      `.lumen/project.toml` (fiecare își rescrie doar tabelul lui, ca nivelurile). Fereastra de
      setări arată blocul „Proiect” al unui modul doar cu un proiect deschis.
      *Făcut:* setările unui proiect sunt un `SettingsService` ca cele globale (tipuri,
      implicite, validare, cache), scris în `<proiect>/.lumen/project.toml` prin același
      adaptor TOML (un tabel al modulului rescris, celelalte — niveluri, framework-uri —
      păstrate; un fișier invalid nu e suprascris); făcut la deschiderea proiectului, în
      datele lui (plecat cu el). `SettingsScope.project`; `choose` / `set_text` primesc
      scope-ul (salvează în global sau în proiect); `WorkbenchHost::project_settings()` pentru
      modulele care rulează ceva. Un modul își declară setările de proiect ca `Setting<T>`.
      Teste: tabelele celorlalți păstrate, setarea unui proiect neajunsă în altul.

- [x] **22c. Contextul „calitatea codului” (domeniu, porturi, evenimente).** Două feluri de
      unelte, separate: **formatoare** (rescriu codul: php-cs-fixer, pint, prettier…) și
      **verificatoare** (raportează probleme: phpcs / CodeSniffer, php-cs-fixer în
      `--dry-run`, phpstan, eslint, stylelint…). Domeniul (`code-quality`): ce e o unealtă (nume,
      limbaje, cum se găsește — `vendor/bin`, `node_modules/.bin`, `PATH` —, argumentele pentru
      un fișier / pentru proiect, cum i se citește ieșirea → probleme: fișier, rând, coloană,
      gravitate, mesaj, regula, unealta), alegerea pe proiect și pe limbaj, regulile („la
      salvare”, ce rulează, ce se întâmplă dacă o rulare nouă vine peste una în curs); porturile:
      rularea (22a), setările proiectului (22b). Evenimente pe magistrala proiectului:
      `FormattingStarted / Finished`, `CheckFinished { file, problems }` — cine arată problemele
      le urmează. Teste fără procese (unelte și ieșiri false).
      *Făcut:* crate-ul de domeniu `code-quality` (`crates/domain/code/quality`, doar kernel):
      `QualityTool` (id, nume, formator / verificator, limbajele, unde se caută — în proiect
      `vendor/bin/…` apoi în `PATH` —, argumentele pentru un fișier / proiect, cititorul
      ieșirii; implicit rândurile `fișier:rând:coloană: mesaj`, și căi Windows), `Problem`
      (fișier, rând, coloană, gravitate, mesaj, regula, unealta, reparabilă), `Choice`
      (niciuna / unealtă / binar / comandă, cu formatul ieșirii ales) și `LanguageChoice`
      (formator, verificator, la salvare — separat); `plan` → `Invocation` (programul sau
      shell-ul, argumentele, ținta — o comandă fără `{file}` e pe tot proiectul —, `{file}`
      citat când trebuie; „negăsit”, „necunoscut” spuse cu numele); `Runs` (două rulări de
      același fel pe aceeași țintă nu se suprapun, cea cerută între timp rulează o dată, tot
      proiectul și un fișier nu se suprapun); evenimentele `QualityRunStarted / Finished`,
      `ProblemsFound`. Teste fără procese.

- [x] **22d. Uneltele date de module, ce alegi tu, cum se văd erorile.**
      - **Fiecare modul de limbaj își contribuie uneltele cunoscute, cu una implicită dacă e
        găsită în proiect**: `lang-php` — php-cs-fixer, pint (formatoare), phpcs, phpstan,
        php-cs-fixer `--dry-run` (verificatoare); `lang-javascript` / `lang-typescript` —
        prettier, eslint; `lang-css` / `lang-scss` — prettier, stylelint; `lang-json` /
        `lang-yaml` — prettier. Fiecare cu cititorul ieșirii lui (JSON unde există).
      - **Ce alegi tu, pe proiect și pe limbaj** (Setări → Calitatea codului, blocul
        proiectului): niciuna / o unealtă găsită / **un binar** (calea) / **o comandă sau un
        script** (ex. `./scripts/linux/shell.sh composer cs:fix`; cu `{file}` — doar fișierul,
        fără — tot proiectul; ieșirea citită ca `fișier:rând:coloană: mesaj`, sau formatul unei
        unelte cunoscute dacă îl alegi); în locul unui buton „Testează”, pagina arată mereu
        ce ar rula exact (`vendor/bin/phpcs --report=json -q {file}`) sau de ce nu se poate
        (unealta nu e instalată, binarul nu există, lipsește comanda).
      - **Când**: comenzile „Reformat File” (⌥⌘L JetBrains, ⇧⌥F VS Code), „Check File”,
        „Format Project” / „Check Project”; **la salvare, pe proiect, separat**: „formatează”
        și / sau „verifică” (oprite implicit). Rulate în fundal, cu timp limită; un fișier
        schimbat de formator se reîncarcă cum se face deja (un pas de undo, cursoarele păstrate).
      *Făcut (1–2/4):* uneltele modulelor (PHP: PHP CS Fixer — și verificatorul lui în
      `--dry-run` —, Pint, phpcbf, phpcs, PHPStan; JS: Prettier — și pentru TS / CSS / SCSS /
      JSON / YAML / HTML / Vue —, ESLint; CSS: Stylelint), cu cititoarele JSON; modulul
      `quality`: serviciul de proiect (rulările în fundal prin `process`, nesuprapuse,
      rezultatele publicate pe firul UI — `QualityRunFinished`, `ProblemsFound` —, eșecul
      spus), comenzile „Reformat File” (⌘⌥L), „Check File”, „Format Project”, „Check
      Project” (fișierul salvat întâi dacă are modificări), la salvare: formatorul, apoi
      verificatorul; alegerile în `[quality-<limbaj>]` din `project.toml`; `ProjectContext`
      primește setările proiectului, limbajul unui fișier, uneltele. Teste cu scripturi reale.
      - **Erorile, frumos**: subliniate în editor (culoarea gravității), iconița în gutter,
        marcaje în scrollbar, la hover mesajul + regula + unealta (și „Fix” dacă formatorul o
        poate repara); în panoul Structura la „Probleme”; numărate în bara de stare; o unealtă
        care eșuează (nu găsită, config greșit) → notificare cu motivul, nu tăcere. Task
        Manager: rulările ca operații. Capturi (temă închisă / deschisă).
- [x] **22e. Scurtăturile — serviciul.** Azi: modulele declară comenzi (id + scurtătura
      implicită), serviciul `keymaps` (unul pentru toate ferestrele) le combină cu setul ales;
      lipsesc scurtăturile proprii, seturile pentru comenzile modulelor (rename rămâne
      `Shift+F6` și în setul VS Code) și contextele. Ținta, în `command-model` (domeniu):
      - **straturi**: implicit (al comenzii) → setul ales → **ale tale** (`[keymap]` în
        `settings.toml`, ca diferențe: adăugate / scoase pe comandă);
      - **contexte** („când”): editorul, terminalul, un view anume, o fereastră modală —
        aceeași tastă poate însemna altceva în locuri diferite; comanda spune unde e valabilă;
      - **modulele își dau scurtăturile pe seturi** (rename: `F2` în setul VS Code) — app-ul
        nu le cunoaște id-urile; seturile de acum nu se schimbă (Go to Line rămâne Ctrl+L pe
        Windows în setul JetBrains);
      - **conflictele** găsite (aceeași combinație, același context).
      Teste: straturile, contextele, conflictele, setul vechi neschimbat.
      *Făcut:* `command_model::bindings` (`When`, `PresetShortcuts` — contribuite de module —,
      `UserShortcuts`, `Layers`, `resolve`, `conflicts`), `Keymap::lookup_in` (legătura
      locului bate pe cea de peste tot); `keymaps` le pune în straturi, `[shortcuts]` din
      `settings.toml` (scrise de mână până la 22g); editorul / terminalul își spun locul.
      Seturile verificate față de `keymaps.golden` (toate comenzile, toate seturile).
      Conflictele vechi păstrate (ex. ⌘⇧= zoom / unfold all; pe Windows Ctrl+G select next /
      find next; din module: Shift+F6 rename tab / rename în JetBrains, ⌘⇧K delete line /
      git push în VS Code, ⌘⇧D duplicate / canvas în Zed și Sublime) — de rezolvat de user
      în 22g.
- [x] **22f. Acțiunile din view-uri, comenzi.** Ce e acțiune (filtrează, următorul /
      anteriorul, aplică, reîmprospătează…) în pânză, compare, Remote, Task Manager, git,
      refactorizări devine comandă cu context (remapabilă); navigarea (săgeți, Enter,
      Page Up / Down), editarea în câmpuri și **Esc pe modale** rămân fixe. Implicitele = tastele
      de acum: un test pe fiecare view (aceleași taste → aceleași acțiuni).
      *Făcut:* `ViewAction` (`workbench-model`): comanda unei acțiuni, valabilă doar în view-ul
      ei (`When::View(id-ul viewer-ului)`), trimisă view-ului care are tastele (tab-ul activ
      fără editor sau fereastra de jos focusată; nu când un câmp de-al lui le ia, nu sub o
      modală). Pânza (zoom, potrivire, modurile 1/2/3), compare (n/j, p/k, [ ], m), diagrama
      (0, ⌫ — care înainte nu ajungea la ea), istoricul local (r), imaginea (zoom, 0, 1, r / R,
      h, v, b), log-ul git (r — înainte nu ajungea, jos). Restul view-urilor au doar navigare,
      câmpuri, Enter și Esc (fixe). De făcut la 22g: tooltip-urile barei imaginii arată încă
      tastele implicite (nu pe cele schimbate).
- [x] **22g. Setări → Scurtături.** Lista tuturor comenzilor (căutare după nume sau după
      taste, grupate pe meniu / modul, unde e valabilă); clic → „Adaugă scurtătură”, apeși
      tastele; sub ele, ce o mai folosește. La un conflict: **Scoate de la
      celelalte / Lasă la toate / Renunță**; lăsată la toate în același context → la apăsare,
      o listă mică din care alegi (cu locul fiecăreia). „Revino la setul ales” pe o comandă
      sau pe toate. Capturi.
      *Făcut:* `shortcuts_editor` (ui-workbench): căutare după nume / taste, rândul deschis
      (tastele cu ×, „Adaugă” — înregistrează următoarele taste, Esc renunță —, „Revino la
      set”), conflictele în rând (Scoate de la celelalte / Lasă la toate / Renunță); o literă
      simplă nu se ia unde se scrie text; aceleași taste la mai multe comenzi în același loc →
      la apăsare o listă din care alegi; tooltip-urile (bara imaginii) arată tastele de acum
      (`ui_components::shortcuts`). Snapshot: `SHORTCUT` / `SHORTCUT_KEYS`.
- [x] **22h. TypeScript / TSX.** `tree-sitter-typescript` (cele două gramatici), modulul
      `lang-typescript` (`.ts`, `.tsx`, `.mts`, `.cts`: evidențiere, pliere, comentarii,
      indexarea declarațiilor ca la JS).
      *Făcut:* în modulul JavaScript (devenit „JavaScript / TypeScript”), nu într-unul separat:
      modulele nu depind unul de altul, iar TypeScript ar fi repetat toată inteligența JS
      (gramatica lui extinde pe a JS). Limbajele `typescript` (`.ts`, `.mts`, `.cts`) și `tsx`,
      gramaticile `tree-sitter-typescript` (evidențierea JS + a TS, la TSX și JSX), plierea
      (interfețe, enum-uri, tipuri obiect), inteligența pe dialect (interfețe, enum-uri,
      tipuri, clase abstracte, câmpurile tipate, `extends`), nefolosite / depreciate cu
      gramatica dialectului, `JSON.parse` și în TS. Snapshot: `editor-typescript`.
- [x] **22i. Vue și htmx.** Vue: `<script lang="ts">` → TypeScript, `<style lang="scss">` →
      SCSS, expresiile din template (`{{ }}`, `:attr`, `@event`, `v-*`) ca JS; htmx: atributele
      `hx-*` evidențiate, JS în `hx-on:*`.
      *Făcut:* HTML — `<script lang="ts|tsx">` TypeScript / TSX, `<style lang="scss">` SCSS,
      atributele-directivă (`v-*`, `:x`, `@x`, `#x`, `hx-*`, `x-*`) colorate ca
      cuvinte-cheie; modulul Vue — valorile directivelor oriunde și `{{ }}` într-o componentă
      (cu `<template>` de nivel superior) ca JS, sau TS când scriptul e `lang="ts"`;
      modulul htmx — `hx-on…` ca JavaScript. Injecțiile merg oriunde e HTML (și în Twig /
      Blade / PHP).
- [x] **22j. Italic și aldin din temă în editor.** Fețele fontului pe porțiuni (ca terminalul),
      cu lățimea caracterelor neschimbată (monospace).
      *Făcut:* `ColorSpan` poartă aldin / italic (din stilul temei; un scope doar italic ia
      culoarea codului); rândul se modelează normal (pozițiile), iar porțiunile stilizate se
      modelează din nou în fața lor și se pun pe aceleași locuri — nimic nu se mută;
      cache-ul de text al fonturilor le păstrează.
- [x] **22k. Indentarea după arbore.** `indents.scm` pe limbaj (PHP, JS / TS, CSS, HTML, YAML,
      JSON); de-indentare la `}` / `]` / `)` și la etichetele HTML de închidere; Enter între
      etichete. Teste pe fiecare limbaj.
      *Făcut:* `indents.scm` (`@indent`: blocuri, obiecte, liste, argumente, elemente) pentru
      PHP, JS / TS / TSX, CSS, SCSS, HTML (și Blade), JSON, YAML (colecțiile `{}` / `[]`);
      `SyntaxSnapshot::indent_anchor`: rândul unde începe cel mai interior nod încă deschis
      (relativ, deci merge și în limbaje injectate). Enter (`newline_with`): un nivel față de
      acea bază, închiderea la bază, între o pereche (`{|}`, `<div>|</div>`) închiderea pe
      rândul ei; doar cu arborele la zi, altfel regula din text. YAML: după `cheie:` un nivel.
      `}` / `]` / `)` tastate primele pe rând → indentarea deschiderii (potrivire în text);
      `</` primul pe rând → indentarea elementului închis (potrivirea etichetelor în text: la
      jumătatea etichetei arborele nu știe).
- [ ] **22l. Pachetele.** macOS: `.dmg` (semnarea Developer ID și notarizarea — când ai
      certificatul); Windows: `.msi` (WiX); Linux: AppImage. Scripturile pregătite și
      verificate pe bucăți; **build-urile de distribuție le rulezi tu**.
- [ ] **22m. Lista de verificare.** Capturi, Windows (clippy), roadmap-ul (planul 10)
      actualizat cu ce e deja făcut.

## Hotărât — formatare și verificare (2026-10-22)

1. La salvare: **tu alegi**, pe proiect, separat formatarea și verificarea (oprite implicit);
   uneltele: una găsită, un binar sau un script (ex. codesniffer fix); erorile arătate de noi.
2. Scriptul proiectului: din comandă, și la salvare dacă vrei (cu `{file}` sau pe tot
   proiectul); verificarea la salvare cu erorile frumos (subliniere, iconiță, hover, Probleme).
3. Flexibil: fiecare modul de limbaj își dă uneltele (una implicită dacă e găsită), totul prin
   domeniu (porturi), contribuții și evenimente pe magistrala proiectului.
4. Scurtăturile și conflictele: straturi, contexte și rezolvarea conflictelor din Setări.

## Hotărât (2026-10-22)

- Scurtăturile: toate patru (straturi, contexte, seturi date de module, pagina din Setări).
- Acțiunile din view-uri remapabile, navigarea / câmpurile / Esc pe modale fixe; implicitele =
  tastele de acum.
- Conflictele: Scoate / Lasă / Renunță (lista la apăsare când rămân mai multe
  în același context).

## De făcut după planul 22 (cerute pe parcurs, 2026-10-22)

- **Tema Matrix** — verde fosforescent pe negru, ca oricare din temele livrate (UI, sintaxă,
  terminal).
- **Cursorul animat** (editor, câmpuri, terminal) — **descris de temă**, ajustabil în Setări.
  Plan (2026-10-23):
  1. **Tema** primește `[caret]` (opțional; lipsă = cum e azi): `shape` (bar / block /
     underline — editorul și câmpurile; terminalul își păstrează alegerea, cu „ca tema”),
     `width` (grosimea barei, 1–4 px), `blink` (none / classic / smooth — se stinge lin),
     `glow` (0–1, un halou din câteva straturi, fără blur), `trail` (urma care se stinge
     când sare, ms), `motion` (none / smooth — alunecă între poziții, ~80 ms), `color`
     (implicit `editor.cursor`). Domeniul (`theming`) îl citește; configuratorul de teme îl
     arată.
  2. **`ui_kit::caret`** (un singur loc): starea clipirii, a alunecării și a urmei pe
     fiecare cursor (după id), `paint` (forma, opacitatea, haloul, urma) și cadrul următor
     cerut doar cât ține o animație (o tranziție a clipirii, o alunecare, o urmă); în repaus
     un singur redraw la următoarea schimbare, ca azi.
  3. **Toate cursoarele** prin el: editorul (și cursoarele multiple), câmpurile
     (`text_input`), terminalul, câmpurile pânzei.
  4. **Setări → Editor → Cursor**: „Ca tema” (implicit) sau ale tale (forma, grosimea,
     clipirea, alunecarea, haloul și urma), declarate în `settings.rs`.
  5. **Matrix**: bară de 2 px, clipire lină, halou, urmă, alunecare; terminalul: bloc cu
     halou. Capturi (temă închisă / deschisă, Matrix).
  *Făcut (2026-10-23):* toți pașii — `[caret]` în teme (la final; tema extinsă, apoi cum
  era), `ui_kit::caret` (o singură desenare; cadre doar cât ține o animație), editorul /
  câmpurile / terminalul / pânza prin el, fiecare fereastră cu cursorul temei ei, Setări →
  Editor → „Cursor” (ca tema / al meu), configuratorul de teme cu secțiunea „Cursorul”
  (păstrată la o temă nouă); Matrix cu bară lină, halou, urmă și alunecare.
- **Proiectele deschise din iconița aplicației** (clic dreapta), 2026-10-23:
  - **macOS** (Dock): ferestrele deschise, cea curentă bifată (meniul „Window” dat ca meniul
    ferestrelor aplicației: `NSApp.windowsMenu`, titlul ferestrei = proiectul — fișierul), apoi
    „Recent Projects ›” (`applicationDockMenu:` pe delegatul aplicației).
  - **Windows** (bara de start, jump list): proiectele recente ca listă proprie
    (`ICustomDestinationList`), ferestrele deschise sunt deja în miniaturile barei; un clic
    pornește aplicația cu proiectul → **o singură instanță**: procesul pornit dă calea celui
    care rulează deja (care o deschide în fereastra lui / una nouă) și iese.
  - Lista ținută la zi la deschiderea / închiderea unui proiect.
  - *Făcut (2026-10-23):* macOS — meniul „Window” ca lista ferestrelor, „Proiecte recente ›”
    în Dock; Windows — jump list-ul (`ICustomDestinationList`); o singură instanță pe
    executabil (`interprocess`: named pipe al utilizatorului pe Windows, socket Unix în
    folderul lui privat altundeva — niciodată rețeaua); un proiect deschis își aduce
    fereastra în față. De verificat pe Windows real: jump list-ul, a doua pornire.
