# 86 — Paginile web sub interfața aplicației, fără decupaje (analiză)

## Problema

Aplicația își desenează singură toată interfața, pe GPU, într-o singură suprafață. O pagină
web (draw.io, răspunsul HTML din clientul HTTP) e componenta sistemului: WebView2 pe Windows,
WKWebView pe macOS. Ea stă în fereastra noastră, dar e o suprafață separată, pe care sistemul o
compune **peste** a noastră. Nimic din ce desenăm nu poate trece peste ea. Aceeași limitare
o are WPF și e cunoscută sub numele de „airspace”.

Azi o ocolim cu **găuri**: decupăm din pagină locurile peste care desenăm ceva (popover-uri,
notificări, tooltip-uri, bara de resurse). Pe macOS decupăm masca stratului, pe Windows
regiunea ferestrei. Funcționează, dar:
- în gaură se vede ce e sub pagină, nu pagina, deci nu putem pune sticlă sau umbră peste ea;
- marginea e o tăietură, nu o suprapunere cu transparență.

## Variantele găsite

### A. Două straturi ale noastre, cu pagina între ele (recomandat ca pas următor)

Interfața se desenează în două suprafețe: **sub** pagină (tot ce e acum) și **peste** pagină
(ce plutește), cu transparență. Pagina rămâne a sistemului, dar e compusă între cele două.

- **macOS**: o a doua vedere (`NSView` cu `CAMetalLayer` transparent), soră a WKWebView și
  pusă deasupra ei. `hitTest:` întoarce `nil` peste tot unde nu e nimic desenat, ca mouse-ul
  să ajungă la pagină. Totul e API public. Pentru sticlă, un `NSVisualEffectView` în modul
  `withinWindow`, sub cartonaș, estompează chiar conținutul paginii. Atenție: pe forumuri
  apare un caz în care `hitTest` cu `nil` n-a lăsat click-ul să treacă printr-un `MTKView`;
  de aceea folosim o vedere a noastră, nu `MTKView`, și o testăm separat.
- **Windows**: WebView2 are un mod oficial făcut pentru „airspace”, **visual hosting**
  (`ICoreWebView2CompositionController`). Pagina devine un vizual DirectComposition pe care
  îl punem unde vrem în arborele de vizuale. Microsoft a făcut pe el
  `WebView2CompositionControl` pentru WPF tocmai ca să poată pune elemente peste pagină.
  wgpu poate desena într-un vizual DirectComposition (`SurfaceTargetUnsafe::CompositionVisual`
  pe DX12), deci avem: vizualul nostru de jos → vizualul paginii → vizualul nostru de sus,
  transparent.
  - Costul: wry nu are acest mod, deci integrăm WebView2 direct (crate-ul `webview2-com`).
  - În acest mod, mouse-ul și cursorul le dăm noi paginii (`SendMouseInput`, hit-test în
    `WM_NCHITTEST`); tastatura și focusul trebuie lucrate separat.
  - Sticla peste pagină pe Windows ar cere `Windows.UI.Composition` cu backdrop brush. Am
    găsit-o ca posibilitate, neconfirmată pentru conținutul WebView2.
- **Ce se schimbă la noi**: desenul se împarte în „sub pagină” și „peste pagină” (popover-uri,
  notificări, tooltip-uri, meniuri, modale). Avem deja straturile separate în scenă, iar
  `floating_areas` / `page_holes` știu ce plutește. A doua suprafață se desenează doar când se
  schimbă ce plutește, deci costul în GPU și RAM e mic.
- **Rezultatul**: ce plutește e desenat cu adevărat peste pagină, cu umbre și colțuri, fără
  tăietură. Pe macOS poate avea și sticlă reală peste pagină.

### B. Ferestre native separate pentru ce plutește

Fiecare popover sau notificare ar deveni o fereastră a sistemului, mereu peste pagină (ca în
WinForms). Ar fi mult cod pentru fiecare lucru care plutește, și n-ar mai fi desenat ca restul
aplicației. Nu merită.

### C. Pagina desenată de noi (randare în afara ecranului, imaginea pusă într-o textură)

