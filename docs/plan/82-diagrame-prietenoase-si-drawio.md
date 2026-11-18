# 82 — Diagrame: un editor mai prietenos, și draw.io însuși, la cerere

**Stare (2026-11-17): propus. Pe branch-ul `drawio` (separat de `main`). De făcut în ordine
(82a → 82g); 82e–82g (draw.io) depind de cercetarea de mai jos.**

Cerut 2026-11-17: „l-aș face mai prietenos, acum îmi e greu să leg elementele între ele: să
trasez elemente, să leg elemente, să pun texte, să am mai multe opțiuni de desenare”; „aș
încerca și un modul draw.io inclus, cu opțiune de descărcare, să nu avem un binar mare, și
să avem măcar suport pentru culori în funcție de temă — cred că trebuie să ținem noi niște
style-uri să facem overwrite după download”.

## Ce avem (verificat în cod)

- **Legături**: peste o formă apar punctele ei de conexiune — doar mijloacele laturilor,
  pătrățele de 8 px (`view/connect.rs`, `edit::connection_points`); o legătură pornește
  doar trăgând exact din unul. Lăsată peste o formă, o leagă; pe foaie, capătul rămâne liber.
  O muchie aleasă are mânere (capete, puncte, mijloace).
- **Texte**: dublu clic pe o celulă îi editează textul pe loc (`view/text.rs`); pe foaia
  goală, dublu clic nu face nimic. Un text nou vine doar din paletă („Text”).
- **Forme**: paleta (5 secțiuni: general, flowchart, entity-relation, UML, săgeți) le pune
  prin tragere; desenul știe dreptunghi (rotunjit), elipsă, romb, triunghi, paralelogram,
  hexagon, cilindru, nor, document, proces, actor, notă, săgeată, săgeată dublă, swimlane.
- **Bara**: paleta, undo / redo, șterge, față / spate, zoom, proprietăți. Nicio unealtă de
  desenat (totul e „selectează și trage”).
- **Web**: aplicația arată pagini cu motorul sistemului (`platform_api::Window::web_view`,
  `wry` 0.57): doar HTML dat, **fără JavaScript**, fără navigare (pagini neîncrezătoare: un
  răspuns HTTP). Nicio pagină de încredere, niciun mesaj între pagină și aplicație.
- **Descărcări la cerere**: serverele de limbaj (`modules/tools/lsp`) au deja tiparul: o
  ofertă în bară (mărime), descărcare cu progres, „offline”, eșec cu motiv.

## Partea întâi: editorul nostru, mai prietenos

### 82a — Legături ușoare

- **Săgețile din jurul formei** (ca la draw.io): peste o formă, câte o săgeată mică în afara
  fiecărei laturi. **Clic** pe una: o formă la fel, alături în direcția ei (la distanța
  grilei), legată de ea, aleasă. **Tras** din una: o legătură nouă.
- **Ținta se vede**: trăgând o legătură, forma de sub mouse se luminează toată (conturul în
  accent), iar punctul de prindere cel mai apropiat (laturi, colțuri, mijloace) se arată; lăsată
  oriunde pe formă, se leagă (de punctul arătat, sau „plutitor” spre centru, ca draw.io când
  e lăsată în mijloc).
- **Din contur**: o apăsare lângă marginea formei (o bandă de câțiva pixeli, cursorul
  arată „+”) pornește o legătură, fără să nimerești un punct.
- **Lăsată pe foaie**: capătul rămâne liber (ca acum); cu ⌥ ținut, o formă la fel apare
  acolo, legată.
- Punctele mai mari (10 px) și o zonă de prindere mai largă decât desenul lor.

