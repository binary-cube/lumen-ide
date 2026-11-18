# 45 — Matrix cu ploaia pornită, tema Cyberpunk și efectele ei (planul 45)

Cerut (2026-11-02, aprobat „go”):

1. **Tema Matrix vine cu ploaia Matrix pornită implicit** (efectul din planul 42: Katakana +
   cifre), discretă — în spatele panourilor, opacitate mică, 20 de cadre / s, editorul curat
   ca la orice efect. Utilizatorul o oprește / o reglează din Theme Studio sau din Setări →
   Aspect, iar alegerea lui se păstrează.
2. **O temă întunecată nouă, „Cyberpunk”**: fundal violet-negru, accente magenta și cyan,
   avertismente galben electric; contrastul verificat ca la celelalte teme (textul secundar
   ≥ 2,8 : 1), toate culorile UI / sintaxă / terminal / iconițe definite. Id `cyberpunk`.
3. **Efecte noi** (aceeași structură ca cele 21: culori, densitate, viteză, mărime,
   opacitate, zonă, „prin panouri” unde are sens; iconițe în setul aplicației; categoria din
   galerie; ro / en):
   - **Glitch**: rar (la fiecare N s, reglabil), o bandă scurtă de 0,2–0,3 s de pixeli
     deplasați / bare colorate despicate RGB, niciodată peste editor (zonele: marginile /
     panourile, ca la ecranul retro), nimic desenat între glitch-uri.
   - **Grila neon synthwave**: grila în perspectivă care curge spre tine jos, în spatele
     panourilor, cu linia orizontului strălucitoare (opțional soarele); ieftină.
   - **Ploaie neon**: dâre fine magenta / cyan cu mici reflexe / stropi jos.
   - **Semne holografice**: câteva semne neon slabe (glife ca niște kanji / semne abstracte)
     în colțuri, care pâlpâie din când în când.
   - **Presetarea „Cyberpunk”**: margine neon + grilă + ploaie neon + glitch rar; tema
     Cyberpunk le are ca decorațiuni implicite (pornite, discrete — ca la Matrix).
4. Bugetele de la 42: < 200 µs pe cadru (mediana mult sub), 0 alocări pe cadru, < 1 MB ținut,
   procentul redesenat măsurat, CPU în aplicație ca la celelalte (~1–5 % dintr-un nucleu la
   20 de cadre / s). Teste de măsură în release ca la 42f.

## Cum vine ploaia cu tema Matrix (și cum câștigă alegerea utilizatorului)

Mecanismul există (41d / 41e): o temă își declară straturile (`[[decorations.layers]]`) și
setările ei (`[[options]]`); un strat poate depinde de o setare (`when = "rain"`) și un câmp
poate urma una (`density = "@rain_density"`). Valorile alese de utilizator se țin în
`[appearance] theme_options = ["matrix/rain=false", …]` — **pe temă**, deci:

- Tema Matrix declară un strat `matrix` (Katakana + cifre, verdele temei `$green`, în spatele
  panourilor cu sticla mică — `glass` 0,35 —, opacitatea 0,2, densitatea legată de setarea
  ei), arătat cu `when = "rain"`; setările temei: **„Ploaie digitală”** (comutator, pornit) și
  **„Densitatea ploii”** (glisor 10–80 %). Apar în Setări → Aspect („Matrix: setările ei”) și
  în studio (Opțiunile temei; stratul la Efecte, editabil, salvat ca temă proprie).
- **Alegerea utilizatorului câștigă**: „rain=false” salvat pentru `matrix` ascunde stratul de
  fiecare dată când tema e folosită; densitatea aleasă la fel. Implicitul temei se aplică doar
  cât utilizatorul n-a ales nimic. Toate decorațiunile oprite (`decorations = off`) sau
  înlocuite de ale lui (`my_decorations_with = replace`) — ca la orice temă.
- **Schimbată tema, ploaia pleacă**: stratul e al temei Matrix; altă temă nu-l are. Excepția,
  clară: dacă utilizatorul a pus el ploaia **peste orice temă** (Setări → Aspect →
  Decorațiuni: presetarea nouă „Matrix”, sau straturile salvate din studio cu „Folosește-le
  cu orice temă”), ea rămâne la orice temă — e suprapunerea lui, nu a temei.