Asta ar fi „cu adevărat în aplicație”: pagina devine o textură pe care o desenăm noi, sub și
peste orice.
- **WebView2**: nu are API pentru asta. Cererea e deschisă din 2019 (WebView2Feedback #20).
  Ocolișul cunoscut e să capturezi vizualul cu Windows Graphics Capture. Merge, dar adaugă
  un cadru de întârziere și o copie la fiecare cadru.
- **WKWebView**: nu are API public pentru asta. Capturarea ecranului cere permisiunea de
  înregistrare a ecranului, deci nu e o variantă.
- **CEF** (Chromium încorporat), modul off-screen cu texturi partajate: e soluția reală și
  matură (D3D11 pe Windows, IOSurface/Metal pe macOS; există crate-ul `cef` pentru Rust).
  Prețul: Chromium întreg în aplicație (sute de MB), actualizări de securitate odată cu
  Chromium și un proces în plus. Contrazice ținta noastră de RAM și mărime.
- **Servo** (motorul web în Rust) se poate încorpora și desenează în afara ecranului (există
  un exemplu pentru iced). E mic și scris în Rust, dar compatibilitatea cu aplicații mari
  precum draw.io nu e garantată. De urmărit, nu de folosit azi.

## Recomandarea

1. **Acum**: găurile (macOS deja, Windows fixul din `web_holes_windows.rs`). Merg și sunt
   ieftine.
2. **Pasul următor**: varianta A, întâi pe **macOS** (API public, câteva zile, cu sticlă
   reală peste pagină), apoi pe **Windows** cu visual hosting (mai mult de lucru, din cauza
   integrării directe cu WebView2 și a intrărilor).
3. **Nu acum**: C. CEF e prea mare pentru ținta noastră; Servo îl reevaluăm mai târziu.

## Surse

- WPF, „airspace” și visual hosting: https://learn.microsoft.com/en-us/microsoft-edge/webview2/platforms/wpf
- `ICoreWebView2CompositionController`: https://learn.microsoft.com/es-es/microsoft-edge/webview2/reference/win32/icorewebview2compositioncontroller
- Specificația `WebView2CompositionControl`: https://github.com/MicrosoftEdge/WebView2Feedback (specs/WPF_WebView2CompositionControl.md)
- WebView2 în afara ecranului, cerere deschisă: https://github.com/MicrosoftEdge/WebView2Feedback/issues/20
- Ghid de comunitate pentru visual hosting și captură: https://gist.github.com/pabloko/5b5bfb71ac52d20dfad714c666a0c428
- wgpu, swapchain din vizual DirectComposition: https://wgpu.rs/doc/wgpu/enum.Dx12SwapchainKind.html
- `hitTest` pe vederi Metal (macOS): https://developer.apple.com/forums/thread/728843
- CEF, randare în afara ecranului cu texturi: https://docs.rs/crate/cef/latest/source/src/osr_texture_import/mod.rs
- Servo încorporat (iced): https://docs.rs/crate/iced_servo/0.1.0/source/README.md

## Făcut (varianta A)

- **Stratul peste pagini** (`platform_api::FloatingLayer`, `Window::floating_layer`): o
  suprafață transparentă a ferestrei, deasupra paginilor, în care aplicația desenează ce
  plutește (ferestrele peste bară, notificările, meniurile, tooltip-urile) cu un renderer al
  ei. Pagina rămâne întreagă, cu umbre și transparență peste ea.
  - **macOS** (`floating_macos.rs`): o vedere proprie peste vederea ferestrei, cu stratul
    Metal al renderer-ului; `hitTest:` nu găsește nimic în ea, deci mouse-ul trece mai jos.
  - **Windows** (`floating_windows.rs`): o fereastră copil desenată prin DirectComposition,
    transparentă unde nu e nimic. Regiunea ei e doar unde plutește ceva (cu loc de umbră),
    iar mouse-ul de acolo e dat ferestrei aplicației.
- Stratul există doar cât e o pagină pe ecran. Găurile din pagină rămân doar pentru mouse
  (`set_holes(…, cut: false)`); se taie și desenul doar dacă stratul nu se poate face
  (`floating_failed`, încercat o singură dată).
- `WebView::set_corners`: colțurile paginii rotunjite ca ale cardului în care stă.

## Lăsat (varianta A)

- Varianta C (CEF / Servo) rămâne neîncercată, din motivele de mai sus.
