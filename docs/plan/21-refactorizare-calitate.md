# 21. Refactorizare și calitate

> **Încheiat** (2026-10-22): 21a–21h.

Cererea: punctele de refactorizare și calitate rămase din planul 11 (etapele 6, 7, 8 și 10),
ținând cont de tot ce s-a construit de atunci (planurile 14–20: ferestrele de unelte,
indexarea, transferurile, modalele pe stivă, Task Manager, pachetele de iconițe, terminalul).
**Fără schimbări de comportament**: ce se vede și ce face aplicația rămâne identic, verificat
pixel cu pixel.

## Azi (măsurat pe cod, 2026-10-22)

Ce s-a rezolvat între timp, deși planul 11 îl are încă nebifat:
- `mouse_down` (~380 de rânduri în planul 11) are acum 28, `edit_key` (~320) are 6: intrarea
  trece prin stiva de overlay-uri (planul 17, `OverlaySlot`), deci și „rutarea overlay-urilor”
  (marcată `[~]`) e făcută.
- `terminal-panel` e deja un modul real (fereastră de unelte, view-uri, setări contribuite).
- Cererile viewer-elor nu mai sunt `match` pe șiruri `theme.*` în `app`.

Ce rămâne:

| Ce | Cât |
|---|---|
| `#[allow(clippy::too_many_arguments)]` | **40**, în 21 de fișiere: `ds.rs` 7, `connect_view.rs` 6, `projects_picker.rs` 3, câte 1–2 în rest (ui-workbench, module: markdown, compare, canvas, remote, image, refactoring, indexing; app 2) |
| Funcții de peste 150 de rânduri | **46**; cele mai mari: `editor_view::paint` 564, `search_panel::paint` 398, `markdown::layout::build` 312, `snapshot::run` 309, `paint_group` 303, `theme-studio::paint` 292, `task-manager::view::paint` 275, `connect_view::paint_contents` 270, `compare::paint` 267, `settings_view::paint_modules` 264, `code_hover::layout` 262, `color_picker::paint` 260, `dialog::paint` 253, `level_picker::paint` 247, `actions::command` 247, `app::event_inner` 246 |
| Logică de aplicație în `app` | `app/closing.rs` (mașina de stări „Salvezi?” la închidere), `usages.rs` (găsirea utilizărilor), `themes.rs` (compunerea / salvarea temelor, tema proiectului), `workbench/session.rs`, `refactor_ports.rs` (541 de rânduri: porturile refactorizărilor implementate în app) |
| `themes-builtin` | folosit direct din `app/themes.rs` (`builtin_sources`, `parse`), nu prin puncte de extensie |
| Cache-uri nemutate pe `kernel::cache` | marcajele din scrollbar, breadcrumbs, titlurile taburilor, bara de stare, panoul Structură, imaginile din Markdown, metricile fonturilor (de verificat fiecare) |
| Reguli încălcate | mesaje de log în română (`report(…, "salvarea")`, `"deschiderea proiectului"`); o deschidere eșuată e doar logată, utilizatorul nu află; `#[allow(missing_debug_implementations)]` ×3 |

## Ținta

- Niciun `too_many_arguments`: structuri de context pentru desenare, ca `ViewerPaint` /
  `SectionArea` (cele bune de azi), lintul lăsat activ peste tot.
- Nicio funcție de peste ~150 de rânduri: fiecare bucată cu un singur rol și un nume care
  spune ce face (o secțiune desenată, un rând, un card), `if`-uri imbricate → `match` /
  early return.
- `app` doar leagă: use case-urile în stratul `application` al contextului lor, testate
  fără fereastră, cu porturi false / în memorie.
- Temele livrate contribuite de modulul lor, ca orice altă extensie.
- Cache-urile rămase pe primitive (golite de „Clear and Rebuild Caches”).
- Codul curat după reguli: engleză peste tot în cod, erorile spuse utilizatorului, `Debug`
  unde lipsește, nume clare, fără duplicare.

