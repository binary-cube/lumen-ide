# Plan 60 — Focus mode (zona de lucru în culori, restul alb-negru)

## Context
Cerere: un „Focus mode” — un buton pe bara din dreapta, chiar deasupra celui de Relax mode,
aprins cât e pornit. Pornit, zona în care lucrezi rămâne colorată și tot restul ferestrei
devine alb-negru (desaturat): grupurile editorului, panoul proiectului, ferestrele laterale
și de jos (cu grupurile lor), bara de sus, bara de stare, barele cu butoane. Zona e cea cu
tastatura; mouse-ul peste altă zonă o colorează pe aceea cât stă acolo. Modalele și
overlay-urile (dialoguri, setări, paleta, popover-e, meniuri, drawer-ul, tooltip-urile,
Relax mode) nu sunt desaturate niciodată. Fără animații: schimbarea zonei și pornirea /
oprirea sunt instantanee, iar efectele animate ale temei (ploaie, zăpadă, glitch, neon,
semne holografice, ghirlande…) stau pe loc cât e pornit (cadrul lor rămâne, doar mișcarea
se oprește) și pornesc de unde au rămas la oprire. Caret-ul rămâne cum e.

## Design
- **Efectul de randare, general** (`render_api::Emphasis`, `Scene::emphasize`): „ce au
  desenat straturile de până acum, estompat — desaturat cu `gray`, întunecat cu `dim` — în
  afara unor zone păstrate (`Keep`: dreptunghi rotunjit, margine moale `feather`, pondere)”.
  Cel mult `MAX_KEEPS` (8) zone, totul într-o structură fixă (nicio alocare). Matematica e
  una singură, în `render-api` (`Emphasis::mask` / `apply`, luminanța Rec.709 pe valorile
  sRGB, ca blending-ul renderer-elor), oglindită în WGSL.
  - *wgpu*: nu un pas în plus pe tot ecranul, ci un operator de culoare liniar aplicat la
    ieșirea fiecărei primitive a straturilor estompate (quad-uri, glife, imagini, fundalul
    zonelor redesenate). Cum blending-ul (premultiplicat) e liniar, `M(src) + (1-a)·M(dst)
    = M(src + (1-a)·dst)`: rezultatul e exact ca un pas final peste cadru, fără copia
    cadrului și fără încă o trecere. Doi uniformi `globals` (simplu / cu efectul); al doilea
    e scris doar când efectul se schimbă; straturile de sub limită îl leagă. Oprit: aceleași
    legături ca înainte, shaderul iese la prima comparație.
  - *software*: după straturile estompate, pixelii zonei redesenate trec prin `apply`.
  - *redesenări parțiale*: efectul face parte din cadru (`DamageTracker` îl ține minte);
    schimbat (altă zonă, pornit / oprit) → un cadru întreg, apoi iar parțiale (efectul e o
    funcție de poziție, aceeași de la un cadru la altul). Blur-urile de sub overlay-uri se
    refac atunci (citesc cadrul estompat, ca pe ecran).
- **Punctul de extensie** (`workbench_model::emphasis`): `RegionEmphasis` — o contribuție pe
  care app-ul o întreabă la fiecare cadru (ieftin) dacă vrea zona de lucru evidențiată și
  cum (`EmphasisRequest`: `gray`, `dim`, `follow` — tastatura / mouse-ul / amândouă —,
  `pause_effects`). Logica pură, testată, tot acolo: `Region` (un dreptunghi al layout-ului,
  cu felul lui: grup de editor, panou, grup de jos, bara de sus, bara de stare, bara cu
  butoane), `RegionFocus` (ce zonă: cea cu tastatura sau cea de sub mouse — „amândouă”: a
  mouse-ului cât e peste o zonă și s-a mișcat după ultima tastă, altfel a tastaturii; ține
  minte ce a ales, înghețat cât e deschis ceva peste fereastră). App-ul nu știe
  de niciun modul: adună zonele layout-ului (un `Vec` refolosit), cere `Emphasis` și îl pune
  în scenă după conținut și înainte de overlay-uri. Popup-urile din conținut (completarea,
  cardul de hover) sunt zone păstrate în plus: nu se estompează niciodată.
- **Butoanele „aprinse” ale comenzilor**: `ToolWindowProvider::active` (implicit `false`): un
  buton-comandă își spune starea (modul pornit), nu doar o fereastră deschisă.
- **Efectele temei pe pauză**: cât cererea are `pause_effects`, decorațiunile se desenează
  înghețate (ca într-o fereastră fără tastatură: ceasul stă, niciun cadru cerut doar pentru
  ele) și pornesc de unde erau.
