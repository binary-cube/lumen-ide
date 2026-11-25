# 99 — Un singur motor web peste tot (Chromium, prin CEF), desenat de noi

Cerut 2026-11-24: „prefer să avem o soluție generală peste tot și să fie […] parte din ce
desenăm, ca să nu mai avem tot soiul de probleme”; motorul descărcat o singură dată, la prima
nevoie. **Hotărât (2026-11-24): Chromium comun pe toate sistemele.** Înlocuiește, ca
direcție, planul 98 (WebView2 doar pe Windows), care rămâne rezervă (vezi „Rezerva”).

**Unde suntem:** pașii 99a–99h sunt făcuți și probați pe macOS. Windows și Linux sunt scrise
și verificate la compilare, de încercat pe mașinile lor; workflow-ul de publicare
(`web-engine.yml`) n-a rulat încă. Ce rămâne e strâns la „Lăsat”, la sfârșit.

## Ce vrem

- **O pagină web (draw.io, răspunsul HTML al clientului HTTP, previzualizările) e o textură
  pe care o desenăm noi**, în scena noastră, ca orice imagine:
  - orice se poate pune deasupra sau dedesubt (tooltip-uri, meniuri, notificări, umbre,
    sticlă, colțuri, animații);
  - nicio fereastră a sistemului peste fereastra noastră, deci nici găuri, nici straturi
    plutitoare, nici ferestre popup ținute lipite.
- **Același motor și același comportament pe Windows, macOS și Linux.** Pe Linux apar astfel
  și paginile încorporate (fără motor, se deschid în browser).
- **Motorul nu vine cu aplicația:** se descarcă o singură dată, cu acordul utilizatorului,
  când e nevoie prima oară de o pagină. Aplicația rămâne mică pentru cine nu folosește
  pagini.

## Hotărâri

### Chromium Embedded Framework (CEF), desenat în afara ecranului

CEF e Chromium făcut să fie încorporat în alte programe: aplicații desktop care își desenează
singure interfața îl folosesc ca să arate pagini web în ea, fără fereastra unui browser.

- **Modul „off-screen rendering” (OSR) cu texturi partajate**: Chromium desenează pagina pe
  GPU și ne dă textura fără copie prin procesor (`OnAcceleratedPaint`).
- **Noi îi dăm tot ce intră**: mouse-ul (`SendMouseMoveEvent`, `SendMouseClickEvent`,
  `SendMouseWheelEvent`), tastatura (`SendKeyEvent`), IME-ul (`ImeSetComposition`,
  `ImeCommitText`), focusul.
- **El ne dă înapoi**: cursorul (`OnCursorChange`), listele derulante ale paginii
  (`OnPopupShow` / `OnPopupSize`: un al doilea strat, desenat de noi peste pagină), caretul
  pentru IME, accesibilitatea (`OnAccessibilityTreeChange`).
- **Pentru Rust**: `cef` / `cef-dll-sys` (tauri-apps/cef-rs), care urmează versiunile
  Chromium și merg pe toate trei sistemele (x86_64, ARM64). Legăturile lor expun ce ne
  trebuie din OSR; API-ul C al CEF e stabil, deci ce ar lipsi se poate completa peste
  `cef-dll-sys`.

### Motorul într-un proces separat, al nostru

Regula proiectului: există doar cât e folosit. CEF nu se poate opri și reporni în același
proces (`CefInitialize` o singură dată, `CefShutdown` la ieșire), deci motorul nu stă în
procesul aplicației. Un crash al lui Chromium nu ia aplicația cu el: pagina e refăcută.

### Cerute după 99a (2026-11-24)

Cerut: varianta cu porturi Mach pe macOS, „super optim totul, bine scris și organizat”,
consumul lui Chromium redus, „dar nici să oprim și să pornim procese la fiecare defocusare de
taburi”, trecerea între procese bună și pe Windows, și „foarte foarte important să fie shared:
să nu mă trezesc că am 2 proiecte deschise și ambele pornesc în spate Chromium”.

- **Un singur motor, împărțit** de toate ferestrele, proiectele și instanțele aplicației.
- **Fără porniri și opriri la fiecare tab**: un tab ascuns își păstrează pagina.
- **Pe macOS, porturi Mach** pentru texturi (nu suprafețe „globale”, vezi „Măsurători”).
- **Pe Windows, handle-uri duplicate** în procesul aplicației.

### Versiunile motorului și curățenia (cerut 2026-11-24)

Cerut: „să avem control să incrementăm versiunea […] dacă au probleme și fac fixuri la
Chromium, userul să beneficieze”, iar actualizarea să fie „curată și controlată: se curăță
ce e vechi și nu mai e nevoie”.

## Cum e făcut

### Arhitectura

```
aplicația (procesul nostru)                    web-host (procesul nostru, cu CEF)
──────────────────────────                     ────────────────────────────────────
WebView (contractul platform_api) ── comenzi ─▶ un browser CEF pe pagină (OSR)
  set_bounds / load / send                       mouse, taste, focus, mesaje
  intrările (mouse, taste)
                              ◀── cadre ──      textura partajată (handle / port Mach / pixeli)
textura desenată în scenă                        cursor, popup-uri, caret, accesibilitate
                              ◀── mesaje ──     mesajele paginii (WebInbox)
```

- **`web-host`** e un program mic al nostru, care leagă CEF. E pornit la prima pagină și se
  oprește singur după ce n-a mai avut nicio pagină o vreme; odată cu el pleacă Chromium
  întreg (procesele lui de randare și GPU).
- **Un singur `web-host` pe utilizator și pe versiunea protocolului**, oricâte ferestre,
  proiecte sau instanțe ale aplicației ar fi:
  - ascultă pe un canal local cu nume fix, `editor-web-host-<protocol>`: pe Windows un pipe
    al utilizatorului, în rest un socket în dosarul lui privat, ca instanța aplicației
    (`instance.rs`). Canalul e ținut cu un lacăt de motor; un al doilea `web-host` nu-l poate
    lua, deci iese, iar aplicația se leagă la primul. Un socket rămas de la un motor căzut e
    preluat;
  - aplicația încearcă întâi să se lege la el și îl pornește doar dacă nu răspunde nimeni;
  - în aplicație, o singură legătură pe proces, împărțită de toate ferestrele și proiectele
    (`web-engine`, ca `kernel::work`), refăcută dacă motorul a plecat; fiecare pagină e un
    „browser” CEF al legăturii;
  - o legătură care se rupe (aplicația închisă sau căzută): `web-host` își închide paginile
    ei, iar ale celorlalte aplicații rămân.
- **Legătura**: mesaje mici, serializate (`postcard`), încadrate cu lungime. Cadrele trec ca
  handle-uri, nu ca pixeli (în afară de calea prin pixeli, mai jos).
- **Celălalt capăt e verificat**: procesul declarat în salut și utilizatorul lui (pe Windows
  pid-ul spus de sistem și SID-ul din token, `peer_windows.rs`; pe Unix `euid` și
  proprietarul dosarului canalului). Pe Windows, pipe-ul e creat doar pentru proprietar și
  sistem (`D:P(A;;GA;;;OW)(A;;GA;;;SY)`), ca primă instanță a numelui; unul făcut înainte de
  altcineva e refuzat, nu împărțit.
- **Salutul** verifică protocolul (acum 2), spune versiunea motorului care rulează și GPU-ul
  lui (vezi „Același GPU”).
- **Pe macOS**, `web-host` și aplicațiile ajutătoare ale CEF (GPU, Renderer, Plugin, Alerts)
  sunt semnate de noi. Pentru că stau în afara pachetului aplicației, încărcarea
  framework-ului e a lor, deci nu atinge semnătura aplicației.

### Ciclul de viață

- **Tab ascuns sau fereastră minimizată**: pagina primește `WasHidden(true)` (Chromium îi
  oprește desenarea și îi rărește timerele); procesele și conținutul rămân; la revenire,
  `WasHidden(false)`, iar pagina e acolo pe loc.
- **Pagină închisă** (tabul închis, sau ascunsă de mult: `HIDDEN_KEPT`): browserul ei se
  închide; procesul de randare rămâne cât îl mai folosește altă pagină.
- **`web-host` se oprește** după `Lifetime::idle` (implicit 5 minute) fără nicio pagină.
  Între timp, o pagină nouă pornește pe loc. Nu pornim motorul dinainte „ca să fie gata”: ar
  ține ~107 MB fără nicio pagină.
- **Cadrele le programează Chromium** (`external_begin_frame` oprit): compozitorul lui
  desenează doar când se schimbă ceva, iar aplicația nu cere cadre. O pagină nemișcată nu
  costă aproape nimic.