Reguli pentru toți pașii:
- **Plasa de siguranță întâi** (21a): capturile de referință ale tuturor ecranelor, apoi după
  fiecare pas comparate pixel cu pixel. Orice diferență e o greșeală, nu o schimbare.
- Pași mici, fiecare cu commit și `check.sh`; testele existente rămân neatinse (dacă un test
  trebuie schimbat, înseamnă că s-a schimbat comportamentul).
- Nimic nou pe UI thread; nicio alocare în plus pe cadru (structurile de context sunt pe
  stivă, cu referințe).

## Pașii (un commit fiecare; `check.sh`; „go” între ei)

- [x] **21a. Plasa de siguranță.** `scripts/modal-snapshots.sh` → `scripts/ui-snapshots.sh`:
      pe lângă modale, toate ecranele (editorul cu erori / hover / completare / căutarea în
      fișier, arborele, taburile și grupurile, terminalul, Task Manager, pânza, compare,
      Markdown, imaginile, studioul de teme, Remote, setările pe toate paginile, bara de sus
      Windows), tema închisă și cea deschisă; referința înregistrată înainte de orice
      schimbare. Scriptul vechi rămâne (alias), aceleași cazuri.
      *Făcut:* `scripts/ui-snapshots.sh record|compare <dir> [grup]` — 58 de ecrane × 2 teme
      = 116 capturi, în grupuri (`modals`, `editor`, `windows`, `modules`, `settings`,
      `chrome`): modalele, editorul (PHP / CSS / eroare, plierea, căutarea, selecția,
      completarea, zoom, tab fixat, împărțit, link ⌘), arborele (căutare, fișier nou),
      terminalul, Structura, pânza ×3, compare (și merge), Markdown, imaginea, studioul, git
      (head / istoric / log / blame / diff, pe un depozit de probă cu autor și date fixe),
      rename / usages / diagrama / semnătura / move / clipboard, cele 9 pagini de setări, bara
      Windows (normală, maximizată, ecran complet), splash-ul. Lăsate deoparte (nu ies la fel
      de două ori): Task Manager, wizard-ul Remote, Local History. Verificat: două rulări
      identice (deterministe), fiecare captură arată ecranul cerut (planșă).
      `modal-snapshots.sh` = grupul `modals`. Referința: înregistrată înainte de 21b.

- [x] **21b. Structurile de context (cele 40 de `too_many_arguments`).** Întâi `ds` (7: un
      `Ctx` + o structură per componentă, ca `SliderStyle`), apoi `ui-workbench` (projects
      picker, splash, editor, taburi, antete, find bar, about, level picker), apoi modulele
      (connect / review view, markdown, compare, canvas, image, refactoring, indexing) și
      `app`. La final, niciun `allow` de felul ăsta. Capturile: identice.
      *Făcut:* 0 din 40. `ds` / `ui-components`: 9 funcții iau `layer, text, ctx` + un
      `*Spec` (`BadgeSpec`, `ChoiceSpec`, `SegmentedSpec`, `StepperSpec`, `RadioCardSpec`,
      `KeyButtonSpec`, `ScrollHintsSpec`, `SliderSpec`, `CodeLine`), apelurile (~53) rescrise
      cu un script care urmărește parantezele. `ui_components::Painter { text, scene,
      palette, icons }` pentru `paint`-urile ecranelor (About, proiectele, taburile, antetul
      de panou, nivelul limbajului, splash, bara de căutare, previzualizarea erorii, viewer-ul
      din app, căutarea în proiect, bara imaginii); structuri cu nume (`SplashFrame`,
      `FindBarSpec`, `ErrorPreviewSpec`, `TabMenuState`, `RowGeometry`, `Run`, `Page`,
      `CellSpec`, `Column`) și un `Pen { layer, text, ds }` în modulele cu desenări ajutătoare
      (Remote, pânza, compare, refactoring — patru copii private: unite în `ds` la 21h).
      Corpurile funcțiilor neschimbate (structurile desfăcute în aceleași nume). Verificat:
      `check.sh`, cele 116 capturi identice cu referința.

