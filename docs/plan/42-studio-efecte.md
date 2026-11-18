# 42 — Theme Studio regândit și efectele animate (planul 42)

Cerut (2026-11-01): „la themes mi-ar fi plăcut să avem și mai multe animații, nu numai cu ce
ai făcut acum, merge una Matrix și încă alte 10. Foarte important e ca Theme Studio să fie
super bine organizat; acum nu e clar ce se întâmplă sau ce setez, și dacă mai adăugăm și
alte efecte o să fie și mai greu — trebuie regândit mult mai bine și mai clar, cu toate
opțiunile.”

Pornește de la planul 41 (41d — straturi generice, 41e — opțiunile temei, comise; 41f —
anteturile de secțiune și editorul de straturi, început, necomis: rămâne în arbore și e baza
pasului 42a, nimic nu se pierde).

## Ce nu merge acum

- **O singură coloană lungă**: numele, „Pornește de la” (30+ pastile), interfața, sintaxa,
  cursorul, decorațiunile, una sub alta — nu vezi unde ești, nu vezi ce ai schimbat.
- **Glisoare cu 5 etichete** („None, 25 %, 50 %, 75 %, 100 %”) care ocupă trei rânduri și
  nu spun ce face valoarea („Lights 0 %”? „Twinkle 20 %”?).
- **Decorațiunile sunt o listă plată de câmpuri** fără grupare, fără descriere, fără
  previzualizare a efectului; cu 15 efecte ar deveni ilizibil.

## Noul Theme Studio

```
┌─ Configurator teme ───────────────────────────────────────────────────────────────┐
│ [🔍 Caută o setare…]                                     ● 3 modificări nesalvate │
├────────────┬───────────────────────────────────┬──────────────────────────────────┤
│ ◐ General  │  ✦ EFECTE ─────────────────────── │                                  │
│ ▣ Interfață│  Efecte active (2)    [+ Adaugă]  │     Previzualizare live          │
│ {} Sintaxă │  ┌─────────────────────────────┐  │  (fereastra în miniatură, cu     │
│ ▏ Cursor   │  │ ⠿ ❄ Zăpadă      ●on   ⋯     │  │   efectele rulând, editorul,     │
│ ✦ Efecte 2 │  │   Fundal · densă · lentă    │  │   bara laterală, un toast)       │
│ ⚙ Opțiuni  │  ├─────────────────────────────┤  │                                  │
│  temei     │  │ ⠿ ▒ Matrix      ●on   ⋯     │◀ │  [▶ Animat | ▮ Static]           │
│            │  │   ▾ Aspect                   │  │  [Întunecat | Luminos]           │
│            │  │     Caractere  [Katakana ▾] │  │                                  │
│            │  │     Culoare    ■ #3cff7a     │  │                                  │
│            │  │   ▾ Mișcare                  │  │                                  │
│            │  │     Viteză   ━━━●━━━  Medie  │  │                                  │
│            │  │   ▸ Intensitate              │  │                                  │
│            │  │   ▸ Zonă                     │  │                                  │
│            │  └─────────────────────────────┘  │                                  │
├────────────┴───────────────────────────────────┴──────────────────────────────────┤
│ [Doar acest proiect | Peste tot]                         [Renunță]  [Salvează]     │
└────────────────────────────────────────────────────────────────────────────────────┘
```

- **Trei zone**: navigarea pe categorii (stânga, cu iconiță și un număr: câte efecte, câte
  modificări), panoul categoriei alese (mijloc, doar ea — fără scroll printr-un perete de
  setări), previzualizarea live (dreapta, mai mare, cu efectele rulând, comutator animat /
  static și întunecat / luminos pentru temele cu ambele variante). Îngust: navigarea devine
  un rând de taburi sus, previzualizarea se ascunde după un buton „Previzualizare”.
- **Categoriile**:
  - **General** — numele temei, „Pornește de la” ca **galerie de cartonașe** cu o mini
    mostră a culorilor (fundal, accent, trei culori de sintaxă), filtrate „Întunecate /
    Luminoase / Toate” și căutare; tema curentă marcată.
  - **Interfață** — culorile grupate (Fundaluri, Text, Accent și selecție, Stări: eroare /
    avertisment / succes, Margini), fiecare rând: mostră, nume, valoare hex, „↺” dacă e
    schimbată față de tema de pornire.
  - **Sintaxă** — grupate (Cuvinte-cheie, Șiruri și numere, Comentarii, Tipuri și funcții,
    Variabile…), cu aceeași mostră și „↺”.
  - **Cursor** — ce era (grosime, clipire, mișcare, halo, urmă), grupat Formă / Animație.
  - **Efecte** — vezi mai jos.
  - **Opțiunile temei** — doar dacă tema are; tot ca rânduri grupate.