**Făcut.** `view/connect.rs`: săgețile (`Side`, `arrows`, `arrow_at`) într-un disc în
accent, la 6 px de laturi, ținute „ale formei” cât mouse-ul e pe ele; `Gesture::Arrow`: clic
→ `make_beside` (stilul și mărimea formei, fără text, la 40 de unități, aceeași părinte;
forma și legătura într-un singur pas), tras pe altă formă → legătură, tras pe foaie → formă la
fel acolo (centrată, pe grilă), legată. Banda de 5 px din interiorul conturului (`outline_at`,
nu pe mânerele de redimensionare, nu la forme sub 30 px) pornește o legătură. Ținta se
luminează (contur + umplere ușoară în accent) cât se trage o legătură sau un capăt. Punctele
10 px, prinse pe 16 px. Teste: `a_shape_s_arrow_makes_a_joined_shape_beside_it`,
`a_shape_s_arrow_dragged_joins_or_makes_a_shape`, `an_edge_is_drawn_from_a_shape_s_outline`;
verificat în snapshot (`EDITOR_SNAPSHOT_HOVER`).

**Lăsat.** Prinderea de un punct fix al formei (`exitX`/`entryX`): legătura se prinde
„plutitor” (spre centru), ca înainte; ⌥ la lăsare (inutil: săgeata face deja forma).

### 82b — Texte ușoare

- **Dublu clic pe foaia goală**: un text nou acolo, cu câmpul deschis.
- **Tastat cu o formă aleasă**: îi înlocuiește textul (ca în draw.io: F2 / Enter îl
  editează păstrându-l).
- **Dublu clic pe o legătură**: eticheta ei (există); una nouă se pune la mijloc.

**Făcut.** Dublu clic pe foaia goală: textul din paletă („Text”, 60 × 30) centrat acolo, pe
grilă, câmpul deschis cu „Text” ales (`new_text_at`); `edit_text` ia geometria celulei cât
încă nu e desenată. Test: `a_double_click_on_the_sheet_makes_a_text`.

**Lăsat.** „Tastat cu o formă aleasă îi înlocuiește textul”: aplicația dă literele unei
vederi doar cât ea cere tastele (`wants_text`), iar atunci ⌘D și lipirea celulelor merg pe
alt drum (s-ar strica). ↩ și dublu clic editează textul.

### 82c — Unelte de desenat

- Pe bară, un grup de unelte: **Selectează (V)**, **Dreptunghi (R)**, **Elipsă (O)**,
  **Romb (D)**, **Text (T)**, **Legătură (L)**. Cu o unealtă de formă, trasul pe foaie
  desenează forma în dreptunghiul tras (cu Shift: pătrat / cerc); cu Legătura, trasul de la o
  formă la alta le leagă, iar pe foaie desenează o linie liberă. După o formă, unealta
  revine la Selectează (ținută cu dublu clic pe ea, ca în alte editoare).
- **Stil rapid** pentru ce e ales, deasupra lui: culoarea de umplere și de contur (culorile
  temei întâi), linie punctată, capetele legăturii, felul ei (dreaptă, în unghi drept,
  curbă).
- Desen liber (creion): **lăsat** — draw.io îl salvează ca formă proprie, pe care n-o
  desenăm; mai târziu, dacă e cerut.

**Făcut.** `view/draw.rs`: grupul de unelte pe bară (Selectează, Dreptunghi, Elipsă, Romb,
Text, Linie; iconițele modulului), `Gesture::Draw` (cutia trasă, colțurile pe grilă, cel puțin
un pas; ⇧ pătrat; un clic: mărimea obișnuită acolo; Textul își deschide câmpul) și
`Gesture::Line` (de la o formă la alta: legate; altfel o linie liberă, `endArrow=classic`).
După o formă unealta revine la Selectează; dublu clic pe ea o ține; Esc o lasă. Peste foaie,
cu o unealtă, săgețile formelor nu apar (unealta desenează). `view/quick.rs`: bara rapidă
deasupra a ce e ales (sub, dacă nu e loc): 8 perechi de culori (umplere + linie; o legătură ia
linia, un text culoarea ei; prima: ale temei), linie punctată, pentru legături felul (dreaptă,
în unghi drept, curbă) și săgeata de la capăt — un pas fiecare (`ops::set_styles`, mai multe
chei odată). Teste: `view_draw.rs`, `view_quick.rs`,
`ops::several_style_keys_are_set_as_one_step`.

**Lăsat.** Litere pentru unelte (V, R, O…): o literă tastată merge la câmpurile vederii
(căutarea paletei, numerele panoului); de pus când cere cineva.