- [x] **21c. Funcțiile lungi — desenările.** Cele 16 de peste 240 de rânduri, apoi restul de
      peste 150: fiecare `paint` împărțit pe ce desenează (fundal, rânduri, antet, cursor,
      overlay-uri…), cu starea comună într-o structură. Ordinea: `editor_view::paint`,
      `search_panel`, `paint_group`, `settings_view`, `code_hover`, `dialog`, `level_picker`,
      `color_picker`, apoi modulele (theme-studio, task-manager, connect, compare, markdown,
      structure, project-tree, canvas). Capturile: identice.
      *Făcut:* cele 38 de desenări de peste 150 de rânduri, împărțite pe ce desenează (în
      aceeași ordine), cu valorile comune într-o structură (`Frame`, `Row`, `Strip`,
      `Preview`, `Look`, `TabLook`, `BlockRows`, `ModuleRows`, `Card`, `Columns`, `Grid`,
      `Page`, `Table`…): `editor_view::paint` 565 → 28 de rânduri (un rol pe bucată: gutter,
      text, selecții, cursoare, scrollbar-e, marcaje), căutarea în proiect 422 → 45,
      `paint_group` 296 → 61, setările, modalele, dialogul, selectoarele, studioul, Task
      Manager, Structura, arborele, Remote, compare, pânza, refactoring, git, diagramele. Cea
      mai lungă desenare rămasă: `paint_theme_cards` (126). Lucrat de patru agenți în paralel
      (worktree-uri, fișiere disjuncte), adus aici; verificat: `check.sh`, cele 116 capturi
      identice. Corectat la preluare: arborele întreabă din nou o singură dată pe rând dacă
      un fișier lipsește de pe disc.

- [x] **21d. Funcțiile lungi — logica.** `actions::command` (247: comenzile pe grupe, fiecare
      grup o funcție), `app::event_inner` (246: un handler pe fel de eveniment),
      `snapshot::run` (309: pașii lui, fiecare cu nume), `markdown::layout::build`,
      `graph::build_graph`, `engine::complete`, `render-wgpu::render`. Teste: neschimbate.
      *Făcut:* `actions::command` 247 → 48 (comenzile pe zone: fișiere, taburi, navigare,
      editare, vedere, restul — id-urile sunt unice, deci fiecare ajunge la același braț),
      `event_inner` 246 → 52 (`app/events.rs`, un handler pe fel de eveniment), `snapshot::run`
      312 → ~15 (pașii cu nume, în aceeași ordine), Markdown `build` 342 → 76 (`Walk`, pe
      feluri de bloc), așezarea pânzei 193 → 31 (pe faze), verificarea nivelului PHP 156 → 68
      (`Checker`), graful 243 → 22 (`GraphBuilder`), completarea 200 → 34 (`Completer`, pe
      context), cadrul GPU 194 → 51 (obținere, pregătire, încărcare, pasul, prezentare).
      Nicio funcție de peste 150 de rânduri în cod. Plasa: un document Markdown complet
      (liste imbricate, citat, linie, tabel, imagine) adăugat la capturi — referința
      reînregistrată pe codul verificat dinainte; 118 capturi identice.