- **Bucla motorului** doarme până la o comandă sau până la lucrul cerut de CEF
  (`on_schedule_message_pump_work`, `pump.rs`). Plasa de lucru rulează doar cât e o pagină
  vizibilă: la 33 ms cât se întâmplă ceva (o comandă sau un cadru, plus 2 s), la 250 ms când
  pagina stă; fără pagini, nimic.
- **Închiderea** lasă browserele să se închidă (cel mult 2 s) înainte de `cef::shutdown`.

### Consumul lui Chromium

- **Procesele**: cel mult 2 procese de randare (`--renderer-process-limit=2`: paginile
  noastre sunt de încredere și puține); fără extensii, sincronizare, traducere, componente
  actualizate de Chromium, rețea de fundal, verificare ortografică; sunet oprit. Jurnalele
  CEF sunt reduse la erori, fără port de depanare.
- **Pe disc**: doar limbile `en`, `en-US` și `ro` (Chromium are 220), tăiate înainte de
  semnare.
- **GPU**: o singură textură pe pagină, cât pagina; Chromium își păstrează doar bufferele
  lui de randare.
- **Profilul paginilor** stă în dosarul de date al aplicației (`--profile`,
  `<date>/web-engine/profile/`), nu în cel temporar.

### Texturile între procese

La fiecare cadru, `web-host` primește de la Chromium textura pe GPU și o dă aplicației, care
o copiază pe GPU într-o textură a ei (păstrată și desenată cât pagina nu se schimbă), apoi
confirmă. Abia atunci `web-host` îi dă bufferul înapoi lui Chromium (`Acks`: apelul de
desenare așteaptă cel mult 100 ms; confirmarea vine direct de pe firul care citește de la
aplicație). O aplicație care nu confirmă e „în urmă” și nu mai e așteptată.

- **macOS**: `IOSurfaceCreateMachPort` în `web-host`, portul trimis printr-un mesaj Mach
  (`mach-ports`). Aplicația își face la legare un serviciu cu nume propriu
  (`bootstrap_check_in`, cu pid-ul ei și un nonce dat prin canalul local), iar `web-host` îl
  caută (`bootstrap_look_up`). Mesajul e verificat în trailer-ul de audit al sistemului, ca
  să vină de la procesul așteptat. Aplicația face suprafața din port
  (`IOSurfaceLookupFromMachPort`), apoi o textură Metal, apoi wgpu
  (`create_texture_from_hal`); importul urmează `cef::osr_texture_import` (Apache-2.0/MIT).
- **Windows**: textura D3D11 partajată (handle NT), duplicată de `web-host` în procesul
  aplicației (`DuplicateHandle`, `transfer/windows.rs`). Aplicația o deschide pe dispozitivul
  ei DX12 (`OpenSharedHandle`), o învelește ca textură wgpu și o copiază pe GPU; starea
  „comună” a resursei e promovată de D3D12 singură la sursă de copiere, deci wgpu nu adaugă
  nicio barieră. Importul e oferit doar când aplicația desenează prin DX12.
- **Calea prin pixeli** (Linux, și rezerva peste tot): pagina își cere cadrele ca pixeli
  (`Open::pixels`) când GPU-ul ferestrei nu poate deschide texturile sistemului (Linux, Vulkan
  sau GL pe Windows), când importul a căzut de 3 ori la rând, când motorul care rulează e pe
  alt GPU, sau cu `EDITOR_WEB_FRAMES=pixels`. Chromium dă cadrul ca pixeli (`on_paint`);
  motorul îl scrie într-un fișier din dosarul canalului (pe Linux `XDG_RUNTIME_DIR`, adică în
  memorie), acceptat doar acolo, cu numele `frames-*.bgra`, citibil doar de proprietar, creat
  și șters de motor. Aplicația îl citește pe firul cititorului, într-un buffer refolosit, și îl
  urcă în textura paginii (`FrameImporter::write`, `queue.write_texture`). Merge pe orice GPU
  și API, cu o copie prin procesor pe fiecare cadru schimbat, la un ritm.
- **Linux, fără copie** (dmabuf în Vulkan): nefăcut, vezi „Lăsat”.

### Același GPU pentru fereastră și motor

- Importul spune ce GPU are fereastra (`FrameImporter::adapter`): pe Windows LUID-ul
  dispozitivului DX12 (`GetAdapterLuid`), adică exact cel pe care se deschid texturile. Pe
  macOS și Linux nu contează (`IOSurface` e împărțit de sistem, pe Linux cadrele vin ca
  pixeli).
- Salutul îl poartă (`Hello::adapter`), iar motorul e pornit cu `--adapter-luid`, trecut în
  Chromium ca `--use-adapter-luid=<high>,<low>` (zecimal, partea de sus cu semn, cum îl
  citește Chromium; altfel l-ar ignora fără nicio eroare). Procesul GPU al Chromium pornește
  cu switch-urile procesului principal.
- Răspunsul spune GPU-ul motorului (`Welcome::adapter`). Un motor care rulează deja pe alt GPU
  (pornit de altă fereastră) e refuzat, nu repornit sub paginile altcuiva, iar paginile trec
  pe pixeli.
- După 3 cadre la rând care nu se pot deschide (laptopuri cu două plăci), motorul nu mai e
  folosit în pornirea asta, iar paginile sunt refăcute de sistem (spus în jurnal).
- **Placa video**, din Setări › Aspect › Placa video (`[graphics] gpu`): „cum alege sistemul”
  (implicit, `PowerPreference::None`: ce a setat utilizatorul pentru aplicație în setările
  sistemului și regulile lui pe baterie), „integrată” sau „dedicată”. Plăcile găsite sunt
  listate, cu cea care desenează acum; setarea e a aplicației întregi, se aplică la
  următoarea pornire, iar motorul o urmează singur.

### Paginile, în aplicație

- **`platform_api::WebView`** rămâne contractul pentru module și aplicație. `EnginePage`
  (`crates/app/src/app/web_engine.rs`) e un `WebView` a cărui imagine o desenează aplicația:
  o imagine „vie” în scenă, în locul paginii. Implementările sistemului (wry) rămân rezerva.
- **Pe firul UI** doar se predau datele și se desenează imaginea. Legarea la motor (și
  pornirea lui) se face pe un worker (`web-engine`); paginile deschise între timp așteaptă cu
  spinnerul vederii. Cadrele sunt importate pe firul cititorului conexiunii, apoi fereastra e
  trezită; evenimentele sunt puse la coadă și predate la cadrul următor (`WebView::poll`).
- **Generația imaginii** crește la fiecare cadru importat; calculul zonelor de redesenat o
  urmărește, deci doar pagina e redesenată (`only_pages`), iar o trezire cu doar un cadru nou
  nu reface fereastra (`WebViews::poll`). Golirea cache-urilor păstrează texturile paginilor.
- **Ce se deschide peste pagină e desenat peste ea**: nici poză, nici pagină ascunsă sub
  meniuri sau dialoguri; estomparea de sub ferestrele de deasupra o acoperă și pe ea. Pagina
  ia colțurile cardului în care stă (`page_corners`, `ImageInstance::corners`).
- **Loader până la primele cadre**: `ds::loading_state` (starea goală cu un spinner) cât
  pagina se încarcă, până la `WebEvent::Shown` („pagina și-a desenat conținutul”, la fel
  pentru ambele motoare: al sistemului la `PageLoadEvent::Finished`, al nostru la primul
  cadru de după încărcarea conținutului). Vederea spune `is_loading` până atunci, niciodată
  cât tab-ul e ascuns.
- **Dacă motorul cade**, pagina spune `is_gone` și e refăcută; după 3 căderi în 30 s, motorul
  e lăsat deoparte 60 s. Dacă nu pornește, paginile sunt ale sistemului 60 s, apoi se încearcă
  din nou.
- **Paginile HTML** (răspunsurile clientului HTTP, `WebSource::Html`) se deschid direct la
  `local-app://page/<cheie>` (cheie aleatoare, textul ținut cât trăiește pagina, `HtmlPage`),
  servite din memorie oricât de mari, cu fundal alb opac și politică strictă: fără scripturi,
  imagini și fonturi doar din pagină, nicio resursă din rețea. Tipul MIME și setul de
  caractere sunt date separat.
- **Paginile de aplicație** (draw.io) vin prin schema `local-app`, servită din `web-pages`:
  fișierele doar din dosarul paginii, politica de conținut, scriptul de mesaje. Pornesc de pe
  `about:blank` și se încarcă după ce dosarul lor e cunoscut. `window.ipc.postMessage` e
  definit în procesul de randare doar pentru ele (`ipc.rs`) și ajunge la aplicație ca mesaj
  de proces; o singură limită pentru mesajele paginilor
  (`web_engine_protocol::MAX_PAGE_MESSAGE`).
