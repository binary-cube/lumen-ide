# 99 — Un singur motor web peste tot (Chromium, prin CEF), desenat de noi (analiză)

Cerut 2026-11-24: „prefer să avem o soluție generală peste tot și să fie […] parte din ce
desenăm, ca să nu mai avem tot soiul de probleme”; motorul descărcat o singură dată, la prima
nevoie. **Hotărât (2026-11-24): Chromium comun pe toate sistemele.** Înlocuiește, ca direcție, planul 98 (WebView2 doar pe Windows): 98 rămâne rezervă
(mai jos). Aici e analiza și planul; codul vine pas cu pas, după aprobare.

## Ce vrem

- **O pagină web (draw.io, răspunsul HTML al clientului HTTP, previzualizările) e o textură
  pe care o desenăm noi**, în scena noastră, ca orice imagine:
  - orice se poate pune deasupra sau dedesubt (tooltip-uri, meniuri, notificări, umbre,
    sticlă, colțuri, animații);
  - nicio fereastră a sistemului peste fereastra noastră, deci nici găuri, nici straturi
    plutitoare, nici ferestre popup ținute lipite.
- **Același motor și același comportament pe Windows, macOS și Linux.** Pe Linux apar astfel
  și paginile încorporate (azi se deschid în browser).
- **Motorul nu vine cu aplicația:** se descarcă o singură dată, cu acordul utilizatorului,
  când e nevoie prima oară de o pagină. Aplicația rămâne mică pentru cine nu folosește
  pagini.

## Cum: Chromium Embedded Framework (CEF), desenat în afara ecranului

CEF e Chromium făcut să fie încorporat. Îl folosesc Steam, Spotify, sursa „browser” din OBS
și altele.