- **Modulul** `crates/modules/tools/focus` (crate `focus-mode`, id `tools.focus`):
  `state.rs` (pornit / oprit), `settings.rs` (`[focus]`: `strength` 50 / 75 / 100 %,
  `dim` 0 / 10 / 20 %, `follow` keyboard / mouse / both; pagina „Focus mode” din Interfață),
  `policy.rs` (setările + starea → `EmphasisRequest`, pur), `button.rs` (butonul `contrast`,
  ordinea 5, deasupra lui Relax, aprins cât e pornit), `lib.rs` (comanda `focus.toggle`, fără
  scurtătură implicită — se alege din Keymap).

## Pași
- **60a** — `render_api::Emphasis`, wgpu și software, `DamageTracker`, teste (matematica pe
  o imagine cunoscută, nimic schimbat cât e oprit, cadrul întreg la schimbare).
- **60b** — punctul de extensie (`RegionEmphasis`, zonele, alegerea lor), app-ul (zonele
  layout-ului, efectul în scenă, popup-urile păstrate, modalele, ieșirea mouse-ului din
  fereastră — `PlatformEvent::CursorLeft`), `ToolWindowProvider::active`, decorațiunile pe
  pauză, teste.
- **60c** — modulul `focus-mode`, setările ro / en, teste, măsurători.

## Stare
- [x] 60a — `render_api::Emphasis` (`Scene::emphasize`, `Keep`, `mute`, `MAX_KEEPS` 8),
  `DamageTracker` (efectul schimbat → cadru întreg), software (după straturile de sub el, pe
  zona redesenată), wgpu (`common.wgsl` comun tuturor shaderelor: `Globals` cu efectul,
  `emphasize` la ieșirea quad-urilor, glifelor, imaginilor și a fundalului; un al doilea
  uniform scris doar la schimbare). Teste: matematica (liniară, deci aplicată per primitivă
  = aplicată cadrului), o imagine cunoscută pe CPU și pe GPU, cadrele parțiale = întregi cu
  efectul, oprit = ca înainte. Cost (release, Apple M, 2400 × 1400, 4 000 de quad-uri):
  cadrul întreg ~0,6–0,8 ms cu sau fără 1–3 zone (în zgomot), ~+0,2 ms cu 8; cadrul parțial
  (un caret) neschimbat (~0,33 ms).
- [x] 60b — `workbench_model::emphasis` (`RegionEmphasis`, `EmphasisRequest`, `Follow`,
  `Region` / `RegionKind`, `RegionFocus`, `region_at`), `LanguageSupport::region_emphasis`;
  în app `workbench/emphasis.rs`: zonele layout-ului (barele cu butoane, grupurile
  editorului — cardul întreg cu un singur grup —, panourile laterale și de jos, grupurile
  ferestrei de jos, bara de stare, bara de sus) într-un `Vec` refolosit, doar cât cere cineva;
  efectul pus după bara de sus, înaintea overlay-urilor; completarea și cardul de hover
  păstrate; mișcarea mouse-ului cere un cadru doar când alege altă zonă; cu ceva deschis
  peste fereastră nimic nu se schimbă în spate. `PlatformEvent::CursorLeft` (winit). Butonul
  unei comenzi aprins: `ToolWindowProvider::active`. Decorațiunile pe pauză
  (`ThemeDecorations::paused`: desenate, ceasul oprit, niciun cadru al lor). Teste (un
  contributor de test, nu modulul): nimic fără cerere, tastatura / mouse-ul / amândouă,
  ieșirea mouse-ului, About nedesaturat și înghețat în spate, efectele temei oprite și
  pornite din nou.
- [x] 60c — modulul `focus-mode` (`tools.focus`): `state.rs`, `settings.rs` (`[focus]`
  `strength` / `dim` / `follow`, pagina „Mod concentrare” din Interfață, ordinea 75, înaintea
  lui Relax), `policy.rs` (`FocusEmphasis`: starea și setările → cererea, cu efectele temei pe
  pauză), `button.rs` (`contrast`, ordinea 5, aprins cât e pornit), comanda `focus.toggle`
  fără scurtătură implicită (se alege în Keymap); ro / en. Captura: `EDITOR_SNAPSHOT_COMMAND=
  focus.toggle` (comenzi rulate după pașii ferestrei). Pe CPU, `Emphasis::mask` calculează
  distanța doar lângă marginea zonelor (`Emphasis::reach`: în afara ei totul e estompat la
  fel): ~16 ms pentru un cadru întreg de 2400 × 1400 cu trei zone (renderer-ul software e
  rezerva; cadrele parțiale doar pe zona lor).
