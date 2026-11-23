# 96 — Încadrarea textului (word wrap), peste tot unde e text

**Cererea:**
- liniile mai lungi decât vederea să continue pe rânduri dedesubt, cu un semn vizibil că
  rândul a fost încadrat;
- oprită implicit; se pornește din setări sau din meniu;
- să se țină cont de ea peste tot unde avem text;
- opțiunile să fie un singur obiect de date transmis vederilor („un soi de DTO”), ca pe
  viitor să mai putem adăuga altele.

## Pasul 1 — opțiunile de text, transmise tuturor vederilor

**Făcut:**
- **`ui_components::TextOptions`** (`text_options.rs`): obiectul de date pentru cum se
  arată textul, deocamdată `word_wrap`. O opțiune nouă e un câmp nou, cu valoarea implicită
  „ca până acum”, deci o vedere care nu o citește rămâne neschimbată.
- **Contractul vederilor:**
  - `Viewer::set_text_options` și `ToolPanel::set_text_options` (implicit nu fac nimic);
  - aplicația le spune fiecărei vederi (tab-uri, ferestrele de jos, panourile laterale,
    vederea modală) la primul cadru, apoi doar când se schimbă: o generație comparată pe
    vedere (`sync_text_options`), nimic altceva pe cadru.
- **Ce au în comun vederile, într-un singur loc:**
  - `wrap_rows`: rândurile unei linii încadrate la N coloane, rupte după un spațiu sau un
    semn în a doua jumătate a rândului, altfel unde rândul e plin; un caracter nu se taie
    niciodată, iar un tab merge până la oprirea lui;
  - `char_columns` (mutat aici din editor): coloanele unui caracter (cele late iau două).
- **Setarea** `[editor] word_wrap` e oprită implicit. Apare în Setări → Editor → „Text” și
  în meniul View → „Încadrare text” (Alt+Z).
- **Teste:** rândurile unei linii; generația; vederea află opțiunile o dată și apoi doar la
  schimbare.

## Pasul 2 — editorul de cod încadrează

**Făcut:**
- **Harta rândurilor** (`editor_view/wrap.rs`, `WrapRows`), peste plieri: ține lățimea
  fiecărei linii (în coloane, independent de fereastră), câte rânduri ia și primul ei rând.
  Rândul unei linii se citește direct, linia unui rând se află prin căutare binară.
- **Cum se ține la zi:**
  - un text nou se măsoară într-o singură trecere, fără copii;
  - după o editare se citesc doar liniile atinse (`changes_since`);
  - o lățime nouă (redimensionare, zoom) sau plieri noi citesc doar liniile mai late decât
    un rând.
- **Măsurat** (release, 200 000 de linii):

  | Ce | Fișier de 16 MB, toate liniile încadrate | Linii scurte |
  |---|---|---|
  | Deschidere | 83 ms | 2,8 ms |
  | Redimensionare | 69 ms | 0,35 ms |
  | O tastă | 0,13 ms | — |

  Pe cod obișnuit, cu puține linii lungi, redimensionarea e sub o milisecundă.
- **Un singur loc rând ↔ linie** (`editor_view/rows.rs`):
  - fiecare rând de ecran e partea liniei lui (`Seg`: de la un byte la altul, primul,
    ultimul);
  - cadrul, clicul, selecția cu mouse-ul, derularea, `caret_rect`, `reveal_cursor`,
    `top_line` și cititorul de ecran întreabă toate aici;
  - cu încadrarea oprită, rândurile sunt exact cele ale plierilor, ca înainte.
- **Desenul pe părți de linie:**
  - textul, cu culorile și stilurile lui;
  - selecțiile (una care începe la ruptură e a rândului următor), erorile, potrivirile;
  - cursorul, textul IME, hint-urile rândului lor;
  - eticheta plierii și lens-urile doar pe ultimul rând;
  - fără ghidaje de indentare pe rândurile care continuă o linie;
  - rândul curent e evidențiat pe toate rândurile liniei.
- **Semnul de încadrare:**
  - „↩” estompat la capătul fiecărui rând care continuă;
  - „↪” în gutter, în locul numărului, pe rândurile următoare, care păstrează și bara de
    modificări.
- **Fără bară orizontală** cât e pornită; o coloană e lăsată pentru semn.
- **↑ ↓ PageUp PageDown** merg pe rânduri de ecran, la același loc în rând
  (`moved_by_rows`). La primul sau ultimul rând, cursorul ajunge la începutul sau la
  sfârșitul textului.
- Clicul după capătul unui rând care nu e ultimul pune cursorul înaintea ultimului
  caracter, pentru că rândul următor începe chiar la capăt.
- Cititorul de ecran primește o linie încadrată o singură dată, cu locul tuturor rândurilor
  ei.
- **Teste:**
  - rândurile hărții;
  - o editare citește doar liniile ei, iar rezultatul e identic cu o măsurare completă;
  - o linie lungă în rânduri, fără nimic pierdut, cu ambele semne desenate, fără derulare
    orizontală, iar oprită revine la un rând pe linie;
  - clicul pe al doilea rând; ↓ pe rândul următor; dincolo de ultimul rând.

**Lăsat:**
- Home și End merg încă la începutul și sfârșitul liniei, nu ale rândului.
- Hint-urile inline nu intră în calculul rândurilor: un rând cu hint-uri poate trece puțin
  de margine.