### 82d — Mai multe forme

- Paleta: secțiunea „Forme de bază” cu ce desenăm deja și nu oferim (triunghi, hexagon, nor,
  notă, săgeți) și câteva noi, desenate de noi (stea, cruce, trapez, pentagon, cerc,
  dreptunghi tăiat), cu scrierea lor în stilul draw.io, ca fișierul să se deschidă la fel acolo.

**Făcut.** Opt forme noi, desenate cu geometria draw.io (`vector-tiles/shapes.rs`): trapez,
pas, card, bulă, bandă, întârziere, conector în altă pagină, intrare manuală, cu stilurile
lor din draw.io (`shape=trapezoid`, `step`, `card`, `callout`, `tape`, `delay`,
`offPageConnector`, `manualInput`) — în secțiunea nouă „Forme de bază”. Teste: fiecare în
cutia ei, își umple mijlocul, colțurile tăiate goale; catalogul desenează fiecare formă.

**Lăsat.** Steaua, crucea, pentagonul: în draw.io sunt forme din biblioteci
(`mxgraph.basic.*`), cu geometria lor; nu le ghicim.

## Partea a doua: draw.io însuși, la cerere

### 82e — Pagini de încredere în motorul web (platformă)

- Un al doilea fel de pagină, pe lângă cea de azi: **o pagină a aplicației** — un folder
  local servit printr-un protocol al nostru (nu `file://`), cu JavaScript, **fără rețea**
  (navigarea doar în folderul ei, CSP care oprește orice cerere în afară), cu **mesaje în
  ambele sensuri** (ale paginii vin pe un canal, cu waker; ale noastre intră ca script).
- Paginile de azi (un răspuns HTTP) rămân cum sunt: fără JavaScript.

**Făcut.** `platform_api::WebSource::{Html, App { root, entry }}`, `WebEvent::{Message,
OpenLink}`; `Window::web_view(source, bounds, data, inbox)`, `WebView::send`. Paginile-aplicație
(`platform-winit/web_app.rs`): servite din folder prin `local-app://localhost/…` (WebView2:
`https://local-app.localhost/`), fișierele citite pe workerul `web-app` (răspuns asincron),
căile rezolvate strict în folder (fără `..`, `\`, `:`, nume de dispozitiv, legături în afară),
doar GET/HEAD, cel mult 128 MB un fișier, MIME + `nosniff`, CSP pe HTML (fără rețea: totul
`'self'`); navigarea doar în aplicație, ferestre noi refuzate (vin ca `OpenLink`), descărcări
refuzate, devtools oprite. Mesajele: pagina → `window.ipc.postMessage` → coada ferestrei (cel
mult 1024 / 256 MB, 64 MB unul) → `Viewer::web_event` al vederii cu acel id; vederea →
`take_web_sends` → `window.__hostReceive(<literal JSON>)` (U+2028/2029 și `<` scăpate).
Teste: rezolvarea căilor (inclusiv o legătură în afară), MIME, CSP, URL-uri permise,
literalele, cozile și rutarea pe id.

**Lăsat.** Verificarea în aplicație pe macOS și Windows (o face 82f, cu draw.io); paginile
rămân incognito (nimic păstrat între deschideri).

### 82f — Modulul draw.io (descărcat la cerere)

- **Nimic în binar**: la prima „Deschide în draw.io”, o ofertă în bara diagramei (mărimea,
  versiunea) ca la serverele de limbaj; descărcat din release-ul oficial, **o versiune
  fixată și verificată** (suma ei ținută la noi), în folderul de date al aplicației (nu în
  proiect), doar fișierele modului embed.
- **Un tab cu draw.io** pentru fișierul `.drawio` (o acțiune, nu în locul editorului nostru):
  fișierul încărcat prin mesaj, salvările lui (autosave) scrise de noi, pe worker, prin
  fișier temporar; o schimbare de pe disc reîncărcată dacă nu e nimic nesalvat acolo.
- **Există doar cât e folosit**: pagina e făcută la deschiderea tab-ului, lăsată la
  închidere.

**Făcut.** Întâi descărcările comune: `fetch.rs`/`archive.rs` din `lsp-client` mutate în
`crates/infra/files/pinned-downloads` (stratul `infra-shared`), gazdele permise date de cine
descarcă (`allowed(url, hosts)`, `Http::new(hosts)`), un filtru al apelantului în `Rules`,
`digest` (hex/base64), `paths` (folosit și de `xtask` prin `#[path]`), `samples` (arhive în
memorie, feature `fake`); `lsp-client` își păstrează `HOSTS` și re-exportă tot (testele lui
trec neschimbate). Modulul `crates/modules/tools/diagram-web` (`tool.diagram-web`):
- **Pachetul** (`package.rs`): draw.io `v32.6.10`, `draw.war` (49 729 549 B) din release-ul
  GitHub, SHA-256 `a57b06b3…a38f` verificat de noi pe o descărcare proprie; doar
  `github.com` + stocarea lui de release-uri. Instalat în `<data>/web-apps/diagram/32.6.10/`
  (niciodată în proiect): `.part` reluat, despachetat de codul nostru într-un `.tmp-<uuid>`,
  `NOTICE.txt` (Apache 2.0, atribuire, fără afiliere) și marcajul scrise, apoi redenumit în loc;
  resturile (temporare, alte versiuni) șterse. Păstrat doar ce folosește embed (`kept`): fără
  `WEB-INF`, `META-INF`, `templates`, `plugins`, `connect`, sursele de dezvoltare
  (`js/diagramly`, `js/grapheditor`, `js/orgchart`, `mxgraph/src`), `integrate`/`viewer*`,
  service worker, paginile altor gazde, `*.map`, `tail.bin` (6 MB de umplutură nefolosită),
  limbile în afară de `dia.txt`/`dia_ro.txt`. **Măsurat: 45,5 MB pe disc în 2386 de fișiere**
  (din 129 MB ai arhivei). Verificat live: zero cereri către fișiere lipsă la încărcare.
