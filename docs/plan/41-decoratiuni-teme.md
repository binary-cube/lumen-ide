# 41 — Decorațiunile temelor și tema de Crăciun (planul 41)

Cerut (2026-11-01): „O temă de Crăciun (întunecată și luminoasă): o ghirlandă subtilă și
elegantă pe marginile ferestrei, cu globuri mici și luminițe care sclipesc; fulgi de zăpadă
foarte discreți în fundal — în spatele conținutului, niciodată peste lizibilitatea textului.
Cu gust, nu kitsch.” Și, apăsat: **decorațiunile trebuie să fie SUPER optime** — bugete
măsurate și verificate în teste release.

## Context: ce avem acum

- Tema e date (planul 07): `[palette]`, `[ui]`, `[syntax]` și, de la planul 35, `[caret]` —
  primul lucru dintr-o temă care nu e o culoare (forma, clipirea, halo-ul, urma), cu
  moștenire pe `extends` (`CaretSpec::over`) și editat în Theme Studio.
- Fereastra e „insule”: fundalul ferestrei (`window.background`, `$base`) se vede în bara de
  sus, în benzile de unelte din stânga / dreapta, în bara de stare și în golurile dintre
  carduri; editorul, panourile laterale și fereastra de jos sunt carduri opace, pictate
  peste el.
- Randarea: scena (`render_api::Scene`) e refăcută la fiecare cadru; rendererele desenează
  doar quad-uri, glife și imagini (o imagine = un draw call); `progress_ring` arată cum se
  rasterizează pe CPU o formă mică la rezoluția ecranului, o singură dată, ținută cât nu se
  schimbă. Aplicația cere cadre doar la nevoie (caret, toast-uri, hover): în repaus 0 % CPU.
- Nu există nimic despre „reduce motion” sau economisirea bateriei în platformă.

## Principii

1. **Un punct de extensie generic**, nu un caz special de Crăciun: tema declară *straturi*
   de decorațiuni (`[decorations.<fel>]`); un modul (`theme-decorations`,
   `crates/modules/appearance/decorations`) le desenează; aplicația doar le găsește printre
   contribuții, le dă geometria ferestrei și le pune în scenă.
2. **Nimic încărcat fără decorațiuni.** Tema fără `[decorations]` ⇒ niciun obiect creat,
   niciun sprite, niciun cadru în plus. Schimbată tema ⇒ desenatorul (cu sprite-urile lui)
   e eliberat.
3. **În spatele conținutului.** Stratul decorațiunilor se pune imediat după fundalul
   ferestrei (și nuanța proiectului), înaintea benzilor și a cardurilor: tot ce e
   deasupra (cardurile opace, textul barei de sus / de stare) acoperă decorațiunile. Fulgii
   nu trec niciodată peste textul din editor — cardul editorului e opac peste ei.
4. **Performanța e cerința de vârf** (bugetele de mai jos, testate).
5. **Accesibilitatea:** setarea „Decorațiuni: animate / statice / oprite”, „reduce motion”
   al sistemului, economisirea bateriei ⇒ cadru static.

## Formatul în fișierul temei (extinde planul 07)

```toml
[decorations.garland]          # ghirlanda pe margini
edges     = "top-sides"        # "top" | "top-sides"
lights    = 0.6                # densitatea luminițelor, 0..1
twinkle   = 0.4                # viteza sclipirii, 0..1 (0: lumină constantă)
glow      = 0.6                # halo-ul constant al becurilor, 0..1 (mai mic pe fundal deschis)
colors    = ["$gold", "$red", "#f3e6c4"]   # culorile luminițelor (1..6)
ornaments = ["$red", "$gold"]  # globurile (0..4 culori; [] = fără globuri)
wire      = "$pine"            # culoarea crenguței
drop      = 0.45               # cât coboară pe laturi (fracție din înălțime, 0..0.8)
opacity   = 0.9                # 0..1

[decorations.snow]             # fulgii din fundal
density = 0.35                 # 0..1
speed   = 0.3                  # 0..1
color   = "#ffffff"
opacity = 0.16                 # 0..0.5 (plafonat: lizibilitatea)

[decorations.corners]          # ornamentele din colțurile de sus
style   = "bow"                # "bow" | "holly" | "star"
colors  = ["$red", "$pine"]    # culoarea principală, accentul (1..2)
opacity = 0.95
```

- Toate cheile sunt opționale (valorile implicite ale fiecărui fel); culorile acceptă
  `$nume` din `[palette]`, ca peste tot.
