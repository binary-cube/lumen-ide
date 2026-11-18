# 81 — Bara de jos: ce nu încape sub „…”, fișierul într-un singur element, resursele ca grafic

**Stare (2026-11-16): 81a–81c făcute; 81d (review-ul separat) de făcut.**

Cerut 2026-11-16 (capturile în conversație):

- Pe o fereastră îngustă, cu multe elemente, bara se desenează „aiurea” (elementele din dreapta
  trec peste cele din stânga): „să apară cele esențiale primele, și restul intră sub `…`, și
  la click pot să le văd”.
- Informațiile fișierului compactate: `{ } JSON · UTF-8 · LF · 4sp`, la hover o fereastră
  cu câte un rând (Limbaj, Engine, Encoding, Line endings, Indentare).
- CPU / RAM / GPU compactate vizual: trei bare mici colorate, care se mișcă la schimbări;
  la hover, o fereastră cu graficul fiecăreia și valoarea („pe designul nostru”); cu o
  opțiune de **pin**: rămâne deschisă tot timpul, peste tot.

## Ce avem deja (verificat în cod)

- `ui-workbench/src/status_bar.rs`: `StatusItem` (iconiță, text, comandă la click, lățime
  rezervată, grup, cale, tooltip). `StatusBar::paint` pune stânga de la stânga (o cale se
  taie la mijloc, un text se scurtează de la început) și **dreapta toată, de la margine
  spre stânga, fără să verifice dacă încape**: de aici suprapunerea.
- `app/src/workbench/status.rs`: elementele, în ordine: semnele modulelor (ex.
  „Kept awake”), lucrul în desfășurare, indexarea, erori / avertismente, starea
  vizualizării, aparițiile, poziția cursorului, nivelul limbajului, limbajul, motorul,
  sfârșitul de rând, codificarea, indentarea, zoom-ul, CPU / RAM / GPU, lacătul.
- Resursele: un eșantion pe secundă (`self.resources`: CPU, memoria, GPU) pentru bară;
  Task Manager-ul are istoria lui (inele), dar e un modul: aplicația nu-l citește.
- Ferestrele peste editor: `OverlaySlot` (`Overlay::ALL`); popover-ele păstrează
  `ds::POPOVER_GAP` față de bară. Meniurile (`ViewMenuEntry`), tooltip-urile, graficele
  mici (sparkline-urile Task Manager-ului și ale widget-ului de pe ecranul de relaxare).

## Regula

1. **Ce contează rămâne vizibil.** Fiecare element are o **prioritate**:
   - esențiale (nu pleacă niciodată): lacătul, erorile / avertismentele, poziția cursorului,
     lucrul în desfășurare (cu Stop-ul lui);
   - obișnuite: fișierul (elementul compact, 81b), resursele (81c), semnele modulelor;
   - de rezervă: zoom-ul, nivelul limbajului, aparițiile, starea vizualizării.

   Când nu încap, pleacă întâi cele de rezervă, apoi cele obișnuite, de la cea mai mică
   prioritate, în locul lor apare `…`; un click pe `…` arată ce lipsește, ca rânduri care
   fac exact ce făceau în bară (același click, aceeași comandă). Stânga (calea) păstrează un
   minim; restul se scurtează ca acum.
2. **O singură implementare, în componentă**: așezarea după prioritate și `…` sunt ale
   `StatusBar` (`ui-workbench`), testate acolo; aplicația doar dă elementele cu prioritatea
   lor. Ferestrele de la hover sunt o componentă comună (`ui-components`: popover-ul
   barei, cu rânduri cheie / valoare, cu rânduri care se pot apăsa, și cu pin).
3. **Cât de ieftin se poate**:
   - nicio citire în plus: graficele folosesc eșantionul pe secundă care există deja,
     ținut într-un inel mic (60 de valori pe resursă, în aplicație: câteva sute de octeți);
   - barele se mișcă **doar când valoarea se schimbă** (o tranziție scurtă, ~200 ms, cadre
     cerute doar cât durează: `frame_deadline`), nu în buclă;
   - fereastra cu grafice desenează din inel, refăcută doar la un eșantion nou;
   - când utilizatorul lipsește (cursorul se odihnește, fereastra în fundal), barele stau:
     nicio animație care să țină aplicația trează.

## Pași (fiecare cu teste, gate verde, un commit)

1. **81a. Ce nu încape, sub „…”.** `StatusItem::priority` (`Essential`, `Normal`, `Spare`).
   `StatusBar` măsoară tot, alege ce încape (stânga cu minimul ei), lasă loc pentru `…`,
   desenează în ordinea dată. `…` deschide fereastra comună cu elementele ascunse, ca
   rânduri (iconița, textul, comanda). Teste pe lățimi: nimic nu se suprapune, esențialele
   rămân, ordinea se păstrează. Snapshot-uri la trei lățimi.
