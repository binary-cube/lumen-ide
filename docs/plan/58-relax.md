# Plan 58 — Modul Relax (un ecran de odihnă, ca un screensaver)

## Context
Cerere: un „Relax mode” — un buton pe bara din dreapta, chiar deasupra Task Manager-ului,
care ascunde panourile și arată peste toată fereastra o scenă animată, liniștită; `esc`
(tasta sau tasta desenată în colțul din dreapta sus, ca la About) iese și totul revine cum era.

## Design
- **Modul nou** `crates/modules/tools/relax` (crate `relax`, id `tools.relax`): un
  `ToolWindowProvider` pe `Dock::Right`, `strip_end`, ordinea 10 (Task Manager are 20: e
  imediat deasupra lui), `ToolContent::Command` cu comanda `relax.start`; iconița
  `moon-star` (Lucide) adusă de modul. Comanda deschide vederea modulului ca overlay modal
  (`host.open_overlay`, fără ramă): nimic din spate nu primește taste / mouse.
- **Vederea** (`RelaxView`, un `Viewer`): desenează toată fereastra (`covers_window`: app-ul
  nu mai desenează nimic sub ea — panourile sunt „ascunse” fără să le schimbe starea, deci
  revin exact cum erau; vederile din spate sunt anunțate ascunse: Task Manager-ul nu mai
  măsoară). `esc` (cheia) / click pe tasta desenată = `Close`. Tasta `esc` apare când se
  mișcă mouse-ul și dispare după ~3 s fără mișcare (ca un screensaver); desenată de un helper
  nou din `ds` (`ds::overlay_key`), folosit și de About (aceeași tastă: albastru-noapte 62 %,
  chenar alb 28 %, text alb 95 %).
- **Ritmul**: `Viewer::frame_deadline` (nou, implicit `None`): vederea cere următorul cadru
  la ritmul ales (60 / 30 pe secundă), nu la al ecranului; fereastra ascunsă / minimizată
  nu desenează nimic (timpul scenei nu sare: pasul e limitat). Nimic nu rulează cu vederea
  închisă (modulul nu ține nimic în afară de setări).
- **Scene** (`RelaxScene`: `id`, `resize`, `set_colors`, `configure` (densitate),
  `update(t)` — matematică pură, deterministă —, `paint`, `memory`) într-un registru
  (`SCENES`): geometria precalculată la `resize`, sprite-urile (strălucire, gradientul
  cerului) făcute o dată pe culori, niciun `Vec` nou per cadru. 3D = puncte proiectate pe
  CPU în perspectivă, desenate ca quad-uri rotunde / imagini (renderer-ul n-are linii rotite
  sau shadere proprii, nu e nevoie de ele):
  - **Stele** (3D): un câmp de stele prin care treci încet, cu o rotire lentă și o ceață
    (nebuloasă) în culorile temei.
  - **Valuri de lumină** (3D): un ocean de puncte (o grilă pe valuri de sinusuri) survolat
    încet, cu soarele / luna la orizont.
  - **Lampioane** (2.5D): lampioane care urcă încet deasupra apei, cu reflexia lor tremurată.
- **Culori**: din paleta temei (fundal, accent, culorile sintaxei / ANSI) — dark, light,
  Matrix (verde), Cyberpunk (neon) — sau paleta proprie a scenei (setare).
- **Setări** (`[relax]`, în `src/settings.rs`, bloc propriu în Interfață): scena (sau
  „prin rotație” la N minute), viteza, densitatea, culori din temă / ale scenei, ceas mare,
  mișcare redusă (urmează sistemul / da / nu), cadre pe secundă (60 / 30), pornire automată
  după N minute fără activitate (oprită implicit).
- **Pornire automată**: un punct de extensie nou în `workbench-model` (`IdleAction`: după
  cât timp, ce comandă) și `IdleWatch` (logica pură: ultima activitate, a pornit deja); app-ul
  ține minte ultima intrare în fereastră și rulează comanda o dată, doar cu fereastra activă,
  vizibilă și fără alt modal deschis.