- **Validare:** chei / feluri necunoscute, valori în afara intervalului, culori invalide,
  liste prea lungi ⇒ eroare de citire a temei cu cheia și valoarea (ca la `[caret]`); tema
  nu poate „sparge” UI-ul (opacitatea zăpezii plafonată la 0,5).
- **Moștenire:** fiecare fel e luat din tema care îl spune, apoi din cea pe care o extinde.
- Temele vechi nu se schimbă: fără secțiune, fără decorațiuni.

## Modelul (straturile)

- `theming::decorations` (domeniu, pur): `Decorations { garland, snow, corners }` cu
  `GarlandSpec`, `SnowSpec`, `CornerSpec` (date complete, limitate), `DecorationsSpec`
  (ce spune o temă, opțional pe fel) cu `over` și `resolve`; `ResolvedTheme::decorations()`.
- `ui-components::decorations` (pur, fără desen): **layout-ul** (ghirlanda în ghirlande
  egale — „swag”-uri — pe lățimea de sus după `titlebar_inset`, laturile până la `drop`,
  luminițele pe fir, globurile la mijlocul ghirlandelor ocolind conținutul barei de sus,
  colțurile, semințele fulgilor), **animația** (sclipirea fiecărei luminițe, poziția
  fiecărui fulg la timpul `t`, deterministe), **ceasul cadrelor** (`DecorationClock`: ≤ 15
  cadre / s, doar cât fereastra e vizibilă și focalizată, oprit după 20 s fără input, timpul
  înghețat — fără salt — și reluat la input) și **contractul**: `DecorationsProvider`
  (contribuit de modul: din `Decorations` face un `WindowDecorations` pe fereastră) și
  `WindowDecorations` (`paint(frame, layer)` fără alocări, `animated()`, `memory()`).
- Modulul `theme-decorations`: desenele (rasterizate pe CPU o singură dată, la scara
  ecranului, cu acoperire anti-aliasată ca `progress_ring`): o ghirlandă (crenguță de brad
  cu ace, becurile cu halo-ul lor, totul într-o singură imagine refolosită pentru fiecare
  „swag”), segmentul de latură, globurile (câte unul pe culoare, cu luciu și cârlig),
  sclipirea (un halo pe culoare), fulgii (trei mărimi), colțurile (fundă / ilice / stea);
  setarea lui și blocul din Setări → Aspect.
- Aplicația: `LanguageSupport::decorations`; `Workbench` ține desenatorul (creat la primul
  cadru al unei teme cu decorațiuni, eliberat altfel), ceasul și indexul stratului în scenă.

## Performanță și resurse (bugete, testate în release)

| Ce | Buget | Cum |
|---|---|---|
| CPU pe cadru animat (decorațiunile) | ≤ 0,2 ms | layout-ul memoizat pe geometrie; pe cadru doar sin-uri și împingeri de instanțe |
| Alocări pe cadru | 0 | `Vec`-urile scenei păstrate, imaginile `Arc` clonate; test cu alocator numărător |
| RAM total (sprite-uri incluse) | ≤ ~1 MB | sprite-uri mici, `ImageData::gpu_only` (pixelii eliberați după upload); test |
| GPU | câteva zeci de imagini mici | un sprite pe „swag” (nu pe bec), sclipirile doar ale becurilor aprinse acum, fulgii ascunși de carduri nedesenați |
| Rată | ≤ 15 cadre / s | `request_redraw_at`, nu cadre continue |
| Repaus | 0 cadre, 0 CPU | după 20 s fără input: cadrul static; testat că nu se mai cere niciun cadru |
| Ascuns / minimizat / nefocalizat | 0 cadre | ceasul nu mai cere cadre; la revenire, reluat |
| Cadru doar al decorațiunilor | fără repictarea workbench-ului | dacă de la ultimul cadru n-a venit niciun eveniment și niciun alt termen nu e scadent, se rescrie doar stratul decorațiunilor în scena păstrată (`Scene::layer_mut`) și se trimite la renderer |

Rendererul nu știe redesena doar o regiune (wgpu redesenează suprafața întreagă): de aceea
rata minimă și cadrul „doar decorațiunile”.

## Setări

- `[appearance] decorations = "on" | "static" | "off"` (implicit `on`), declarată în
  `crates/modules/appearance/decorations/src/settings.rs`, blocul „Decorațiuni” din Setări →
  Aspect (`SettingsContribution` a modulului). `static`: un singur cadru (fără animație);
  `off`: nimic desenat, nimic ținut.
