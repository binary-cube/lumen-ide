# Plan 50 — Fundalul de sub overlay-uri: o singură componentă, cu sticlă (blur) opțional

## Context
Fiecare fereastră peste editor își desenează singură fundalul din spate:
- `ds::backdrop` (negru 16%) — dialogurile (`ui-workbench/src/dialog.rs`), modalele
  (`modal.rs`: Settings, proiecte, căutări…), About (`about.rs`), căutarea în proiect
  (`modules/project/indexing/src/search_panel.rs`);
- drawer-ul de meniu (plan 49, `menu_drawer.rs`) are propriul `scrim(palette)`;
- alte ferestre își fac fundalul cu culori scrise pe loc (`BLACK.with_alpha(…)`: de verificat
  `projects_picker.rs`, `splash.rs`, `choice_menu.rs`, `context_menu.rs`, `code_hover.rs`… —
  unele sunt umbre, nu fundaluri: doar fundalurile intră aici).
Motorul de desenare nu are blur: umbrele sunt straturi semitransparente concentrice.

## Ce vrem
1. **O singură componentă** pentru fundalul de sub orice overlay (modal, dialog, drawer, About,
   căutare, Theme Studio dacă are), în `ui-components` (modulele o folosesc la fel ca app-ul),
   cu un stil dat, nu culori scrise de fiecare.
2. **Stil configurabil**: simplu (culoare cu opacitate, ca acum) sau sticlă (blur mic al ferestrei
   din spate + o tentă de culoare peste).
3. **Setări** (Interfață → Aspect, un bloc „Fundalul ferestrelor suprapuse”): Stil = Simplu /
   Sticlă; Intensitate culoare (opacitatea tentei); Blur (mic / mediu, doar pentru Sticlă);
   „Din temă” ca implicit pentru culoare. Previzualizare mică în bloc.
4. **Tema decide implicitul**: tokenuri noi în paletă (`backdrop` culoare + opacitate, `backdrop_blur`),
   cu valori pentru fiecare temă (mai închis pe temele întunecate, mai blând pe cele deschise;
   temele cu efecte — Matrix, cyberpunk — pot cere sticlă). Setarea utilizatorului le suprascrie.
   **Theme Studio** primește controalele lor (culoare, opacitate, blur) într-o secțiune
   „Fundal overlay”, cu previzualizare live, salvate în tema utilizatorului.

## Design
- **Componenta** `ui_components::Backdrop { style, tint, opacity, blur }` rezolvată o dată din
  (temă, setări) într-un `BackdropStyle` ținut de paletă / contextul `ds` (nu calculat per cadru);
  `ds::backdrop(layer, ctx, viewport)` rămâne semnătura (toate apelurile existente merg la fel),
  plus `ds::backdrop_faded(…, shown)` pentru animații (drawer-ul). `scrim()` al drawer-ului
  dispare în ea. Ce era 16% negru rămâne implicitul temelor existente unde nu se cere altceva,
  ca snapshot-urile să nu se schimbe decât unde vrem.
- **Blur-ul** (doar stilul Sticlă), un primitiv nou în `render-api`: `Layer::backdrop_blur(rect,
  radius)` = „ce s-a desenat sub acest strat, încețoșat, în rect”.
  - **wgpu**: la primul cadru cu un strat blur: se copiază ținta de până atunci, se micșorează la
    ¼ (sau ½ pentru blur mic), blur gaussian separabil în 2 treceri (sau dual Kawase, 2–3 nivele)
    într-o textură mică, compusă sub strat cu tenta peste. **Cache**: textura rămâne cât overlay-ul
    e deschis și se refolosește dacă ce e sub el nu s-a schimbat (generația scenei de sub strat);
    se reface doar când fundalul chiar se schimbă (un terminal care scrie, o animație). La închidere
    texturile se eliberează. Pe animația de intrare (drawer, ~160 ms) blur-ul crește odată cu
    opacitatea (aceeași textură, amestecată).
  - **software** (snapshot-uri): același algoritm pe CPU (box blur de 3 ori ≈ gaussian), ca
    snapshot-urile să arate efectul.
  - Fallback: un GPU / backend care nu poate (sau setare „Reduce efectele”) → stilul Simplu.
- **Cost**: zero cât nu e niciun overlay deschis; la deschidere o singură încețoșare la rezoluție
  redusă (țintă < 1 ms pe GPU la 4K); cât stă deschis, nimic în plus dacă fundalul nu se schimbă.
  Măsurat pe macOS și verificat că pe Windows (DX12 / Vulkan prin wgpu) merge la fel.

