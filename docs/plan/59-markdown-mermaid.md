# Plan 59 — Markdown: Mermaid, matematică și ce mai randează GitHub

## Context
Cerere: preview-ul Markdown (`crates/modules/viewers/markdown/`: `lib.rs`, `layout.rs`) să
deseneze diagramele Mermaid (blocurile ```` ```mermaid ````) și ce mai suportă de obicei
GitHub / GitLab / editoarele de cod / Obsidian. Fără browser și fără motor JS: totul nativ, cu
renderer-ul nostru, în culorile temei.

## Ce avem azi
Parserul e `pulldown-cmark` 0.13 (fără feature-uri implicite), cu `TABLES`, `STRIKETHROUGH`,
`TASKLISTS`, `FOOTNOTES`. `Layout::build` transformă evenimentele în `Item`-uri (text, rect,
chenar, imagine) + zonele link-urilor; layout-ul e refăcut **în `paint`, pe firul UI**, la
orice schimbare de sursă / lățime / scară / paletă. Imaginile se decodează în fundal
(`image-decode`, `GenerationalCache` de 8).

| Element | Azi |
|---|---|
| Titluri H1–H6 | da (mărimi, H1/H2 cu linie); fără ancore, fără cuprins |
| Paragrafe, bold / italic / cod inline / tăiat / link | da (link-ul relativ se deschide în editor) |
| Liste (puncte, numerotate, imbricate) | da |
| Task list `- [ ]` | doar glife ☐ / ☑ (nu se pot bifa) |
| Citat `>` (imbricat) | da (bară accent, text estompat) |
| Alerte `> [!NOTE]`… | **nu** (apare ca citat cu textul `[!NOTE]`) |
| Bloc de cod | fundal + eticheta limbajului; **fără colorare**; liniile lungi ies din cutie |
| Tabele | da, coloane egale; **alinierea (`:--:`) ignorată** |
| Imagini | doar locale; `http(s)://` și `data:` → `🖼 alt` |
| Note de subsol | activate în parser, dar referința `[^1]` **dispare** și definiția apare ca paragraf oarecare |
| Linie `---` | da |
| HTML | doar `<br>`; restul ignorat (`<details>`, `<kbd>`, `<sup>`, `<img>`…) |
| Autolink-uri GFM (`https://…` simplu în text) | **nu** (doar `<https://…>`) |
| Emoji `:rocket:` | **nu** (emoji Unicode da — atlasul are glife color) |
| Front matter YAML / TOML | **nu** (apare ca text / linie) |
| Matematică `$…$`, `$$…$$` | **nu** |
| Mermaid | **nu** (bloc de cod) |
| Selecție / copiere text | nu |

Mărunțiș de reparat pe drum: `status()` scrie „cuvinte” direct în cod (trebuie în locale).

## Ce randează ceilalți (ținta)
- **GitHub (GFM)**: tabele cu aliniere, task lists, tăiat, autolink-uri extinse, note de
  subsol, alerte (NOTE / TIP / IMPORTANT / WARNING / CAUTION), emoji `:shortcode:`, cod
  colorat, matematică (`$…$`, `$$…$$`, ```` ```math ````), Mermaid, `<details>/<summary>`,
  ancore pe titluri (`#titlu-cu-cratime`), front matter ca tabel, HTML sigur
  (`<kbd>`, `<sup>`, `<sub>`, `<img width>`, `<picture>`, `<p align>`).
- **GitLab**: la fel + `[[_TOC_]]`, PlantUML / Kroki (nu facem).
- **Editoarele de cod**: CommonMark + GFM, cod colorat, front matter ascuns; Mermaid / math prin extensii.
- **Obsidian**: wikilinks `[[pagină]]`, callouts (= alerte, cu mai multe tipuri și pliere
  `[!note]-`), `==evidențiat==`, math, Mermaid, front matter ca proprietăți.

`pulldown-cmark` 0.13 dă deja, cu opțiuni: `ENABLE_GFM` (alertele: `BlockQuoteKind`),
`ENABLE_HEADING_ATTRIBUTES` (`{#id}`), `ENABLE_YAML_STYLE_METADATA_BLOCKS` /
`ENABLE_PLUSES_DELIMITED_METADATA_BLOCKS` (front matter), `ENABLE_MATH`
(`InlineMath` / `DisplayMath`), `ENABLE_DEFINITION_LIST`, `ENABLE_SUPERSCRIPT` /
`ENABLE_SUBSCRIPT`, `ENABLE_WIKILINKS`. Autolink-urile extinse și emoji-urile le facem noi
(o trecere peste `Event::Text`).

## Decizii

### Arhitectura preview-ului (înainte de orice diagramă)
- **Layout-ul iese de pe firul UI pentru blocurile grele.** Documentul e o listă de blocuri
  (paragraf, cod, tabel, diagramă, formulă…), fiecare cu hash-ul sursei lui. Textul obișnuit
  rămâne cum e (rapid); blocurile grele (cod colorat lung, Mermaid, math) au un **cache pe
  bloc** (`kernel::cache::GenerationalCache`, cheia = hash(sursă, tip, scară, culorile
  folosite)) calculat pe un `kernel::Pool` propriu (`markdown-render`, ca `markdown-image`),
  cu waker; până sosesc, locul lor e o cutie cu înălțimea de data trecută (fără salturi) sau
  un schelet. La tastare, doar blocul schimbat se reface (debounce ~150 ms), restul vin din
  cache. „Clear and Rebuild Caches” le golește (`purge`).
- **Desenul formelor**: renderer-ul are doar quad-uri (rotunjite, cu chenar), imagini și
  glife — fără căi. Formele care nu sunt dreptunghiuri (romb, hexagon, cerc, cilindru,
  săgeți oblice / curbe, felii de plăcintă, arce) se rasterizează cu `tiny-skia` (deja în
  workspace, prin `render-software`) într-un **strat de forme** per diagramă, la pixelii
  exacți ai ecranului (scara curentă), pe pool; **textul se desenează nativ peste** (fontul
  UI, clar, temă). Stratul e `ImageData::gpu_only`: pixelii stau doar pe GPU, iar când
  renderer-ul eliberează textura (în afara vederii) se reface din modelul ținut în cache
  (mic: geometrie + etichete). Dreptunghiurile și liniile ortogonale rămân quad-uri.

### Mermaid: parser + layout propriu, desenat de noi (varianta b)
Evaluate:
- **`merman`** (MIT/Apache-2.0, 0.9.3 stabil / 0.10-alpha, ~28 versiuni din feb. 2026, folosit
  deja într-un editor Rust de producție): cel mai complet și mai fidel (ținta e paritatea cu Mermaid 11/12), crate-uri
  separate (`merman-core` doar parser, `merman-render` layout + SVG). Dar: ieșirea e SVG
  (rasterizat de resvg → text în alt font, nu se poate colora din paletă decât prin
  `themeVariables`, se re-rasterizează la fiecare zoom), layout-ul implicit aduce ELK
  (**EPL-2.0**) și math cu fonturi OFL, refuză fonturi date de apelant (nu putem impune
  fontul UI), alpha, arbore mare de dependențe, MSRV ridicat.
- **`mermaid-rs-renderer`** (MIT, 0.3.1, din ian. 2026, 23 de tipuri): rapid, are și un
  „Scene API” (comenzi de desen), dar textul devine contururi din fonturile sistemului (diferă
  de la mașină la mașină), „active early development”, layout-ul nedocumentat ca API; ~71 de
  crate-uri fără feature-urile implicite.
- **`ariel-rs`** (MIT, 0.2): port al Mermaid JS, foarte nou.

**Recomandare: ale noastre** — parser propriu per tip (sintaxa Mermaid e mică pe fiecare tip,
dar neregulată; vrem erorile noastre, cu linia), layout propriu, desen cu `ui-components`.
Motive: (1) aspectul e al aplicației — fontul UI, paleta temei (light / dark / Matrix…),
aceleași carduri, săgeți și etichete ca diagramele PHP (`insight/diagrams`) și schema bazei
(`database/diagram`); (2) clar la orice zoom (textul e text, formele se rasterizează la scară);
(3) fără zeci de crate-uri / licențe EPL / SVG intermediar, buget RAM / GPU mic, nimic
dependent de fonturile sistemului (Windows); (4) „datele limbajului sunt ale noastre”, ca la
stub-uri; (5) avem deja jumătate: `diagram::Viewport` (pan / zoom / fit), `diagram::edge` /
`arrow` / `label_spots` / `place_labels`, `tidy` (Reingold–Tilford, perfect pentru mindmap).
Riscul (multă muncă pentru coada lungă de tipuri) îl limităm: tipurile neacoperite apar ca un
**card „încă nesuportat”** cu sursa colorată, iar pentru fidelitate folosim exemplele din
documentația Mermaid ca teste (snapshot-uri). Dacă vreodată vrem coada lungă (C4, sankey,
xychart…) repede, `merman-core` doar ca parser rămâne o opțiune în spatele aceluiași model.

Unde stă codul:
- Un crate nou de domeniu, pur, `crates/domain/viewers/diagram-text` (`diagram-text`): modelele și parserele
  Mermaid per tip (`flowchart`, `sequence`…) → model tipat + erori `(linie, coloană, mesaj)`.
- `ui-components::diagram::layered` (nou, refolosit și de diagramele PHP / schema):
  layout Sugiyama — cicluri sparte, ranguri (longest path + rafinare), ordonare pe niveluri
  (barycenter, câteva treceri), coordonate (Brandes–Köpf simplificat), muchii ortogonale cu
  benzi, sub-grafuri ca clustere (dreptunghi în jurul rangurilor lor), direcții TD / BT / LR / RL.
- Modulul markdown: blocul ```` ```mermaid ```` → job pe pool (parse + layout + stratul de
  forme) → `Item::Diagram` în layout.

Tipuri, pe prioritate (ce folosesc README-urile și documentația):
1. **flowchart / graph** — TD / LR / BT / RL; forme `[ ]` `( )` `([ ])` `[[ ]]` `[( )]` `(( ))`
   `{ }` `{{ }}` `>]` `[/ /]` `[\ \]`; muchii `-->` `---` `-.->` `==>` `--o` `--x` `<-->`, cu
   lungime (`--->`), etichete (`-->|text|`, `-- text -->`), lanțuri `A --> B --> C`, `&`;
   `subgraph … end` (cu direcție proprie); `classDef` / `class` / `:::` / `style` /
   `linkStyle` (culorile mapate peste paletă); `click` (link); comentarii `%%`; Markdown în
   etichete (bold / italic / `<br>`).
2. **sequenceDiagram** — participant / actor, mesaje (`->>` `-->>` `-x` `-)`), activări,
   note, `loop` / `alt` / `opt` / `par` / `critical` / `break`, `autonumber`, `box`. Layout
   liniar (coloane + rânduri), fără graf.
3. **classDiagram** (refolosește cardurile `diagram::card_*` și relațiile UML ale
   diagramelor PHP), **stateDiagram-v2** (stări compuse, `[*]`, fork / join, note),
   **erDiagram** (carduri cu coloane, cardinalități „crow's foot” în stratul de forme).
4. **gantt** (axă de timp, secțiuni, `done` / `active` / `crit`, dependențe `after`),
   **pie** (felii + legendă), **mindmap** (`tidy`, radial mai târziu), **timeline**,
   **gitGraph** (ramuri ca benzi, commit-uri, merge, cherry-pick, tag-uri).
5. Restul (journey, quadrant, xychart, sankey, C4, block, architecture, kanban, packet,
   radar, treemap, requirement, zenuml): card „Diagramă de tip X — încă nesuportată” + sursa.

Comune: `%%{init: …}%%` / front matter `config` (temă, direcție) acceptate parțial (ce nu
înțelegem e ignorat, nu eroare); `accTitle` / `accDescr` ca tooltip.

### Matematică: layout TeX propriu, cu un font matematic (varianta „subset KaTeX”)
Evaluate: crate-urile care rulează KaTeX / MathJax (`katex`, `mathjax_svg`) aduc un motor JS —
exclus; `typst` (math excelent) e uriaș pentru asta; `ReX` (layout TeX în Rust) e
neîntreținut; `pulldown-latex` (MIT) parsează LaTeX în evenimente tip MathML, fără layout.
**Recomandare**: `pulldown-latex` ca parser (sau unul propriu, dacă iese mic) + **layout
propriu** după regulile TeX (Appendix G, simplificat) folosind tabelul OpenType `MATH`
(citit cu `ttf-parser`, deja în lock) al unui font matematic livrat de modul (**STIX Two
Math** sau **Latin Modern Math**, OFL; încărcat leneș, doar când un document are formule).
Glifele trec prin sistemul nostru de text (atlas → clare, culoarea temei); liniile de
fracție / radicalul sunt quad-uri, delimitatorii mari din variantele / asamblările din
`MATH`. Subsetul: litere grecești și simboluri, `^ _`, `\frac`, `\sqrt[n]`, `\sum \int \prod
\lim` cu limite, `\left \right`, matrice (`matrix`, `pmatrix`, `bmatrix`, `cases`,
`aligned`), accente (`\hat \bar \vec \dot`), `\text`, `\mathbb \mathcal \mathbf`,
spațieri. Ce nu știm: sursa în roșu-estompat, cu tooltip-ul erorii (ca GitHub).
Sintaxe: `$…$`, `$$…$$`, ```` ```math ````, `$`…`$` (GitLab).