- „Reduce motion” (macOS `accessibilityDisplayShouldReduceMotion`, Windows
  `SPI_GETCLIENTAREAANIMATION`) și economisirea bateriei (macOS Low Power Mode, Windows
  Battery Saver) ⇒ static. Citite prin `platform_api::Window` (`reduce_motion`,
  `power_saving`), cel mult o dată la câteva secunde, nu pe cadru.

## Tema de Crăciun

- `christmas-dark` și `christmas-light` (`themes/christmas-*.toml`), elegante: fundaluri
  neutre cu o umbră caldă (cărbune cu o tentă de brad / ivoriu), accentul vișiniu, sintaxa în
  tonuri de brad, auriu, vișiniu și albastru de iarnă, contrastul testat ca la celelalte.
- Ghirlanda sus și pe laturi (până la ~45 % din înălțime), luminițe calde (auriu, alb cald,
  vișiniu) care sclipesc rar și încet, globuri vișinii / aurii, funde în colțuri, fulgi
  foarte discreți (opacitate ~0,15 pe întunecat, gri-albăstrui pe luminos).

## Theme Studio

- Draftul ține și decorațiunile (`ThemeDraft::decorations`): citite din `[decorations.*]`,
  scrise la `compose` (cu culorile în hex). Secțiunea „Decorațiuni”: ghirlanda (fără / sus /
  sus și laturi), luminițele (densitate), sclipirea (viteză), zăpada (fără … multă), viteza
  zăpezii, colțurile (fără / fundă / ilice / stea), opacitatea; previzualizate live.

## Pașii

- **41a** — domeniul (`theming::decorations`), citirea și validarea în `themes-builtin`,
  modelul și contractul din `ui-components`, platforma (`reduce_motion`, `power_saving`),
  modulul `theme-decorations` cu desenele și setarea, aplicația (desenatorul pe fereastră,
  stratul, ceasul, cadrele doar ale decorațiunilor, oprirea în repaus / ascuns). Teste:
  citire, validare, moștenire, layout-ul care nu se suprapune peste conținut, animația
  deterministă, ceasul (oprit ascuns / nefocalizat / în repaus, ≤ 15 cadre / s), 0 alocări,
  bugetele CPU / RAM în release.
- **41b** — tema de Crăciun (cele două variante) și capturile `theme-christmas` (întunecată
  și luminoasă) și `theme-christmas-light`, cadru static determinist.
- **41c** — Theme Studio: editarea decorațiunilor; captura `theme-studio-decorations`.

## Ce nu facem acum

- ~~Redesenarea parțială a suprafeței~~: făcută la auditul C (planul 06, „Damage tracking”).
- ~~Alte feluri de decorațiuni (frunze de toamnă…)~~: planul 41d.

## Decorațiuni pentru orice temă și opțiunile temelor (41d–41f)

Cerut (2026-11-01, la final): „setările de decorațiuni din Theme Studio par forțate pe
Crăciun; ar trebui să fie per temă, sau măcar personalizabile liber de utilizator”. Și:
temele își pot declara propriile setări; panoul din stânga al studioului mai lizibil.

### Straturi, nu trei feluri fixe

- O temă declară o **listă de straturi** (`[[decorations.layers]]`), desenate în ordine (primul
  cel mai în spate). Fiecare strat are un **fel** (`kind`) și opțiunile lui; oricâte straturi
  de același fel (o ghirlandă sus cu fanioane și una pe laturi cu becuri…).
- Felurile, generice:
  - `garland` — șirul de pe margini: `strand` = `evergreen` (crenguța de brad de acum) |
    `wire` (sârmă subțire) | `ribbon` (panglică); `items` = `bulbs` (becuri) | `beads`
    (mărgele) | `flags` (fanioane) | `none`; `edges`, `lights` (densitatea lucrurilor de pe
    fir), `twinkle`, `glow`, `colors` (1..6), `ornaments` (0..4), `wire` (culoarea firului),
    `drop`, `opacity` — ca până acum.
  - `particles` — în fundal: `shape` = `snow` | `leaves` | `petals` | `stars` | `sparkles` |
    `confetti` | `rain`; `density`, `speed`, `colors` (1..6), `opacity` (≤ 0,5). Zăpada,
    frunzele, petalele, confetti și ploaia cad (frunzele, petalele și confetti se rotesc: 4
    poziții desenate o dată); stelele stau și sclipesc lin; scânteile apar scurt, rar.
  - `corners` — colțurile de sus: `style` = `bow` | `holly` | `star` | `flower` | `leaf` |
    `icon` (`icon = "<nume>"`: o pictogramă din setul aplicației, colorată), `colors`
    (principala, accentul), `opacity`.
  - `glow` — lumina marginilor: `style` = `edge` (o lumină colorată dinspre margini) |
    `vignette` (umbrire spre margini), `color`, `opacity` (≤ 0,6), `width`; statică (nu cere
    cadre; o pulsație ar însemna toată rama redesenată la fiecare cadru).