- **Fiecare setare = un rând clar**: eticheta, o descriere scurtă de un rând (gri, „Câte
  luminițe are ghirlanda”), controlul, „↺” când diferă de implicit. **Glisorul nou**: o
  singură linie, valoarea în dreapta ca text cu sens („Rare / Medii / Dese”, „Lentă”,
  „40 %”), fără rândul de 5 etichete; trepte la Shift / săgeți.
- **Căutarea** („Caută o setare…”) filtrează peste toate categoriile și sare la rând.
- **Modificările nesalvate** se văd: punct pe categorie, numărul sus, „Renunță” le
  anulează pe toate.
- Anteturile de secțiune `[icon] TITLU ────` (41f, `ds::section_header`) cu ritmul cerut:
  margine sus doar de la a doua secțiune, margine jos doar până la penultima.

## Efectele: galeria și cartonașele

- **„+ Adaugă efect”** deschide o galerie (popover mare), grupată după unde apare efectul:
  **Margini** (ghirlandă, neon), **Fundal** (zăpadă, Matrix, stele, aurora…), **Colțuri**,
  **Ecran** (strălucire / vinietă). Fiecare cartonaș: o miniatură care se animă doar sub
  mouse, numele, o descriere de un rând, eticheta „ușor / mediu” pentru cost.
- **Preseturi** (Crăciun, Iarnă, Toamnă, Primăvară, Halloween, Revelion, Minimal + noi:
  Cyberpunk, Spațiu, Ocean, Synthwave) sus în galerie: aplică un set de efecte; după aceea
  fiecare se editează liber.
- **Lista efectelor active**: cartonaș pe efect, restrâns la un rând (mâner de tras pentru
  ordine, iconiță, nume, comutator on/off, rezumat „Fundal · densă · lentă”, meniu ⋯:
  Duplică, Resetează, Elimină). Deschis: opțiunile efectului grupate în **Aspect**
  (stil, caractere, culori), **Mișcare** (viteză, direcție), **Intensitate** (densitate,
  opacitate, strălucire), **Zonă** (unde apare). Cel mult 8 efecte active.
- **Globale** (sub listă, grupul „Performanță și accesibilitate”): Animate / Statice /
  Oprite (setarea existentă), „Oprește după N s fără activitate”, „Respectă reduce motion”,
  cadre pe secundă maxime (10 / 15 / 20 / 30), și costul estimat al efectelor active.

## Efectele noi (11)

Toate în modulul `decorations`, aceleași reguli ca la 41: sprite-uri rasterizate o dată și
ținute în cache, 0 alocări pe cadru, ≤ 30 fps (implicit 15–20), oprite cât fereastra e
ascunsă / fără focus / inactivă, nimic încărcat fără efecte, regiuni de redesenare strânse
(planul de audit C).

1. **Matrix** — coloane de caractere care cad (Katakana / cifre / binar / latin), capul
   coloanei luminos, coada se stinge; culoare, viteză, densitate, lungimea cozii. Glifele
   dintr-un atlas mic, rasterizat o dată.
2. **Câmp de stele / warp** — stele care se apropie din centru (sau derivă lent), cu
   sclipire.
3. **Aurora** — benzi de lumină care ondulează lent sus, în culorile alese.
4. **Licurici** — puncte luminoase care rătăcesc și pulsează.
5. **Bule** — bule care urcă, cu reflex, se sparg sus.
6. **Artificii** — explozii rare de particule (Revelion), cu gravitație și stingere.
7. **Stele căzătoare** — o dungă rară care traversează cerul.
8. **Valuri** — două-trei valuri care se mișcă lent pe marginea de jos.
9. **Margine neon** — un contur care pulsează / își rotește nuanța (synthwave, cyberpunk).
10. **Constelație** — puncte care derivă și se leagă cu linii fine când sunt aproape.
11. **Grilă synthwave** — o grilă în perspectivă care curge la marginea de jos, cu un
    „soare” în gradient.

Plus din 41: ghirlandă, particule (zăpadă, frunze, petale, stele, sclipiri, confetti,
ploaie), colțuri, strălucire / vinietă.

## Unde se văd efectele de fundal

Acum fundalul se vede doar în bara de sus, benzile laterale, bara de stare și golurile dintre
carduri — prea puțin pentru Matrix, aurora sau stele. Fiecare efect de fundal primește la
„Zonă”:

- **Marginile ferestrei** (implicit, ca acum): doar în spațiile dintre carduri.
- **Și în spatele panourilor**: cardurile (arbore, panouri laterale, fereastra de jos, și
  opțional editorul) devin ușor translucide (opacitate a fundalului cardului 85–97 %,
  reglabilă), efectul se vede discret prin ele; **textul rămâne complet opac**, iar efectul
  e plafonat la o opacitate care păstrează contrastul (verificat cu aceeași regulă de
  contrast 4,5:1 ca la teme). Editorul e separat, „și în spatele editorului”, oprit
  implicit.

## Pașii

- **42a — Bazele UI**: `ds::section_header` (din 41f), rândul de setare (`ds::setting_row`:
  etichetă, descriere, control, „↺”), glisorul nou cu valoare textuală, cartonașul
  restrângibil, galeria cu miniaturi, navigarea pe categorii — componente pure în
  ui-components, testate.