2. **81b. Fișierul într-un singur element.** `{ } JSON · UTF-8 · LF · 4sp` (iconița
   limbajului în culoarea lui); hover / click: fereastra cu rândurile Limbaj, Engine
   (unde e de ales), Encoding, Line endings, Indentare; fiecare rând deschide alegerea pe
   care o deschidea elementul lui (aceleași comenzi). Tooltip-urile vechi rămân în rânduri.
3. **81c. Resursele ca grafic.** Un element cu trei bare mici (CPU verde, RAM albastru,
   GPU violet, culorile din paletă): înălțimea lor e valoarea față de un maxim care se
   adaptează (CPU la 100%, memoria la vârful ultimului minut). Hover: fereastra cu trei
   rânduri (numele, graficul ultimului minut, valoarea în cifre tabulare), ca în captura
   dată, pe designul nostru (`ds`). Click: Task Manager-ul (ca acum). În fereastră, **pin**:
   rămâne deschisă (un `OverlaySlot` plutitor, care nu ia tastele și nu blochează nimic sub
   el), ținută minte între porniri (o setare); din nou pin, se închide la ieșirea mouse-ului.
4. **81d. Review de un agent separat**, reparat ce găsește; „Făcut” / „Lăsat” aici; regula
   barei (prioritățile, componenta) în `.claude/rules/project-conventions.md`.

## De hotărât de utilizator

- Prioritățile de mai sus: e ceva ce vrei să rămână mereu vizibil și nu e în „esențiale”?
- Pin-ul: fereastra fixată stă deasupra barei, în dreapta (unde apare la hover), sau o pot
  muta?

## Cerut după 81c (2026-11-16)

5. **81e. Resursele fixate, mici.** Fereastra fixată cu pin e prea mare (graficele peste
   editor). Fixată, devine o bandă de „pastile” ca elementele de odinioară ale barei
   (`CPU 2,2%`, `RAM 500 MB`, `GPU 136 MB`, fiecare cu culoarea seriei lui), cu pin-ul la
   capăt (Desprinde), în colțul de jos-dreapta al editorului: **deasupra** barelor lui de
   derulare (cea de jos, banda din dreapta), niciodată peste ele. Fereastra de la hover
   (nefixată), puțin mai mică: rânduri mai joase, grafice mai înguste. Ține locul lui
   „fereastra fixată se poate muta” (lăsat): stă în colț.

   **Făcut (2026-11-16):** fixate, resursele sunt pastile de 24 px (punct în culoarea
   seriei, eticheta, valoarea în fontul de cod) și pin-ul aprins la capăt, cu colțul la
   12 px (banda barelor de derulare) + 8 px de marginea cardului editorului; se arată și
   când bara n-are loc de elementul lor (`StatusPopover::paint_pills`, `Pill`,
   `resource_pills`). Fereastra de la hover: rânduri de grafic 30 px (din 38), grafice de
   110 px (din 150), antet 26 px, lățime minimă 240 px. Test:
   `pinned_pills_end_at_their_corner_with_the_pin_last`.

6. **81f. Rândurile ferestrei fișierului aleg.** Limbaj, codificare, sfârșit de rând,
   indentare: fiecare rând deschide lista de alegeri a aplicației (ca Motorul), cu cea de
   acum bifată.

   **Făcut (2026-11-16):** comenzile `file.choose_language`, `file.choose_encoding`,
   `file.choose_line_ending`, `file.choose_indent` (și în paletă), în
   `workbench/file_format.rs` (`ChoicePurpose::Format`, `FormatPick`). Codificarea: „scris
   așa la salvare” (`Document::change_encoding`; Latin-1 refuzat, cu un anunț, dacă textul
   are ce nu poate scrie) sau „Recitește ca …” (`document::decode_as`, pe worker-ul
   `file-reload`, oferit doar fără modificări nesalvate). Sfârșitul de rând:
   `change_line_ending`. Documentul e nesalvat cât codificarea / sfârșitul de rând diferă de
   ale fișierului (`saved_format`), înapoi la al lui: salvat; regulile `.editorconfig` de la
   deschidere (`set_*`) nu fac fișierul nesalvat. Indentarea: 2/4/8 spații sau tab. Limbajul:
   sintaxa tabului refăcută (ca la redenumire). Teste: `a_format_chosen_is_unsaved_until_saved`,
   `bytes_read_as_the_encoding_chosen`, `workbench/file_format.rs` (4).

   **Lăsat:** limbajul ales nu e ținut minte după închiderea tabului (o asociere pe fișier
   în setările proiectului e un pas al ei); codificările sunt cele cinci pe
   care documentul le știe (Windows-1250/1252 ar cere un tabel de conversie: la nevoie).