- **Navigarea** merge doar spre URL-urile paginii; o pagină de aplicație nu poate naviga la un
  document `data:`. Orice altă legătură și orice fereastră nouă e trimisă aplicației, după un
  gest al utilizatorului, nu deschisă de motor.

### Intrările

- **Contractul** (`platform_api`): `PageInput` (mouse, rotiță, tastă, text, focus),
  `WebView::takes_input` / `input` / `cursor`. Paginile sistemului primesc intrarea de la
  sistem.
- **Rutarea** (`WebViews`, fără nume de pagini): pagina sub mouse primește intrarea doar
  dacă nu e nimic al aplicației deschis sau plutind peste ea (`floating_over_pages`); o
  apăsare ține mouse-ul până la eliberare; clicul dublu și triplu sunt numărate ca în rest;
  butoanele ținute sunt trimise și la mișcare; tastatura merge la pagina cu focus, iar un clic
  pe fereastră i-l ia înapoi; cursorul e cel cerut de pagină (`CursorIcon` are cursoarele unui
  desen: cruce, mutare, apucare, interzis, diagonale).
- **Tastele** (`web_keys.rs`): pe Windows doar codul Windows; pe macOS și codul macOS și
  caracterul AppKit al tastelor speciale. Textul tastat merge caracter cu caracter, fără
  scurtături (⌘/Ctrl), dar cu AltGr; emoji ca pereche UTF-16. Restul fracționar al derulării
  de pe trackpad e păstrat; mărimile cerute sunt limitate (cel mult 16384 px, scara între
  0,5 și 8). Scara ferestrei e urmărită: alt ecran înseamnă altă așezare.
- **Scurtăturile aplicației rămân ale ei**: cu pagina focalizată, o tastă cu ⌘/Ctrl sau o
  tastă F legată de o comandă a aplicației (de editare exceptate) e a aplicației; literele,
  săgețile, Esc și restul sunt ale paginii.
- **Editarea**: comenzile Editare (anulează, refă, taie, copiază, lipește, selectează tot)
  merg la pagina cu tastatura ca acțiunile de editare ale Chromium (`Command::Edit`); pe
  macOS din meniul aplicației (⌘C nu ajunge la pagină ca tastă), pe Windows și Linux din
  tastele lor.
- **Listele derulante (`<select>`)**: Chromium le desenează ca popup (`on_popup_show` /
  `on_popup_size`, cadre `POPUP`) pe aceeași cale (textură sau un fișier de pixeli alăturat);
  aplicația le desenează peste pagină, unde spune motorul.
- **Meniul contextual**: meniul Chromium (care nu se poate arăta fără fereastră) e golit;
  aplicația arată meniul ei unde s-a cerut, cu mouse-ul: Taie / Copiază / Lipește, după ce e
  selectat și dacă e un câmp editabil, apoi Selectează tot. O pagină cu meniul ei (draw.io)
  îl păstrează.
- **Compunerea (IME)**: textul compus apare subliniat (`ime_set_composition`, cu intervalul
  de înlocuire „invalid”, cum îl cere CEF); alegerea îl înlocuiește (`ime_commit_text`); un
  text fără compoziție merge ca tastat; fereastra cu variante stă lângă textul compus
  (`on_ime_composition_range_changed`); deplasamentele sunt numărate în UTF-16.
- **Fișierele trase în pagină** (draw.io: imagini) sunt ale ei cât sunt deasupra ei: intră,
  se mișcă, ies și sunt lăsate (`Command::Drag`, prin `drag_target_*`, doar copiate), cu locul
  din momentul lăsării. În afara paginii, sau peste o notificare de deasupra ei, sunt ale
  editorului. O pagină HTML nu le primește. O pagină închisă sau refăcută în timpul tragerii
  nu le mai ia; una refăcută le primește de la început.

### Accesibilitatea

- Cât rulează un cititor de ecran (sistemul cere arborele ferestrei), fiecare pagină
  desenată de motor e rugată să-și facă arborele (`Command::Access`); fără cititor nu se face
  nimic, iar arborii ținuți sunt lăsați odată cu el.
- Motorul ține arborele din actualizările Chromium și trimite doar ce citește un cititor:
  rolul, numele, valoarea, starea (focus, dezactivat, bifat, deschis, nivelul titlului) și
  locul. Lasă deoparte schela Chromium (noduri ignorate, cutiile de text, containerele fără
  nume, textul care repetă numele butonului). Nodurile mutate doar de așezare își primesc
  locul nou (`on_accessibility_location_change`); un nod dintr-un bloc transformat (CSS sau
  SVG) e spus unde se vede (matricea 4×4 a fiecărui nod, apoi locul containerului, până la
  pagină). Arborele e trimis cel mult o dată la 100 ms, cel mai nou; în aplicație contează doar
  ultimul venit, iar numărul lui doar crește.
- Aplicația traduce rolurile Chromium în ale noastre și pune nodurile sub pagină, cuibărite
  (`ui_kit::access::nested`). Ce cere cititorul unui nod (apăsare, focus) merge pe drumul
  mouse-ului; o valoare scrisă de cititor într-un câmp e tastată după un clic și selectarea
  textului (`WebViews::set_value`).
- **Limitele (pagina e ostilă):** cel mult 4000 de noduri spuse, 50 000 ținute, nume de cel
  mult 1000 de caractere, adâncime de cel mult 256, fiecare nod parcurs o dată.

### Sandbox-ul

- **macOS**: fiecare proces ajutător (randarea, GPU-ul, rețeaua) intră în sandbox-ul
  Chromium înainte de orice (`libcef_sandbox.dylib` din framework); niciun ajutător nu rulează
  cu `--no-sandbox`.
- **Linux**: sandbox-ul Chromium cu spațiile de nume ale utilizatorului, acolo unde sistemul
  le permite (fără ajutătorul setuid, pe care un motor descărcat nu-l poate instala); fără el
  unde sistemul le oprește (comutatorul nucleului pe Debian vechi, o limită de zero,
  restricția AppArmor pe Ubuntu 23.10 și mai noi). Verificarea citește doar fișierele
  sistemului (`sandbox.rs`).
- **Windows**: fără sandbox încă (vezi „Lăsat”).
- `EDITOR_WEB_NO_SANDBOX=1` îl oprește oricum, pe orice sistem.

### Versiunea motorului

- Motorul are versiunea lui, separată de a aplicației: `<versiunea Chromium>-<a noastră>`, de
  exemplu `156.0.8078.100-1`. Prima parte e a Chromium-ului din CEF; a doua o creștem noi la
  orice pachet nou cu același Chromium (o reparație în `web-host`, altă configurație).
- Partea noastră e `engine::BUILD` (`web-host/src/engine.rs`); versiunea întreagă o spune
  programul însuși (`web-host --version`), citită la pachetare. Versiunea din
  `web-host/Cargo.toml` e doar a crate-ului. Creșterea e un commit, cu tag-ul lui
  `web-engine-v<versiune>`; actualizarea aplicației nu ia niciodată aceste tag-uri drept
  versiuni (testat).
- **Ce merge cu ce aplicație**: canalul are o versiune de protocol (un număr). Aplicația spune
  ce protocol vorbește, pachetul pe al lui; un pachet cu alt protocol nu e ales, ci unul
  potrivit. O aplicație veche nu ia un motor pe care nu-l înțelege, iar una nouă nu pornește
  un `web-host` vechi.
- **Un motor mai vechi care rulează încă** (o actualizare a venit după ce a pornit) e folosit
  până se oprește singur, după ultima lui pagină; următoarea pornire e cea nouă (oprirea lui
  ar închide paginile altor ferestre, iar un al doilea motor ar folosi același profil). O
  pornire e trecută în istoria versiunii doar dacă versiunea ei a răspuns în salut; o
  versiune mai veche care rulează e scrisă în jurnal.

### Pachetul și lista

- **Pachetul** (`cargo xtask web-engine-package`, după `web-engine-trim`): `web-host`, CEF
  (biblioteca, resursele, limbile noastre), ajutătoarele, `engine.json` (ce spune
  `web-host --version`), ca `web-engine-<sistem>-<versiune>.tar.xz`, cu descrierea lui
  (`<arhivă>.json`). Un pachet cu limbile netăiate e refuzat (semnătura nu le-ar acoperi).
  Arhiva e scrisă de codul nostru (`xtask/src/web_engine/tar.rs`, xz-ul din `lzma-rust2`, cu
  care o și desface aplicația), nu de `tar`-ul sistemului: intrările sortate, proprietar 0:0,
  ora fixă, 0755 pentru dosare și programe, 0644 în rest, fără atribute extinse și fără
  fișierele `._*` pe care le adaugă `tar`-ul macOS (ele strică verificarea semnăturii după
  despachetare). Aceleași fișiere dau aceiași octeți, pe orice sistem. Compresia: xz preset
  6, pe blocuri de 24 MiB comprimate în paralel.