- **42b — Studio regândit**: cele trei zone, categoriile, galeria temelor de pornire,
  culorile grupate, căutarea, modificările nesalvate, responsive. Tot ce făcea înainte rămâne
  (fiecare setare existentă își găsește locul, verificat de un test care le enumeră).
- **42c — Efectele în studio**: galeria efectelor și a preseturilor, cartonașele grupate
  Aspect / Mișcare / Intensitate / Zonă, ordinea prin tragere, on/off, previzualizarea live
  animată, grupul „Performanță și accesibilitate”.
- **42d — Efectele noi 1–6** (Matrix, stele, aurora, licurici, bule, artificii) + zona „în
  spatele panourilor”.
- **42e — Efectele noi 7–11** (stele căzătoare, valuri, neon, constelație, grilă synthwave)
  + preseturile noi (Cyberpunk, Spațiu, Ocean, Synthwave).
- **42f — Măsurători și imagini**: fiecare efect măsurat în release (CPU pe cadru, alocări,
  RAM, timp GPU, procentul redesenat), bugetele verificate în teste; imagini de test pentru
  fiecare categorie a studioului (întunecat și luminos), galeria, fiecare efect într-un cadru
  static determinist.

Fiecare pas: un commit, `check.sh` verde, temele și setările salvate vechi merg mai departe.

## Decizii (recomandarea mea, schimbabile)

1. Zona „în spatele panourilor” cu carduri translucide: **da**, opțională pe efect, editorul
   separat și oprit implicit.
2. Cadre pe secundă implicite pentru efecte: **20**, reglabile 10–30.
3. Matrix cu caractere **Katakana + cifre** implicit (ca în film), plus binar / latin.

## Stare

- **42a făcut.** Pornește de la 41f (necomis până atunci, păstrat întreg: `ds::section_header`,
  rândurile generalizate `rows.rs`, editorul de straturi `deco.rs`, „Folosește-le cu orice
  temă” prin `AppRequest::SetSettingText`, comenzile `preset:` / `scroll-to:`, capturile
  `studio-layers` și `studio-options`). Nou în `ui-components::form` (pur, testat în
  `tests/unit/form.rs`): `setting_row` (eticheta, descrierea de un rând, controlul la dreapta
  — sub etichetă când e îngust —, „↺” și semnul de modificat; `row_height`, `stacks`),
  `group_quad` / `row_divider` (rândurile grupate într-un card), `value_slider` (glisorul pe
  un singur rând, valoarea în cuvinte la dreapta; `stepped`: săgeți / Shift), cartonașul
  restrângibil (`fold_card_head`: mânerul, iconița în plăcuța ei, titlul și rezumatul,
  comutatorul, „⋯”, săgeata; `fold_card_quad`; `fold_section` pentru Aspect / Mișcare / …),
  galeria (`Grid`, fără alocări; `gallery_card` cu locul miniaturii, eticheta de cost),
  navigarea pe categorii (`nav_list` pe lateral, `nav_tabs` când e îngust: numele doar la
  tabul ales dacă nu încap). Iconițe noi în setul aplicației: `sparkles`, `text-cursor`,
  `sun`, `moon`.
- **42b făcut.** Studioul regândit (`theme-studio`): `layout.rs` (cadrul: antetul cu titlul și
  tema de pornire, căutarea, numărul modificărilor nesalvate; navigarea pe categorii — listă
  pe lateral de la 1180 px logici, taburi sub ea; previzualizarea în dreapta de la 760 px,
  sub ea ascunsă după butonul „Previzualizare”, care o arată peste panou; căutarea pe rândul
  ei când e îngust; subsolul pe două rânduri sub 620 px), `model.rs` (categoriile: General,
  Interfață, Sintaxă, Cursor, Efecte, Opțiunile temei — doar pentru o temă care le are;
  grupurile de culori: Fundaluri, Text, Accent și selecție, Stări, Margini / Cuvinte-cheie,
  Șiruri și numere, Comentarii, Tipuri și funcții, Variabile; Cursorul: Formă / Animație;
  catalogul tuturor setărilor — ce caută căutarea și pe ce se numără modificările; căutarea
  fără majuscule și fără diacritice, toate cuvintele), `panel.rs` (titlul categoriei și
  rândul ei, secțiunile cu `ds::section_header`, rândurile din `ui-components::form` în
  carduri: mostră + hex + pipetă, segmente, pastile, glisorul pe un rând cu valoarea în
  cuvinte — „Rare / Medii / Dese”, „Lent”, „42 % din înălțime”, „220 ms” —, comutator,
  câmpul numelui; „↺” pe rândul schimbat față de tema de pornire; galeria temelor de
  pornire cu miniaturi — fundalul, panoul, editorul cu trei rânduri în culorile sintaxei,
  accentul —, filtrul Toate / Întunecate / Luminoase, „folosită” pe tema activă). Opririle
  cantităților: din zece în zece procente (11). „Renunță” anulează toate modificările
  (fără modificări: „Închide”, ca înainte). Glisorul clicuit ultimul se mișcă din săgeți
  (Shift: un pas mare). Comenzi noi ale vederii: `category:<id>`, `search:<text>` (`!`: primul
  rezultat arătat); `scroll-to:` merge mai departe (o categorie sau o secțiune). Culorile de
  stare (`red`, `yellow`, `green`) se editează unde paleta le are (`theming::studio::
  STATE_TOKENS`). Previzualizarea: un toast în colț, butonul de accent doar când încape,
  comutatoarele Animat / Static și Întunecat / Luminos (pentru o temă cu ambele variante:
  culorile celeilalte, efectele draftului). Studioul deschis într-o împărțire pornește și
  acolo de la aceleași teme (`mirror_data`). Efectele, până la 42c: presetările, câte un
  card pe strat cu un rând pe opțiune, „Adaugă”, „Folosește-le cu orice temă”. Test care
  enumeră toate setările de dinainte și le găsește rândul în categoria lor. Capturi noi:
  `studio-interface`, `studio-syntax`, `studio-search`, `studio-narrow` (într-o împărțire);
  se schimbă `studio`, `studio-caret`, `studio-layers`, `studio-options`,
  `theme-studio-decorations`.