7. **81g. Ce s-a schimbat cât fereastra a fost lăsată.** Măsurat: cu discul foarte ocupat
   (un build care scrie mii de fișiere alături), macOS pierde evenimentele unui folder urmărit
   fără niciun semn (1 din 40 auzit, niciun „reverifică”; fluxul își revine după). Arborele
   și tab-urile pot rata atunci fișiere făcute, șterse sau scrise. Ca „Synchronize files on frame
   activation” (fișierele resincronizate la revenirea în fereastră), dar nu la fiecare revenire în fereastră și
   niciodată pe firul UI.

   **Făcut (2026-11-16):** la plecare se ține doar ora (nimic citit). La revenire:
   - panoul de proiect (`project-tree`, `away.rs`, pe `ProjectLeft` /
     `ProjectBroughtForward`) se uită, pe firul lui de listare, la data folderelor citite
     deja; doar cele schimbate de atunci (2 s marjă pentru discurile cu timp grosier) sunt
     anunțate ca `EntriesChanged` (ca de watcher: le recitește arborele, le văd și
     indexurile); nimic schimbat, nimic citit;
   - tab-urile (`Workbench::check_open_files`): ștampilele comparate pe worker-ul
     `file-reload`, citite doar fișierele schimbate, aplicate ca la o scriere auzită; o
     singură verificare odată.
   Teste: `only_the_folders_changed_while_away_are_told`,
   `back_without_having_left_does_nothing`, `back_in_the_window_what_changed_meanwhile_shows`.

   **Lăsat:** folderele necitite încă (închise în arbore) nu sunt privite: se citesc oricum
   când sunt deschise; indexurile pot rata un fișier făcut acolo cât evenimentele s-au
   pierdut (o plimbare prin tot proiectul la fiecare revenire ar costa prea mult).

## Făcut (2026-11-16)

- **81a** (`96100171`): `Priority` (`Essential` / `Normal` / `Spare`), `joined`, `fit` în
  `StatusBar`; stânga păstrează 160 px; `…` (`STATUS_MORE`) deschide meniul nostru cu ce
  lipsește, rândurile fac ce făceau în bară (un rând fără comandă doar spune și închide
  lista). Snapshot-uri: `RESOURCES=1`, `STATUS_MORE=1`.
- **81b** (`357e5359`): fișierul ca un singur element (`{ } JSON · UTF-8 · LF · 4sp`, iconița
  limbajului în culoarea lui); fereastra lui: Limbaj, Engine (rândul comută motorul, unde e
  de ales), Encoding, Line endings, Indentare. Componenta `status_popover` (rânduri valoare /
  grafic, pin), plutitoare (`Overlay::StatusPopover`, sub notificări), deschisă la hover,
  închisă când mouse-ul pleacă de pe element, fereastră și drumul dintre ele.
- **81c** (`75a89dac`): resursele ca trei bare (culorile seriilor din Task Manager:
  CPU albastru, memorie verde, GPU violet), care alunecă 220 ms la schimbare și cer cadre
  doar cât se mișcă; fereastra cu graficul ultimului minut și valoarea; pin ținut minte
  (`[status] resources_pinned`); click: Task Manager-ul. Un inel de 60 de valori pe resursă
  din eșantioanele care existau deja.

## Lăsat

- **Culorile barelor** sunt ale Task Manager-ului (CPU albastru), nu ale capturii date
  (CPU verde): aceleași peste tot; se schimbă într-un loc (`chart::Series::tag`).
- **Fereastra fixată** stă deasupra elementului ei; mutarea ei nu e făcută.
- **81d** (2026-11-16): review-ul de un agent separat (fără concluziile autorului). Găsite
  și reparate (fiecare cu un test, cel al copiilor verificat că pica înainte):
  - rândurile din `…` ale elementelor cu fereastră (Fișierul, Resursele) nu făceau nimic
    (id-ul lor nu e o comandă): acum `STATUS_MENU` + `status_more_chosen` deschid fereastra
    deasupra `…`-ului (Resursele: Task Manager-ul, când există);
  - un format ales și salvat lăsa copia fișierului din alt grup „nesalvată” (o salvare nu
    schimbă versiunea, sincronizarea nu rula): `save_tab` salvează și copiile care au
    textul scris, în formatul scris;
  - resursele fixate (pastilele) țineau singura fereastră a barei: niciun alt element nu-și
    mai deschidea fereastra la hover. Pastilele au acum overlay-ul lor
    (`Overlay::ResourcePills`, sub tot); pin-ul ferestrei le fixează, al pastilelor le
    desprinde;
  - „Recitește ca UTF-8 / UTF-8 BOM” spune marca pe care fișierul o are (o salvare nu mai
    adaugă / scoate BOM necerut; conversia o face); octetul nepereche al unui UTF-16 apare
    ca `�`, nu dispare; un fișier crescut peste 64 MB nu e recitit întreg;
  - alegerea din listă merge la documentul pentru care s-a deschis lista, nu la tabul activ
    în clipa alegerii; Latin-1 verificat și pentru un document nou.

  **Lăsat:** pastilele își refac textele la fiecare cadru desenat (trei etichete, trei
  valori): cadrele vin doar la o schimbare, costul e mic — de memorat pe eșantion dacă
  măsurătorile o cer; un document nou și gol trecut pe CRLF devine „nesalvat” (cerut
  explicit); „Salvează ca” aplică regulile `.editorconfig` ale locului nou peste un format
  ales înainte (ale folderului câștigă, ca la deschidere).