- **Oferta** (`install.rs`, `view.rs`): starea instalării e a procesului (doar memorie la
  pictare; căutarea și descărcarea pe workerul `diagram-web-install`, vederile trezite la
  fiecare schimbare, progresul la cel mult 1%); în tab: titlul, versiunea, mărimea, atribuirea,
  Descarcă → progres + Oprește → offline/eșuat cu motivul + Reîncearcă.
- **Deschiderea**: comanda „Editează în draw.io” (paleta; meniul unui `.drawio` din arbore și
  al tab-ului lui; `enabled` după `target_path`, din memorie) deschide tab-ul virtual
  `<nume>.view-diagram-web` cu `OpenData { path, events }`; editorul nostru rămâne implicit.
- **Pagina** (`page.rs`): `host.html` + `host.js` scrise la fiecare deschidere din textul
  compilat; draw.io într-un iframe (`embed=1&proto=json&configure=1&offline=1&stealth=1&
  lockdown=1&plugins=0&pwa=0&splash=0&noExitBtn=1&lang=ro|en&ui=kennedy&dark=1|0`, verificate
  în `Init.js`/`App.js`/`EditorUi.js` ale versiunii; `suppressNewWindows` e o cheie de
  configurare, nu de URL); mesajele cadrului (doar ale lui) → `window.ipc`, ale noastre →
  `__hostReceive`. Iframe-ul `position:fixed; inset:0`, iar un `ResizeObserver` (plus `init` /
  `load`) îi trimite un `resize`: **verificat live** — fereastra mărită, containerul diagramei
  a crescut de la 661×556 la 1198×982 (cât aria minus bare și panouri).