### Interacțiuni
- **Click pe diagramă** → se deschide mare într-un overlay modal cu ramă (`host.open_modal`,
  închis doar din ×, Esc, butoane): `diagram::Viewport` (pan, zoom cu pinch / ⌘-scroll, `0` =
  fit, dublu-click = zoom), stratul de forme refăcut la scara zoom-ului (debounce; între timp
  cel vechi scalat). Pe hover în preview: butoane mici în colț (Mărește, Copiază).
- **Copiere**: ca PNG (rasterizare completă cu `tiny-skia` + textul nostru, la 2×; nou
  `SystemHost::set_clipboard_image`, arboard cu `image-data`), ca SVG (scris de noi din
  model — geometria o avem; textul ca `<text>`), sursa Mermaid; „Salvează ca…” PNG / SVG.
  Același meniu de click dreapta pe formule (Copiază TeX / PNG).
- **Teme**: culorile vin din `Palette` (accent, border, text, `terminal_ansi` pentru seriile
  pie / gantt / gitGraph); o temă nouă invalidează doar cheia de culoare → refacem straturile.
- **Erori inline**: cardul diagramei arată „Eroare la linia N: …” cu linia sursei
  subliniată; click pe ea → editorul la linia aceea (`ViewerAction::OpenAt`, în Split).