- **Modul „off-screen rendering” (OSR) cu texturi partajate**: Chromium desenează pagina pe
  GPU și ne dă textura fără copie prin procesor (`OnAcceleratedPaint`). Pe fiecare sistem:
  - **Windows**: o textură D3D11 partajată (handle NT). wgpu pe DX12 o deschide
    (`OpenSharedHandle`) și o folosește ca pe o textură a lui.
  - **macOS**: un `IOSurface`. Metal face din el o textură (`newTextureWithDescriptor:iosurface:`),
    pe care wgpu o primește din partea lui de jos (`create_texture_from_hal`).
  - **Linux**: un dmabuf (unul sau mai multe planuri, ca descriptori de fișier), importat în
    Vulkan (`VK_EXT_external_memory_dma_buf`), apoi în wgpu.
    - **Atenție**: pe NVIDIA au fost erori recente, deci de verificat pe versiunea de CEF
      aleasă:
      - texturi goale (CEF #4237);
      - cadre citite înainte să fie gata (#4313, reparația #4314).
    - Rezerva pe Linux: desenarea prin procesor (`OnPaint`, o copie pe cadru, doar zona
      schimbată), mai scumpă, dar sigură.
- **Noi îi dăm tot ce intră**:
  - mouse-ul: `SendMouseMoveEvent`, `SendMouseClickEvent`, `SendMouseWheelEvent`;
  - tastatura: `SendKeyEvent`;
  - IME: `ImeSetComposition`, `ImeCommitText`;
  - focusul.
- **Ce ne dă el înapoi:**
  - cursorul (`OnCursorChange`);
  - listele derulante ale paginii (`OnPopupShow` / `OnPopupSize`: un al doilea strat, pe care
    îl desenăm noi peste pagină);
  - caretul pentru IME;
  - accesibilitatea (`OnAccessibilityTreeChange`).
- **Pentru Rust**: `cef` / `cef-dll-sys` (tauri-apps/cef-rs) urmează versiunile Chromium și
  merg pe toate trei sistemele (x86_64, ARM64).
  - Ce trebuie verificat la pasul 99a: dacă legăturile lor expun tot ce ne trebuie din OSR
    (`OnAcceleratedPaint` cu informațiile texturii).
  - Dacă nu, completăm noi legăturile lipsă peste `cef-dll-sys` (API-ul C al CEF e stabil).

## Arhitectura: motorul într-un proces separat, al nostru

Regula proiectului: există doar cât e folosit. CEF nu se poate opri și reporni în același
proces (`CefInitialize` o singură dată, `CefShutdown` la ieșire). De aceea motorul nu stă în
procesul aplicației:

```
aplicația (procesul nostru)                    web-host (procesul nostru, cu CEF)
──────────────────────────                     ────────────────────────────────────
WebView (contractul de azi)  ── comenzi ──▶     un browser CEF pe pagină (OSR)
  set_bounds / load / send                       mouse, taste, focus, mesaje
  intrările (mouse, taste)
                              ◀── cadre ──      textura partajată (handle / IOSurface / dmabuf)
textura desenată în scenă                        cursor, popup-uri, caret, accesibilitate
                              ◀── mesaje ──     mesajele paginii (WebInbox)
```

- **`web-host`** e un mic program al nostru, care leagă CEF. Îl pornim când e nevoie de prima
  pagină și îl oprim după ce nu mai e nicio pagină o vreme. Odată cu el pleacă Chromium întreg
  (procesele lui de randare și GPU).
- **Texturile trec între procese fără copie:**
  - Windows: `DuplicateHandle`;
  - macOS: `IOSurface` trimis prin port Mach;
  - Linux: dmabuf trimis prin socket Unix (`SCM_RIGHTS`).
- **Legătura**: un canal local, ca cel dintre instanțe (`interprocess`), cu mesaje mici,
  serializate. Cadrele trec ca handle-uri, nu ca pixeli.
- **Un crash al lui Chromium nu ia aplicația cu el.** Pagina arată „s-a oprit, reîncarcă”.
- Pe macOS, `web-host` și aplicațiile ajutătoare ale CEF (GPU, Renderer, Plugin, Alerts) sunt
  semnate de noi. Pentru că stau în afara pachetului aplicației, încărcarea framework-ului e
  a lor, deci nu atinge semnătura aplicației.

## Descărcarea, o singură dată

- **Ce se descarcă**: „motorul web” pentru sistemul și arhitectura utilizatorului: `web-host`,
  CEF (biblioteca, resursele, limbile), ajutătoarele.
  - Găzduit de noi (un fișier al release-urilor noastre, nu un CDN străin).
  - Semnat ca actualizările aplicației (`self-update::signature`) și verificat înainte de
    folosire.
  - Descărcat cu ce avem deja pentru serverele de limbaj (descărcări fixate, cu progres,
    oprire, reluare).
- **Când**: la prima pagină de care e nevoie (prima diagramă, primul răspuns HTML), cu o
  întrebare clară: ce e, cât are (de măsurat: ~100 MB comprimat, ~250 MB pe disc, de
  confirmat), unde stă.
  - Până se termină, pagina se deschide ca azi: motorul sistemului unde e (macOS, Windows),
    altfel în browser.
  - Refuzat: tot ca azi, fără să mai întrebăm la fiecare pagină.
- **Unde**: în dosarul de date al aplicației (`<date>/web-engine/<versiune>/`), nu lângă
  program. Așa funcționează fără administrator și nu e atins de actualizările aplicației.
- **Actualizările**: vezi secțiunea următoare.
- **În Setări**:
  - starea motorului (versiunea, mărimea);
  - „Șterge” (se descarcă din nou la nevoie) și „Caută actualizări”.

## Versiunile motorului și actualizarea lui (cerut 2026-11-24)

Cerut: „să avem control să incrementăm versiunea […] dacă au probleme și fac fixuri la
Chromium, userul să beneficieze”, iar actualizarea să fie „curată și controlată: se curăță
ce e vechi și nu mai e nevoie”.

### Versiunea

- Motorul are versiunea lui, separată de a aplicației: `<versiunea Chromium>-<a noastră>`, de
  exemplu `146.0.7680.80-2`.
  - Prima parte e a Chromium-ului din CEF.
  - A doua o creștem noi la orice pachet nou cu același Chromium (o reparație a noastră în
    `web-host`, altă configurație).
- **Ce versiune merge cu ce aplicație**: canalul dintre aplicație și `web-host` are o versiune
  de protocol (un număr).
  - Aplicația spune ce protocol vorbește.
  - Pachetul spune ce protocol are `web-host`-ul lui.
  - Un pachet cu alt protocol nu e folosit, ci se descarcă unul potrivit.
  - Astfel o aplicație veche nu ia un motor pe care nu-l înțelege, iar una nouă nu pornește
    un `web-host` vechi.
- **Unde e scrisă**: în `Cargo.toml` al crate-ului `web-host` (ca versiunea aplicației), citită
  de `cargo xtask` la pachetare. Creșterea e un commit ca `1.0.0-beta.N`, cu tag-ul lui
  `web-engine-v<versiune>`.

### Publicarea

- **Workflow-ul**: unul de CI al lui (`web-engine.yml`), pornit de tag-ul `web-engine-v*`.
  Pentru fiecare sistem și arhitectură:
  - descarcă distribuția CEF a versiunii alese;
  - construiește `web-host` și ajutătoarele;
  - le semnează (macOS: Developer ID și notarizare; Windows: SignPath, ca aplicația);
  - face arhiva și o pune într-un release al lui, ca ciornă, publicată de mână, ca release-urile
    aplicației.
- **Lista pachetelor**: `web-engine.json`, semnat ca `latest.json` al aplicației.
  - Pentru fiecare sistem: versiunea, protocolul, adresa, mărimea, SHA-256.
  - Mai are o versiune minimă sigură: una mai veche e înlocuită chiar dacă merge (o breșă
    gravă).

### Actualizarea, la utilizator

- **Când se uită**:
  - la pornire, cu același ritm și aceeași setare ca actualizările aplicației (se pot opri);
  - din Setări › Motorul web › „Caută actualizări”.
- **Pașii**:
  1. **Descărcată în fundal**, într-un dosar de lucru (`<date>/web-engine/.staging/`), cu
     progres, reluare și oprire, ca descărcările serverelor de limbaj.
  2. **Verificată**: semnătura listei, SHA-256 al arhivei, mărimea. Ceva nu se potrivește:
     totul șters, nimic folosit, spus în jurnal.
  3. **Desfăcută** în `<date>/web-engine/<versiune>/`, cu regulile de despachetare pe care
     le avem deja: fără căi în afara dosarului, fără legături simbolice, limite de mărime și
     număr de fișiere.
  4. **Aleasă**: un fișier mic `<date>/web-engine/current` e scris atomic, prin redenumirea
     unui fișier temporar, ca la setări. Până în clipa asta, versiunea veche era singura
     știută.
  5. **Folosită de la următoarea pornire a lui `web-host`.** Paginile deschise nu sunt
     întrerupte. `web-host` se oprește oricum când nu mai e nicio pagină. Dacă rulează de mult
     (pagini mereu deschise), la următoarea pornire a aplicației.
- **Curățenia, controlată**:
  - versiunea veche e ștearsă după ce niciun `web-host` nu mai rulează din ea. Fiecare
    `web-host` ține un lacăt pe dosarul versiunii lui, iar un dosar fără lacăt și diferit de
    `current` se șterge;
  - se uită la pornirea aplicației și după fiecare oprire a lui `web-host`;
  - tot atunci se șterg: `.staging` rămas de la o descărcare întreruptă, arhivele descărcate
    (nu se păstrează după desfacere), datele temporare ale versiunilor vechi.
- **Ce nu se șterge**: profilul paginilor (`<date>/web-engine/profile/`: stocarea locală a
  draw.io, cookie-urile paginilor aplicației), separat de versiune ca să treacă de la una la
  alta. Se șterge doar la „Șterge motorul” din Setări.
- **Întoarcerea**: o versiune nouă care nu pornește (`web-host` cade la pornire de două ori)
  se marchează rea.
  - `current` revine la cea dinainte, păstrată până când cea nouă a mers o dată.
  - Se spune în jurnal și într-o notificare.
  - Nu se mai încearcă până la următoarea versiune publicată.
  - De aceea, în afară de `current`, ținem cel mult una în plus: ultima care a mers, până când
    noua a mers o dată.

### Ce vede utilizatorul

- Prima dată: întrebarea cu mărimea, apoi progresul în bara de stare.
- La actualizări: nimic, sau o notificare scurtă dacă au fost descărcate cât era pe date
  mobile sau limitate (cum face aplicația).
- În Setări › Motorul web:
  - versiunea, mărimea pe disc, data ultimei verificări;
  - „Caută actualizări” și „Șterge motorul”.

### Teste

- Lista semnată: o semnătură greșită, un SHA greșit, o mărime greșită sunt refuzate.
- Despachetarea refuză căile în afara dosarului și legăturile.
- `current` scris atomic: o întrerupere la mijloc lasă versiunea veche aleasă.
- Curățenia:
  - nu șterge o versiune cu lacăt sau pe `current`;
  - șterge `.staging` și versiunile vechi fără lacăt;
  - păstrează profilul.
- Întoarcerea: o versiune care cade la pornire de două ori revine la cea dinainte.
- Protocolul: un pachet cu alt protocol nu e ales.

## Ce se schimbă la noi

- **`platform_api`**:
  - `WebView` rămâne contractul pentru module și aplicație;
  - o implementare nouă, `web-engine` (un crate în `infra`), vorbește cu `web-host` și
    desenează pagina ca imagine în scenă;
  - implementările de azi (wry) rămân rezerva.
- **În scenă**: o imagine nouă, „textura unei pagini” (`render_api`), adusă de renderer din
  handle-ul primit, la fiecare cadru nou al paginii. Doar zona schimbată, cât se poate.
- **Intrările**: ce primește azi fereastra peste o pagină (mouse, taste, text, IME, rotița,
  drag & drop) trimitem lui `web-host`. Contractul `Control` al vederilor are deja tot ce
  trebuie (`press`, `drag`, `release`, `hover`, `scroll`, `key`, `text`).
- **Ce se scoate, când motorul e folosit**:
  - straturile plutitoare (`floating_*`);
  - găurile din pagini (`web_holes_*`);
  - stratul `FloatingLayer` în general;
  - pe Windows, planul 98.

  Rămân doar pentru rezervă.

## Costuri

- **Mărime**: aplicația nu crește. Motorul se descarcă o dată, cam 100 MB comprimat și 250 MB
  pe disc (de măsurat la 99a pe fiecare sistem).
- **RAM**:
  - cât e o pagină deschisă: procesele Chromium, de obicei 150–300 MB (de măsurat cu draw.io);
  - fără pagini: nimic, pentru că `web-host` se oprește.
- **GPU**:
  - o textură cât pagina, pe pagină (cum ține azi motorul sistemului);
  - fără ferestre sau straturi în plus.
- **Muncă**: cel mai mare plan de până acum pe partea de platformă. Câteva săptămâni, pentru
  că pe trei sisteme facem noi intrările, popup-urile, IME-ul, accesibilitatea și ambalarea.
- **Securitate**:
  - Chromium e al nostru de ținut la zi (vezi descărcarea);
  - paginile aplicației păstrează regulile de azi: nimic de pe disc sau din rețea, nicio
    navigare în afară, nicio fereastră nouă. Un test cu un server de probă pe loopback le
    verifică.
- **Licențe**: CEF e BSD, Chromium are licențele lui. Se adaugă la lista de licențe arătată în
  aplicație.

## Pașii

Fiecare pas se termină cu `./scripts/check.sh` 4 ok și o încercare pe sistemele lui.

- **99a. Încercarea, pe macOS** (cel pe care îl putem rula aici):
  - `web-host` minimal, care deschide draw.io în OSR și trimite cadrele ca `IOSurface`;
  - aplicația le desenează ca textură; mouse-ul și tastatura merg.
  - **De măsurat**: mărimea motorului, RAM-ul, GPU-ul, întârzierea unui cadru, pornirea. Cu
    cifrele, hotărâm dacă mergem mai departe.
- **99b. Descărcarea**:
  - pachetul motorului făcut în CI pentru fiecare sistem, semnat și pus la release;
  - întrebarea, descărcarea cu progres și verificarea semnăturii;
  - pagina din Setări.
- **99c. Intrările complete**:
  - tastatura cu scurtăturile aplicației (care rămân ale noastre), IME-ul și focusul;
  - listele derulante (`OnPopupShow`), meniul contextual al paginii, drag & drop.
- **99d. Windows**: texturile D3D11 partajate, aceeași listă de încercări ca la 99a și 99c.
- **99e. Linux**: dmabuf în Vulkan, cu rezerva prin procesor; încercat pe Intel/AMD și pe
  NVIDIA.
- **99f. Accesibilitatea**: arborele paginii legat sub nodul ei din arborele nostru
  (`ui_kit::access`), ca să-l citească VoiceOver, Narrator și Orca.
- **99g. Actualizările motorului** (secțiunea „Versiunile motorului”):
  - lista semnată, descărcarea în fundal, alegerea atomică, curățenia cu lacăte, întoarcerea
    la versiunea dinainte;
  - workflow-ul de publicare.
- **99h. Curățenia codului**: straturile plutitoare și găurile rămân doar pentru rezervă.

## Rezerva

Până când motorul e descărcat (sau dacă e refuzat ori nu pornește), paginile merg ca azi:
- **macOS și Windows**: motorul sistemului (wry). Pe Windows, cu stratul plutitor ca fereastră
  popup.
- **Linux**: în browser.

Planul 98 (WebView2 visual hosting) nu mai e nevoie să fie făcut. Rămâne scris, ca variantă
fără descărcare, dacă 99a arată cifre pe care nu le vrem.

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