- **Protocolul** (`protocol.rs`, pur): `configure` → configurarea; `init` → `load` (cu
  `autosave:1`, `exportProtocol:true`), și după o pagină refăcută; `autosave`/`save` → scris
  pe workerul `diagram-web-file` prin fișier temporar, câte unul, ultimul text așteaptă,
  apoi `status`; `is_dirty` cât e ceva de scris (sau scrierea a eșuat); `exit` → tab-ul se
  închide; `openLink` (și `WebEvent::OpenLink`) doar http/https → browserul sistemului;
  mesaje greșite / prea mari ignorate. O schimbare de pe disc (evenimentele proiectului:
  `FilesWritten`, `FileRenamed` urmat) fără nimic nesalvat → `load` din nou (`merge`, încercat
  live, a fost ignorat de draw.io în embed: nu are mereu un fișier al lui); scrierea noastră
  auzită înapoi nu schimbă nimic. Exporturile (PNG, SVG, HTML, XML…) vin ca mesaj `export` →
  dialogul „Save As” al sistemului (`PickSavePath`, lângă diagramă) → scrise pe worker.
- **Meniurile**: ce nu poate merge aici e ascuns prin `hideMenuItems` (configurarea lor):
  Import from / Import, Print, exporturile făcute de serverele lor (PDF, VSDX, URL, Embed,
  Advanced), fișierele lor (New, Open, Save As, Make a Copy, recente, biblioteci), Keyboard
  Shortcuts (pagină în fereastră nouă), Plugins, Configuration, Exit. Rămân Save (⌘S → scrierea
  noastră), exporturile lor, Edit, View, Arrange, Extras, Help (linkurile → browser).
- **Verificat live** (macOS, HOME izolat): descărcarea reală prin codul nostru (3,3 s,
  redirecționările GitHub verificate), draw.io încărcat în tab (configure → init → load),
  editări → autosave → fișierul scris (formele noi cu `fillColor=default;strokeColor=default;
  fontColor=default`), redimensionarea. **Memorie** (footprint `top`): aplicația cu tab-ul
  draw.io 89–193 MB față de 78 MB fără (același fișier în editorul nostru), plus procesele
  WebKit ale paginii: WebContent 300–400 MB, GPU ~34 MB, Networking ~6 MB — toate plecate la
  închidere (procesul aplicației oprit: WebContent dispărut).

**Lăsat.** Nu am putut verifica vizual (fără permisiuni de captură / accesibilitate pentru
agent): culorile aplicate interfeței lor, comportamentul meniurilor ascunse, ⌘C/⌘V/⌘X/⌘A/⌘Z
în pagină (WKWebView le primește întâi ca first responder; meniul nativ al aplicației ar
trebui să le primească doar dacă pagina nu le folosește — de verificat de mână), închiderea
tab-ului (doar procesul întreg), Windows (WebView2). Undo-ul draw.io se pierde la o schimbare
de pe disc (e `load`, nu `merge`): `merge` al lor diferențiază fișierul nou de cel încărcat
**la început** (`initialData`), nu de ultima salvare — după autosave-urile noastre ar aplica
din nou schimbări deja făcute (eroare de checksum); sincronizarea lor exactă (`diffSync`,
`patch`) cere patch-uri în formatul lor intern, calculate de noi. Nu e făcut fără o
verificare live. Import din fișier: ascuns, nu trecut prin gazdă (un picker + `merge` ar fi
pasul următor). Șabloanele (`templates/`) lipsesc.

**Reparat după (2026-11-17).** Două fișiere cu același nume din foldere diferite împărțeau
un tab (calea virtuală era rădăcina + numele): acum tab-ul e numit după calea fișierului în
proiect (`view_name`), iar un tab refăcut din sesiune își află fișierul din calea lui
(`file_of_view`; înainte, `Provider::open` o ignora și tab-ul rămânea gol), evenimentele
proiectului urmate când vine comanda. ⌘P / Ctrl+P: oprit în pagina-gazdă înainte să ajungă
la draw.io, `print` din frame nu mai face nimic.

### 82g — Culorile temei

- **Ale noastre, peste ale lor**: tema lor de bază după temă (deschisă / închisă), apoi
  stilurile noastre (CSS-ul și configurarea editorului) puse după descărcare, la fiecare
  deschidere: fundalul, panourile, accentul, culorile implicite ale formelor și ale
  legăturilor noi, culorile oferite — din paleta temei. Culorile din fișierele existente
  nu se schimbă (fișierul arată la fel pentru toată lumea).