- [x] **21e. Use case-urile în `application`.** Pe contexte (planul 02):
      - Document: `CloseDocuments` — mașina de stări din `app/closing.rs` cu un port pentru
        întrebarea „Salvezi?” și unul pentru salvare; `SaveDocument`.
      - Workspace: `OpenProject`, `RestoreSession` / `SaveSession` (din
        `workbench/session.rs`), `RevealInProject`.
      - Intelligence: `FindUsages` (din `app/usages.rs`; regulile per limbaj din
        `LanguageIntelligence`).
      - Refactoring: porturile din `refactor_ports.rs` mutate în infra / modulul lor;
        `app` doar le leagă.
      - Theming: `ComposeTheme`, `SaveUserTheme`, `SelectProjectTheme` (din `app/themes.rs`).
      Fiecare cu teste fără fereastră (porturi false / în memorie); `app` le apelează.
      Verificat cu `cargo xtask check-deps` (straturile).
      *Făcut (1/5):* `document::CloseDocuments<T>` (`application/closing.rs`): coada
      documentelor de închis, pasul următor (`Close` / `Ask` / `Done`) prin portul
      `OpenDocuments` (deschis? modificat?), `resolved` după salvare / renunțare, `T` = ce
      urmează (fereastra, ieșirea, alt proiect); `app` doar întreabă, salvează și închide
      taburile (`OpenTabs` = portul). Teste fără fereastră.
      *Făcut (2/5):* sesiunea, în contextul Workbench (unde stă și `Session`, nu Workspace):
      `Session::capture` (doar fișierele de pe disc, relative la proiect; un grup fără
      fișiere → totul într-unul), `Session::reopen` (fișierele care mai există, cel activ
      marcat), `SessionKeeper` (cel mult o dată la jumătate de secundă, doar la schimbare,
      nu după închidere — `freeze` / `thaw` / `reset`); `app` doar adună taburile și le
      deschide. Deschiderea proiectului rămâne în `app`: e legare (indexul, watcher-ul,
      git, serviciile modulelor); citirea fișierelor recente → `storage` la 21h.
      *Făcut (3/5):* `code_intelligence::usages` — `UsageQuery`, regulile pe fel de simbol
      (`matches`), căutarea prin fișierele citite de apelant (`find_usages`: una pe rând, fără
      declarația însăși, cel mult 200, anulabilă); unde poate apărea un simbol: al unui
      limbaj — spus de modulul lui (`LanguageIntelligence::usage_extensions`: PHP, JS), al
      CSS — după fel (folosit în mai multe limbaje). `app/usages.rs` doar firul, anularea și
      citirea de pe disc (fișierele mari sărite). Teste fără fereastră (citire falsă).
      *Făcut (4/5):* `app/refactor_ports.rs` (541 de rânduri) împărțit după cui îi aparține:
      adaptoarele porturilor de refactorizare peste index (`ProjectSources`, `IndexResolver`,
      `IndexConflicts`, regulile de text) → modulul `refactoring` (`ports.rs`); sintaxa
      parametrilor pentru „Change Signature” → modulul limbajului (`lang-php` `PhpParams`,
      `lang-javascript` `JsParams`), contribuită ca `SignatureSyntax` (care spune ce fișiere
      citește: `extensions()`) și aleasă de app după fișierul declarației — nu un `if php`
      în app; `split_default` (comun) → domeniul `refactor`. Testele mutate cu codul.
      *Făcut (5/5):* `theming::catalog::ThemeCatalog` — temele livrate și ale utilizatorului
      (una cu același id o înlocuiește), tema aleasă înainte (dacă mai există, altfel prima
      închisă), sistemul hotărăște până alegi, următoarea, o temă scrisă în configurator
      înregistrată / salvată; porturile `ThemeSettings` (alegerea, `[appearance]`) și
      `UserThemeFiles` (folderul `themes`); textul temelor citit de apelant (formatul e al
      modulului lor). `app/themes.rs` doar adaptoarele și paleta. Tema proiectului e deja a
      sesiunii (21e.2). Teste fără fereastră (teme și porturi false); 118 capturi identice.
- [x] **21f. Temele livrate ca modul.** `themes-builtin` contribuie temele printr-un punct de
      extensie (`ThemeSource`), descoperit de app ca orice contribuție; studioul și temele
      utilizatorului la fel. Teste: temele găsite, alegerea salvată (id-urile vechi merg).
      *Făcut:* punctul de extensie `theming::catalog::ThemeProvider` (temele aduse, în
      ordine, și formatul lor — cu care se citesc și temele utilizatorului: primul furnizor
      care le înțelege). `themes-builtin` e modulul `themes` (`ThemesModule`, furnizorul
      `Bundled`), înregistrat ca oricare; app încarcă modulele, apoi temele din furnizorii
      descoperiți (nu mai cheamă modulul direct). Un modul cu alte teme / alt format = doar
      o contribuție. Capturi: identice, în afară de numărul de module (42 → 43 în About și
      în Setări → Module) — modulul nou.