- **42c făcut** (comis împreună cu 42b și 42d: lucrul s-a suprapus în arbore și pașii nu
  se mai puteau despărți fără să rescriu fișierele). Categoria Efecte: câte un cartonaș
  restrângibil pe efect (`form::fold_card_head`: mânerul de tras pentru ordine — o linie în
  accent arată unde cade —, iconița, numele cu „arătat cu …” pentru `when`, rezumatul
  „Fundal · medii · lent”, comutatorul on/off — `DecorationLayer::enabled`, scris în temă
  `enabled = false`, citit înapoi, sărit la rezolvare —, meniul ⋯: Duplică, Resetează,
  Elimină); deschis, opțiunile în secțiunile Aspect / Mișcare (deschise implicit),
  Intensitate / Zonă (închise), fiecare cu „↺” față de stratul temei de pornire. „+ Adaugă
  efect” deschide galeria peste panou (×, Esc sau o alegere o închid; nimic din spate nu ia
  clicuri cât e deschisă): presetările, apoi felurile grupate Margini / Fundal / Colțuri /
  Ecran, fiecare cu miniatura desenată de modulul decorațiunilor (se mișcă doar sub mouse;
  pictorii miniaturilor ținuți cât e deschisă), o descriere de un rând și costul (ușor /
  mediu: `LayerSpec::cost`). Previzualizarea arată efectele draftului rulând
  (`DecorationsProvider::preview`, furnizorul găsit prin registrul din `ui-components::
  decorations` — `register` / `provider`, ca la servicii —; cadre în ritmul unui loader, 20
  pe secundă, doar după input și până la inactivitate, nimic cât e „Static”; `purge` le
  eliberează). Grupul „Performanță și accesibilitate”: Animate / Statice / Oprite, cadre pe
  secundă (10 / 15 / 20 / 30, implicit 20), oprite după 10 s / 20 s / 1 min / niciodată,
  „Respectă reduce motion”, costul estimat al efectelor active — setări noi ale modulului
  `theme-decorations` (`decorations_fps`, `decorations_idle`, `decorations_follow_system`,
  și rândurile lor în Setări → Aspect), cerute prin `AppRequest::SetSettingChoice`;
  furnizorul le dă ferestrelor (`fps`, `idle`, `respects_system`), ceasul le folosește
  (`Moving::fps` / `idle`; `FPS` implicit 20, cel mult 30). Comenzi noi: `open-all`,
  `gallery`, `still`. **Abatere:** „Fără” (toate efectele scoase) nu mai e în galerie —
  efectele se elimină din meniul fiecăruia (comanda `preset:none` rămâne).