## Pași (câte un commit)
- **50a** — componenta: `BackdropStyle` în paletă / ds, tokenurile temelor, toate fundalurile
  existente mutate pe ea (modale, dialoguri, About, căutare, drawer); stilul Simplu identic cu ce e
  acum (snapshot-uri identice, cu excepția drawer-ului dacă tema îi dă alt implicit).
- **50b** — blur-ul în motor: `render-api` + wgpu + software, cu cache și eliberare; teste
  (blur-ul software pe o imagine cunoscută, cache-ul refolosit / invalidat).
- **50c** — setările (Interfață → Aspect) și Theme Studio (secțiunea „Fundal overlay”,
  previzualizare live, salvare în temă), locale ro / en.
- **50d** — snapshot-uri (Simplu / Sticlă pe Settings, un dialog, drawer; dark și light; Matrix),
  clippy Windows, măsurători (cost la deschidere, cât stă deschis, memorie).

## Stare
- [x] 50a — `theming::backdrop` (`BackdropKind` simplu / sticlă; blur-ul o rază în px logici,
  0–12, implicit 4 — blând, cum a cerut utilizatorul; `BackdropSpec` cu `over`), secțiunea `[backdrop]` a temelor
  (`style`, `color` — `#…` sau `$nume` —, `opacity` 0–1, `blur` 0–12; o valoare greșită e spusă),
  moștenită prin `extends`. `ui_components::BackdropStyle` (rezolvat o dată în
  `Palette::from_theme`: `palette.backdrop`; `with_user` pentru alegerea utilizatorului peste
  temă, `without_blur` pentru un motor care nu poate) și `BackdropRole` (Dialog / Drawer): ce nu
  spune nimeni rămâne ca acum — negru 16% sub dialoguri, sub drawer opacitatea după luminozitatea
  temei (~52% închis, ~27% deschis); o opacitate dată (temă sau utilizator) se aplică tuturor.
  `ds::backdrop` (semnătura de până acum) și `ds::backdrop_faded(…, rol, shown, gaură)` (drawer-ul,
  butoanele ferestrei pe Windows lăsate libere); `scrim()` / `around()` din drawer au dispărut în
  ele. Modale, dialoguri, About, căutarea în proiect, drawer-ul: toate pe componentă. Fundalul
  „valorii mari” din grila de date rămâne al ei (local, nu peste fereastră). Snapshot-uri identice.
- [x] 50b — blur-ul în motor. `render-api`: `Layer::backdrop_blur(rect, rază, opacitate)` (desenat
  primul în strat: ce au desenat straturile de dedesubt, încețoșat), `BlurPlan` (comun: micșorare
  ×2 / ×4 / ×8 după rază — box filter —, gaussian separabil ~3σ, mărit înapoi biliniar),
  `blur_layers`; `Damage::first_layer()` (cel mai de jos strat schimbat): un blur rămâne valabil
  cât nu s-a schimbat nimic sub el — altfel se reface, dintr-un cadru întreg. Blur-urile intră în
  damage (apar, dispar, își schimbă opacitatea). `Renderer::backdrop_blur()` (wgpu: cât are cadrul
  păstrat; software: da); app-ul dă `without_blur` unui renderer care nu poate (Sticla devine
  Simplu). **wgpu**: pipeline-urile la primul blur (`blur.wgsl`: reduce + gaussian, `blur_composite.wgsl`),
  două texturi mici (la rezoluția redusă, ping-pong) per strat care încețoșează, citind cadrul
  păstrat (acum și `TEXTURE_BINDING`); cadrul se împarte în pase doar când un blur se reface;
  texturile pleacă odată cu blur-ul (și la ascunderea ferestrei); fără cadru păstrat (desen direct,
  ex. în timpul redimensionării pe macOS) nu se încețoșează. **software**: același plan pe CPU
  (snapshot-urile). Teste: imaginea cunoscută (bară albă: margini moi, lumina păstrată, pe jumătate
  arătat între clar și încețoșat), refolosit cât se schimbă doar overlay-ul, refăcut când se schimbă
  ce e sub el, cadre parțiale = întregi, nimic păstrat după — pe CPU și pe GPU (headless, Metal aici).
  Snapshot-uri identice (nimic nu cere încă Sticla).