**Făcut.** `theme.rs` (pur, testat): `Theme::from_palette` (culorile opace, cele translucide
puse peste ce acoperă; `dark` după paletă) → `ui=kennedy&dark=1|0` în adresă și configurarea:
`darkColor`/`lightColor` + `defaultDarkPageBackgroundColor`/`defaultDarkGridColor` pe o temă
închisă, `defaultPageBackgroundColor`/`defaultGridColor` pe una deschisă; `presetColors`:
accentul și culorile sintaxei / VCS ale temei întâi, apoi cele obișnuite ale lor (hex fără
`#`, cel mult 36); `defaultVertexStyle`/`defaultEdgeStyle` cu `default` (formele noi urmează
aspectul fără culori fixe în fișier — verificat live în fișierul scris); `css`: variabilele
interfeței lor (ambele aspecte: panouri, bare, margini, text, accent, câmpuri, dialoguri) din
paletă. Făcută o dată pe paletă (o comparație de câteva culori la pictare); o temă schimbată
cu tab-ul deschis reface pagina (alt id) dacă nu e nimic nesalvat, altfel după scriere.
`ui=dark` (vechi) nu e folosit: `kennedy` + `dark=1`.

**Lăsat.** Aspectul final al culorilor nu l-am văzut (vezi 82f); unele elemente ale lor pot
avea culori fixe în afara variabilelor CSS.

### 82h — draw.io deschide `.drawio`; editorul nostru ascuns (un „feature flag”)

Cererea: până ne întoarcem la editorul nostru, draw.io e cel care deschide diagramele; butonul
ferestrei „Diagrame” și „Edit in draw.io” dispar. Și: orice modul cu interfață poate fi
pornit, ascuns, sau oprit.

**Făcut.**
- **Un modul ascuns** (`[modules] hidden`) pornește și răspunde la ce îi cer celelalte
  (cereri, servicii, limbaje, iconițe), dar nu arată nimic al lui. Hotărăște aplicația, o
  singură dată pentru toate modulele, când adună contribuțiile (`modules::hide_contributions`,
  `extensions::Contributions::drop_from`): fără butoane de ferestre, comenzi (paleta, meniurile,
  scurtăturile), meniuri și rânduri de meniu, acțiuni din gutter, bare peste editor, semne
  (status bar, sistem, bara de sus), blocul din setări (valorile lui se aplică în continuare).
  Vizualizatoarele lui sunt întrebate ultimele (`modules::order_viewers`): nu mai sunt
  implicite pentru un fișier pe care îl deschide altcineva, dar rămân pentru ce deschid doar ele.
- **Implicit ascunse**: `settings::HIDDEN_BY_DEFAULT = ["tool.diagram-editor"]`; arătat din
  nou, e ținut minte în `[modules] shown`, ca un implicit schimbat mai târziu să nu-l ascundă
  iar. Id-urile nu se schimbă.
- **Setări › Module**: fiecare rând are, lângă comutator și „La pornire”, „Ascuns” (de la
  următoarea pornire, ca pornit / oprit); cât modulul e oprit, „Ascuns” rămâne cum era.
- **draw.io deschide și fișierul însuși** (`ViewerKind::Replace` pentru `.drawio`), întrebat
  după editorul nostru (înregistrat înaintea lui): cât al nostru e arătat, nimic nu se
  schimbă; ascuns sau oprit, dublu clic deschide tab-ul draw.io al fișierului (cu oferta de
  descărcare cât nu e instalat). Tab-ul urmează redenumirile (`Viewer::moved`) și schimbările
  de pe disc (`file_changed`). „Edit in draw.io” e ascuns cât draw.io e deja cel care
  deschide (`presence::opener`, spus de aplicație).
- Teste: contribuțiile unui modul ascuns lipsesc (fereastră, comandă), el rulează; un modul
  ascuns (terminalul) tot răspunde la `RunInTerminal`; setarea dus-întors și implicitul;
  `.drawio` deschis de editorul nostru arătat și de draw.io când e ascuns.