- **42d făcut.** Efectele 1–6 în domeniu (`theming::decorations::EffectSpec`, un singur fel
  de date pentru toate: stil, culori, densitate, viteză, mărime, strălucire, opacitate ≤ 0,5,
  zonă, „prin panouri”; câmpurile pe fel în `effect::fields`, deci citite din teme, scrise
  de studio și codate în setări ca celelalte) și desenate în modulul decorațiunilor
  (`fx.rs`): **Matrix** — coloane de caractere (Katakana oglindite + cifre implicit, cifre,
  binar, latin), glife desenate ca iconițe ale setului aplicației (`glyphs.rs`, contribuite de
  modul ca `fx-…`, rasterizate o dată în atlas, toate într-o singură trecere, fiecare cu
  culoarea și luminozitatea ei), capul luminos, coada stingându-se, caracterele schimbându-
  se din când în când; **câmp de stele** (warp din centru, cu dâră la viteză mare, sau derivă
  cu sclipire; puncte rotunde ca quad-uri); **auroră** (trei benzi, fiecare o imagine de o
  perioadă repetată și mutată, rasterizată la o treime din rezoluție și întinsă — lumina
  moale nu pierde nimic, memoria de nouă ori mai mică); **licurici**, **bule** (urcă,
  se umflă și se sting sus), **artificii** (explozii rare, scântei cu gravitație, un
  fulger la mijloc; una înflorită în cadrul static). **Zona** fiecărui efect de fundal
  (și a particulelor): marginile ferestrei (implicit), în spatele panourilor, și în spatele
  editorului; desenul ascuns doar sub ce acoperă zona lui (`DecorationFrame::editor`).
  Cardurile devin translucide (`Decorations::see_through`: opacitatea cerută 97–85 %,
  `ui_components::decorations::glass_alpha` o ridică până când textul — principal și
  secundar — păstrează contrastul 4,5 : 1, sau cât avea pe cardul opac; panourile laterale își
  desenează conținutul cu fundalul lăsat cardului, fereastra de jos și editorul la fel; textul
  rămâne opac); terminalul își pictează singur fundalul și rămâne opac. Previzualizarea din
  studio face la fel. Capturi noi: `studio-effect-open`, `studio-gallery`, `effect-matrix`,
  `effect-starfield`, `effect-aurora`, `effect-fireflies`, `effect-bubbles`,
  `effect-fireworks` (`MY_DECORATIONS=<fel>[@<zonă>]`). Presetări: noi Cyberpunk (Matrix
  în cyan și magenta, o lumină magenta pe margini), Spațiu (auroră violet-albastră, stele în
  spatele panourilor), Ocean (bule, licurici turcoaz); Anul Nou primește artificiile (testul
  cere ca presetările, între ele, să arate fiecare fel). 42e le completează (neon, stele
  căzătoare, constelație, valuri) și adaugă Synthwave. Bugetele (release, Apple Silicon): un
  cadru animat al fiecărui efect nou 0,1–6 µs la 1200 × 700 @2 și 0,1–9,3 µs la 2560 × 1440
  @2 (buget 200 µs), 0 alocări pe cadru (scara 1 și 2, și în spatele panourilor), memoria
  sub 1 MB înainte de încărcare și sub 64 KB după.
- **42e făcut.** Efectele 7–11 (aceeași formă de date, `EffectSpec`): **stele căzătoare**
  (o dungă rară — perioada după densitate —, capul luminos și coada de puncte care se sting;
  una la jumătatea drumului în cadrul static), **valuri** (1–3 după densitate, fiecare o
  imagine de o perioadă — rasterizată la jumătate din rezoluție — repetată pe marginea de jos,
  mișcate în sensuri opuse, legănate), **margine neon** (o linie strălucitoare în jurul
  ferestrei și haloul ei din câteva linii mai late și mai slabe; pulsează sau își rotește
  nuanța prin culorile ei; doar pe margini, fără zonă), **constelație** (puncte care derivă,
  legate prin linii punctate fine cu cel mult două vecine aflate aproape), **grilă synthwave**
  (liniile care converg spre orizont — o imagine la o treime din rezoluție —, liniile
  orizontale care vin spre tine ca quad-uri, soarele cu benzile lui la jumătate din
  rezoluție; stilul „doar grila” fără soare). Presetările completate: Cyberpunk (neon
  magenta–cyan în loc de lumina marginilor, Matrix), Spațiu (+ constelație, stele
  căzătoare), Ocean (+ valuri) și **Synthwave** nou (grila cu soarele, neon care își
  rotește nuanța). Redesenarea parțială (`render_api::Damage::add`): două dreptunghiuri care
  se ating se unesc doar dacă reuniunea lor nu risipește mult (cel mult un sfert în plus) —
  laturile unui chenar (neonul) rămân patru dreptunghiuri subțiri, nu toată fereastra.
  Contrastul textului peste ce se vede prin carduri urmează acum regula temelor: textul
  4,5 : 1, cel secundar 2,8 : 1 (sau cât avea pe cardul opac, dacă era mai puțin) —
  regula 4,5 : 1 pentru textul secundar ținea panourile opace mereu (pe tema implicită el
  are doar 4,2 : 1 pe panou). Capturi noi: `effect-shooting-stars`, `effect-waves`,
  `effect-neon`, `effect-constellation`, `effect-synthwave`, `decorations-cyberpunk`,
  `decorations-space`, `decorations-ocean`, `decorations-synthwave`. Test: fiecare fel,
  opțiune și alegere are numele și descrierea în ambele limbi.