- Presetare nouă **„Matrix”** (ploaia verde), ca să poată fi pusă ușor peste orice temă (și
  fiindcă Cyberpunk nu mai are Matrix: presetările, între ele, trebuie să arate fiecare fel).

## Tema Cyberpunk

`themes/cyberpunk.toml`, după structura temei Matrix (toate cheile UI, terminalul ANSI,
iconițele `icon.*`):

| Rol | Culoare | Contrast pe editor (`#100a1c`) |
|---|---|---|
| fundalul ferestrei / editor / panouri | `#0b0614` / `#100a1c` / `#150d24` | — |
| text / cod | `#e8dcff` / `#e4d8ff` | 14,4 : 1 |
| text secundar | `#8a74b8` | 4,9 : 1 (≥ 2,8 cerut) |
| accent (magenta) | `#ff2bd6` | 6,1 : 1 |
| cyan | `#00e5ff` | 12,6 : 1 |
| avertisment (galben electric) | `#fcee0a` | 16,1 : 1 |
| eroare | `#ff3b5c` | 5,6 : 1 |
| comentarii | `#6b5a8e` | 3,2 : 1 (≥ 2,2 cerut) |

Cursorul: o bară cyan cu halo, care alunecă, cu clipire clasică (una care se stinge treptat
cere cadre tot timpul: ~3,8 % dintr-un nucleu în plus, măsurat — de aceea nu, vezi mai jos).
Sintaxa: cuvintele-cheie magenta îngroșate, șirurile cyan, funcțiile galbene, tipurile
violet, numerele portocalii, variabilele PHP cyan deschis, proprietățile albastre.
Decorațiunile ei implicite (pornite, discrete, fiecare cu comutatorul ei în setările temei):
marginea neon (magenta–cyan, slabă), grila neon jos în spatele panourilor; din 45b ploaia
neon pe margini și glitch-ul rar; semnele holografice declarate, oprite implicit.

## Efectele noi

- **Glitch** (`glitch`, la „Ecran” în galerie): o lovitură rară — perioada din densitate,
  4–30 s —, 0,25 s: 2–5 benzi orizontale de pixeli „deplasați” (quad-uri subțiri în culoarea
  benzii, mutate stânga / dreapta cu câțiva pixeli, pâlpâind) despicate RGB (aceeași bandă în
  roșu și cyan, decalate), opțional („blocuri”) dreptunghiuri scurte în loc de benzi întregi;
  între lovituri nimic desenat (nicio redesenare). Zonele: doar marginile / panourile
  (`everywhere` refuzat, citit ca „panouri”, ca la ecranul retro). Opțiuni: stil (benzi /
  blocuri), culori (1–3), densitate (cât de des), mărime (cât de groase / câte), opacitate
  (≤ 0,5), zonă, sticlă.
- **Grila neon synthwave**: grila există din 42e (`synthwave`: evantaiul de linii o imagine la
  o treime din rezoluție, liniile orizontale quad-uri, soarele). **Decizie:** nu fac un al
  doilea fel aproape identic (galeria ar avea două grile); grila primește ce cere planul:
  **linia orizontului strălucitoare** (un halou din câteva linii mai late și mai slabe, ca
  la marginea neon, după `glow`), liniile care vin spre tine **mutate cu pixeli întregi**, un
  stil nou **„oraș”** (siluetele unor clădiri cu ferestre aprinse pe orizont, o imagine mică,
  rasterizată o dată) pe lângă „soare” și „doar grila”. Presetarea Cyberpunk o folosește fără
  soare, în cyan și magenta.
- **Ploaie neon** (`neon-rain`, „Fundal”): dâre fine (quad-uri de 1 px fizic, cu un cap mai
  luminos) în culorile ei (magenta / cyan), căzând ușor oblic; jos fiecare picătură lasă un
  strop scurt — două-trei puncte care sar și se sting — și un reflex (o linie scurtă care se
  lățește și se stinge). Opțiuni: culori (1–4), densitate, viteză, mărime (lungimea dârelor),
  strălucire (capul și reflexele), opacitate, zonă, sticlă.