**Lăsat.** Schimbarea ține de la următoarea pornire (contribuțiile se citesc o dată), ca
pornit / oprit. Un modul ascuns care își folosește propria fereastră pentru ce i se cere
(terminalul jos) nu o mai are: e alegerea celui care îl ascunde.

**Corectat după (2026-11-17).** Fereastra „Diagrame” (lista fișierelor `.drawio` din
`<proiect>/diagrams/`, Diagramă nouă, foldere, redenumiri, mutări) dispăruse cu editorul
ascuns, deși nu ține de el: e acum modulul ei, `diagram-files` (`tool.diagram-files`), mereu
arătat. Un fișier din ea se deschide ca orice fișier, cu ce deschide `.drawio` (draw.io cât
editorul nostru e ascuns). Pastilele resurselor fixate scurtează pagina draw.io ca fereastra
fixată (înainte o „acopereau”: pagina rămânea la „Opening draw.io…” sau era înlocuită de
imaginea ei, fixă).

### 82i — Pagina rămâne văzută sub ferestrele deschise peste ea

O pagină a motorului web e o vedere a sistemului, mereu peste ce desenăm noi: cât e deschis
ceva peste ea (About, un dialog, un meniu, o notificare) trebuie ascunsă, și rămânea gol.

**Făcut.** Când se deschide ceva peste o pagină văzută, i se cere o imagine
(`WebView::snapshot`; pe macOS `takeSnapshotWithConfiguration` din WebKit, desenată în
pixeli RGBA de mărimea ei pe ecran). Pagina rămâne pe ecran până vine imaginea (cel mult
500 ms), încă un cadru ca imaginea să fie desenată sub ea, apoi e ascunsă; imaginea e
desenată în locul ei, sub ce s-a deschis (estompată de fundalul unui modal ca restul).
Imaginea pleacă imediat ce pagina se vede din nou sau tab-ul ei nu mai e văzut (doar pe
GPU, ținută cât e acoperită). Măsurat: 1788×1312 px, ~300 ms în build-ul de debug.

**Găuri în pagină (macOS).** Ce plutește peste o pagină (pastilele resurselor fixate,
fereastra lor, notificările, tooltip-urile) nu o mai ascunde și nu o mai scurtează: e
decupat din ea. Stratul paginii primește o mască (`CAShapeLayer`, regula even-odd) cu
formele desenate acolo (fiecare pastilă și pinul, rotunjite ca ele; fereastra; pastila
tooltip-ului și săgeata lui), iar un monitor local de evenimente dă mouse-ul din găuri
ferestrei (aplicației): o apăsare începută într-o gaură e a aplicației până la eliberare,
una începută pe pagină rămâne a paginii. Orientarea măsurată la rulare: vederea paginii e
„flipped”, masca la fel (`contentsAreFlipped`), y de sus. Forma e trimisă paginii doar când
se schimbă (pastilele: când se schimbă lățimea unei valori). Imaginea paginii rămâne pentru
ce o acoperă mult (About, dialoguri, meniuri, fișiere trase peste fereastră).

**Lăsat.** Windows (WebView2 `CapturePreview` dă un PNG asincron; găurile ar fi o regiune
pe fereastra lui): deocamdată pagina se ascunde fără imagine și se scurtează sub ce e fixat,
ca înainte. Umbra a ce plutește nu trece peste pagină (gaura e exact forma). O pagină deja ascunsă când se deschide ceva (alt tab) nu
are imagine (n-are ce arăta oricum).

## Cercetarea despre draw.io (2026-11-17, v32.6.10)

- **Embed**: `embed=1&proto=json&configure=1` + `offline=1&stealth=1&lockdown=1`,
  `plugins=0`, `pwa=0`, `splash=0`, `suppressNewWindows=1`; mesajele: `configure` →
  `{action:'configure', config}`, `init` → `{action:'load', xml, autosave:1, title}`,
  `autosave`/`save {xml}` → scris pe disc + `{action:'status', modified:false}`, `exit`;
  `openLink` deschis de noi; `merge {xml}` pentru o schimbare de pe disc.