- **42f făcut — măsurători și imagini.** În teste (release, Apple Silicon, `cargo test -p
  theme-decorations --release budget`): bugetele de la 41 pentru fiecare efect nou, singur,
  pe marginile ferestrei și în spatele panourilor — un cadru animat al decorațiunilor în
  0,1–6 µs la 1200 × 700 @2 și 0,1–9,3 µs la 2560 × 1440 @2 (buget 200 µs), 0 alocări pe
  cadru (400 de cadre, scara 1 și 2), memoria sub 1 MB înainte de încărcarea pe GPU (cel
  mai mult: grila synthwave, ~0,93 MB la 1600 × 900 @3) și sub 64 KB după; plus cât din
  fereastră redesenează un cadru (urmărit cu `DamageTracker`-ul rendererelor, 20 de cadre
  pe secundă, 1200 × 700 @2):

  | Efect | Redesenat pe cadru |
  |---|---|
  | Matrix (margini / prin panouri) | 5,9 % / 16 % |
  | Câmp de stele | 0–0,4 % |
  | Auroră | 30,6 % (benzile de sus se mută toate) |
  | Licurici, bule, constelație | 0,1–0,6 % |
  | Artificii, stele căzătoare | 0 % între apariții, puțin cât trec |
  | Valuri | 8,9 % |
  | Margine neon | 3,4 % (doar laturile, după schimbarea din `Damage::add`) |
  | Grilă synthwave | 1,5 % |

  În aplicație (release, macOS, 2400 × 1400 @2, HOME izolat, fereastra focalizată, tema
  `default-dark` cu un singur efect ca decorațiunile utilizatorului, 12 s animat; scriptul
  în afara depozitului), procent dintr-un nucleu:

  | Efect | CPU | GPU | RSS |
  |---|---|---|---|
  | niciunul | 0,58 % | 0,15 % | 101 MB |
  | zăpadă (41, referință) | 3,9 % | 1,35 % | 102 MB |
  | Matrix (margini / prin panouri) | 4,1 % / 4,7 % | 1,5 % / 1,8 % | 103 / 102 MB |
  | câmp de stele (prin panouri) | 4,6 % | 1,4 % | 102 MB |
  | auroră (prin panouri) | 4,6 % | 1,9 % | 103 MB |
  | licurici | 4,7 % | 1,5 % | 98 MB |
  | bule | 4,6 % | 1,5 % | 119 MB* |
  | artificii | 1,9 % | 0,4 % | 103 MB |
  | stele căzătoare | 1,1 % | 0,2 % | 118 MB* |
  | valuri (prin panouri) | 4,6 % | 1,6 % | 119 MB* |
  | margine neon | 4,5 % | 1,6 % | 118 MB* |
  | constelație (prin panouri) | 4,6 % | 1,5 % | 102 MB |
  | grilă synthwave (prin panouri) | 4,8 % | 1,6 % | 102 MB |

  Costul de ~4–5 % e, ca la 41, cel fix al prezentării unui cadru (~2 ms) la 20 de cadre pe
  secundă, aproape același pentru toate efectele; artificiile și stelele căzătoare costă
  puțin fiindcă între apariții cadrele nu schimbă nimic (fără daune, rendererul le sare).
  (*) RSS-ul variază de la o rulare la alta cu ~±15 MB (alocatorul, driverul), nu cu efectul:
  ce țin efectele e sub 1 MB (testul de mai sus). Inactiv (după oprirea configurată) sau
  nefocalizat: niciun cadru, ca la 41.

  Imagini (`scripts/ui-snapshots.sh`): fiecare categorie a studioului în ambele teme
  (`studio` — General, `studio-interface`, `studio-syntax`, `studio-caret`, `studio-layers` —
  Efecte, `studio-options` / `theme-studio-decorations` — pe tema de Crăciun), căutarea
  (`studio-search`), îngust (`studio-narrow`), un efect deschis (`studio-effect-open`),
  galeria (`studio-gallery`), fiecare efect nou într-un cadru static determinist
  (`effect-*`) și presetările noi (`decorations-cyberpunk`, `-space`, `-ocean`,
  `-synthwave`). Restul capturilor neschimbate (diferențele rămase: căile directorului de
  lucru în bara de stare / în popover-ul „Copy”, din scriptul meu cu alt director;
  `settings-notifications` — blocul „Decorațiuni” are rânduri noi; capturile consolei, ale
  coordonatorului).