- **Schema câmpurilor e una singură** (`theming::decorations::LayerKind::fields`): cheia,
  felul valorii (alegere, cantitate cu maximul ei, culoare, listă de culori cu limitele ei,
  text) — din ea citesc temele (`themes-builtin`), scrie studioul (`compose`), se codează
  suprapunerea utilizatorului în setări și se desenează rândurile editorului din studio.
- **Compatibilitate:** `[decorations.garland]`, `[decorations.snow]`, `[decorations.corners]`
  (formatul 41a) se citesc mai departe, ca straturi (`snow` → `particles` cu `shape = "snow"`,
  `color` → `colors = [color]`), cu moștenirea de atunci (fiecare fel declarat îl înlocuiește
  pe cel al temei extinse). O listă `[[decorations.layers]]` înlocuiește lista temei extinse
  în întregime; `[decorations] layers = []` = fără decorațiuni (și fără cele moștenite).
  Temele livrate de Crăciun trec la formatul nou; testele păstrează un fișier în cel vechi.
- **Presetările** (`theming::decorations::presets`, date pure, în varianta întunecată și cea
  luminoasă): Crăciun, Iarnă, Toamnă, Primăvară, Halloween, Anul Nou, Minimal — puncte de
  plecare, nu încuietori: alese în studio, devin straturile temei, editabile.

### Suprapunerea utilizatorului („decorațiunile mele pe orice temă”)

- În setări (modulul `theme-decorations`, `src/settings.rs`): `[appearance]
  my_decorations` = `none` (implicit: doar ale temei) | id-ul unei presetări | `custom`
  (straturile salvate din studio, `my_decorations_layers`: câte un strat pe rând, codat
  `kind=…;cheie=valoare;…` din aceeași schemă) și `my_decorations_with` = `add` (peste ale
  temei) | `replace` (în locul lor). Blocul „Decorațiuni” din Setări → Aspect le arată;
  studioul are butonul „Folosește-le cu orice temă” (cererea generică
  `AppRequest::SetSettingText` către blocul modulului).
- Schimbarea temei le păstrează; `version()` crește la fiecare schimbare, ferestrele își
  refac decorațiunile. Fără nimic de desenat (nici tema, nici suprapunerea): nimic făcut.

### Opțiunile temei (41e)

- O temă își declară setările (`[[options]]`), arătate **doar pentru ea** — în studio și în
  Setări → Aspect cât e activă (blocul „Opțiunile temei”, al aplicației, generic):

```toml
[[options]]
id       = "lights"
kind     = "choice"               # toggle | choice | slider | color
label.en = "Lights"
label.ro = "Luminițe"
default  = "warm"

[[options.choices]]
id       = "warm"
label.en = "Warm"
label.ro = "Calde"
palette.amber = "#f0b860"         # ce nume din paletă schimbă alegerea

[[options]]
id       = "snow"
kind     = "slider"
label.en = "Snow"
min = 0.0
max = 0.8
step = 0.1
default = 0.5
```

- Ce schimbă: o **alegere** suprascrie nume din paletă (`palette.<nume>`) — interfața, sintaxa
  și decorațiunile care le folosesc prin `$nume` le urmează; o **culoare** cu `palette =
  "<nume>"` la fel, iar un câmp de culoare al unui strat poate fi `"@id"`; un **glisor** dă
  valoarea unui câmp numeric (`density = "@snow"`); un **comutator** (și orice opțiune) poate
  arăta / ascunde un strat (`when = "id"`, `"!id"`, `"id=alegere"`).
- Valorile: `[appearance] theme_options = ["<temă>/<opțiune>=<valoare>", …]` (`Setting` al
  aplicației), validate la folosire (o valoare invalidă sau o opțiune care nu mai există:
  implicitul / ignorată). În fișier: o opțiune greșită (fel necunoscut, fără alegeri,
  interval invalid, cheie necunoscută) e ignorată cu o notă (în jurnal), nu strică tema; un
  strat care cere o opțiune necunoscută rămâne cu valoarea lui (notă).