- **Lista** (`web-engine.json`, semnată ca `latest.json` al aplicației, cu aceeași cheie și
  aceeași verificare, `pinned_downloads::signature`): pentru fiecare sistem și protocol,
  versiunea, adresa, mărimea, SHA-256; plus o versiune minimă sigură (una mai veche e
  înlocuită chiar dacă merge: o breșă gravă). O listă nouă e unită cu cea publicată (cu
  semnătura ei verificată): rămâne cel mai nou pachet al fiecărui sistem pentru fiecare
  protocol, ca o aplicație care vorbește protocolul vechi să-și găsească motorul și după o
  creștere a protocolului. O descriere pe care aplicația n-ar lua-o e refuzată la facere.

### Publicarea (`.github/workflows/web-engine.yml`)

- **Tag-ul `web-engine-v<versiune>`** construiește pe macOS (ARM pe `macos-latest`, Intel pe
  `macos-15-intel`), Windows (`windows-latest`) și Linux (`ubuntu-22.04`, cel mai vechi
  glibc oferit). Pentru fiecare sistem:
  - un singur CEF pe rulare (`CEF_PATH`, în `$RUNNER_TEMP/cef`), pentru unealta de
    împachetare a CEF și pentru `cef-dll-sys` al motorului; unealta e instalată după commit-ul
    tag-ului ei (`--rev`), iar dependențele lui `web-host` sunt luate cum le fixează
    `web-host/Cargo.lock` (`cargo fetch --locked`, `--locked` la construcție);
  - pe macOS, unealta face structura pachetului (aplicațiile ajutătoare, `Info.plist`,
    framework-ul, cu identificatorul `<bundle_id>.web-engine`, din `brand.toml`), apoi
    programele release le înlocuiesc pe cele de depanare (al gazdei, și programul ajutător
    copiat în fiecare din cele 5 aplicații ajutătoare, sub numele ei);
  - pe Linux, `web-host` își găsește `libcef.so` lângă el prin rpath (`$ORIGIN`,
    `web-host/.cargo/config.toml`), ca atunci când îl pornește aplicația;
  - limbile Chromium, în afară de ale noastre, sunt scoase înainte de semnare
    (`cargo xtask web-engine-trim`);
  - pe macOS, semnat din interior spre exterior, cu runtime-ul întărit: bibliotecile
    framework-ului, framework-ul, fiecare ajutător, gazda. Doar renderer-ul (JIT-ul lui V8,
    `web-engine-renderer.entitlements`) și GPU-ul (shaderele rasterizatorului software,
    `web-engine-gpu.entitlements`) au voie la JIT; restul nu au nimic
    (`web-engine.entitlements`). Cu certificatul proiectului (pus într-un keychain al rulării
    de `scripts/ci/macos-keychain.sh`, ca la aplicație), validarea bibliotecilor rămâne
    pornită; ad hoc, fiecare program o are oprită (fără echipă, runtime-ul întărit n-ar
    încărca framework-ul);
  - împachetat, apoi semnătura verificată pe pachetul desfăcut (macOS).
- **Ciorna** (doar pentru un tag): lista publicată e descărcată
  (`scripts/ci/web-engine-published.sh`: lipsește doar dacă nu s-a publicat nimic; o listă
  publicată care nu se poate citi oprește rularea), unită cu pachetele noi, semnată și
  verificată; apoi un release ciornă cu pachetele și lista.
- **Publicarea**, de mână (`publish` și versiunea): ciorna e verificată din nou
  (`cargo xtask web-engine-verify`: semnătura listei e a proiectului, fiecare sistem are
  pachetul versiunii, fiecare arhivă e acolo cu mărimea și SHA-256 din listă, lista nu pierde
  și nu coboară niciun pachet publicat între timp), apoi urcată în release-ul `web-engine`, pe
  care îl citește aplicația: întâi arhivele (nume noi, nimic citit de un client nu se
  schimbă), apoi semnătura, apoi lista. În cele câteva secunde dintre ultimele două, un client
  vede o listă și o semnătură care nu se potrivesc și le refuză pe amândouă: își păstrează
  motorul și se uită din nou mai târziu. După listă, arhivele pe care ea nu le mai numește sunt
  șterse din release (`scripts/ci/web-engine-prune.sh`), iar ciorna e ștearsă (tag-ul rămâne).
- **Doar versiunea minimă** (de mână, `relist` și `minimum`): nimic construit, nicio arhivă
  atinsă. Lista publicată e descărcată (cerută), primește minimul nou (refuzat dacă vreun sistem
  n-ar avea niciun pachet la fel de nou), e semnată din nou
  (`web-engine-manifest --relist`) și urcată cu semnătura ei.
- **Rulările stau la rând** (`concurrency`): ciorna, publicarea și `relist` nu ating lista
  publicată în același timp. GitHub ține doar una în așteptare pe grup: o a treia o anulează
  pe cea care aștepta (se pornește din nou de mână).
- **Drepturile**: doar citire pentru tot workflow-ul; scriere (`contents: write`) doar la
  ciornă, la publicare și la `relist`. Construcțiile rulează cod străin (unealta CEF,
  descărcarea și CMake-ul CEF, uneltele din `brew` / `choco`), deci nu primesc dreptul de
  scriere și nici tokenul în checkout (`persist-credentials: false`).

### Descărcarea și actualizările, la utilizator

- **Unde**: în dosarul de date al aplicației, `<date>/web-engine/<versiune>/`, nu lângă
  program: merge fără administrator și nu e atins de actualizările aplicației. Starea e
  `state.json`, scris atomic (`current`, `previous`, `bad`, căderile la rând); `.staging/`
  pentru ce se descarcă; `profile/` separat de versiuni.
- **Prima dată**: când o pagină vrea motorul (`want`), modulul `web-engine-install` îl
  oferă o dată, într-o notificare cu ce e și cât are (~100 MB, ~300 MB pe disc), cu
  Descarcă / Nu acum. „Nu acum” e păstrat, iar descărcarea rămâne în Setări. Progresul apare
  în aceeași notificare, cu Oprește, apoi „gata” sau eșecul, cu Încearcă din nou. Până atunci,
  paginile merg prin rezervă.
- **Actualizările**: un motor mai nou e căutat o dată pe pornire, la un minut după prima
  fereastră, dacă setarea „Ține-l la zi” e pornită, și din Setări › Motorul web (Descarcă /
  Caută unul mai nou / Șterge, și setarea). O actualizare se descarcă în fundal; cea veche
  rămâne folosită până la următoarea pornire a motorului.
- **Pașii instalării** (`web_engine::install`): descărcat fixat pe SHA-256 și mărime, din
  gazdele GitHub, cu reluare și oprire, în `.staging`; desfăcut de codul nostru (fără căi în
  afara dosarului, fără legături, cel mult `MAX_UNPACKED`, 2 GiB, desfăcuți); verificat întreg
  (`engine.json` = lista, programul acolo); mutat la locul lui; ales în `state.json`. Ceva nu
  se potrivește: totul șters, nimic folosit, spus în jurnal.
- **Încălzirea**: după ce o versiune e instalată (prima dată sau la o actualizare), motorul e
  pornit o dată cu `--warm` (încarcă biblioteca și se oprește, cel mult 30 s), ca macOS să-și
  facă verificarea programului nou în fundal, nu la prima pagină a utilizatorului.
- **Curățenia, controlată**: se șterg versiunile nici alese, nici păstrate ca „dinainte”, din
  care nu rulează niciun motor (lacătul pe `.in-use`, ținut de motor prin `--hold`), și
  `.staging` rămas de la o descărcare întreruptă. Niciodată profilul (stocarea locală a
  draw.io, cookie-urile paginilor aplicației, care trec de la o versiune la alta) sau starea;
  profilul se șterge doar cu „Șterge” din Setări. Curățenia și o instalare în curs nu se calcă
  una pe alta.
- **Întoarcerea**: două porniri căzute la rând fac versiunea rea și o aduc înapoi pe cea
  dinainte (spus în jurnal); una rea nu mai e aleasă. Cea dinainte e ștearsă abia după ce cea
  nouă a mers o dată, deci în afară de cea aleasă se ține cel mult una.

### Cum e organizat codul