- **42g (finisaje, făcut)**: notificarea din previzualizare („Tema a fost salvată”, desenată
  mereu ca mostră a culorilor popup-urilor, nu trimisă de aplicație) se citea ca una reală
  peste un studio fără modificări: acum spune „Exemplu de notificare” și stă în colțul
  ferestrei-miniatură, nu peste marginea ei. Galeria „Adaugă un efect” acoperă panoul până la
  margini (sub ea se vedeau „Efecte” și comutatorul „Animat / Static / Oprit”), iar rândul ei
  de sub titlu trece pe două rânduri când e lung. Nota din subsol e mai scurtă („Se salvează
  și în temele tale, pentru orice proiect.”), ține cont de lățimea reală până la butoane, trece
  pe două rânduri când e strâmt și dispare doar când nu mai încape deloc. Descrierea unui rând
  de setare (`ui-components::form`) trece pe al doilea rând în loc să fie tăiată (rândul crește
  cu o linie). În previzualizare, panoul lateral e puțin mai lat și numele fișierelor nu mai
  ies din el, iar al doilea tab apare doar când încape.
  **Bug găsit în aplicație (Matrix + efecte în studio)**: panoul lateral devenea verde aprins
  (culoarea accentului), la fel butoanele active din benzile de unelte, câmpul de căutare și
  segmentul ales. Cauza: `theming::studio::compose` (tema în lucru aplicată ferestrei) muta pe
  accent orice culoare din `[ui]` cu nuanța accentului și saturația HSL > 0,5 — iar HSL dă
  saturație mare și negrurilor nuanțate (`#020803`, fundalul panourilor Matrix). Acum croma
  (canalul cel mai luminos minus cel mai întunecat) decide și ea: un aproape-negru e un fundal,
  nu un accent; iar o culoare ale cărei repere (fundal, text, accent) n-au fost schimbate
  rămâne exact cum era (adăugarea efectelor nu mai atinge culorile temei). Teste: Matrix
  compusă neschimbată / cu alt fundal, și fiecare temă inclusă compusă neschimbată își
  păstrează culorile UI; captura `studio-matrix-live` (`EDITOR_SNAPSHOT_STUDIO_LIVE=1`: tema
  în lucru aplicată ferestrei, ca în aplicație) arată panourile închise, cu efecte în spatele lor.
  Teste instabile reparate la rădăcină: `frames_are_asked_only_while_the_window_is_in_use`
  presupunea 20 de cadre pe secundă, dar modulul citește ritmul din setări (30 la utilizator):
  testul verifică acum ritmul în vigoare; testele SQLite / CSV ale depozitului așteptau primul
  `Refreshed`, care poate veni (de la anunțul schimbării pe magistrală) înaintea lui `Applied`:
  acum așteaptă rezultatul commit-ului și reîmprospătarea de după el.

- **42h făcut — încă cinci efecte (1–5 din cererea „încă 10 efecte”).** Aceeași formă de date
  (`EffectSpec`), aceleași reguli: **fulgere** (o lovitură rară — perioada după densitate,
  4–15 s —: cerul luminat de sus, o imagine mică de 4 × 64 întinsă, care pâlpâie de două-trei
  ori, un halou unde pornește fulgerul și fulgerul ramificat din puncte; stilul „doar
  lumina” fără fulger; între lovituri nimic desenat, deci nicio redesenare), **picături pe
  geam** (stau o vreme, apoi alunecă tot mai repede, cu o urmă de puncte mici; sprite-uri cu
  margine mai închisă, reflex și un punct de lumină), **nori** (trei forme, câmpuri de
  „puf” adunate și tăiate moale — o uniune netedă —, la 2 px logici pe texel și întinși;
  plutesc pe sus, cei îndepărtați mai mici, mai slabi, mai lenți), **ceață** (1–3 benzi, câte
  o imagine de o perioadă repetată, la sol sau „văluri” peste fereastră) și **fluturi**
  (stau cu aripile deschizându-se rar, apoi zboară scurt între două locuri apropiate,
  bătând din aripi — patru cadre pe culoare). Norii, ceața și fluturii se mută cu pixeli
  întregi: un cadru în care s-au mișcat mai puțin e identic și nu se redesenează. Fiecare are
  culorile, densitatea, viteza (fulgerele: cât de des), mărimea, opacitatea, zona și „prin
  panouri”; iconițe noi în setul aplicației (`zap`, `droplet`, `cloud-fog`, `butterfly`).
  Presetări: **Furtună** nouă (fulgere, nori, ploaie, picături pe geam), Primăvara primește
  fluturii, Halloween ceața. Desenul unui efect folosește zona rezolvată a stratului
  (`LayerSpec::zone`), nu câmpul brut. Capturi noi: `effect-lightning`, `effect-raindrops`,
  `effect-clouds`, `effect-fog`, `effect-butterflies`, `decorations-storm`.
  Bugetele (release, Apple Silicon, `cargo test -p theme-decorations --release budget`): un
  cadru 0,04–1,6 µs median (fulgerele: 6–14 µs în cadrul unei lovituri) la 1200 × 700 @2 și
  2560 × 1440 @2, 0 alocări pe cadru, sub 1 MB ținut (cel mai mult rămâne grila synthwave);
  redesenat pe cadru la 20 de cadre / s: fulgere 0 % între lovituri, picături 0–0,1 %, nori
  8,3 %, ceață 18,4 %, fluturi ~0 % cât stau. În aplicație (ca la 42f, % dintr-un nucleu):

  | Efect | CPU | GPU | RSS |
  |---|---|---|---|
  | fulgere (prin panouri) | 1,25 % | 0,29 % | 108 MB |
  | picături pe geam (prin panouri) | 4,3 % | 1,25 % | 102 MB |
  | nori | 4,1 % | 1,3 % | 100 MB |
  | ceață (prin panouri) | 3,7 % | 1,3 % | 102 MB |
  | fluturi (prin panouri) | 2,05 % | 0,52 % | 103 MB |

  Se schimbă și `studio-gallery` (miniaturile Primăverii și ale lui Halloween) și
  `settings-notifications` (presetarea „Furtună” în lista decorațiunilor).