- **Performanță**: parse + layout + rasterizare pe pool; cache pe hash de bloc; straturile
  `gpu_only`, eliberate în afara vederii; limite (ex. > 500 noduri: cardul „prea mare”, cu
  sursa); nimic per cadru în afară de quad-uri / glife vizibile. Windows: fără fonturi de
  sistem în calcul, căi doar prin `Path`.
- Texte (titluri, butoane, erori) în `crates/domain/core/i18n/locales/{ro,en}.toml`.

## Pași (fiecare un commit)
- **59a — golurile GFM.** `ENABLE_GFM` + atribute titluri + front matter + definition list +
  super / subscript; alerte (5 tipuri GitHub, cu icoană + culoare; aliasurile Obsidian
  `[!tip]`, `[!warning]-` pliabil); note de subsol (referința ca exponent-link, definițiile
  la final cu ↩, click = scroll); tabele aliniate și coloane după conținut (min / max);
  autolink-uri extinse; emoji `:shortcode:` (tabel generat din gemoji, MIT, prin
  `cargo xtask web-data`); front matter ca tabel mic (sau ascuns — setare a modulului);
  ancore pe titluri (slug GitHub) + link-uri `#…` care derulează + cuprins (`[[_TOC_]]`,
  `[TOC]`) + titlurile în `crumbs`; task list-uri bifabile (click → `SetText` pe linia
  sursei); `<details>/<summary>` pliabil, `<kbd>`, `<sup>/<sub>`, `<img src width>`,
  `<p align="center">`, comentarii HTML ascunse; `==evidențiat==`; wikilinks; imagini
  remote (opțional, setare, pe pool, cu limită de mărime). Cod: colorat lexical acum
  (`ui_components::code_tokens::runs`, după extensia limbajului din fence), linii lungi
  tăiate cu scroll orizontal pe bloc sau wrap (setare), buton Copiază; `status()` în locale.