- [x] **21g. Cache-urile rămase.** Verificat fiecare din listă: cele recalculate pe cadru
      (sau pe fiecare schimbare mică) mutate pe `Memo` / `GenerationalCache` (imaginile din
      Markdown cu limită în octeți), toate golite de „Clear and Rebuild Caches”. Măsurat:
      CPU pe cadru și memoria, înainte și după.
      *Făcut:* întâi un inventar (ce se recalculează la fiecare cadru), apoi doar ce merita:
      - erori de cache: imaginile din Markdown nu se eliberau la golire (așezarea le ținea)
        și nu aveau limită — acum `GenerationalCache` (8 pe generație), cheia (cale, mtime)
        (reîncărcate când se schimbă pe disc), pixelii doar pe GPU (`gpu_only`, redecodate
        dacă textura a fost eliberată); metricile fonturilor ignorau grosimea și italicul —
        acum cheia e fontul întreg; cheia marcajelor din scrollbar putea rata o schimbare la
        aceeași versiune (acum și o generație a lor); breadcrumbs nu știau de setul de
        iconițe ale fișierelor;
      - munca de pe fiecare cadru: iconițele taburilor (~30 de alocări / tab / cadru) într-un
        `GenerationalCache` pe nume (golit la schimbarea temei / setului de iconițe);
        cerințele din `composer.json` doar când se refac marcajele; mesajul de pe rândul
        cursorului cu `partition_point`, nu `line_of` pe fiecare marcaj; panoul Structura
        fără copii ale listelor, cu titlul rupt pe rânduri și etichetele scurtate în cache;
      - golirea („Clear and Rebuild Caches”) ajunge acum și la breadcrumbs, la iconițe, la
        view-urile ferestrelor de unelte și la panourile laterale (`ToolPanel::purge`).
      Restul (taburile, breadcrumbs, metricile) era deja în cache sau ieftin. Verificat:
      `check.sh`, testele noi, cele 118 capturi identice. Măsurat: alocările scoase din
      cadru (numărate în cod); memoria imaginilor câștigată doar pe wgpu (capturile folosesc
      renderer-ul software, care le ține în RAM).

- [x] **21h. Review pe crate-uri și lista de verificare.** Mesajele de log în engleză
      (`report`), o deschidere / salvare eșuată spusă utilizatorului (notificare, din
      locale); `Debug` în locul celor 3 `allow(missing_debug_implementations)`; nume,
      duplicare (desenări de carduri / rânduri / comutatoare repetate → `ds`; cele patru
      `Pen` private din 21b → unul în `ds`), comentarii
      vechi; planul 11 bifat (și ce s-a făcut între timp, de mai sus); capturile finale
      comparate cu referința din 21a.
      *Făcut:* `report()` (loguri în română / engleză amestecate, eșecul doar logat) →
      `succeeded(rezultat, Doing::Saving | Opening)`: logul în engleză și o notificare
      pentru utilizator din locale (`files.save_failed`, `files.open_failed` nou) — o
      deschidere / salvare eșuată se vede acum; documentația rătăcită (procese separate pe
      fereastră — nu mai e cazul) scoasă. `Debug` scris pentru `ViewerPaint`,
      `PanelContext`, `IndexContext` (fără `allow`). Cele patru `Pen` identice → `ds::Pen`
      (cu `lend()`); al Remote-ului (cu iconițe) rămâne al lui. Fișierele recente ale
      proiectului → `storage::recent_files` (citire / scriere atomică). Planul 11 bifat
      (etapele 6–8, 10). Verificat: `check.sh`, capturile finale identice cu referința.

## De hotărât

1. Pragul pentru funcțiile lungi: propun **~150 de rânduri** (cu excepții motivate, ex. un
   `match` de comenzi lung dar plat).
2. Ordinea: propun cea de mai sus — întâi plasa de siguranță, apoi schimbările mecanice
   (21b–21d, fără risc de comportament), apoi cele de arhitectură (21e–21g).
3. 21e e cel mai mare pas: îl împart pe contexte (câte un commit), cu „go” între ele?