- **42i făcut — efectele 6–10 și presetările noi.** **Baloane** (urcă legănându-se, fiecare cu
  sfoara ei — un șir de quad-uri subțiri care se leagănă în urma balonului; corpul luminat din
  stânga sus, cu reflex și nod), **inimioare** (plutesc în sus, apar jos și se sting sus, se
  umflă de două ori pe bătaie; strălucirea dă haloul și cât de tare bat), **lilieci**
  (traversează din când în când pe o cale ondulată cu smucituri, siluete cu urechi și aripi
  festonate, trei cadre ale bătăii aripilor; între traversări nu sunt pe ecran), **lampă cu
  lavă** (pete de ceară care urcă și coboară încet; o singură imagine mică, moale, pe
  culoare, întinsă la orice mărime; unde două pete de aceeași culoare se apropie, între ele
  apare un „gât” care crește cu apropierea — aspectul de metaball, unirea și despărțirea, fără
  să desenăm câmpul la fiecare cadru; vecinii împart culoarea câte doi, ca să se întâlnească;
  mutate cu pixeli întregi) și **ecran retro** (linii de baleiaj — o singură imagine de 2 px
  lățime repetată în jos, rândurile desenate unu la unu —, sau „ecran curbat”: marginile
  umbrite, cu liniile doar acolo; un glitch rar — benzi despicate în două culori și o bară
  luminoasă care coboară, 0,3 s —; discret: opacitatea plafonată la 30 %, implicit 7 %, și
  **niciodată în spatele editorului**: zona oferă doar marginile și panourile, „everywhere”
  e refuzat la citire și citit ca „panouri” dacă vine altfel). Iconițe noi: `balloon`,
  `heart`, `bat`, `lava-lamp`, `tv`; ecranul retro stă în galerie la „Ecran”. Presetări noi:
  **Valentine's Day** (o lumină roz pe margini, flori în colțuri, inimioare), **Petrecere**
  (stegulețe, confetti, baloane prin panouri), **Ecran retro** (lampa cu lavă discretă și
  liniile de baleiaj prin panouri); Halloween primește liliecii. Testul cere în continuare ca
  presetările, între ele, să arate fiecare fel. Bugetele (release): un cadru 0,08–3,3 µs median
  (baloanele, cu sforile, cel mai mult), 0 alocări, sub 1 MB; redesenat pe cadru: baloane
  0,1–0,3 %, inimioare 0,2 %, lilieci ~0 % (cât nu trec), lampa cu lavă 9–10 %, ecranul retro
  0 % între glitch-uri. În aplicație (% dintr-un nucleu):

  | Efect (prin panouri) | CPU | GPU | RSS |
  |---|---|---|---|
  | baloane | 4,6 % | 1,36 % | 115 MB* |
  | inimioare | 4,5 % | 1,29 % | 114 MB* |
  | lilieci | 3,2 % | 0,79 % | 114 MB* |
  | lampă cu lavă | 4,8 % | 1,5 % | 115 MB* |
  | ecran retro | 1,16 % | 0,17 % | 100 MB |

  Capturi noi: `effect-balloons`, `effect-hearts`, `effect-bats`, `effect-lava-lamp`,
  `effect-crt`, `decorations-valentine`, `decorations-party`, `decorations-retro-crt`,
  `decorations-halloween`; se schimbă `studio-gallery` și `settings-notifications`
  (presetările noi).

### Ce nu s-a făcut / abateri

- 42b, 42c și 42d sunt un singur commit (lucrul s-a suprapus în arbore).
- Terminalul din fereastra de jos își pictează singur fundalul: rămâne opac și în zona „în
  spatele panourilor”.
- Studioul își animă previzualizarea în ritmul unui loader (20 de cadre / s, ca setarea
  implicită), nu după setarea „cadre pe secundă” (vederile nu au alt ritm de cadre).
- „Fără” (toate efectele scoase deodată) nu mai e în galerie; fiecare efect se elimină din
  meniul lui (comanda `preset:none` rămâne).
- 42h–42i: efectele noi se văd, ca toate cele de fundal, doar în golurile dintre carduri
  (sau discret prin panouri, cât permite contrastul textului); într-un cadru static unele
  (inimioare, baloane) apar puține. Lampa cu lavă imită metaball-urile prin „gâturi” între
  pete, nu printr-un câmp calculat (imaginile nu se pot rescrie pe GPU fără alocare la
  fiecare cadru).
- Regula contrastului pentru textul secundar e cea a temelor (2,8 : 1), nu 4,5 : 1: cu 4,5 : 1
  panourile rămâneau mereu opace pe tema implicită.