- **59a′ — cod colorat cu tree-sitter.** Un port `SnippetHighlighter` (workbench-model:
  limbaj + text → spans de culoare), implementat de app peste `syntax-treesitter` (gramatica
  limbajului încărcată leneș), rulat pe pool-ul preview-ului, cache pe hash; fallback lexical.
  Mutarea layout-ului blocurilor grele pe pool (arhitectura de mai sus) intră aici.
- **59b — Mermaid: fundația + flowchart + sequence.** Crate-ul de modele / parsere,
  `diagram::layered`, stratul de forme `tiny-skia`, `Item::Diagram` cu cache pe bloc, cardul
  de eroare și cel „încă nesuportat”, overlay-ul mărit, copiere PNG / SVG / sursă. Teste:
  parsere pe exemplele din documentația Mermaid, layout fără suprapuneri, snapshot-uri.
- **59c — class, state, ER.**
- **59d — gantt, pie, mindmap, timeline, gitGraph.**
- **59e — matematică**: parser, layout TeX cu tabelul `MATH`, fontul livrat leneș, inline
  (aliniat la linia de bază a textului) și display (centrat), erori inline, copiere TeX / PNG.
- **59f (opțional) — coada lungă**: journey, quadrant, xychart, sankey, block, kanban, după cerere.

## Continuare (59g…)
Cerere după 59e: punctele rămase, fiecare un commit.
- **59g — imagini și diagrame optimizate, aceeași calitate.** Măsurat întâi (un document cu 20 de diagrame și 20 de fotografii mari, derulat): RAM, GPU, timp. Apoi: formele diagramelor rasterizate ca **măști alfa** (R8, 1 octet / pixel) pe grupuri de culoare, în **plăci** (doar cele cu ceva pe ele, doar cele vizibile, la nevoie, refolosite la derulare), în loc de o imagine RGBA întreagă; un **buget global** de texturi pentru preview-uri (48 MB GPU pentru toate, LRU, eliberate în afara vederii — `ImageData::release`); imaginile **decodate la mărimea afișată** (rotunjită la 256 px, nu la 4096); buffere refolosite; nimic rasterizat din nou la derulare.
- **59h — fontul matematic livrat**: STIX Two Math (OFL 1.1), comprimat (deflate) în binar, decomprimat la prima formulă într-un sistem de text al modulului (doar fontul acesta: aceleași formule pe orice sistem), eliberat odată cu ultimul preview cu formule; glifele formulelor rasterizate împreună cu barele lor (măști). Licența OFL în repo, lângă font.
- **59i — imagini de pe web** (`http(s)://`): descărcate în fundal, păstrate pe disc în cache-ul aplicației (limită de mărime, LRU, golit de „Clear and Rebuild Caches”), cu limite (mărime, timp, redirecționări, doar tipuri de imagine, SVG fără referințe externe), placeholder cât se încarcă, card de eroare; setarea „Încarcă imaginile de pe web în Markdown” (implicit da); fără cookie-uri / credențiale, un User-Agent simplu.
- **59j — liniile lungi din cod derulează orizontal** în blocul lor (rotița cu Shift / trackpad, bară orizontală subțire care se ascunde); setarea „Împachetează liniile lungi din blocurile de cod” (implicit nu).
- **59k — mindmap radial** (aspectul implicit Mermaid): rădăcina la mijloc, ramurile în jur după greutatea subarborilor, legături curbe, niveluri colorate; stânga / dreapta rămâne pentru hărțile foarte late, ales automat.
- **59l — tipurile Mermaid rămase (fost 59f)**: journey, sankey, xychart (bare / linii), quadrantChart, requirementDiagram, C4 (Context / Container / Component), block, packet, kanban, architecture, radar, treemap — parser în `diagram-text`, așezare nativă, temă, teste pe exemplele din documentația Mermaid.
- **59m — selectarea textului în preview**, ca într-o pagină web: tras peste blocuri în ordinea documentului, dublu click pe un cuvânt, triplu click pe paragraf / rând, Shift + click extinde, ⌘/Ctrl + A prin Select All-ul aplicației (și în Split, după un click în preview), ⌘/Ctrl + C copiază text simplu (rânduri și paragrafe, celulele tabelelor cu tab, marcajele listelor, codul exact) și „Copiază ca Markdown”; culoarea de selecție a paletei; derulare singură lângă margini cât se trage; meniu de click dreapta Copiază / Selectează tot / Copiază ca Markdown; click fără mișcare pe un link îl urmează, tras de pe un link selectează; diagramele și formulele selectate întregi (copiate ca sursă); hit-testing pe glifele așezate deja, nimic pe cadru când nu se întâmplă nimic.
- **59n — bara de derulare a preview-ului interactivă**: `EdgeScrollbar`-ul comun (ascunsă în repaus, ca toate listele): degetul se trage, un click pe pistă sare acolo, se lărgește la hover; culoarul ei ia apăsarea înaintea selecției de text.
- **59o — derulare sincronizată în Split, în ambele sensuri**: textul derulat mută preview-ul la același loc și invers; caretul mutat aduce preview-ul acolo doar dacă e în afara vederii. Harta rânduri ↔ înălțime din blocurile așezate (interpolare în bloc, imaginile / diagramele proporțional), fără bucle (partea derulată de utilizator conduce, cealaltă urmează fără ecou, cu un scurt „mandat” al celui care conduce), ieftin (fără relayout, căutare binară). Setarea „Sincronizează derularea textului cu preview-ul” (implicit da).