- **Semne holografice** (`holo-signs`, „Colțuri”): 1–4 semne verticale (2–3 glife ca niște
  kanji / semne abstracte, din setul aplicației ca `fx-h-…`, ca glifele Matrix) în colțuri
  (sus / jos după densitate; în spatele panourilor laterale, nu sub butoanele benzilor), fiecare cu rama lui de neon (quad-uri subțiri) și un halou moale
  (o imagine mică întinsă); stau aprinse slab și pâlpâie din când în când (o pâlpâire scurtă
  la câteva secunde, nu continuă). Opțiuni: culori (1–4), densitate (câte), viteză (cât de des
  pâlpâie), mărime, strălucire, opacitate, zonă, sticlă.
- **Presetarea Cyberpunk** (înlocuiește Matrix-ul de dinainte): margine neon magenta–cyan +
  grila (stilul „doar grila”, cyan, în spatele panourilor) + ploaia neon + glitch rar +
  semnele holografice (slabe; sunt cel mai „cyberpunk” dintre efectele noi și presetările
  trebuie să arate, între ele, fiecare fel).

## Pașii

- **45a** — ploaia Matrix implicită (stratul și setările temei Matrix), presetarea „Matrix”,
  tema Cyberpunk (paleta, contrastul, decorațiunile ei cu felurile existente: neon, grila).
  Teste: Matrix are ploaia la implicite, „rain=false” salvat o ascunde, densitatea aleasă o
  urmează, altă temă n-o are, suprapunerea utilizatorului o păstrează la orice temă;
  Cyberpunk trece testele de contrast și de compunere în studio. Capturi: `theme-cyberpunk`,
  `theme-matrix`.
- **45b** — glitch, ploaia neon, semnele holografice, grila cu orizontul strălucitor și
  stilul „oraș”, presetarea Cyberpunk refăcută, decorațiunile temei Cyberpunk completate;
  iconițele noi; măsurătorile (release). Capturi: `effect-glitch` (un cadru în mijlocul unui
  glitch), `effect-synthwave-grid`, `effect-neon-rain`, `effect-holo-signs`,
  `decorations-cyberpunk`, `studio-gallery`.

## Stare

- **45a făcut.** `themes/matrix.toml`: setările temei „Ploaie digitală” (comutator, pornit)
  și „Densitatea ploii” (glisor 10–80 %, implicit 40 %), stratul `matrix` cu `when = "rain"`
  și `density = "@density"` (Katakana + cifre, `$green`, opacitatea 0,2, în spatele
  panourilor cu sticla 0,35 — panourile rămân ~93 % opace, editorul opac). Presetarea nouă
  „Matrix” (aceeași ploaie, pentru orice temă; varianta luminoasă un verde mai închis).
  `themes/cyberpunk.toml` (paleta din tabelul de mai sus, cursorul cyan cu halo, toate
  cheile UI / terminal / `icon.*`), înregistrată după Matrix; decorațiunile ei: marginea
  neon (stilul „hue”, magenta → cyan, opacitatea 0,4) și grila (`synthwave` „doar grila”, în
  cyan, în spatele panourilor, 0,2), fiecare cu comutatorul ei („Margine neon”, „Grilă
  neon”). Teste: Matrix plouă la implicite (Katakana, în spatele panourilor, slab, în
  verdele temei), „rain=false” o ascunde, densitatea aleasă o urmează, nicio altă temă n-o
  are; în aplicație, comutatorul din Setări → Aspect salvează `matrix/rain=false` doar pentru
  Matrix și tema rezolvată după aceea n-are ploaie (pornită la loc: o are); ploaia pusă de
  utilizator peste orice temă (presetarea „Matrix”) rămâne la orice temă, adăugată peste a
  temei Matrix sau în locul ei; Cyberpunk: fundalul violet-negru, accentul magenta,
  avertismentul galben, șirurile cyan, decorațiunile slabe și comutatoarele lor; cele două
  teme trec testele de contrast, de compunere în studio (decorațiunile și setările păstrate
  exact) și „culorile UI neschimbate la compunere”. Capturi noi: `theme-matrix`,
  `theme-cyberpunk` (în ambele rulări, temele fiind întunecate).