- [x] 50c — setările și Theme Studio. **Setări → Interfață → Aspect**, blocul „Fundalul ferestrelor
  suprapuse” (`app/src/backdrop.rs`, declarate în `settings.rs`: `[appearance] backdrop` —
  theme / simple / glass —, `backdrop_opacity` și `backdrop_blur`, -1 = din temă): Stilul (Din
  temă / Simplu / Sticlă), Intensitatea culorii (slider: Din temă, 0–60%), Încețoșarea (slider:
  Din temă, 0–12 px; cum a cerut utilizatorul — în loc de mic / mediu). Ce alege trece peste temă
  (`BackdropStyle::with_user`) în paleta fiecărei ferestre (`Workbench::set_palette`); se vede pe
  loc, chiar sub setări, și cât se trage slider-ul (doar fundalul ferestrei acesteia e refăcut; celelalte
  îl iau cu semnalul temei). Previzualizarea e chiar fundalul de sub setări (nu un desen separat).
  **Theme Studio**, în Interfață, secțiunea „Fundal overlay”: culoarea (swatch), stilul (Simplu /
  Sticlă), intensitatea (Automat — a fiecărei ferestre — sau 0–60%), încețoșarea (0–12 px), fiecare
  cu „↺”, găsite de căutare; în tema salvată ca `[backdrop]` (citit înapoi, culoarea și prin `$nume`);
  previzualizarea din dreapta arată un dialog pe fundalul ciornei (încețoșat cu adevărat la Sticlă)
  cât se schimbă (comanda `backdrop:glass` pentru snapshot-uri). Locale ro / en. Teste: setările
  (din temă / ale mele / înapoi, fără blur unde motorul nu poate), studio (rândurile, „↺”, reset,
  previzualizarea), `[backdrop]` compus și citit înapoi.
- [x] 50d — snapshot-uri, teme, măsurători, două reparații. **Blândețea** (cerută de utilizator):
  raza e cât ajunge gaussianul (σ = rază / 3); implicit 4 px logici — ce e în spate rămâne
  recognoscibil, doar înmuiat; 12 px (maximul) e încă ușor de citit ca formă. **Temele**: Matrix și
  Cyberpunk cer Sticla (`[backdrop] style = "glass"`, culoarea `$base`, opacitatea cea obișnuită).
  **Snapshot-uri** noi (dark și light): `glass-settings`, `glass-dialog`, `glass-drawer`,
  `glass-matrix`, `settings-backdrop` (pagina Aspect derulată până la capăt: blocul nou și „Meniu”),
  `studio-backdrop` (secțiunea, cu dialogul previzualizării pe Sticlă); `EDITOR_SNAPSHOT_BACKDROP=
  <stil>[:<blur>[:<tentă>]]` și `EDITOR_SNAPSHOT_SETTINGS_SCROLL=<px>`. **Reparat** (raportat de
  utilizator): pagina Aspect, mai lungă acum, nu se mai derula după ce partea ei fixă ieșea deasupra
  ferestrei — `blocks_bottom` pornea de la 0 (nu de la „nimic”), așa că blocurile se așezau într-un
  loc fix și înălțimea creștea cu fiecare rotire a rotiței (bara se micșora, conținutul stătea, „Meniu”
  nu se vedea); test pe o pagină lungă cu Sticlă: înălțimea aceeași, ultimul comutator vizibil și
  apăsabil, înapoi sus. **Slider-ele „din temă”** stau acum la valoarea temei (16% · din temă, 4 px ·
  din temă); ales acea valoare înseamnă din nou „din temă”. **Memorie**: a doua textură mică pleacă
  după ce blur-ul e făcut (rămâne doar rezultatul). **Măsurat** (release, Apple M-series, Metal, 4K,
  4000 de dreptunghiuri sub el; timp de perete cu așteptarea GPU-ului): un cadru întreg fără blur
  ~1,1 ms; cadrul în care se face blur-ul ~2,0 ms (raza 8 px fizici = implicitul la scala 2) / ~1,7 ms
  (24 px) — adică ~0,4–0,9 ms în plus o singură dată; cât stă deschis (un cursor clipind peste)
  ~0,35–0,4 ms, ca orice cadru parțial; memorie 8 MB cât stă (16 MB cât se face) la 4K cu raza
  implicită, 2 MB la 24 px; nimic după închidere (texturile eliberate; pipeline-urile, câțiva KB,
  rămân). Neîncercat pe un Windows real (DX12 / Vulkan): doar API-uri wgpu portabile, clippy Windows
  verde.