- Rezolvarea: `ThemeRegistry::resolve_with(id, valori)` — opțiunile pe lanțul `extends`
  (copilul le suprascrie după id), valorile efective, suprascrierile paletei aplicate
  tokenilor scriși `$nume` (temele țin ce nume a folosit fiecare token), straturile
  filtrate de `when` și legate de opțiuni. `ResolvedTheme` ține opțiunile și valorile (blocul
  din setări le arată). Nimic de Crăciun în aplicație: tema de Crăciun își declară schema
  de luminițe (calde / colorate / reci) și densitatea zăpezii.

### Studioul (41f)

- **Antetele secțiunilor**: `ds::section_header` (ui-components) — pictograma, titlul cu
  majuscule, o linie subțire până la capătul lățimii; spațiu deasupra fiecărei secțiuni în
  afară de prima și dedesubt în afară de ultima (prima / ultima păstrează doar padding-ul
  containerului). Aceleași pentru toate: Numele temei, Pornește de la, Interfața, Sintaxa,
  Cursorul, Decorațiunile, Opțiunile temei.
- **Decorațiunile**: rândul presetărilor (fără / Crăciun / … / Minimal: înlocuiesc lista),
  apoi fiecare strat într-un card — felul lui și „Elimină”, câmpurile din schemă (alegeri ca
  pastile, cantități ca glisoare, culori ca mostre deschise cu selectorul de culoare,
  listele cu „+” / „−”) —, „Adaugă: ghirlandă / particule / colțuri / lumină” și „Folosește-le
  cu orice temă”. Un câmp legat de o opțiune a temei arată de care; editat, devine valoarea
  lui.
- **Opțiunile temei** în studio: valorile lor (devin implicitele temei salvate).
- Draftul primește straturile și opțiunile din tema citită (`StudioInput` le aduce de la
  aplicație), nu din textul ei; `compose` le scrie (`[[options]]`, `[[decorations.layers]]`).

### Pașii 41d–41f

- **41d** — modelul straturilor (felurile, schema câmpurilor, presetările, legăturile),
  citirea în formatul nou și în cel vechi, desenele noilor feluri (fire, mărgele, fanioane,
  panglică; frunze, petale, stele, scântei, confetti, ploaie; floare, frunză, pictogramă;
  lumina marginilor), suprapunerea utilizatorului (setări, blocul din Aspect), temele de
  Crăciun în formatul nou, studioul adus la noul model (aceleași rânduri, pe primul strat
  al fiecărui fel). Capturi noi: `decorations-autumn` (presetarea Toamnă peste tema
  rulării, ambele variante), `decorations-new-year`. Teste: citire (nou, vechi, erori),
  moștenire, schema (fiecare câmp citit / scris / codat dus-întors), presetările valide,
  layout-ul fiecărui fel pe margini și în fereastră, desenele, 0 alocări și bugetele pe
  presetările animate, suprapunerea (adăugată / înlocuind / fără).
- **41e** — opțiunile temei (domeniul, citirea, rezolvarea cu valori, setarea aplicației,
  blocul din Setări → Aspect, tema de Crăciun cu opțiunile ei). Captura
  `settings-theme-options` (Aspect pe tema de Crăciun). Teste: citire și note, valori
  invalide ignorate, alegerea schimbă paleta (UI și decorațiuni), glisorul densitatea,
  comutatorul ascunde un strat, moștenirea opțiunilor.
- **41f** — studioul: antetele secțiunilor, editorul de straturi, presetările, opțiunile
  temei, „Folosește-le cu orice temă”. Capturi: `studio*` și `theme-studio-decorations` se
  schimbă (antetele, editorul), noi `studio-layers` (editorul de straturi pe o temă fără
  Crăciun, cu presetarea Toamnă), `studio-options` (opțiunile temei de Crăciun). Teste:
  adăugat / eliminat / editat un strat, culoarea aleasă, presetarea, opțiunile, ritmul
  antetelor (spațiul dintre secțiuni, nimic lipit de margini).

## Stare