## Pași
- **58a** — modulul, butonul, overlay-ul cu `esc`, `covers_window` / `frame_deadline`, cele
  trei scene și registrul (rotația, ← →), setările, ro / en, teste.
- **58b** — pornirea automată după N minute fără activitate (`IdleAction` / `IdleWatch`),
  măsurători.

## Stare
- [x] 58a — modulul `relax` (`tools.relax`): butonul `moon-star` în grupul de jos al barei
  din dreapta (ordinea 10, deasupra Task Manager-ului), overlay modal fără ramă care acoperă
  fereastra (`Viewer::covers_window`: app-ul nu desenează nimic sub el, vederile din spate
  sunt anunțate ascunse; `esc` îl închide, panourile sunt cum erau), ritm propriu
  (`Viewer::frame_deadline`: 60 / 30 de cadre, pasul ceasului scenei limitat la 100 ms),
  tasta `esc` din `ds::overlay_key` (și în About), vizibilă cât se mișcă mouse-ul (3 s, apoi
  dispare). Scene: Stele (3D, 5200 de stele, rotire, nebuloasă), Valuri de lumină (3D, o
  grilă fixă de camera peste trei valuri de sinusuri, soarele la orizont), Lampioane (2.5D,
  cu reflexii); culorile temei (fundal, accent, sintaxă) sau ale scenei. Setările `[relax]`
  pe pagina proprie din Interfață. Mișcarea redusă a sistemului citită de app la 5 s
  (`decorations::set_reduce_motion`). CPU per cadru (release, 2400 × 1400): Stele ~0,04 ms,
  Valuri ~0,19 ms (5 900 de quad-uri), Lampioane ~0,003 ms.
- [x] 58b — `workbench_model::IdleAction` (după cât timp, ce comandă) și `IdleWatch` (o
  dată pe perioadă de inactivitate; intrarea o ia de la capăt); fereastra ține un ceas per
  acțiune, cere un cadru la termen și rulează comanda doar în fereastra din față (ultima
  care a primit focusul, activă sau nu acum — deci una singură din mai multe), vizibilă și
  fără nimic deschis peste ea (altfel perioada e sărită). Relax mode: `[relax] idle_minutes`
  (0 = niciodată, implicit). Cu fereastra neactivă scena merge la cel mult 30 de cadre
  (ritmul ales revine la focus; ~0,2 ms CPU per cadru la Valuri → ~6 ms pe secundă);
  ascunsă / minimizată nu desenează nimic.
- [x] 58b după — efectele temei (decorațiile, Matrix, glitch, CRT… — toate în stratul
  decorațiilor) suspendate cât e deschisă o vedere care acoperă fereastra: nedesenate, fără
  cadre doar ale lor (stratul vechi nu mai e redesenat peste scenă: pâlpâia), fără cadre
  cerute, ceasul lor oprit; la Esc revin de unde erau.
- [x] 58c — previzualizări în setări: un rând generic de carduri (`SettingsRow::Cards` +
  `ui_components::CardPreview`, pictate de modul în straturile lor, decupate la conținut;
  oricare modul îl poate folosi): scena aleasă (sau cea de sub mouse) rulează mică, la ~30
  de cadre, cu setările aplicate pe loc; celelalte un cadru fix, făcut o dată și păstrat
  până se schimbă ce arată; nimic nu rulează pe altă pagină / cu setările închise / sub
  Relax mode (ce au făcut e eliberat). Butonul „Previzualizare” (`SettingsContribution::
  command`) deschide Relax mode peste setări; Esc revine la ele. Sub o vedere care acoperă
  fereastra nu se mai desenează nici overlay-urile de sub ea (setările, About).
- [x] 58d — două scene noi: „Valuri curgătoare” (benzi mari, line, în straturi, fiecare un
  gradient mic întins în felii verticale la pixeli întregi, cu luciu pe creastă, culorile
  trecând încet prin paletă; ~0,04 ms / cadru, 3 000 de imagini) și „Panglică” (cer albastru,
  două panglici din curbe paralele de puncte, mai luminoase unde se pliază, scântei și
  lumini difuze; ~0,08 ms / cadru, 11 000 de quad-uri), în registru și în cardurile din setări.