- Rândurile care continuă o linie încep de la margine, fără indentarea liniei.

## Pasul 3 — celelalte vederi cu text, și indicatorul

**Făcut:**
- **Editorul:** Home și End merg întâi la începutul și la sfârșitul rândului încadrat; a
  doua apăsare merge la ale liniei (`row_home_end`).
- **Compararea (diff), deci și istoricul local care o folosește**
  (`compare/src/wrap.rs`, `Heights`):
  - un rând aliniat ia atâtea rânduri de ecran câte îi trebuie celei mai lungi dintre
    părți; celelalte rămân goale dedesubt;
  - derularea, clicurile pe diferențe, alegerile din „Compune” și banda din dreapta urmează
    înălțimile;
  - selecția cu mouse-ul merge pe rândurile încadrate;
  - nu mai există derulare laterală cât e pornită;
  - culorile de sintaxă sunt calculate pe linia întreagă și desenate pe părți, cu o
    funcție nouă în componenta comună (`draw_code_part`), ca un șir rupt pe două rânduri
    să-și păstreze culoarea.
- **Diff-ul din git** (vederea commit-ului, `commit_view/wrap.rs`, `DiffWrap`):
  - liniile de cod se încadrează, iar antetele hunk-urilor rămân pe un rând;
  - geometria stă în selecția codului, deci desenul și mouse-ul citesc același lucru;
  - selecția e desenată pe fiecare rând pe care îl atinge (`bands`).
- **Corpul răspunsului din clientul HTTP:** rândurile fiecărei linii la lățimea corpului,
  refăcute doar când se schimbă corpul sau lățimea.
- **Jurnalul (Output și celelalte):** își păstrează butonul propriu, dar opțiunea comună îi
  dă starea. Funcția lui proprie de rupere a fost înlocuită cu `wrap_rows`, deci peste tot
  se rupe la fel.
- **Vederile care țin alte vederi le dau opțiunile mai departe**, inclusiv celor create
  după: panoul Servicii și tab-ul de rezultate al consolei.
- **Indicatorul din bara de jos**, lângă detaliile fișierului:
  - iconița de încadrare: colorată, cu „Încadrat”, cât e pornită; estompată cât e oprită;
  - tooltip-ul spune starea și scurtătura;
  - un clic o pornește sau o oprește (aceeași comandă ca meniul și Alt+Z).
- **Teste:**
  - înălțimile diff-ului și clicul pe al doilea rând;
  - selecția încadrată din git;
  - rândurile corpului HTTP;
  - jurnalul;
  - panoul Servicii dă opțiunile unei vederi făcute mai târziu;
  - indicatorul (starea, tooltip-ul, comanda);
  - Home și End.

**Lăsat:**
- Hint-urile inline nu intră în calculul rândurilor editorului.
- Rândurile care continuă o linie încep de la margine, nu de la indentarea ei.
- Consola SQL e editorul aplicației, deci încadrează deja. Terminalul își rearanjează
  singur textul și nu e afectat.

## Recenzia (agent separat), verificată și reparată

Fiecare problemă a fost verificată în cod înainte de reparare. Unde era nevoie, are un test
care ar fi picat pe codul vechi.

- **Cititorul de ecran, pornit din nou:** numărul cererii revenea la aceeași valoare, așa
  că putea primi o diferență în loc de arborele întreg. Cererile se numără acum mereu în
  sus (`Asks`), iar o actualizare făcută pentru o cerere veche e aruncată.
- **O linie foarte lungă încadrată:** editorul o rupea din nou pentru fiecare rând, deci
  O(rânduri × linie) la fiecare tastă. Acum părțile liniei se calculează o dată (după
  document, versiune, linie, coloane și tab), iar rândul cursorului se caută binar
  (o linie de 500 KB: imediat).
- **Selecția cititorului de ecran în editorul de cod** era ignorată. Acum se pune, după
  liniile spuse (`select_in_editor`, `told_line`).
- **Diff-ul din git, încadrat:** cheia era adresa din memorie a rândurilor, care se poate
  refolosi pentru alt fișier. Acum fiecare citire are un id unic (`FileDiff::id`).
- **HTML cu tabele imbricate peste limită:** închiderea unui tabel ignorat îl închidea pe
  cel din jur. Tabelele ignorate se numără acum, iar rândurile lor nu mai trec în tabelul
  din jur.
- **CSV ostil:** se țineau toate câmpurile unui rând. Acum se țin doar câte coloane are
  tabelul, iar un câmp se oprește la 64 KB.
- **Selecție uriașă pentru grafic:** după limită, restul se numără dintr-odată, fără să fie
  parcurs (50 de milioane de rânduri: imediat).
- **Rândurile pentru panou în Explorer:** clientul bazei de date se apela sub lacăt. Acum
  lacătul doar mută starea. Răspunsurile sosite înainte să fie ținut tichetul se păstrează
  pentru după.
- **Lipirea pe un rând:** un tab devine spațiu, ca în căutarea de dinainte, iar o linie nouă
  singură e tot un spațiu.
- **Comentarii de documentație** puse înapoi deasupra funcțiilor lor (`sync_text_options`,
  `sync_shown`, `access::end`, `queue_write`).