- **41a făcut.** `theming::decorations` (`Decorations`, `DecorationsSpec::over` / `resolve`,
  limitele: zăpada ≤ 0,5, `drop` ≤ 0,8, ≤ 6 culori de lumini, ≤ 4 de globuri),
  `ResolvedTheme::decorations()` (un `Arc`, împărțit cu desenatorul ferestrei);
  `themes-builtin` citește și validează `[decorations.*]` (`ThemeFileError::Decoration`
  cu cheia; fel / cheie necunoscută: eroare de sintaxă). `ui-components::decorations`:
  `DecorationMode`, `Geometry`, `DecorationFrame`, contractul `DecorationsProvider` /
  `WindowDecorations`, `layout` (swag-uri egale de ~150 px logici de la `titlebar_inset`,
  luminițe în număr par pe swag, globul la mijlocul swag-ului dacă nu atinge conținutul
  barei de sus, plăcile laterale până la `drop`, colțurile, fulgii deterministici),
  `twinkle` (o umflare scurtă și rară, ~16 % din ciclul fiecărui bec), `flake_at`,
  `hidden`, `DecorationClock` (15 cadre / s, oprit după 20 s fără input, timpul înghețat
  și reluat fără salt, `STATIC_TIME` pentru static). `platform_api::Window::reduce_motion`
  / `power_saving` (macOS: `NSWorkspace.accessibilityDisplayShouldReduceMotion`,
  `NSProcessInfo.isLowPowerModeEnabled`; Windows: `SPI_GETCLIENTAREAANIMATION`,
  `GetSystemPowerStatus().SystemStatusFlag`), întrebate cel mult o dată la 5 s. Modulul
  `theme-decorations` (id `decorations`): `art` (pânză premultiplicată, discuri, capsule,
  elipse, halouri; crenguța din ace în trei verzi, becurile cu soclu, halo și luciu,
  globul cu capac, ață și luciu, sclipirea cu raze fine, fulgii, funda / ilicele / steaua),
  `paint::Painter` (layout memoizat pe geometrie și pe ce arată bara de sus, desenele
  refăcute doar pentru altă scară, altă lățime de swag în pași de 8 px sau când GPU-ul le-a
  eliberat; `ImageData::gpu_only`), `settings` (`[appearance] decorations`, blocul
  „Decorațiuni” în Aspect, `version()` ca ferestrele să-și refacă decorațiunile).
  Aplicația: `LanguageSupport::decorations` (+ modulul lor, ca memoria lor să apară la el
  în Task Manager), `Workbench`: stratul după nuanța proiectului, `covered` (cardul
  editorului, panourile, fereastra de jos), `TopBar::occupied` (ce arată bara de sus),
  `repaint_decorations` (`Scene::layer_mut`); `EditorApp::render`: un cadru doar al
  decorațiunilor când nu a venit niciun eveniment de la ultimul cadru întreg și niciun alt
  termen nu e scadent (cel mult o secundă între cadre întregi), cererea următorului cadru
  al decorațiunilor după fiecare cadru, inputul (taste, mouse, scroll, IME) le repornește.
- **Bugete măsurate** (release, Apple Silicon, `cargo test -p theme-decorations --release
  budget`): un cadru animat al decorațiunilor ~1 µs la 1200 × 700 @2 și ~2,5 µs la
  2560 × 1440 @2 (buget 200 µs); primul cadru (layout + desene) ~0,7 ms, o dată pe scară;
  0 alocări pe cadru (alocator numărător, 400 de cadre la scara 1 și 2); memoria ținută
  ~108 KB la 1200 × 700 @2, ~236 KB la 1600 × 900 @3 înainte de upload, sub 64 KB după
  (pixelii pleacă pe GPU); la 1200 × 700 @2 un cadru are cel mult câteva zeci de imagini
  (~8 swag-uri, 6 plăci laterale, globurile, 2 colțuri, sclipirile din acel moment, fulgii
  vizibili).
- **41b făcut.** `themes/christmas-dark.toml` și `christmas-light.toml` (după structura
  temelor implicite: cărbune cu tentă de brad / ivoriu, accent vișiniu, cursorul auriu pe
  întunecat, sintaxa scorțișoară / brad / auriu / albastru de iarnă / roz; ghirlanda sus și pe
  laturi până la 42 %, luminițe chihlimbar / ivoriu / vișiniu cu halo 0,7 pe întunecat și
  0,3 pe luminos, globuri vișinii și aurii, funde, zăpadă 0,26 albă pe întunecat și 0,35
  gri-albăstruie pe luminos); capturile `theme-christmas` (`THEMED=christmas`: varianta
  temei rulării) și `theme-christmas-light`, grupul `themes` din `scripts/ui-snapshots.sh`;
  decorațiunile sunt mereu un cadru static în capturi (`STATIC_TIME`).
