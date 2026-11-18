# 34 — Grupuri de proiecte în ecranul de start

Lista „Recent projects” (ecranul gol și fereastra Projects din bara de sus) se poate
organiza în foldere-grup:
- imbricate;
- expandate și restrânse;
- cu proiecte mutate prin drag & drop.

Un proiect se poate scoate din listă și, opțional, șterge de pe disc.

## Modelul (domeniul `workspace`, testat)

- `RecentProjects` (în `recent.rs`) ține:
  - **proiectele:** calea și grupul în care e (`None` înseamnă la rădăcină);
  - **grupurile:** id UUID v4, nume, părinte (`None` înseamnă la rădăcină), restrâns sau
    nu, ordinea între frați.
- Operații pure, fiecare cu teste:
  - `open(path)`: urcă proiectul în capul listei, fără să-i schimbe grupul;
  - `move(path, group)` și `move_group(group, parent)` (fără cicluri: un grup nu intră în
    propriul descendent);
  - `new_group(name, parent)`, `rename_group`, `toggle(group)`;
  - `delete_group(group)`: proiectele și subgrupurile lui urcă la părinte, nu dispar;
  - `forget(path)`: scos din listă.
- Limita de 20 se aplică doar proiectelor din afara grupurilor. Unul pus într-un grup
  rămâne până îl scoți.
- **Stocarea:** `recent.toml` păstrează `projects = [...]`, ca fișierele vechi să meargă,
  și adaugă `[[groups]]` și `[placed]` (cale → grup). Un fișier fără ele înseamnă totul
  la rădăcină.

## Interfața (ecranul `projects_picker`, al app-ului)

- **Grupurile:**
  - sunt rânduri cu săgeată (▸ / ▾), iconiță de folder, nume și numărul de proiecte;
  - copiii sunt indentați pe nivel;
  - un click pe săgeată sau pe rând restrânge sau expandează, iar starea se salvează;
  - pe tastatură, ← și → restrâng și expandează, iar ↵ comută.
- **„Grup nou”** e un buton lângă titlul „Recent projects”: un câmp de nume pe loc, cu
  Enter sau Esc.
- **Click dreapta pe un proiect:**
  - Deschide;
  - Mută în grup ▸ (lista grupurilor, plus „Fără grup”);
  - Scoate din listă;
  - Șterge…
- **Click dreapta pe un grup:**
  - Redenumește;
  - Grup nou în el;
  - Șterge grupul (proiectele rămân, urcă un nivel).
- **Drag & drop:**
  - un proiect sau un grup tras peste un grup intră în el, iar peste titlul listei ajunge
    la rădăcină;
  - ținta e evidențiată;
  - un grup nu poate fi tras în el însuși.
- **Filtrul:** cât timp scrii, lista e plată, doar cu proiectele care se potrivesc, iar
  calea grupului apare estompată (`Clienți / Northwind`).
- **„Șterge…”** deschide un dialog: „Scoate *nume* din listă?”.
  - Căsuța „Șterge și folderul de pe disc” e nebifată implicit.
  - Bifată, dialogul trece în stil de pericol, cu avertisment, iar butonul devine
    „Șterge de pe disc”. Folderul merge în coșul sistemului (Trash / Recycle Bin), deci
    se poate recupera.
  - Un proiect deschis acum nu poate fi șters de pe disc (butonul e dezactivat, cu
    motivul).
- Modalul nu se închide la click în afară. Click-urile pe rândurile ascunse de scroll nu
  ajung la ele.
- Toate textele sunt în i18n (ro și en): grupurile, meniurile, dialogul, avertismentul.

## Teste

- **Domeniu:**
  - mutări;
  - cicluri refuzate;
  - ștergerea unui grup (copiii urcă);
  - limita doar pentru proiectele fără grup;
  - TOML vechi citit;
  - TOML nou păstrat întreg.
- **App:**
  - rândurile vizibile după restrângere;
  - drag & drop pe un grup;
  - dialogul de ștergere (nebifat: doar din listă; bifat: în coș, testat pe un folder
    temporar).
- **Capturi noi:**
  - lista cu grupuri;
  - meniul unui proiect;
  - dialogul de ștergere cu căsuța bifată.

## Ordinea

34a modelul și stocarea → 34b afișarea grupurilor (expandare, restrângere, filtru) →
34c meniurile, grupul nou, redenumirea, ștergerea → 34d drag & drop. Un commit la final
(sau pe etape), după `check.sh` și capturi.

## Stare

- **34a, gata.** `workspace::recent::RecentProjects` (grupuri cu `GroupId` UUID v4, `open`,
  `move_to`, `move_group` fără cicluri, `new_group`, `rename_group`, `set_collapsed`,
  `delete_group`, `forget`, `repaired` pentru un fișier stricat) și `recent::update`
  (citește din nou, apoi salvează). `TomlRecentStore` scrie `[[groups]]` și `[placed]`;
  fișierele vechi merg.
- **34b, gata.** Grupurile apar înaintea proiectelor fără grup, indentate, cu săgeată,
  folder și număr; ← → le restrâng și le expandează, ↵ și click-ul le comută. Filtrul dă
  o listă plată, cu calea grupului.