| Crate | Stratul | Ce face | Depinde de (ale noastre) | Îl folosesc |
|---|---|---|---|---|
| `crates/infra/system/web-engine-protocol` | `infra-shared` | mesajele (`Command`, `Event`, `Frame`, `Texture`, intrările), versiunea protocolului, numele canalului, încadrarea; descrierea pachetelor și lista; fără I/O | — (`serde`, `postcard`) | `web-host-core`, `web-engine`, `web-host`, `xtask` |
| `crates/infra/system/web-host-core` | `infra-shared` | motorul fără CEF: canalul unic cu lacătul lui, clienții, paginile lor, oprirea după `Lifetime::idle`, contractul `Engine`, `Acks` | `web-engine-protocol` (`interprocess`, `tracing`; `libc` pe Unix, `windows-sys` pe Windows) | `web-host`, `web-engine` |
| `crates/infra/system/web-pages` | `infra-shared` | regulile unei pagini de aplicație: schema `local-app`, fișierele doar din dosarul ei, tipurile, politica de conținut, scriptul de mesaje | — (`tracing`, `serde_json`) | `platform-winit` (motorul sistemului), `web-host` |
| `crates/infra/system/mach-ports` | `infra-shared` (doar macOS) | porturi Mach între două procese: serviciul, dreptul de trimitere, verificarea trimițătorului | — | `web-engine`, `web-host` (macOS) |
| `crates/domain/tools/web-engine-versions` | `domain` | regulile pachetelor: versiunile comparate pe părți, cel mai nou pachet pentru sistem și protocol, minimul | — | `web-engine`, `xtask` |
| `crates/infra/system/web-engine` | `infra` | partea aplicației: legătura împărțită (găsit sau pornit `web-host`), paginile, instalarea (`install`: lista, așezarea, descărcarea, curățenia, întoarcerea, starea comună `install::service`) | `web-engine-protocol`, `web-host-core`, `web-engine-versions`, `pinned-downloads`, `files`, `kernel`; `mach-ports` pe macOS | `app`, `web-engine-install`; `web-host` (testul de consum) |
| `crates/modules/tools/web-engine-install` | `module` | oferta, descărcarea cu progres, actualizările în fundal, Setări › Motorul web | `web-engine`, `kernel`, `i18n`, `extensions`, `command-model`, `workbench-model`, `settings-model` | `app` (înregistrat ca modul) |
| `web-host/` | în afara workspace-ului și a straturilor | programul motorului, cu CEF: `web-host` și `web-host-helper` (ajutătoarele de pe macOS), aplicația CEF (`app.rs`), bucla (`pump.rs`), trecerea texturilor (`transfer/{windows,macos,linux}.rs`), `ipc.rs`, `sandbox.rs` | `web-engine-protocol`, `web-host-core`, `web-pages`, `mach-ports` (macOS); `cef` | — (testele lui: `web-engine`, `process-stats`) |

- `web-host/` e un proiect Cargo separat: CEF e descărcat la compilare și există doar pentru
  MSVC pe Windows, iar verificarea noastră pentru Windows e `windows-gnu`. Construit de
  workflow-ul lui, iar local cu `cargo build` în `web-host/` (în `target/web-host` al
  depozitului). `./scripts/check.sh` îi verifică mereu formatarea și, unde e un CEF pe mașină
  (`CEF_PATH` sau `~/.local/share/cef`), clippy-ul; fără CEF, clippy-ul e sărit cu o notă.
- **În `render-api` / `render-wgpu`**: `ImageData::live` (o imagine fără pixeli în memorie,
  cu textura umplută din afară, neeliberată cât trăiește), `FrameImporter` și `NativeFrame`
  (`Renderer::frame_importer`), importul în `render-wgpu/src/import.rs`; colțurile imaginii
  (`ImageInstance::corners`, `ImageCorners`, implicit drepte).
- **În aplicație**: `crates/app/src/app/web_engine.rs` (`EnginePage`), `web_views` (rutarea,
  generațiile), `web_keys.rs`.
- **`xtask/src/web_engine`**: `web-engine-trim`, `web-engine-package`, `web-engine-manifest`
  (cu `--relist`), `web-engine-verify`, cu tipurile aplicației (`web-engine-protocol`,
  `web-engine-versions`) și semnătura ei (`pinned_downloads::signature`), ca cele două să nu
  se poată despărți.

### Teste

- Protocolul: toate mesajele, dus și întors, trimise în bucăți de orice mărime; un mesaj prea
  lung sau stricat e refuzat fără să fie alocat.
- Motorul comun: un motor fals și trei aplicații; un motor fals pe alt GPU refuzat fără să
  fie pornit altul; formatul switch-ului GPU-ului, la scriere și la citire.
- Lista semnată: o semnătură greșită, un SHA greșit, o mărime greșită sunt refuzate; un
  pachet cu alt protocol nu e ales; o ciornă care ar pierde un pachet publicat e refuzată;
  `--relist` schimbă doar minimul.
- Despachetarea refuză căile în afara dosarului și legăturile; arhiva noastră, desfăcută de
  `pinned_downloads::archive`, iese la fel, bit de executare inclus; un pachet cu limbi
  netăiate e refuzat.
- `state.json` scris atomic; curățenia nu șterge o versiune cu lacăt sau aleasă, șterge
  `.staging` și versiunile vechi fără lacăt, păstrează profilul; o versiune care cade la
  pornire de două ori revine la cea dinainte.
- Pe GPU: o suprafață albastră apare albastră; la altă mărime textura e refăcută; o imagine
  eliberată nu mai primește cadre; colțurile rotunjite.
- Accesibilitatea: rolurile, cuibărirea, limitele.
- Probele cu motorul real (ignorate fără el): `web-engine` de la un capăt la altul
  (`EDITOR_WEB_HOST`), `crates/app/tests/unit/app/web_engine.rs` (HTML, mesajele, draw.io cu
  tema, intrările), `web-host/tests/consumption.rs`, `web-host/tests/cold_start.rs`.

## Rezerva

Până când motorul e descărcat (sau dacă e refuzat ori nu pornește), paginile merg prin
motorul sistemului:
- **macOS și Windows**: wry. Pe Windows, cu stratul plutitor ca fereastră popup.
- **Linux**: în browser.

Găurile din pagini și stratul plutitor există doar cât se vede o pagină a sistemului.
Planul 98 (WebView2 visual hosting) rămâne scris, ca variantă fără descărcare.

## Costuri

- **Mărime**: aplicația nu crește. Motorul se descarcă o dată: 98,6 MB comprimat pe macOS ARM
  (măsurat); Linux și Windows nemăsurate încă. Aplicația spune „circa 100 MB”.
- **RAM**: cât e o pagină deschisă, procesele Chromium (vezi „Măsurători”); fără pagini,
  nimic, pentru că `web-host` se oprește.
- **GPU**: o textură cât pagina, pe pagină; fără ferestre sau straturi în plus.
- **Muncă**: cel mai mare plan de până acum pe partea de platformă (estimat la câteva
  săptămâni: pe trei sisteme facem noi intrările, popup-urile, IME-ul, accesibilitatea și
  ambalarea).
- **Securitate**: Chromium e al nostru de ținut la zi (actualizările, versiunea minimă);
  paginile aplicației păstrează regulile lor: nimic de pe disc sau din rețea, nicio navigare
  în afară, nicio fereastră nouă.
- **Licențe**: CEF e BSD, Chromium are licențele lui; se adaugă la lista de licențe arătată
  în aplicație.

## Pașii

Fiecare pas se termină cu `./scripts/check.sh` ok și o încercare pe sistemele lui.

### 99a. macOS: încercarea, apoi motorul de la un capăt la altul

**Făcut:**
- **Încercarea** cu exemplul `osr` al `cef-rs` (vezi „Măsurători”): mărimea, RAM-ul cu
  github.com și cu draw.io, trecerea texturii între procese. Hotărât: porturi Mach pe macOS.
- **Fundația**, în workspace-ul aplicației: `web-engine-protocol`, `web-host-core` (canalul
  unic, serverul cu un fir de citire și unul de scriere pe aplicație, nimic scris sub lacăt,
  salutul, paginile fiecărei aplicații, `Engine`), `web-engine` (legătura, `Launch` separat de
  aplicație și fără fereastră de consolă, paginile `WebPage` închise la eliberare, „s-a
  oprit” trimis tuturor când motorul pleacă), `web-pages` (mutat din `platform-winit` cu
  `git mv`, aceleași reguli pentru `wry` și `web-host`), `mach-ports` (declarațiile de sistem
  scrise de mână).
- **Confirmarea cadrelor**: `Acks` în `web-host-core`; `ArrivedFrame` în `web-engine`, care
  trimite singur confirmarea când e eliberat, deci un cadru nu rămâne neconfirmat. Trezirea
  buclei (`Server::waker`), `Engine::joined` / `left`.