- **41c făcut.** `ThemeDraft::decorations` (citite din `[decorations.*]` cu numele din
  paletă rezolvate, scrise de `compose` cu culorile în hex, după `[syntax]` și `[caret]`;
  secțiunile lor nu mai intră în `[syntax]`); studioul are secțiunea „Decorațiuni” — rândurile
  generalizate (`Row::Caret` / `Row::Deco`): ghirlanda (fără / sus / sus și pe laturi),
  luminițele și sclipirea (glisoare), zăpada (glisor, primul pas: fără), colțurile (fără /
  fundă / ilice / stea), previzualizate live; culorile decorațiunilor rămân ale temei (sau
  cele implicite). Captura `theme-studio-decorations` (studioul pe tema de Crăciun,
  derulat la decorațiuni); `studio-caret` se schimbă (sub cursor apar decorațiunile).
- **Măsurat în aplicație** (release, macOS Apple Silicon, 2400 × 1400 @2, HOME izolat,
  `ps` pe procesul ei; scriptul în afara depozitului): cu decorațiunile animate
  (`christmas-dark`, fereastra focalizată, primele 20 s) ~3,1 % dintr-un nucleu, față de
  ~0,6 % cu ele oprite sau cu `default-dark`; în repaus (după 20 s fără input) ~0,55 % față
  de ~0,50 % — adică nimic în plus (restul e monitorul de resurse, comun). RSS în repaus
  101,3 MB cu ele / 100,7 MB oprite / 100,9 MB cu `default-dark` (diferența în zgomot, sub
  1 MB). Jurnalul (`RUST_LOG=lumen=debug`) arată că aproape toate cadrele animate sunt
  „doar decorațiunile” (~0,4–0,7 ms cu tot cu retrimiterea scenei la renderer), cu ~1–2
  cadre întregi pe secundă; costul rămas e al rendererului, care redesenează toată
  suprafața.
- **Abateri:** densitatea zăpezii e mai mare decât pare (~150 de fulgi la densitate 1): cei
  mai mulți cad în spatele cardurilor și nu se desenează; se văd doar în benzi, în bara de
  sus și în cea de stare. Cadrele doar ale decorațiunilor țin scena întreagă și o retrimit
  rendererului; de la auditul C rendererul redesenează doar zonele schimbate (plan 06,
  „Damage tracking”: ~2–6 % din suprafață pe cadru, GPU 1,7–1,8 % → 1,0 % dintr-un nucleu),
  dar CPU-ul rămâne ~3,2–3,7 %: costul fix al prezentării unui cadru (~2 ms) la 15 cadre / s. Pe macOS, cu butoanele
  sistemului, latura stângă începe sub bara de sus și funda stângă după ele.
- **41d făcut.** `theming::decorations`: straturile (`LayerSpec`: `GarlandSpec` cu `strand` /
  `items`, `ParticleSpec`, `CornerSpec` cu `icon`, `GlowSpec`), schema câmpurilor
  (`LayerKind::fields`, `LayerSpec::get` / `set` / `color_at` / `set_color_at`, validate),
  legăturile (`Binding`: `$nume` / `@opțiune`, `Condition` pentru `when`), `DecorationsSpec`
  cu `Merge` (`Inherit` / `ByKind` pentru formatul întâi / `Replace`), `resolve(ResolveWith)`
  (valori, suprascrierile paletei, întunecată), `with_overlay`, codul pe un rând (`encode` /
  `decode`), `presets` (7, în ambele variante). Tot acum (domeniul și cititorul, fiindcă
  straturile le folosesc): `theming::options` (schema, valorile, `effective`,
  `palette_overrides`, lista din setări) și `Theme::{options, ui_refs, syntax_refs, notes}`,
  `ThemeRegistry::resolve_with` / `declared`. `themes-builtin` citește ambele formate (o
  singură funcție, după schemă; erorile cu `layers[i].cheie` / `garland.cheie`), `[[options]]`
  cu note pentru ce ignoră. `ui-components::decorations`: `garland_layout`, `corners_layout`,
  `particles_layout`, `particle_at` (căderea pe formă), `tumble`, `particle_level`,
  `glow_reach`; `DecorationFrame::icons`. Modulul: un pictor pe strat; desenele noi (sârmă,
  panglică, mărgele, fanioane, frunze, petale, confetti, ploaie, stele, floare, frunză, benzile
  luminii; colțul-pictogramă e o glifă din setul aplicației); setările `my_decorations`,
  `my_decorations_with`, `my_decorations_layers` și rândurile lor în blocul „Decorațiuni”
  (`set_text("custom")` primește straturile studioului). Aplicația lasă furnizorul să decidă
  (suprapunerea poate desena peste o temă fără decorațiuni), notele temelor în jurnal;
  `StudioInput` aduce `StudioTheme` (decorațiunile și opțiunile citite). Temele de Crăciun în
  formatul nou (aceeași ordine: capturile `theme-christmas*` și `theme-studio-decorations`
  identice). Studioul păstrează rândurile de până acum, pe primul strat al fiecărui fel.
  Capturi noi: `decorations-autumn`, `decorations-new-year`, `decorations-winter`
  (`MY_DECORATIONS=<presetare>`, peste tema rulării).