- **34c, gata.**
  - Click dreapta pe proiect: Deschide, Mută în grup ▸ (Fără grup, grupurile, Grup nou…),
    Scoate din listă, Șterge…
  - Pe grup: Redenumește…, Grup nou în el…, Șterge grupul. Pe titlu: Grup nou.
  - Numele se cere într-un dialog cu câmp (nu pe loc, ca la redenumirea fișierelor).
  - Dialogul are acum o căsuță opțională (`DialogCheck`), nebifată la deschidere. Bifată:
    ton de pericol, avertisment și butonul „Șterge de pe disc”. Folderul merge în coș pe
    un fir separat (`trash-project`). Un proiect deschis în orice fereastră nu are căsuța.
- **34d, gata.** Un click pe proiect sau grup se decide la eliberare: click sau drag.
  Ținta e evidențiată, iar titlul listei înseamnă „sus”. Un grup nu intră în propriul
  subgrup.
- **Teste:**
  - domeniu și stocare;
  - lista (arbore, restrângere, drag, filtru, ținte);
  - căsuța din dialog;
  - fluxul prin workbench, pe un fișier temporar per fir (testele nu ating lista ta).
  - Mutarea în coș nu e testată automat, ca să nu umple coșul real.
- **Capturi noi:** `projects-groups`, `projects-menu`, `dialog-delete`.

## 34e — derulare la drag și ordinea aleasă (cerut după 34d)

- **Derulare în timpul drag-ului:**
  - cu mouse-ul aproape de marginea de sus sau de jos a listei (~40 px), lista se derulează
    singură și se oprește când mouse-ul iese din zonă;
  - viteza crește cu cât mouse-ul e mai aproape de margine;
  - ținta de drop se recalculează după fiecare pas de derulare;
  - merge și când mouse-ul stă pe loc (cadre cât timp e în zonă, apoi nimic).
- **Ordinea în grupuri, aleasă de tine:**
  - **Proiect peste un proiect:**
    - jumătatea de sus îl pune înainte, jumătatea de jos după, în grupul acelui proiect;
    - o linie de inserție arată locul.
  - **Proiect peste un grup:** intră la sfârșitul grupului.
  - **Grup peste un grup:**
    - marginea de sus îl pune înainte, ca frate;
    - marginea de jos îl pune după;
    - mijlocul îl bagă înăuntru, la sfârșit.
  - Deschiderea unui proiect dintr-un grup nu-i mai schimbă locul (acum îl urcă primul).
  - Proiectele fără grup rămân „recente”: cel deschis urcă primul, ca până acum.
- **Modelul:**
  - `move_to(path, group, before)` și `move_group(id, parent, before)`, cu `before`
    `None` pentru sfârșit;
  - ordinea e cea din fișier, fără câmpuri noi în `recent.toml`.
- **Teste:**
  - inserarea înainte și după;
  - sfârșitul grupului;
  - deschiderea care nu mută;
  - ținta după poziția mouse-ului în rând;
  - derularea la margine.
- **Captură:** linia de inserție în timpul unui drag.

### 34e, ajustat după discuție

- **Folderul „Recente”:**
  - proiectele fără grup stau și ele într-un folder;
  - se poate muta (ordinea între grupuri, chiar și în alt grup) și restrânge;
  - nu se poate șterge sau redenumi și nu primește subgrupuri;
  - limita de 20 se aplică doar lui.
- **Ordinea peste tot, implicit:** cele deschise recent stau primele, în „Recente” și în
  fiecare grup.
  - Un drop la o poziție anume (linia de inserție) trece grupul pe ordine manuală, cu
    proiectul exact acolo.
  - Un drop pe rândul grupului pune proiectul la sfârșit (în ordinea manuală) sau după
    ordinea de deschidere.
  - Din meniul grupului, „Sortează după deschidere” revine la ordinea automată.
  - „Recente” e mereu după deschidere.
- **Liniile de ghidaj:** linii verticale subțiri, ca în arborele de fișiere, de la un grup
  deschis până la ultimul lui rând.
- **Stocarea:** `[[groups]]` primește `order = "manual"` și `projects = [...]` (ordinea
  aleasă). „Recente” are id-ul zero. Un fișier vechi primește „Recente” primul.

### Stare 34e

- **Gata.**
  - Folderul „Recente” (`RECENT`, id zero) există mereu. Se mută și se restrânge; nu se
    șterge, nu se redenumește și nu primește subgrupuri.
  - `Order::Recent` e implicit. Un drop la o poziție trece grupul pe `Order::Manual`, iar
    meniul grupului are „Sortează după deschidere”.
  - Drop-ul unui proiect:
    - jumătatea de sus a unui proiect îl pune înainte, cea de jos după;
    - pe rândul grupului, intră înăuntru.
  - Drop-ul unui grup: sfertul de sus îl pune înainte; mijlocul îl bagă înăuntru, iar
    sfertul de jos al unui grup restrâns îl pune după.
  - Lista se derulează singură lângă margini (`EDGE` 44 px, până la 900 px/s), cu cadre
    doar cât timp mouse-ul e în zonă.
  - Liniile de ghidaj sunt discrete (text cu alfa 0,12). Merg de sub săgeata grupului
    până la mijlocul ultimului copil (un cot), cu o ramură scurtă spre fiecare rând.
    Indentarea e de 28 px pe nivel, deci liniile nu ating fundalul de hover sau selecție.
  - Titlul arborelui e acum „Toate proiectele”.
- **Captură nouă:** `projects-drag` (linia de inserție).