- **Programul `web-host`**: aplicația CEF cu consumul redus, bucla, paginile fără fereastră,
  navigarea, ce trimite (cursorul, „încărcată”, „căzută”), intrările, trecerea texturii
  (macOS prin `mach-ports`, Windows prin `DuplicateHandle`).
- **Pagina ca imagine în renderer**: IOSurface → textură Metal → wgpu, fără copie prin
  procesor; copia pe GPU în textura păstrată a paginii (refăcută doar la altă mărime),
  așteptată doar până la terminarea ei; lacătul stării partajate ține doar cât se pune textura
  în tabel.
- **Paginile prin motorul nostru** (`EnginePage`), **intrările** spre ele, **loader-ul**
  până la primele cadre, **colțurile rotunjite** (shaderul de imagini nu desenează ce trece de
  colț, cu margine netezită, ca la dreptunghiurile rotunjite).
- **Cadrele le programează Chromium** (`external_begin_frame` oprit), măsurat.
- **Testul de consum** și **pornirea la rece**, măsurate (vezi „Măsurători”).
- **Reparat pe drum**: tipul MIME dat întreg (`text/html; charset=utf-8`) făcea CEF să arate
  pagina ca text; mesajele pagină → aplicație (`ipc.rs`), de care depind draw.io și tema lui;
  paginile HTML direct la adresa lor, fără `about:blank`.
- **Recenzia fundației** (agent separat), reparate:
  - bucla motorului se învârtea la 100% CPU fără pagini: plasa de lucru rulează doar cât e o
    pagină vizibilă;
  - Windows: pipe-urile nu acceptă limită de timp la citire, deci nimic nu se lega; salutul e
    așteptat pe un canal, cu limită;
  - Mach: un cadru al cărui port întârzie e confirmat oricum; porturile vechi ale paginii se
    curăță, cele ale paginilor închise se uită; dreptul de primire e eliberat cu adevărat
    (`mach_port_mod_refs`); o eroare la primire nu mai oprește primirea; forma mesajului e
    verificată (mărime, un descriptor, tipul și dispoziția lui), restul distrus cu
    `mach_msg_destroy`;
  - paginile HTML servite din memorie, nu ca URL `data:` (care se tăia la ~2 MB), cu politica
    strictă; orice resursă din rețea anulată în toate paginile; nicio navigare la `data:`;
    legăturile trimise doar după un gest;
  - intrările: butoanele ținute trimise la mișcare, emoji ca pereche UTF-16, restul derulării,
    mărimile limitate;
  - robustețea: aplicația „în urmă” nu mai e așteptată; un motor care se oprește nu mai
    acceptă legături, iar aplicația pornește unul nou dacă salutul eșuează; erorile la
    acceptare nu mai fac buclă; confirmările ținute doar pentru paginile deschise;
  - procesul motorului: profilul în dosarul de date (`--profile`); procesul pornit e
    așteptat (fără „zombie”) și lucrează în dosarul lui; închiderea lasă browserele să se
    închidă; jurnalul e deschis după lacăt, deci un al doilea motor nu-l mai golește.
- **Recenzia pașilor 2–3** (agent separat), reparate: un motor care nu pornește reface
  paginile care așteptau ca ale sistemului; o pagină deschisă exact când motorul se oprește e
  refăcută; după 3 căderi în 30 s, motorul e lăsat deoparte 60 s; un cadru nou nu mai reface
  fereastra; golirea cache-urilor păstrează texturile; o comandă prea mare e sărită (pagina
  ei anunțată ca dispărută), o legătură ruptă marchează conexiunea ca moartă; trezirile nu se
  mai adună cât se leagă motorul; stratul plutitor nu mai e făcut pentru paginile noastre;
  HTML-ul are fundal alb opac; `Shown` vine și pentru o pagină care nu mai pictează după
  încărcare și pentru una pe care sistemul n-a putut-o face; scara ferestrei e urmărită;
  porturile Mach întârziate eliberate imediat; programul motorului căutat o singură dată, nu
  pe firul UI; celălalt capăt al canalului verificat; paginile de aplicație pornesc de pe
  `about:blank` (într-un motor abia pornit, cererile lor veneau înaintea dosarului:
  `ERR_UNKNOWN_URL_SCHEME`); o singură limită pentru mesajele paginilor.
- **Recenzia a doua** (agent separat), partea ei: lacătul canalului ținut de motor; după
  cadrele date înapoi, următoarea copie e întreagă; pagina nu mai clipește la schimbarea
  tab-ului; numărarea căderilor motorului; textura unei pagini nu mai rămâne fără stăpân
  când e scoasă din memorie; estomparea de sub ferestrele de deasupra acoperă și paginile.

**Lăsat:** vezi „Lăsat” (scara la facerea paginii, profilul motorului comun, mesajele mari,
profilul paginilor HTML, CPU-ul fără pagini, trezirile în repaus, mărimea cadrului).

### 99b. Descărcarea

**Făcut:**
- **Pachetul** (`cargo xtask web-engine-package`), **lista** (`web-engine-manifest`),
  **verificarea dinainte de publicare** (`web-engine-verify`) și **workflow-ul**
  (`web-engine.yml`), cum sunt descrise la „Pachetul și lista” și „Publicarea”.
- **Semnătura**: un singur loc pentru verificarea Ed25519 (`pinned_downloads::signature`, cu
  `ring`), folosit și de actualizarea aplicației și de `release-manifest`.
- **Modulul `web-engine-install`** (`tools.web_engine`): oferta, progresul, Setări › Motorul
  web. Motorul instalat e folosit de la sine; `EDITOR_WEB_HOST` rămâne pentru dezvoltare.
  Când motorul tocmai s-a instalat, paginile sistemului deschise sunt refăcute de el.
- **Probat cu pachetul real**: instalat de codul nostru în 5 s (din sursă locală, compilare
  release), pornit din dosarul instalat, răspunde la salut, iar versiunea folosită nu e
  ștearsă cât rulează.
- **Recenzia a doua** (agent separat), partea ei: reluarea descărcărilor întrerupte; CI-ul
  (`CEF_PATH` comun, rpath-ul `$ORIGIN` pe Linux, drepturile doar unde trebuie); `xtask`
  semnează cu `ring` (aceeași cheie, același format), refuză o versiune minimă mai nouă decât
  a release-ului și o descriere pe care aplicația n-ar lua-o, iar un `--version` căzut e
  raportat cu ce a spus programul.
- **Recenzia a treia** (agent separat), partea workflow-ului și a `xtask`:
  - lista publicată nu mai poate lipsi pe tăcute (un `|| true` o pierdea): un script comun,
    `scripts/ci/web-engine-published.sh`, citește release-ul și oprește rularea dacă lista
    numită nu se descarcă sau n-are semnătura;
  - `minimum` singur nu mai reconstruiește motoarele (și nu mai înlocuiește arhivele
    publicate): modul `relist` (`web-engine-manifest --relist`);
  - după publicare, arhivele nenumite de listă și ciorna sunt șterse (tag-ul rămâne);
  - `web-host` construit cu dependențele fixate (`cargo fetch --locked`, `--locked`);
  - cache-ul Rust și pentru workspace (xtask) și în pașii ciornei și ai publicării; `--repo`
    la fiecare apel `gh`; uneltele `brew` instalate doar dacă lipsesc, fără erori ascunse;
    identificatorul pachetului macOS din `brand.toml` (`<bundle_id>.web-engine`);
  - keychain-ul macOS într-un singur script (`scripts/ci/macos-keychain.sh`), folosit și de
    `release.yml`;
  - `xtask`: fără `sha2` (sumele din `pinned_downloads::digest` / `ring::digest`), `ring` și
    `lzma-rust2` ca dependențe ale workspace-ului; tăierea limbilor și pachetarea parcurg
    pachetul cu aceeași funcție, iar un pachet netăiat e refuzat, nu tăiat pe tăcute;
    `release-manifest` verifică cu cheia construită în aplicație
    (`pinned_downloads::signature::PUBLIC_KEY`), nu cu fișierul citit de pe disc; un singur
    `flag_value` pentru argumente;
  - `./scripts/check.sh` verifică și `web-host` (formatarea; clippy-ul unde e un CEF).

**Lăsat:** prima rulare a workflow-ului și a publicării (ale utilizatorului), notarizarea pe
macOS, construcția de depanare pe macOS (vezi „Lăsat”).

### 99c. Intrările complete

