# 98 — Paginile web pe Windows în același arbore de compoziție cu aplicația (WebView2 visual hosting)

Continuă planul 86 (varianta A pe Windows). Aici e doar planul; codul vine pas cu pas, după
ce e aprobat.

**Notă (2026-11-24):** direcția aleasă e planul 99 (Chromium prin CEF, desenat de noi, pe
toate sistemele). Planul ăsta rămâne ca variantă fără descărcare, dacă 99a arată cifre pe
care nu le vrem.

## De ce

Pe Windows, o pagină web (draw.io, răspunsul HTML din clientul HTTP, previzualizări) e azi o
**fereastră copil** a WebView2, făcută de `wry`. Ce plutește peste ea (tooltip-uri, meniuri,
notificări, pastilele fixate) nu poate fi desenat în fereastra aplicației, pentru că pagina
stă deasupra. Ce avem azi:

- **Stratul plutitor ca fereastră copil** (planul 86): o fereastră copil își taie locul din
  compoziția părintelui, deci sub pixelii ei transparenți (umbra, colțurile) se vedea
  desktopul. Acestea erau „găurile” albastre raportate pe 2026-11-24.
- **Stratul plutitor ca fereastră popup** (beta.23): compus de sistem peste fereastra
  aplicației, deci fără găuri. Merge, dar e tot o fereastră în plus, care trebuie ținută lipită
  de fereastra aplicației (mutare, redimensionare, minimizare, ordinea între ferestre,
  dezactivare). E soluția pe care o folosește orice program pentru tooltip-uri și meniuri,
  dar rămâne o sincronizare între două ferestre.

**Soluția curată**: pagina nu mai e o fereastră, ci un **vizual DirectComposition** în
arborele de compoziție al ferestrei noastre. WebView2 are un mod oficial pentru asta,
*visual hosting* (`ICoreWebView2CompositionController`). Microsoft l-a făcut pentru WPF
(`WebView2CompositionControl`) tocmai ca să poată pune elemente peste pagină. Arborele
ferestrei devine:

```
vizualul rădăcină al ferestrei (IDCompositionTarget pe HWND)
├── interfața aplicației          (swapchain wgpu, opac)
├── pagina 1, pagina 2 …           (vizualele WebView2, fiecare cu clip-ul ei, colțuri rotunjite)
└── ce plutește                    (swapchain wgpu, transparent)
```

Totul e compus de DWM ca o singură fereastră:
- fără ferestre în plus și fără sincronizare la mutare;
- ordinea e mereu corectă;
- umbrele și transparența peste pagină sunt reale;
- colțurile paginii se rotunjesc cu un clip al vizualului (nu cu o regiune GDI dintată).

## Pe celelalte sisteme

- **macOS**: nu are WebView2. Motorul sistemului e **WKWebView** (WebKit, al lui Safari), o
  vedere `NSView` în fereastra noastră. Acolo avem deja soluția curată: stratul plutitor e o
  vedere soră deasupra paginii, iar Core Animation compune tot într-un singur arbore
  (`floating_macos.rs`). Planul ăsta nu schimbă nimic pe macOS.
- **Linux**: azi nu avem pagini web încorporate (`Window::web_view` e doar pe macOS și
  Windows; pe Linux pagina se deschide în browser). Dacă le adăugăm (WebKitGTK), problema se
  va pune separat.

## Ce rămâne la fel (contractele)

Aplicația și modulele nu știu nimic de asta: totul se schimbă în `platform-winit`, sub
aceleași contracte din `platform_api`.

- **`WebView`**:
  - rămân metodele `set_bounds`, `set_visible`, `load_html`, `send`, `snapshot`,
    `release_focus`, `set_corners`;
  - `set_holes` nu mai e nevoie să taie nimic pe Windows (rămâne pentru mouse, cât e nevoie).
- **`FloatingLayer`**: pe Windows devine vizualul de sus din același arbore; `set_shapes` spune
  doar unde ia stratul mouse-ul.
- **`Window::web_view`**: întoarce noua implementare pe Windows. Implementarea `wry` rămâne pe
  macOS. Pe Windows rămâne ca rezervă (mai jos).