- **Bugete (41d, release, Apple Silicon):** un cadru animat ~0,3–2,3 µs la 1200 × 700 @2 și
  ~0,6–4,5 µs la 2560 × 1440 @2 pentru fiecare presetare (buget 200 µs); 0 alocări pe cadru
  pentru toate presetările (scara 1 și 2); memoria sub 1 MB înainte de upload, sub 64 KB după,
  pentru toate.

- **41e făcut.** Setarea aplicației `[appearance] theme_options` (`settings.rs`, validată:
  `"<temă>/<opțiune>=<valoare>"`), `Themes` rezolvă tema folosită (și cea a proiectului, și
  temele studioului) cu valorile ei (`resolve_with`; `declared` dă studioului valorile ca
  implicite). Blocul aplicației `theme-options` (`theme_options.rs`, generic): primul din
  Aspect, sub teme, cu titlul „<Tema>: setările ei” — un rând pe opțiune (comutator,
  alegeri, glisorul cu opririle lui, culoarea scrisă în hex), cheile rândurilor din id-uri
  păstrate o dată pe proces; o valoare salvată crește `version()`, iar fiecare fereastră
  reaplică tema (`sync_shared`). Un bloc fără rânduri nu se mai arată (orice contribuție).
  Temele de Crăciun își declară opțiunile: luminițele (calde / colorate / reci: schimbă
  `$light1..3`, pe care le urmează ghirlanda), zăpada (glisor 0–80 %, `density = "@snow"`),
  fundele (comutator, `when = "bows"`); la implicite, aceleași decorațiuni (capturile de
  Crăciun identice). Captura nouă `settings-theme-options` (Aspect căutat după „opțiuni”, pe
  tema de Crăciun); `settings-notifications` se schimbă (blocul „Decorațiuni” are rânduri
  noi). **Abatere:** opțiunile temei (domeniul și cititorul) au intrat deja în 41d — straturile
  se leagă de ele.
- **41f absorbit în planul 42** (pasul 42a, comis împreună cu el; studioul e apoi regândit în
  42b–42c, vezi [42-studio-efecte.md](42-studio-efecte.md)). Ce adusese: `ds::section_header` (ui-components: pictograma, titlul cu majuscule, linia
  subțire până la capăt; `SECTION_HEADER`, `SECTION_SPACE`) și studioul pe secțiuni: Numele
  temei, Pornește de la, Interfața, Sintaxa, Cursorul, Decorațiunile, Opțiunile temei (doar
  pentru o temă care le are) — prima direct sub titlu, între oricare două de două ori
  `SECTION_SPACE`, ultima doar cu padding-ul containerului. Rândurile generalizate
  (`rows.rs`: `Row::{Caret, Field, Option, Preset, Add, Icon}`, alegerile / curentul /
  alegerea din schema felului sau a opțiunii); `deco.rs`: presetările (cea care se potrivește
  aprinsă), câte un card pe strat (felul, „arătat cu …” pentru `when`, „Elimină”, toate
  opțiunile felului: pastile, glisoare de 0–100 % din maxim, mostre de culoare deschise cu
  selectorul, „+” / „−” în limitele listei, pictogramele colțului), „Adaugă un strat” (cele
  patru feluri, până la 8 straturi), „Folosește-le cu orice temă” (`AppRequest::SetSettingText`
  către blocul `decorations`, cheia `custom`; nota de după), opțiunile temei (devin
  implicitele temei salvate; culorile ca mostre). Un câmp legat de o opțiune arată „urmează
  …”; editat, își ia valoarea lui. Comenzile vederii `preset:<id>` și `scroll-to:<secțiune>`
  (capturile le folosesc: `STUDIO_COMMAND`). Capturi noi: `studio-layers` (presetarea Toamnă
  pe tema rulării), `studio-options` (opțiunile temei de Crăciun); se schimbă `studio`,
  `studio-caret` (acum `scroll-to:caret`) și `theme-studio-decorations` (`scroll-to:
  decorations`).