**Făcut** (probate cu motorul real, pe texturi și pe pixeli): editarea, scurtăturile
aplicației, listele derulante, meniul contextual, IME-ul, tragerea fișierelor în pagină (o
pagină care ascultă `drop` primește numele fișierului), cum sunt descrise la „Intrările”.
- **Recenzia a doua**, partea ei: meniul contextual al paginii, deschis cu mouse-ul; tastele
  editorului nu mai ajung în câmpurile paginii.
- **Recenzia pașilor 99c–99h**, partea ei: locul lăsării e cel din momentul lăsării, nu cel
  de la ultima mișcare; o pagină închisă sau refăcută în timpul tragerii nu mai ia fișierele,
  iar una refăcută le primește de la început; peste o notificare de deasupra unei pagini,
  fișierele trase sunt ale ferestrei, ca mouse-ul.

**Lăsat:** tragerea din pagină în afară; fișierele verificate din nou pe disc la întoarcerea
de pe o pagină în fereastră (vezi „Lăsat”).

### 99d. Windows

**Făcut** (scris și verificat la compilare pentru Windows): texturile D3D11 deschise în
DX12, canalul doar al utilizatorului și verificarea celuilalt capăt, tastele doar cu codul
Windows, alt GPU decât al ferestrei, același GPU (`--use-adapter-luid`), placa video din
Setări; workflow-ul pe `windows-latest` (Ninja, `bundle-cef-app --release`, pachetul făcut de
`xtask`, toți pașii în bash, Git Bash).
- **Recenzia a doua**, partea ei: un handle care scăpa pe Windows e închis.

**Lăsat:** încercarea pe o mașină Windows (că textura CEF e un handle NT fără keyed mutex,
altfel e nevoie de `IDXGIKeyedMutex`; cadrele, intrările, consumul și pornirea, ca la 99a),
sandbox-ul, semnarea pachetului (vezi „Lăsat”).

### 99e. Linux, și rezerva peste tot: cadrele ca pixeli

**Făcut:** calea prin pixeli, descrisă la „Texturile între procese”; sandbox-ul cu spațiile
de nume (cu teste pe verificare); workflow-ul pe `ubuntu-22.04`, cu bibliotecile Chromium.
Probat cu motorul real, forțat pe macOS: toate cele patru probe trec (HTML, mesajele, draw.io
cu tema închisă și deschisă, intrările), iar fișierele nu rămân după închidere.
- **Recenzia a doua**, partea ei: calea prin pixeli are un ritm.

**Lăsat:** dmabuf în Vulkan, încercarea pe o mașină Linux (vezi „Lăsat”).

### 99f. Accesibilitatea

**Făcut:** arborele paginii sub nodul ei din arborele nostru, cum e descris la
„Accesibilitatea”. Probat cu motorul real: o pagină cu un titlu și un buton își spune
arborele, cu locurile lor; un buton într-un bloc scalat de 2 ori și mutat cu 50 px e spus
unde se vede.
- **Recenzia pașilor 99c–99h**, partea ei: arborele nu urmărea mutările de așezare, iar
  trimiterea lui nu era adunată.

**Lăsat:** selecția din textul paginii urmărită de cititor (vezi „Lăsat”).

### 99g. Actualizările motorului

**Făcut** (`web_engine::install`, testat fără rețea): lista semnată (`manifest.rs`: pachetele
pe sisteme, protocolul fiecăruia, minimul, alegerea celui mai nou potrivit, fără cele căzute
aici), așezarea pe disc (`layout.rs`), instalarea (`mod.rs`), curățenia cu lacăte,
întoarcerea, starea comună (`install::service`), căutarea în fundal, încălzirea (`--warm`),
cum sunt descrise la „Descărcarea și actualizările”. Aplicația pornește versiunea aleasă
(programul ei, profilul comun, lacătul pe versiune) și spune istoriei dacă pornirea a mers.
- **Recenzia a doua**, partea ei: curățenia și o instalare în curs nu se mai calcă una pe
  alta.
- **Recenzia pașilor 99c–99h**, partea ei: încălzirea n-avea limită (un motor blocat ținea
  instalarea pe loc); acum e oprită după 30 s.

**Lăsat:** din proiectul de la început, nefăcute: data ultimei verificări în Setări și o
notificare scurtă după o actualizare descărcată pe o rețea mobilă sau limitată (vezi
„Lăsat”).

### 99h. Curățenia codului

**Făcut:** găurile sunt calculate și trimise doar cât se vede o pagină a sistemului;
paginile motorului nostru nu sunt tăiate și nici rotunjite de sistem, aplicația le desenează
sub ce plutește; stratul plutitor exista deja doar cât se vede o pagină a sistemului.
- **Reparat pe drum:** acolo unde fereastra nu poate tăia găuri, un clic pe o notificare sau
  pe o fereastră a barei de jos peste o pagină a motorului ajungea la pagină; acum mouse-ul de
  acolo e al aplicației întotdeauna (`floating_over_pages`), separat de găurile tăiate în
  paginile sistemului.
- **Recenzia pașilor 99c–99h**, partea ei: fără nicio pagină a motorului, mouse-ul nu mai
  calculează ce plutește peste pagini; un al doilea test care schimba setările modului relax
  ține și el procesul.
- **Clientul HTTP**: previzualizarea HTML a unui răspuns trece prin același contract
  (`WebSource::Html`), deci cu motorul nostru e desenată de el, fără nimic schimbat în modul.

## Măsurători

### 99a, încercarea (2026-11-24, macOS ARM, Apple M2 Max)

Cu exemplul `osr` al `cef-rs` (CEF 156.0.31, Chromium 156.0.8078.100), construit fără
modificări în scratchpad: pagina desenată în afara ecranului, textura importată în wgpu și
desenată într-o fereastră winit, pornit cu un HOME izolat.

- **Mărimea motorului** (distribuția „minimal” pentru macOS ARM): arhiva bz2 are **145 MB**;
  pe disc, framework-ul are **341 MB** (biblioteca 241 MB, resursele 84 MB, în mare parte
  traduceri, restul 16 MB); pachetul `.app` cu cele 5 aplicații ajutătoare are **366 MB**.
- **RAM** (amprenta fizică, `footprint`, fiecare proces; `github.com` încărcat, 15 s):

  | Proces | Amprenta fizică |
  |---|---|
  | principal (cu wgpu-ul exemplului) | 109 MB |
  | GPU | 146 MB |
  | randarea paginii | 169 MB |
  | randare (a doua) | 26 MB |
  | două utilitare | 35 MB, 20 MB |
  | **total** | **≈ 505 MB** |

  Suma RSS (912 MB) socotește de mai multe ori biblioteca, mapată în fiecare proces; nu e
  măsura bună.
- **Cu draw.io** (pachetul aplicației, v32.6.10, încărcat local din `file://`, 20 s):

  | Pornit | Total | Principal | GPU | Randare | Utilitare |
  |---|---|---|---|---|---|
  | implicit | **≈ 300 MB** | 106 MB | 56 MB | 69 MB | 22 + 21 + 26 MB |
  | `--renderer-process-limit=1 --disable-extensions` | **≈ 276 MB** | 109 MB | 57 MB | 68 MB | 22 + 20 MB |

  Procesul principal ține și fereastra și wgpu-ul exemplului; `web-host` n-are fereastră.
- **Textura între procese pe macOS** (probă: două procese, o suprafață de 64×64): un
  `IOSurface` „global” (`kIOSurfaceIsGlobal`) e găsit după ID în alt proces și citit (merge pe
  macOS 27); unul obișnuit, ca ale lui Chromium, nu. Copia într-o suprafață globală e simplă,
  dar Apple o marchează veche și nesigură (orice proces al aceluiași utilizator poate citi
  pagina, ghicind ID-ul) și costă o copie în plus pe GPU. Portul Mach al suprafeței
  (`IOSurfaceCreateMachPort`) e calea lui Chromium (`MachPortRendezvous`): sigură, fără copie,
  cu câteva sute de rânduri de cod Mach `unsafe`. Aleasă: portul Mach.
- **Ce a cerut construcția**: `cmake` și `ninja`, pentru stratul C++ al CEF
  (`libcef_dll_wrapper`); local într-un mediu Python izolat, nimic instalat în sistem.

### Motorul nostru, pe macOS (M2 Max)

- **De la un capăt la altul** (testul ignorat din `web-engine`): aplicația pornește
  `web-host`, deschide o pagină de 400×200, primește suprafața prin Mach, cadrul e confirmat.
  Legarea cu motorul pornit la rece: ~2 s; primul cadru: ~4 s (procesele GPU și de randare
  pornite la rece). Din 214 cereri de cadru, Chromium a desenat o dată. După reparații: primul
  cadru la 1,5 s, legarea la un motor care rulează 0,19 s.
