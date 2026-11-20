# 90 — Module descărcate la cerere: aplicația ca gazdă mică (plan de evaluat)

Plan mare, de evaluat bine înainte de orice cod. Spune ce vrem, ce stă în cale, variantele
cu ce costă fiecare, ce trebuie măsurat și în ce ordine s-ar face. Hotărârile se iau după
măsurători, la porțile scrise la sfârșit.

## Ce vrem

- **Aplicația devine o gazdă mică**, cu ce e de bază: editorul, proiectul, setările,
  ferestrele, actualizările și catalogul de module.
- **Modulele se publică separat:** se construiesc din git, în CI, și se pun la dispoziție
  printr-un catalog JSON publicat.
- **Din aplicație, utilizatorul vede catalogul,** fiecare modul cu iconiță, titlu și
  descriere, ca într-o galerie. Își alege ce descarcă; restul nu ocupă nici disc, nici
  memorie.
- **Un modul se poate actualiza, dezactiva sau șterge** fără o versiune nouă a aplicației,
  dacă e compatibil cu ea.
- **Regulile de acum rămân** (`CLAUDE.md`): modulele nu se cunosc între ele și se întâlnesc
  doar prin cele patru mecanisme (contribuție, eveniment, cerere, gazdă); ce văd utilizatorii
  nu se strică.

## Ce e acum

- **76 de module în 11 grupuri:** limbaje (19), baze de date (9), unelte (14), framework-uri
  (6), aspect (5), proiect (7), AI (4), editare (5), insight (3), vizualizatoare (3), git
  (1). Fiecare e un crate Rust, legat static în același binar. Binarul macOS are 123 MB.
- **Contractul** (`extensions::Module`): `manifest`, `activate`, `preload`, `release_idle`,
  `memory`. Un modul contribuie obiecte Rust (`ctx.contribute(...)`): trait objects ținute ca
  `Box<dyn Any>` și găsite după `TypeId`. Vederile lor (`Viewer`) desenează direct în
  `Scene`, cu `TextSystem`, `Icons` și `Palette`, pe firul interfeței.
- **Selecția există deja** (`[modules] mode / disabled`): un modul dezactivat nu se
  activează, dar rămâne în binar.
- **Ce se instalează deja separat**, pe care putem construi:
  - **driverele de baze de date externe** (plan 37i): un program în orice limbaj, cu
    `driver.toml` și un protocol propriu, care rulează în procesul lui;
  - **serverele de limbaj**, descărcate și verificate (`lsp-client/downloads`);
  - **agenții și uneltele MCP**, ca procese separate;
  - **datele**: temele, pachetele de iconițe, seturile de iconițe de fișiere.
- **Actualizările** (plan 84/85): un `latest.json` semnat cu cheia de release, verificat
  înainte de orice instalare. Același lanț de încredere poate semna și catalogul.

## Ce stă în cale

1. **Rust nu are ABI stabil.** Un trait object (`Box<dyn Viewer>`) trece între două binare
   doar dacă amândouă sunt compilate cu exact același compilator și cu exact aceleași
   versiuni ale crate-urilor comune (`kernel`, `ui-components`, `workbench-model`…). O
   singură schimbare de câmp în `ui-components` strică toate modulele construite înainte.
2. **Starea globală dublată.** Un modul compilat ca bibliotecă separată (`cdylib`) își aduce
   propria copie a crate-urilor comune, deci propriile `static`: limba (`i18n`), setările,
   iconițele, registrele. Gazda și modulul n-ar mai vedea aceleași date.
3. **Semnarea pe macOS.** Cu „hardened runtime”, aplicația încarcă doar biblioteci semnate de
   Apple sau de aceeași echipă (Team ID), altfel e nevoie de un drept special care slăbește
   protecția. Pe Windows, SmartScreen și antivirusul privesc cu suspiciune un DLL descărcat.
4. **Încrederea.** Un modul nativ are acces la tot ce are aplicația: fișiere, chei, rețea.
   Numai noi îl putem publica, semnat; module de la alții ar cere izolare.
5. **Interfața.** Modulele desenează singure, direct și repede, pe firul interfeței. Peste o
   graniță (proces sau sandbox), desenul trebuie descris și trimis: alt mod de a scrie
   vederile.

## Variantele