Codul nou ține de un singur loc, `platform-winit/src/web_composition/` (un modul pe folder:
`environment.rs`, `view.rs`, `input.rs`, `tree.rs`), fără nimic din el în aplicație.

## Pașii

Fiecare pas se termină cu `./scripts/check.sh` 4 ok, clippy pentru Windows pe tot workspace-ul
și o încercare de mână pe Windows (lista de la fiecare pas). Pe macOS codul doar se compilează,
deci fiecare pas e mic și se poate încerca separat.

### 98a. Arborele de compoziție al ferestrei (fără pagini încă)

- Facem noi dispozitivul DirectComposition și ținta pe HWND-ul ferestrei. wgpu desenează
  interfața în vizualul nostru (`SurfaceTargetUnsafe::CompositionVisual`), nu în al lui
  (`Dx12SwapchainKind::DxgiFromVisual` îl face azi singur).
- Stratul plutitor devine al doilea vizual al aceluiași arbore. Fereastra popup rămâne în
  cod, dar nefolosită.
- **De măsurat**:
  - memoria GPU înainte și după (popup față de vizual);
  - timpul unui cadru;
  - întârzierea la redimensionare (`Commit` al DirectComposition față de `Present`).
- **De încercat pe Windows**:
  - interfața arată la fel (colțurile, umbra ferestrei, redimensionarea fără pâlpâire);
  - tooltip-urile și pastilele fixate peste interfață.

### 98b. Pagina ca vizual

- Mediul WebView2 creat o dată pe proces, cu dosarul de date al aplicației, cum face azi
  `wry` cu `WebContext`. În plus, controlerul de compoziție
  (`CreateCoreWebView2CompositionController`) are vizualul paginii ca `RootVisualTarget`.
- `set_bounds` mută vizualul și dimensiunea controlerului. `set_corners` pune un
  `IDCompositionRectangleClip` cu colțuri rotunjite. `set_visible` îl scoate din arbore sau îl
  pune la loc.
- Încărcarea paginilor, cu aceleași reguli ca azi:
  - **Paginile aplicației**: protocolul nostru (`web_app.rs`: fișierele din binar, nimic de
    pe disc sau din rețea), prin `AddWebResourceRequestedFilter`.
  - **Pagini HTML**: `NavigateToString`, fără scripturi.
  - **Navigarea**: blocată în afara paginii (`NavigationStarting`), ferestrele noi
    refuzate (`NewWindowRequested`).
- **Mesajele**:
  - `send` folosește `PostWebMessageAsString`, iar pagina primește prin
    `window.__hostReceive`, ca azi;
  - ce trimite pagina vine din `WebMessageReceived` în `WebInbox`.
- **De încercat**:
  - draw.io se deschide, desenează, salvează;
  - răspunsul HTML din clientul HTTP se vede;
  - colțurile rotunjite ale paginii;
  - nimic de pe disc sau din rețea nu e încărcat de o pagină a aplicației.

### 98c. Intrările

În modul ăsta, sistemul nu mai dă singur mouse-ul și tastatura paginii; le dăm noi.

- **Mouse-ul**: fereastra noastră primește tot. Peste o pagină și nu peste ce plutește
  (`FloatingLayer::set_shapes`), îl dăm paginii cu `SendMouseInput` (mișcare, butoane, rotița
  pe ambele axe, dublu-clic, ieșirea din pagină), în coordonatele ei. Cursorul paginii vine din
  `CursorChanged` și îl punem noi.
- **Trackpadul și stiloul**: `SendPointerInput` pentru atingere și creion, unde e cazul.
- **Tastatura și focusul**:
  - **Ce primește focusul**: un clic în pagină îl dă paginii (`MoveFocus`), iar unul în
    afara ei ni-l dă nouă înapoi (`release_focus`). Tab-ul care iese din pagină vine din
    `MoveFocusRequested`.
  - **Scurtăturile**: cele ale aplicației (⌘/Ctrl) ajung la noi prin
    `AcceleratorKeyPressed`, cum hotărâm azi.
- **IME**: în modul ăsta, compunerea textului (chineză, japoneză) e a paginii. De verificat că
  fereastra de compunere apare la caret.
- **Drag & drop**: fișierele trase din Explorer peste pagină. În modul ăsta, WebView2 cere
  `ICoreWebView2CompositionController3::DragEnter/DragOver/Drop`. De făcut doar dacă azi
  merge (draw.io acceptă imagini trase).