- **45b făcut.** Felurile noi în domeniu (`LayerKind::Glitch` `glitch`, `NeonRain`
  `neon-rain`, `HoloSigns` `holo-signs`; câmpurile în `effect::fields`, deci citite din teme,
  scrise de studio și codate în setări ca celelalte; `LayerKind::never_behind_editor` — ecranul
  retro și glitch-ul: „everywhere” refuzat la scriere și citit ca „panouri”) și desenate în
  `fx.rs`:
  - **Glitch**: perioada 30 − 26 × densitatea (4–30 s), 0,25 s (`GLITCH_FLASH`), unul la
    jumătate în cadrul static; 2–5 benzi (după mărime) pe toată lățimea sau, stilul
    „blocuri”, câte trei bucăți scurte, fiecare în prima culoare cu copia ei în a doua
    decalată câțiva pixeli (culorile despicate) și uneori o linie subțire luminoasă; sar de 30
    de ori pe secundă și pâlpâie; între glitch-uri nimic (0 % redesenat). Galerie: „Ecran”,
    iconița `glitch`.
  - **Ploaie neon**: 20–180 de dâre (după densitate și mărimea ferestrei, `MAX_STREAKS`),
    fiecare trei quad-uri de 1 px fizic tot mai luminoase spre cap, capul alb-colorat, un halou
    îngust după strălucire; jos stropul (0,35 s): reflexul — o linie care se lățește și se
    stinge — și trei picături care sar și cad înapoi. Fără imagini. „Fundal”, `cloud-rain`.
  - **Semne holografice**: 1–4 (după densitate) în colțuri, în spatele panourilor laterale (nu
    sub butoanele benzilor de unelte), 2–3 glife `fx-h-0…11` (12 semne ca niște kanji, desenate
    ca iconițe ale setului aplicației, ca glifele Matrix: un singur atlas), rama de neon (un
    quad rotunjit cu margine) peste un geam slab, haloul (o imagine moale de 32 px pe culoare,
    întinsă); stau aprinse, iar la câteva secunde (după viteză) pâlpâie 0,45 s, glifele
    deplasate un pixel — o hologramă care își pierde semnalul. „Colțuri”, `holo-sign`.
    Zona implicită: în spatele panourilor.
  - **Grila synthwave**: câmpul nou `glow` — linia orizontului mai luminoasă și trei benzi
    mai largi și mai slabe în jurul ei (nemișcate: nu se redesenează) —, liniile care vin
    spre tine la pixeli întregi (test), stilul nou „oraș”: siluetele clădirilor (o imagine la o
    treime din rezoluție, rasterizată o dată: clădiri în a doua culoare întunecată, ferestre
    aprinse în prima, antene) pe orizont, în locul soarelui.
  - **Presetarea Cyberpunk** refăcută: neon magenta → cyan (slab), grila „doar grila” în
    spatele panourilor, ploaia neon, glitch rar (densitate 0,2: la ~25 s), semnele (slabe);
    Matrix a trecut în presetarea „Matrix” (45a). Tema Cyberpunk primește ploaia neon și
    glitch-ul pornite, semnele declarate oprite (comutatoarele „Ploaie neon”, „Glitch rar”,
    „Semne holografice”).
  - Capturile acceptă acum și stilul unui efect: `MY_DECORATIONS=synthwave:city@panels`.
  Bugetele (release, Apple Silicon, `cargo test -p theme-decorations --release budget`), un
  cadru animat median / maxim la 1200 × 700 @2 și 2560 × 1440 @2:

  | Efect | 1200 × 700 | 2560 × 1440 | Redesenat pe cadru (20 / s) |
  |---|---|---|---|
  | glitch | 0,04 µs (în glitch ≤ 0,8 µs) | 0,04 µs | 0 % între glitch-uri |
  | ploaie neon | 3,3 µs | 5,2 µs | 7,2 % margini / 11,9 % prin panouri |
  | semne holografice | 0,08 µs | 0,08 µs | 0 % cât nu pâlpâie |
  | grila (cu orizontul strălucitor) | 0,13 µs | 0,13 µs | 1,5 % |
  | presetarea Cyberpunk | 2,9 µs | 5,3 µs | — |
  | tema Matrix (ploaia ei) | 4,4 µs | 7,0 µs | — |

  0 alocări pe cadru (400 de cadre, scara 1 și 2, pentru fiecare fel nou, pe margini și prin
  panouri), sub 1 MB ținut înainte de încărcarea pe GPU (testele de la 41 / 42 iau automat
  felurile noi).

  În aplicație (release, macOS, HOME izolat, fereastra în față, 20 de cadre / s, 12 s
  animat; scriptul în afara depozitului), procent dintr-un nucleu:

  | Ce | CPU | RSS |
  |---|---|---|
  | niciun efect (`default-dark`) | 0,3 % | 122 MB |
  | zăpadă (referința de la 41) | 4,4 % | 122 MB |
  | glitch (prin panouri) | 0,6 % | 121 MB |
  | ploaie neon (prin panouri) | 2,9 % | 127 MB |
  | semne holografice (prin panouri) | 0,55 % | 127 MB |
  | grila cu orașul (prin panouri) | 5,0 % | 124 MB |
  | Matrix, efectul singur (prin panouri) | 4,8 % | 124 MB |
  | presetarea Cyberpunk peste `default-dark` | 4,3 % | 123 MB |
  | tema Cyberpunk (cu decorațiunile ei) | 4,5 % | 123 MB |
  | tema Matrix, cu ploaia / fără ploaie | 7,1–7,4 % / 4,1 % | 123–127 MB |

  Tema Matrix costă mai mult decât ploaia singură fiindcă **cursorul ei** (de la planul 35:
  clipire care se stinge treptat, halou, urmă) cere cadre tot timpul — ~4 % fără nicio
  decorațiune; ploaia adaugă ~3 %, cât orice efect animat. Nu l-am schimbat (e aspectul temei,
  dinainte); tema Cyberpunk are de aceea clipirea clasică (cu ea: 4,5 % cu tot cu efecte, față
  de 6,9 % cu clipirea treptată). Glitch-ul și semnele costă aproape nimic: între glitch-uri /
  pâlpâiri cadrele nu schimbă nimic și rendererul nu le redesenează.

  Capturi (`scripts/ui-snapshots.sh`, comparate cu referința): noi `theme-matrix`,
  `theme-cyberpunk`, `effect-glitch` (un cadru în mijlocul unui glitch), `effect-synthwave-grid`
  (`synthwave:city@panels`: orizontul strălucitor și orașul), `effect-neon-rain`,
  `effect-holo-signs`, `studio-gallery-effects` (galeria derulată la capăt — comanda nouă
  `gallery:end` a studioului —, unde e glitch-ul, la „Ecran”); se schimbă, cum era de așteptat,
  `decorations-cyberpunk` (presetarea refăcută), `effect-synthwave` și `decorations-synthwave`
  (orizontul strălucitor), `studio` (galeria temelor de pornire are Cyberpunk),
  `settings-theme-options` (un cartonaș de temă în plus în listă), `studio-matrix-live` (tema
  Matrix are acum setări proprii, deci studioul arată și categoria „Opțiunile temei”: cu un
  tab în plus, taburile nu mai încap și doar cel ales își arată numele). Celelalte diferențe față de
  referință nu sunt ale planului 45: `settings-query` (setările reorganizate în 44, referința e
  de dinainte), `console*`, `sql-file-attached`, `services-output` / `-result` (bara
  instrucțiunii curente, 4acdb264), `task-manager*` (cifra memoriei, vie). **Cele trei capturi
  întunecate care nu se randau** (`sql-file`, `sql-file-attached`, `services-output`) se
  randează acum, în ambele rulări: eșecul fusese al compilării paralele a altui agent (binarul
  înlocuit în timpul rulării), nu al codului.

### Ce nu s-a făcut / abateri

- Grila neon nu e un fel nou: grila synthwave exista (42e), a primit orizontul strălucitor,
  liniile la pixeli întregi și stilul „oraș” (vezi decizia de mai sus).
- Semnele holografice stau în spatele panourilor (zona lor implicită); peste editor nu se văd
  (colțurile din dreapta, când panoul din dreapta e închis, sunt sub editor), iar prin panouri
  se văd slab: contrastul textului plafonează transparența cardurilor (regula de la 42d).
- Glitch-ul nu poate „muta pixeli” cu adevărat (decorațiunile sunt sub conținut și nu citesc
  imaginea ferestrei): benzile lui sunt quad-uri colorate, decalate, care imită pixeli deplasați.
- Varianta luminoasă a efectelor noi folosește culorile lor implicite (presetarea Cyberpunk
  are culori mai închise pentru temele luminoase).