### A. Biblioteci native (`dylib` / `cdylib`) încărcate la pornire
- **Pentru:** codul modulelor rămâne cum e; viteza e aceeași.
- **Împotrivă:**
  - Modulele sunt legate de **aceeași versiune a aplicației**: orice release le reconstruiește
    pe toate. Câștigăm doar la descărcare și la disc, nu și la actualizarea independentă.
  - `cdylib` dublează starea globală (punctul 2), iar `dylib` cu ABI-ul Rust cere ca și gazda
    să fie spartă în biblioteci dinamice: build fragil.
  - Semnarea fiecărei biblioteci, pe fiecare sistem.
  - Un modul nativ căzut poate lua aplicația cu el.
- **Măsurat de făcut:** cât scade binarul gazdei dacă scoatem, de exemplu, limbajele și
  bazele de date; dacă `abi_stable` (interfețe C stabile peste Rust) ar acoperi contractele
  noastre fără să le rescriem.

### B. WebAssembly (componente WASI, `wasmtime`)
- **Pentru:**
  - un singur fișier pe modul, pentru toate sistemele;
  - izolat: modulul vede doar ce îi dă gazda;
  - versiuni independente, prin interfețe descrise (WIT) și versionate;
  - poate fi și de la alții, odată ce avem izolarea.
- **Împotrivă:**
  - `wasmtime` adaugă 15–25 MB la gazdă (de măsurat), iar prima compilare a unui modul durează
    (se ține în cache);
  - fiecare trecere a graniței costă;
  - vederile nu mai pot desena direct: e nevoie de un set de componente declarative (liste,
    arbori, formulare, grile, butoane) pe care gazda le desenează cu `ui-components`;
  - bibliotecile native (tree-sitter în C, driverele de baze de date cu rețea, `git2`) trebuie
    compilate în WASM, sau cerute gazdei;
  - rescrierea fiecărui modul mutat.
- **Măsurat de făcut:**
  - mărimea gazdei;
  - timpul primei încărcări și al unei porniri din cache;
  - costul unui apel peste graniță (de 10.000 de ori pe cadru?);
  - un modul mic, mutat de la cap la coadă.

### C. Procese separate, cu protocol (ca driverele externe, LSP, MCP)
- **Pentru:**
  - le avem deja pentru drivere, servere de limbaj și agenți;
  - orice limbaj;
  - căderea unui modul nu ia aplicația cu ea;
  - actualizare independentă.
- **Împotrivă:**
  - potrivit doar pentru ce **lucrează fără să deseneze** (date, analiză, rețea, interogări);
  - un proces pe modul costă memorie;
  - fiecare punct de extensie are nevoie de protocolul lui.

### D. Fără cod: module de date
- Temele, pachetele de iconițe, seturile de iconițe de fișiere, framework-urile descrise prin
  date (rutele, șabloanele, convențiile), datele limbajelor (stub-uri PHP, date CSS/HTML/JS,
  fragmente de cod) și gramaticile tree-sitter compilate în WASM (tree-sitter le știe încărca
  așa).
- **Pentru:** fără risc de securitate (doar date verificate), actualizate des, independente de
  versiunea aplicației.
- **Împotrivă:** acoperă doar ce poate fi descris prin date.

## Propunerea: hibrid, în trepte

1. **Întâi catalogul și fereastra „Modules”**, valabile pentru orice fel de modul:
   - modulele de azi (încorporate) apar în galerie cu iconiță, titlu și descriere, și pot fi
     pornite sau oprite: selecția există deja;
   - catalogul publicat aduce, pe lângă ele, ce se poate descărca.
2. **Apoi ce se poate muta fără ABI:**
   - datele (D): temele, pachetele de iconițe, datele limbajelor și ale framework-urilor,
     gramaticile;
   - procesele (C): driverele de baze de date (protocolul există), serverele de limbaj
     (descărcarea există), agenții.
3. **Un experiment WASM (B)** pe două module mici, cu interfețe WIT și componente
   declarative. Se măsoară și se hotărăște la poarta 2.
4. **Doar dacă poarta 2 iese bine:** mutarea treptată a modulelor fără desen greu
   (framework-urile, calitatea codului, uneltele mici). Editorul, arborele proiectului,
   terminalul, git-ul și consola SQL rămân în gazdă, cât cer viteză de desen.
- **Varianta A rămâne respinsă** atâta timp cât modulele ar fi legate de aceeași versiune a
  aplicației: nu ne dă actualizări independente, ci doar descărcări mai mici, cu mult risc.