- **De încercat**:
  - în draw.io: clic, tragere, rotiță, scris în forme;
  - Tab înăuntru și în afară, scurtăturile aplicației cu pagina în focus;
  - meniul contextual al paginii;
  - tastatura chineză sau japoneză, dacă avem unde.

### 98d. Accesibilitatea

- Arborele de accesibilitate al paginii vine prin
  `ICoreWebView2CompositionController2::AutomationProvider`. Îl legăm sub nodul paginii din
  arborele nostru (`ui_kit::access`, planul 93d), ca cititorul de ecran să intre în pagină.
- **De încercat**: Narrator citește interfața, apoi conținutul paginii.

### 98e. Captura paginii și restul a ce face azi `wry`

- **Captura**: `snapshot` folosește `CapturePreview`, cum face `wry` azi (imaginile din lista
  proiectelor deschise).
- **Ce mai trebuie făcut ca azi**:
  - culoarea de fundal a paginii (`DefaultBackgroundColor`), ca să nu clipească alb la
    încărcare;
  - scara ecranului (`RasterizationScale`, `ShouldDetectMonitorScaleChanges`), cu fereastra
    mutată pe alt monitor;
  - tema întunecată (`PreferredColorScheme`);
  - devtools doar în build-urile de dezvoltare.

### 98f. Rezerva și curățenia

- Implementarea `wry` pe Windows rămâne ca rezervă, aleasă automat dacă:
  - runtime-ul WebView2 e prea vechi pentru visual hosting (de stabilit versiunea minimă);
  - controlerul de compoziție nu se poate face.
- **Scos din cod**:
  - fereastra popup a stratului plutitor, dacă rezerva nu mai are nevoie de ea;
  - regiunile GDI ale paginilor (`web_holes_windows.rs`).
- `EDITOR_WEB_HOSTING=window` forțează varianta de azi (de diagnostic, ca
  `EDITOR_NO_FLOATING_LAYER`).

## Costuri și riscuri

- **Muncă**: câteva zile, cea mai mare parte la intrări (98c). wry ne dădea gata tastatura,
  focusul, IME-ul și drag & drop-ul; acum le facem noi.
- **Verificare**: nimic din asta nu se poate rula pe macOS. Fiecare pas se încearcă pe un
  Windows real (al tău sau al lui Dimi), cu lista lui.
- **Dependențe**: `webview2-com` / `webview2-com-sys` (bindings-urile oficiale, deja în
  arbore prin `wry`) și `windows` pentru DirectComposition. Fără biblioteci noi de la terți.
- **Resurse**:
  - fără fereastra în plus;
  - un vizual și un swapchain transparent cât fereastra, cât e o pagină pe ecran (ca azi);
  - paginile, în procesele WebView2, ca azi.

  Ce câștigăm în GPU sau RAM măsurăm la 98a și scriem aici.
- **Siguranța**:
  - paginile aplicației păstrează aceleași reguli: nimic de pe disc sau din rețea, nicio
    navigare în afară, nicio fereastră nouă;
  - un test cu un server de probă pe loopback verifică ce e refuzat.

## Până atunci

Beta.23 are stratul plutitor ca fereastră popup (fără găuri, verificat doar prin compilare
până se încearcă pe Windows). El rămâne rezerva până când 98a–98c sunt făcute și încercate.

## Surse

- WebView2 și „airspace”, visual hosting: https://learn.microsoft.com/en-us/microsoft-edge/webview2/platforms/wpf
- `ICoreWebView2CompositionController` (și `…2`, `…3`): https://learn.microsoft.com/en-us/microsoft-edge/webview2/reference/win32/icorewebview2compositioncontroller
- Specificația `WebView2CompositionControl` (WPF): https://github.com/MicrosoftEdge/WebView2Feedback (specs/WPF_WebView2CompositionControl.md)
- wgpu, swapchain într-un vizual DirectComposition: https://wgpu.rs/doc/wgpu/enum.SurfaceTargetUnsafe.html
- Ghid de comunitate pentru visual hosting: https://gist.github.com/pabloko/5b5bfb71ac52d20dfad714c666a0c428