## Stare
- [x] 59a — golurile GFM: alerte (cele 5 GitHub, cu icoană și culoare din temă) și callout-urile Obsidian (`[!info]- Titlu`, pliabile); note de subsol numerotate după prima referință, adunate la final cu ↩; tabele cu coloane după conținut și aliniere; autolink-uri (`https://`, `www.`); emoji `:shortcode:` (`data/emoji.data`, 1913 coduri din gemoji, MIT, prin `cargo xtask emoji-data`); front matter ca tabel; ancore pe titluri (slug GitHub, `{#id}`), link-uri `#…` care derulează, cuprins `[TOC]` / `[[_TOC_]]`; task list-uri bifabile (click → textul fișierului); liste de definiții; `==evidențiat==`; wikilinks `[[pagină]]`; HTML: `<details>/<summary>` pliabil, `<kbd>`, `<sup>/<sub>`, `<b>/<i>/<s>/<u>/<mark>/<code>`, `<img width/height>` (și în rând), `<p/div/h1… align>`, `<a href/name>`, `<br>`, `<hr>`, liste, comentarii ascunse, entități. Cod: colorat lexical, liniile lungi împachetate (nu scroll orizontal), buton Copiază la hover. Numărul de cuvinte în locale. Rămas: imaginile remote (http) nu se descarcă.
- [x] 59a′ — cod colorat cu tree-sitter: portul `ui_components::snippets::SnippetHighlighter` (extensia + codul → capturile), implementat de app (`app/src/snippets.rs`) cu gramaticile și interogările editorului (PHP fără `<?php` primește unul în față); capturile colorate cu paleta (`SyntaxColors::capture`: keyword, string, type, constant, property, tag, attribute, operator, punctuation…). Rulat pe pool-ul modulului (`markdown-render`, `jobs::Jobs`: cache pe hash-ul blocului, golit cu „Clear and Rebuild Caches”, ce nu mai folosește documentul pleacă); până sosește, colorarea lexicală. Același mecanism îl folosesc diagramele și formulele.
- [x] 59b — Mermaid: crate-ul pur `diagram-text` (parserele, erorile cu linia, front matter `title`, `accTitle` / `accDescr`, stiluri și culori CSS); `ui_components::diagram::layered` (Sugiyama: cicluri, niveluri, noduri ascunse pe muchiile lungi și pentru etichete, ordine prin mediane cu cele mai puține încrucișări, poziții prin regresie izotonă); în modul: `Scene` (căi + texte) rasterizată cu `tiny-skia` (BSD-3, deja în workspace) la scara exactă, textele cu fontul UI peste; job pe pool cu cache pe hash (diagrama anterioară rămâne cât se calculează cea nouă; imaginea `gpu_only` refăcută dacă GPU-ul a eliberat-o; altă rasterizare când e îngustată). **flowchart / graph**: toate formele, legăturile (linie, punctat, gros, invizibil, capete săgeată / cerc / x la unul sau ambele capete, lungimi, etichete `|x|` și `-- x -->`), lanțuri și `&`, subgraph-uri imbricate (fiecare așezat separat, cu direcția lui, apoi ca o cutie în părinte), `classDef` / `class` / `:::` / `style` / `linkStyle`, `click` (link). **sequenceDiagram**: participanți / actori, alias-uri, `box`, mesaje (`->`, `-->`, `->>`, `-->>`, `-x`, `-)`, `<<->>`), activări (`+` / `-`, `activate`), note, `loop` / `alt` / `else` / `opt` / `par` / `and` / `critical` / `break` / `rect`, `autonumber`, mesaje către sine. Erori: card cu mesajul (locale) și sursa cu linia marcată (click → editorul la linia aceea); tipurile nedesenate: card „încă nu sunt desenate”. Click / butonul din colț → modalul modulului (`view-diagram`: pan, zoom, `0` încadrează, `1` = 100 %, rasterizare din nou la zoom), meniul de click dreapta (și în preview): copiază ca imagine (`AppRequest::CopyImage` → `Window::set_clipboard_image`, arboard cu `image-data`), ca SVG, sursa; salvează PNG / SVG. 200 de noduri + 267 de muchii: ~70 ms (layout + rasterizare).
- [x] 59c — un motor comun pentru diagramele „cutii și linii” (`mermaid/graph.rs`: grupuri așezate recursiv, linii, capete, etichete și etichete la capete); flowchart-ul trecut pe el. **classDiagram**: clase (generice `~T~`, etichete, `<<adnotări>>`), membri în corp sau `Clasă : membru` (`$` subliniat, `*` cursiv), relații (moștenire, compoziție, agregare, asociere, dependență, realizare, legături, ambele capete, cardinalități, etichete), note (`note for`), `namespace` ca grupuri, `direction`, `classDef` / `cssClass` / `style`, `click`. **stateDiagram(-v2)**: stări (descrieri, `state "…" as`), `[*]` început / sfârșit pe fiecare compusă, tranziții cu etichete, stări compuse imbricate cu regiuni concurente (`--`), fork / join / choice, note (pe un rând sau până la `end note`), stiluri. **erDiagram**: entități (alias), atribute (tip, nume, PK / FK / UK, comentariu) în coloane, relații cu „crow's foot” la ambele capete, identificatoare (linie plină) sau nu (punctată), etichete.
- [x] 59d — **pie** (felii din seriile temei, procente, legendă, `showData`); **gantt** (`dateFormat` cu YYYY / MM / DD / HH / mm / ss / x, `axisFormat` strftime, `excludes weekends`, secțiuni ca benzi, sarcini `done` / `active` / `crit` / `milestone`, `after a b`, `until id`, durate ms…y, axă cu ~8 diviziuni rotunde); **mindmap** (arbore după indentare, rădăcina la mijloc, ramurile la dreapta și la stânga cu `tidy`, culoare pe ramură, linii tot mai subțiri, formele `[ ]` `( )` `(( ))` `)) ((` `) (` `{{ }}`, texte lungi împachetate); **timeline** (perioade pe un rând, evenimente sub ele, secțiuni deasupra, axa timpului); **gitGraph** (culoar pe ramură după `order`, commit-uri în ordinea scrierii, `id` / `tag` / `type`, branch / checkout / switch / merge / cherry-pick, `LR:` / `TB:`). Titlul (front matter / `title`) desenat deasupra oricărei diagrame.
- [x] 59e — matematică: crate-ul pur `tex` (parser propriu, nu `pulldown-latex`: atomi cu clasa TeX și alfabetul, scripturi, `\frac` / `\dfrac` / `\binom`, `\sqrt[n]`, operatori mari și funcții cu limite, `\left … \right`, `matrix` / `pmatrix` / `bmatrix` / `Bmatrix` / `vmatrix` / `Vmatrix` / `cases` / `aligned` / `array`, accente, `\text`, `\mathbb` / `\mathcal` / `\mathbf` / `\mathfrak`…, `\color`, spații; comenzile necunoscute rămân, în roșu; acoladele / mediile neînchise = eroare). Așezarea în modul (`math.rs`): cutii TeX simplificate (spațiere pe clase, axa matematică, scripturi, fracții, radicali, limite deasupra / dedesubt în display, delimitatori și matrice crescute după conținut), simbolurile ca glife ale **fontului matematic al sistemului** (STIX Two Math pe macOS, Cambria Math pe Windows; altfel un serif) cu literele în Mathematical Alphanumeric Symbols, barele / radicalii / delimitatorii ca trasee (`tiny-skia`). Abatere de la plan: niciun font livrat (nu crește binarul); unde sistemul n-are font matematic, un serif cu italice. `$…$` în rând (pe linia de bază, rândul crește după formulă), `$$…$$` și ```` ```math ```` centrate; formulele cache-uite pe text / mărime / culori; una greșită apare ca sursă, în roșu.
- [x] 59f — tipurile Mermaid rămase (făcut în 59l)
- [x] 59g — imagini și diagrame optimizate. Măsurat (test `measure_a_heavy_document`, release, 20 de diagrame + fotografii 4000×3000, ecran 1600×1800 la 2×, renderer-ul simulat): **înainte**, cu 6 fotografii: 299 MB pe GPU (fiecare fotografie 48 MB: decodată la 4000×3000), 240 MB RAM la vârf, 80 ms primul ecran; cu 12 sau 20 de fotografii documentul **nu se mai așeza niciodată** (cache-ul de 8 imagini le decoda din nou pe rând, la nesfârșit: 144 GB urcați pe GPU în 2 minute). **După**: 6 fotografii — 44 MB pe GPU în total pentru tot documentul derulat, 7 MB RAM la vârf, 96 ms primul ecran; 20 de fotografii — 148 MB urcați pe parcursul derulării (cel mult 48 MB ținuți cât ecranele trec mai încet decât în test), 7 MB RAM, 98 ms primul ecran. Diagramele: măști pe culori (R8) în plăci de 512 px, tăiate în celule de 128 px doar unde e culoarea — 13–25 % din octeții unei imagini RGBA (flowchart 18 %, sequence 13 %, class 25 %, pie 21 %), aceeași imagine (compunerea măștilor e exact pictura, testată). Ce s-a schimbat: `ImageData::mask` (R8 + culoare, în wgpu și în renderer-ul software, testat pe GPU) și `ImageData::release`; bugetul global (`budget.rs`: 48 MB pentru toate preview-urile, LRU, ce s-a văzut în ultima jumătate de secundă nu pleacă niciodată); plăcile doar pentru ce e la un ecran de vedere, făcute pe pool, păstrate la derulare; imaginile: layout-ul citește doar mărimea (antetul), pixelii se decodează la un ecran distanță, la lățimea afișată rotunjită la 256 px (`image_decode::load_for_width`; SVG la mărimea lui × scara); buffere refolosite la rasterizare; formulele tot ca măști. Modalul diagramelor desenează și el plăci (pentru zoom-ul rotunjit la o optime, cele vechi întinse până sosesc).
- [x] 59h — fontul matematic livrat: **STIX Two Math 2.13** (SIL OFL 1.1; ales față de Libertinus Math pentru calitate — e referința după care au fost potrivite formulele; Latin Modern Math are licența GUST, nu OFL), 839 KB, comprimat **xz** la **552 KB** în binar (`data/STIXTwoMath-Regular.otf.xz`, licența lângă el: `data/STIXTwoMath-OFL.txt`; decodorul xz era deja în binar, prin descărcările serverelor de limbaj). Decomprimat o dată, la prima formulă, într-un sistem de text al modulului care are **doar** fontul acesta (`TextSystem::with_fonts`): aceleași formule pe orice sistem, fontul sistemului nu mai e folosit. Ținut cât un preview are formule (`Caches::math_text`) și eliberat cu ultimul. Formulele se așază pe pool (sursa lor, estompată, cât se lucrează); glifele, barele, radicalii și delimitatorii se rasterizează împreună într-o mască pe culoare. Aplicația nu are o pagină de licențe terțe: licența stă în repo, lângă font.
- [x] 59i — imagini de pe web: crate-ul nou `web-fetch` (infra; `ureq` deja în workspace): doar `http(s)://` fără `user:parolă@`, fără cookie-uri (ureq fără `cookies`), User-Agent simplu, 20 s, 5 redirecționări, doar răspunsuri `image/…`, cel mult 20 MB (și după `content-length`, și numărând); `DiskCache` în cache-ul aplicației (`web-images`, 200 MB, cele mai puțin folosite pleacă, scris prin `.part` + rename). În modul (`web.rs`): pool propriu `markdown-web`, aceeași adresă cerută de două preview-uri se descarcă o dată; cât se descarcă, numele imaginii; la eșec, numele cu motivul („HTTP 404”, „adresă nepermisă”, „oprite în setări”); un SVG descărcat se desenează fără să încarce ceva al lui (resvg fără rezolvarea `href`-urilor). „Clear and Rebuild Caches” golește folderul (`Module::release_idle` cu durata zero). Setarea `[markdown] web_images` (implicit da) pe pagina nouă „Markdown” (Editare); oprită, nu se cere nimic. Teste cu un server local (niciodată rețeaua, niciodată folderul utilizatorului: `places::isolate`).
- [x] 59j — liniile lungi din blocurile de cod nu mai sunt împachetate: blocul mai lat decât pagina își derulează textul pe orizontală, tăiat în rama lui (`Item::Scroll`, un strat cu clip; derularea fiecărui bloc ținută în vedere, fără re-layout), cu mișcarea mai mult orizontală a trackpad-ului / rotiței (Shift + rotiță pe macOS); bara orizontală subțire (componenta comună `Scrollbar`) apare doar cu mouse-ul peste bloc, se lărgește la hover și se trage; limbajul blocului stă deasupra textului derulat. Setarea `[markdown] wrap_code` (implicit nu) le împachetează ca înainte. Rămas: pe Windows Shift + rotița verticală nu e tradusă în orizontal de aplicație (evenimentul vine vertical).
- [x] 59k — mindmap radial: rădăcina la mijloc, fiecare nivel un inel, fiecare ramură un sector cât frunzele ei, inelul atât de departe cât le trebuie cutiilor să încapă una lângă alta pe el (întinderea fiecăreia după unghi), legături curbe, ramurile colorate, liniile tot mai subțiri; stânga / dreapta (`tidy`) rămâne și e aleasă singură când radialul ar lua de peste 1,6 ori suprafața (hărți late, cu multe niveluri și texte lungi).
- [x] 59l — tipurile Mermaid rămase, fiecare cu parserul lui în `diagram-text` și așezarea nativă în modul, în culorile temei, testate pe exemplele din documentația Mermaid: **journey** (secțiuni ca benzi, sarcini în carduri cu actorii ca puncte, scorul ca față — zâmbet / neutru / încruntat — mai sus cu cât e mai bun, legenda actorilor, axa timpului); **sankey** (`sankey-beta`, CSV cu ghilimele; coloane după cel mai lung drum de la surse, chiuvetele în ultima, ordinea în coloană după media surselor, panglici curbe cât valoarea, etichete cu valoarea); **xychart** (`xychart-beta`, categorii sau interval pe x, interval pe y sau ales după valori, bare alăturate pe serii, linii cu puncte, grilă cu diviziuni rotunde, titlurile axelor, `horizontal`); **quadrantChart** (cele patru cadrane nuanțate, numele lor, capetele axelor, punctele `[x, y]` cu numele); **requirementDiagram** (cerințele cu «tipul», titlul și câmpurile, elementele, relațiile «satisfies» / «contains»… punctate); **C4** (`C4Context` / `C4Container` / `C4Component` / `C4Dynamic` / `C4Deployment`: persoane cu siluetă, sisteme, containere, componente în culorile notației C4 — externe gri —, bazele de date ca cilindri, `[Container: tehnologie]`, granițele punctate cu tipul lor, `Rel` / `BiRel` / `Rel_Back` cu eticheta și tehnologia); **architecture** (`architecture-beta`: grupuri imbricate, servicii cu icoane desenate — database, disk, internet, cloud, server —, joncțiuni ca puncte, muchii cu săgeți la unul sau ambele capete); **block** (`block-beta`: grila `columns`, `a:2` pe mai multe coloane, `space`, blocuri imbricate cu coloanele lor, formele flowchart-ului, săgeți cu etichete); **packet** (`packet-beta`: 32 de biți pe rând, câmpurile tăiate între rânduri, numerele biților la capete, textul micșorat / împachetat în câmpurile înguste); **kanban** (coloane și carduri, `@{ ticket, assigned, priority }` — prioritatea ca dungă colorată); **radar** (`radar-beta`: axe, grilă poligonală, curbe umplute, `max` / `min`, legendă); **treemap** (`treemap-beta`: dreptunghiuri „squarified” cât valorile, secțiunile imbricate cu titlul și totalul). Graful comun (`graph.rs`) așază cerințele, C4 și architecture; restul au așezarea lor (`plots.rs`, `cards.rs`). Card „încă nu sunt desenate” doar pentru ce a rămas necunoscut (`zenuml`).
- [x] 59m — selectarea textului în preview: layout-ul păstrează textul ca „run”-uri în ordinea documentului (`select.rs`: un cuvânt, o bucată dintr-un rând de cod, marcajul unei liste, o formulă, o diagramă), fiecare cu cutia rândului, glifele deja formate (`ShapedLine`, din cache-ul sistemului de text), cum se leagă de cel dinainte la copiere (nimic / rând nou / rând gol / tab) și blocul lui; selecția e o pereche de offset-uri în textul lor, deci rămâne cât se reface layout-ul aceluiași text (o imagine sosită, altă lățime) și pleacă la un text nou. Tras (de oriunde, și de pe un link sau o diagramă: urmate doar la eliberare fără mișcare de 4 px), dublu click (cuvântul, și peste stiluri: `**bold**er`), triplu click (paragraful, celula, rândul de cod), tras după dublu / triplu click crește pe cuvinte / blocuri, Shift + click extinde; derulare singură lângă marginea de sus / jos cât se trage (viteza după cât de aproape / dincolo e mouse-ul; cadre doar atunci — `is_animating`). Desenată cu `palette.selection` sub tot ce acoperă (textul, glifele formulelor, formele diagramelor), în stratul tăiat al blocurilor de cod care derulează (mutată cu ele). Copiere: textul simplu (rânduri înfășurate unite, `\n` la rânduri noi și între itemi, `\n\n` între blocuri, tab între celule, marcajele `•` / `1.` / `[x]` cu indentarea listei, codul exact cu rândurile goale, formulele `$…$` / `$$…$$`, diagramele ca sursa lor, fără spații la capăt de rând) și „Copiază ca Markdown” (rândurile întregi din sursă ale blocurilor atinse). ⌘/Ctrl + A și ⌘/Ctrl + C prin comenzile aplicației: în Preview direct (`select_all` / `selected_text`); în Split, metoda nouă `Viewer::selects_in_split` — după un click în preview, Select All și Copy merg la el, celelalte taste rămân editorului. Meniul de click dreapta: Copiază, Copiază ca Markdown (cu ceva selectat), Selectează tot; peste o diagramă neselectată, meniul diagramei. Cursorul I-beam peste text. Cost: un run pe cuvânt (cutia, textul, un `Arc` la glifele deja formate); hit-testing liniar doar la apăsare / tragere; cutiile selecției doar pentru run-urile din selecție aflate în vedere, la desenare.
- [x] 59n — bara de derulare a preview-ului: `EdgeScrollbar::auto_hiding` în locul indicatorului desenat; apare la derulare sau când mouse-ul vine pe culoarul ei, se lărgește la hover, degetul se trage, un click pe pistă sare acolo (ca la celelalte liste); apăsarea pe culoar e a ei înaintea selecției de text; cadre doar cât se estompează (`frame_deadline`).
- [x] 59o — derulare sincronizată în Split: harta `sync::LineMap` făcută cu layout-ul (un punct unde începe textul fiecărui bloc — primul lui rând din sursă la vârful primului run —, plus începutul și sfârșitul documentului; între puncte rândurile împărțite după înălțime: blocurile lungi derulează lin, imaginile / diagramele mai înalte decât rândurile lor le iau proporțional; niciodată înapoi — o notă de subsol de la final nu trage harta), căutată binar în ambele sensuri. Metode noi cu implicit în `Viewer`: `source_line_at_top`, `scroll_to_source`, `reveal_source_line`; în editor `EditorView::top_line` / `scroll_to_line`. Aplicația (`preview_split.rs`, `ScrollSync`): înaintea fiecărui cadru compară vârfurile (în rânduri de sursă) cu cele văzute după cel dinainte; partea mișcată de utilizator conduce 300 ms (`LEASE`), cealaltă o urmează, iar urmarea, văzută după cadru, nu se întoarce ca ecou; caretul mutat fără derulare aduce preview-ul la rândul lui doar dacă e în afara vederii; la prima afișare în Split, preview-ul vine unde e textul. Preview-ul își ține rândul de sus când e refăcut (o imagine sosită, altă lățime). Setarea `[markdown] sync_scroll` (implicit da, pagina Markdown); oprită, preview-ul nu spune și nu urmează nimic.