## Catalogul (`modules.json`)

Publicat la fiecare release, ca asset pe GitHub, semnat cu aceeași cheie ca `latest.json`;
aplicația îl verifică înainte să-l folosească. Un modul arată așa:

```json
{
  "id": "lang.zig",
  "version": "1.2.0",
  "kind": "data | process | wasm | builtin",
  "host": { "api": 3, "min": "1.0.0" },
  "title": { "en": "Zig", "ro": "Zig" },
  "description": { "en": "Zig syntax, structure and completion.", "ro": "…" },
  "icon": "icons/lang.zig.svg",
  "group": "languages",
  "depends_on": ["lang.c"],
  "platforms": ["macos-aarch64", "windows-x86_64", "linux-x86_64"],
  "size": 412345,
  "files": [{ "url": "…/lang.zig-1.2.0.zip", "sha256": "…" }]
}
```

- **`host.api`:** versiunea contractelor pe care modulul le folosește. Gazda refuză un modul
  care cere mai mult decât are ea și îl arată „necesită o versiune mai nouă”.
- **`kind`:** cum se încarcă modulul. `builtin` înseamnă că e deja în aplicație și apare doar
  în galerie.
- **Iconița:** un SVG verificat ca orice SVG venit din afară (dimensiune, fără scripturi,
  fără resurse externe), desenat în stilul iconițelor noastre. Titlul și descrierea vin
  traduse, iar unde lipsește o limbă se folosește engleza.
- **Fișierele:** un zip pe modul, verificat după `sha256` înainte de despachetare, cu limite
  (mărime, număr de fișiere, fără căi care ies din dosar).

## Fereastra „Modules”

- O galerie pe grupuri (limbaje, baze de date, unelte…), cu căutare. Fiecare card are
  iconița, titlul, descrierea, mărimea, versiunea și starea (încorporat, instalat, disponibil,
  de actualizat, incompatibil).
- **Acțiuni:** instalare, actualizare, dezactivare, ștergere, toate în fundal, cu progres.
  Dependențele se instalează împreună; înainte se spune ce mai vine.
- **Ce se schimbă la pornirea unui modul descărcat:**
  - datele și procesele se pornesc pe loc;
  - un modul WASM se încarcă la prima folosire;
  - cel încorporat se pornește la următoarea deschidere a ferestrei, dacă activarea pe loc nu
    e sigură.
- **Fără rețea:** se vede ce e instalat, iar catalogul ultimei verificări rămâne în cache.

## Construcția din git

- Același repo. CI-ul construiește catalogul și pachetele modulelor la fiecare tag; un
  modul de date poate avea și tag-ul lui, pentru o actualizare fără release de aplicație.
- Fiecare modul are în dosarul lui un `module.toml` (id, versiune, tip, titlu, descriere,
  iconiță), din care se generează catalogul. Manifestul din cod (`ModuleManifest`) se
  verifică să spună același lucru.
- Semnarea: catalogul cu cheia de release; modulele WASM sau procesele, prin `sha256` din
  catalogul semnat; programele native ale proceselor, cu certificatele de acum (Apple,
  SignPath).

## Porțile de hotărâre

1. **După fereastra „Modules” și catalog (treapta 1):** galeria are sens și pentru modulele
   încorporate? Ce module cer utilizatorii să lipsească?
2. **După experimentul WASM (treapta 3):**
   - cât adaugă la gazdă;
   - cât durează pornirea;
   - cât costă un apel;
   - cât de greu a fost de mutat un modul.
   Dacă depășește bugetele scrise atunci (de exemplu +20 MB, +50 ms la pornire, un cadru
   întârziat), varianta B se oprește la module mici sau deloc.
3. **Înainte de orice modul de la alții:** izolarea e completă (permisiuni declarate,
   aprobate de utilizator), iar catalogul are o politică de publicare.

## Ce nu facem

- Nu mutăm editorul, arborele proiectului, setările, ferestrele sau actualizările: ele sunt
  gazda.
- Nu încărcăm cod nativ descărcat, cât timp nu avem o soluție de ABI și de semnare
  verificată (varianta A).
- Nu descărcăm nimic fără ca utilizatorul să ceară. Verificarea catalogului se poate opri din
  setări, ca verificarea actualizărilor.