- **Culori**: `darkColor` / `lightColor`, `defaultPageBackgroundColor`,
  `defaultDarkPageBackgroundColor`, `defaultGridColor`, `defaultVertexStyle` /
  `defaultEdgeStyle` (doar formele noi), `presetColors`, `defaultColorSchemes`, `css` (pentru
  interfața lor), `dark=1`. Formele cu `fillColor=default` urmează tema la afișare, fără să
  schimbe fișierul; culorile explicite rămân (modul „adaptive” e al paginii, scris doar dacă
  utilizatorul îl schimbă).
- **Pachetul**: `draw.war` din release-ul GitHub (`v{V}/draw.war`), 49,7 MB comprimat,
  ~120 MB desfăcut; fără `WEB-INF/lib`, `integrate.min.js`, `viewer*.min.js`, șabloane:
  rămân ~40–50 MB (de măsurat). Servit dintr-un protocol al nostru (XHR din `file://` e oprit
  în WebKit / WebView2). Suma SHA-256 fixată la noi (GitHub o arată, jgraph nu semnează).
- **Licență și nume**: cod Apache 2.0 (păstrăm `LICENSE`/`NOTICE`); „draw.io” e marcă
  înregistrată: modulul are un nume generic, cu atribuire. README-ul lor numește repo-ul „o
  versiune de test, pentru evaluare” (există o versiune plătită pentru producție) — de știut.

## Review (2026-11-17)

Două review-uri separate (editorul nostru; paginile web și draw.io), reparate:

- **Bara rapidă** recalcula unde stă la fiecare cadru și mișcare de mouse căutând fiecare
  celulă aleasă printre toate: totul ales pe 2.000 de forme + 2.400 de legături, un cadru
  44,8 ms → **0,74 ms**, o mișcare 28 ms → **1,8 µs** (o trecere peste desen la paint, mouse-ul
  citește bara pictată).
- Un clic (fără tras) pe conturul unei forme sau pe un punct făcea o legătură de lungime
  zero: acum alege forma. Dublu clic pe o săgeată făcea și un text. Mânerele laterale ale
  formei alese erau prinse de punctele de conexiune. Bara stătea peste săgeata de sus.
- Un text nou e făcut abia când se tastează ceva (un pas; lăsat gol: nimic, nicio celulă
  invizibilă). „Curbă” se scrie ca la draw.io (`orthogonalEdgeStyle;curved=1`). O săgeată pe
  un grup face o cutie simplă; trasă înapoi pe forma ei, nimic.
- Banda, întârzierea, bula: geometria exactă din draw.io v32.6.10 (citită din codul lor).
- Un singur helper pentru grilă (`grid_point`), unul pentru ecran → diagramă (`at`).
- **draw.io**: o scriere nu mai trece peste o schimbare făcută de altcineva (un `git pull`):
  verificată pe worker, fișierul rămâne al lor, ce era în pagină e salvat alături („(al
  meu)”), spus; fișierul e privit la 2 s cât tab-ul e văzut (un tab refăcut din sesiune nu
  aude evenimente); un fișier dispărut nu mai e luat drept diagramă goală; „Salvează” la
  închidere scrie (înainte, tab-ul virtual era „fără nimic de salvat”); ce aștepta să fie
  scris se scrie și când tab-ul se închide; ce cere pagina (închidere, link, dialogul de
  export) se face la cadrul ei, nu la următoarea trezire; o pagină ascunsă e lăsată la 60 s
  chiar fără alte cadre (WebKit: sute de MB); o temă nouă așteaptă următoarea salvare (nu
  pierde ce e în pagină); o redenumire înainte de prima citire citește din nou.

**Lăsat.** Parsarea JSON a mesajelor mari (o diagramă de 5–20 MB cu imagini) pe firul UI;
clipboard-ul paginii și deschiderea linkurilor fără confirmare (o vulnerabilitate a draw.io
ar fi nevoie; riscul e mic); un export anulat ține octeții până la următorul.

## Lăsat deocamdată

- Desenul liber (creionul), layout-urile automate, bibliotecile de forme ale draw.io în
  editorul nostru: în draw.io însuși (82f) le are pe toate.