- **Cadrele programate de Chromium** față de cerute de 60 de ori pe secundă: o pagină
  nemișcată 1,8% față de 3,9%, draw.io 1,7% față de 3,5%.
- **Consumul** (`web-host/tests/consumption.rs`: CPU-ul și amprenta fizică ale motorului și
  ale tuturor proceselor lui, cu limite verificate), după reparații:

  | Stare | CPU (un nucleu) | RAM | Procese |
  |---|---|---|---|
  | fără pagini | 0,96% | 107 MB | 4 |
  | pagină nemișcată | 1,1% | 191 MB | 6 |
  | pagină ascunsă | 1,0% | 191 MB | 6 |
  | draw.io deschis, nemișcat | 2,1% | 210 MB | 6 |
  | toate paginile închise | motorul se oprește singur după timpul de inactivitate | 0 | 0 |

  Înainte de reparații, fără pagini: 100%. Cu sandbox-ul pornit, draw.io ține ~400 MB, la fel
  ca fără: sandbox-ul nu costă memorie.
- **Pornirea la rece** (`web-host/tests/cold_start.rs`): un motor pornit din nou la fiecare
  rulare, 5 rulări, mediana:

  | Pornire | Legat (cea mai lentă) | HTML încărcat | HTML, primul cadru | draw.io încărcat | draw.io, primul cadru |
  |---|---|---|---|---|---|
  | profil folosit înainte | 208 ms (2011 ms) | 79 ms | 93 ms | 147 ms | 160 ms |
  | profil nou | 236 ms (249 ms) | 72 ms | 84 ms | 127 ms | 130 ms |

  De la cerere la primul cadru: ~300 ms pentru un răspuns HTML (acoperite de spinnerul
  vederii); draw.io e gata să lucreze (după `init`) în ~1,9 s, din care cea mai mare parte e
  încărcarea lui. Singura pornire scumpă e prima de după instalare sau actualizare, 2 s, cât
  macOS verifică programul nou (`syspolicyd`); cu încălzirea, verificarea se face în fundal
  (2,08 s), iar pornirea următoare durează 0,03 s.
- **Probele cu motorul real** (`crates/app/tests/unit/app/web_engine.rs`): pagina HTML,
  albastrul ei desenat de aplicație, apare la 2,2 s cu pornirea motorului cu tot; pagina de
  aplicație vorbește în ambele sensuri, primul mesaj la 25 ms cu motorul pornit; draw.io cu
  tema editorului: luminozitate 38 cu tema închisă, 192 cu cea deschisă, gata în ~1,9 s.
- **Pachetul** pe macOS ARM: **98,6 MB** comprimat (măsurat cu `tar -cJ`; compresia noastră
  e tot xz, preset 6, pe blocuri: de măsurat la prima rulare). Linux și Windows: nemăsurate.

## Lăsat

- **Publicarea**: workflow-ul n-a rulat încă niciodată. Nimic din el nu e probat în CI:
  construcția pe Linux și Windows, `bundle-cef-app` cu `CEF_PATH`, rpath-ul `$ORIGIN` de pe
  Linux (pachetarea rulează `web-host --version` fără cale de biblioteci, deci prima rulare îl
  probează), ștergerea arhivelor vechi și a ciornei, modul `relist`. Pe macOS, pașii au fost
  încercați doar local. Prima rulare și pasul `publish` sunt ale utilizatorului (release-urile
  sunt ale lui).
- **Notarizarea pe macOS** (ca la aplicație): pachetul e semnat, dar nenotarizat. Descărcat
  de aplicație, nu poartă marca web-ului, deci Gatekeeper nu-l oprește.
- **Drepturile pe macOS, probate pe un pachet real semnat cu certificatul**: JIT doar pentru
  renderer și GPU e ce cere Chromium; de încercat paginile cu JavaScript și WebGL
  (rasterizatorul software) sub runtime-ul întărit.
- **Construcția de depanare pe macOS**: unealta CEF construiește și varianta de depanare
  (minute în plus pe rulare), fiindcă nu are `--release`; un program mic al nostru care
  cheamă `cef::build_util::mac::bundle` cu dosarul release ar scuti-o.
- **Windows**: încercarea pe o mașină Windows (keyed mutex, cadrele, intrările, consumul,
  pornirea); sandbox-ul (cere programul de pornire al CEF, `bootstrap.exe`, cu motorul ca
  bibliotecă, adică altă ambalare); semnarea pachetului, ca la aplicație. Descărcat de
  aplicație, pachetul nu poartă marca web-ului.
- **Linux**: calea fără copie, dmabuf în Vulkan (planurile prin `SCM_RIGHTS`, dispozitivul
  wgpu cu `VK_EXT_external_memory_dma_buf` și `VK_EXT_image_drm_format_modifier`, imaginea
  Vulkan învelită în wgpu), de făcut și probat pe Intel/AMD și NVIDIA (erori recente pe
  NVIDIA: texturi goale, CEF #4237; cadre citite înainte să fie gata, #4313, reparația #4314);
  până atunci, calea prin pixeli. Sandbox-ul, de încercat pe o mașină Linux.
- **Scara**: e luată la facerea paginii; o fereastră mutată pe un ecran cu altă scară trebuie
  să i-o spună paginii.
- **Profilul motorului comun**: e al primei aplicații care l-a pornit.
- **Profilul paginilor HTML**: e profilul persistent; la sistem e incognito. Fără scripturi și
  fără rețea, riscul e mic.
- **Mesajele mari**: ar trebui trimise prin memorie partajată, nu ca text în mesajul de
  proces.
- **Mărimea cadrului**: e luată din `coded_size`; de verificat față de `visible_rect` (copia
  folosește mărimea suprafeței, deci e corectă; doar marginea poate avea pixeli în plus).
- **CPU-ul fără pagini (~1%)**: vine din procesele proprii ale Chromium (GPU, utilitare); de
  măsurat pe proces și de redus.
- **Trezirile în repaus**: firul porturilor din aplicație se trezește de 2 ori pe secundă,
  fiecare cititor al motorului o dată pe secundă; ar putea aștepta până la închidere.
- **Tragerea din pagină în afară** (o imagine din draw.io în Finder): rară; CEF cere ca
  aplicația să pornească tragerea sistemului (`start_dragging`), iar stratul nostru de
  ferestre (winit) nu poate porni o tragere spre alte programe (ar cere cod al fiecărui
  sistem: NSDraggingSession, OLE DoDragDrop, XDND).
- **Tragerea înapoi în fereastră**: la întoarcerea de pe o pagină în fereastră, în timpul
  tragerii, fișierele sunt verificate din nou pe disc (ca la prima intrare; câteva fișiere,
  puțin).
- **Accesibilitatea**: selecția din textul paginii urmărită de cititor (cititorul citește
  textul și nodurile, dar nu mută selecția paginii).
- **Din proiectul de la început, nefăcute**: data ultimei verificări în Setări › Motorul web;
  o notificare scurtă după o actualizare descărcată pe o rețea mobilă sau limitată; un test
  cu un server de probă pe loopback care să verifice că paginile aplicației nu ajung în rețea.
- **Memoria, din proiectul de la început, nespusă la Făcut** (de verificat ce a pus CEF și
  ce lipsește): pagini fără istoric de navigare păstrat, cache-ul paginilor pe disc cu
  limită, fără cache de cod în memorie peste limită.
- **Mărimile pe Linux și Windows**: pachetul, nemăsurat.

## Surse

- CEF, informațiile texturii partajate (`cef_accelerated_paint_info_t`): https://cef-builds.spotifycdn.com/docs/132.3/structcef__accelerated__paint__info__t.html
- CEF pe Linux, texturile partajate pe NVIDIA: https://github.com/chromiumembedded/cef/issues/4237
- CEF pe Linux, cadre citite înainte să fie gata: https://github.com/chromiumembedded/cef/issues/4313, https://github.com/chromiumembedded/cef/pull/4314
- dmabuf-urile CEF și cât trăiesc (un client Rust, bevy_cef): https://github.com/slyedoc/bevy_cef/pull/1
- java-cef, `OnAcceleratedPaint` importat prin EGL: https://github.com/chromiumembedded/java-cef/pull/524
- Exemplu OSR cu texturi D3D11 (cef-mixer): https://github.com/mediabuff/cef-mixer
- CEF pentru Rust (tauri-apps/cef-rs): https://github.com/tauri-apps/cef-rs, https://docs.rs/crate/cef/latest
- Aplicațiile ajutătoare cerute pe macOS: https://groups.google.com/g/cef-announce/c/YK6CTvGNhlY
- Planul 86 (variantele cântărite atunci, CEF printre ele) și planul 98 (WebView2 visual hosting).
