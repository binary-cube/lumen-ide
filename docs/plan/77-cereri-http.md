# 77 — Cereri HTTP: fișiere `.http`, medii, înlănțuire, graf, previzualizare

**Stare (2026-11-11): aprobat; deciziile utilizatorului la final. De făcut tot, în ordine
(77a → 77k), pe branch-ul `http-client`.**

Cerut 2026-11-11 (textul în `TODO.md`; capturile, păstrate de utilizator în afara repo-ului): un modul în care se
testează cereri, ca un client HTTP dedicat sau cel integrat într-un IDE — organizate în colecții,
cu variabile și medii, salvate în proiect ca fișiere care se pot comite (mediile personale,
`.local` / private, nu se comit), cu răspunsul formatat după tip (JSON, YAML, XML), HTML-ul
randat, cererile înlănțuite (o valoare din răspunsul uneia, ca `$.data.items[0].id`, dată
alteia, cu autocompletare), lanțul văzut ca graf cu play / stop, formatoare; și fișierele
`*.http` în formatul lor obișnuit. „Super profesional, bine gândit.”

## Ce avem deja (verificat în cod)

- **Un client HTTP**: `ureq` 3 (cu rustls), în `crates/infra/media/web-fetch` — limitat cu
  intenție (fără credențiale, fără cookie-uri, doar ce e de un fel permis), pentru imaginile
  previzualizării Markdown. Bun ca bibliotecă, dar cererile utilizatorului au nevoie de un
  adaptor al lor (orice metodă, antete, corp, cookie-uri, redirecționări la alegere).
- **Consola SQL** e modelul cel mai apropiat, și contractele ei sunt generale:
  - `EditorToolbarProvider` / `EditorToolbar` (`ui-components/src/editor_toolbar.rs`): bara
    de deasupra editorului unui fișier (Run, sursa aleasă), semnele din gutter pe fiecare
    instrucțiune (`StatementMark`), instrucțiunea de la cursor evidențiată (`current`),
    comenzile ei;
  - fereastra **Services** (`ServiceProvider` / `ProjectServices`): rezultatele ca taburi ale
    unui nod (de forma „HTTP Request › #1 Status: 200 (550 ms)”);
  - lucrul care rulează, arătat în Task Manager și întrebat la închidere (`running-work`).
- **Limbajele** sunt module (`crates/modules/languages/*`): gramatică (tree-sitter, sau
  vendorată, ca la `.env`), culori, structură, pliere, completare (`DocumentAssist`).
- **Previzualizarea Markdown** randează pe GPU un subset de HTML (cel din README-uri): fără
  CSS, fără JavaScript — nu o pagină web. **Nu avem un motor web.**
- **Parolele** păstrate sigur (`crates/infra/system/secrets`: Keychain, Credential Manager, Secret
  Service; sau un fișier al proiectului criptat cu o cheie din keychain).
- **JSON / YAML / XML-ul** au deja culori, pliere și structură (modulele lor de limbaj): un
  răspuns arătat ca document read-only al limbajului lui le are pe toate, fără nimic nou.

## Ce se face de obicei

- **Fișierele `.http`**: medii în `http-client.env.json` (public, comis) și
  `http-client.private.env.json` (privat, nu se comite), scripturi JavaScript după răspuns
  (`> {% client.global.set("token", response.body.token) %}`), răspunsurile în Services,
  previzualizarea cu un Chromium încorporat (JCEF).
- **Clienții pe Electron**: își aduc Chromium întreg (sute de MB), colecții într-o bază
  locală, înlănțuire prin „response tags”.
- **Clienții pe Tauri**: folosesc **motorul web al sistemului** (WKWebView pe macOS, WebView2
  pe Windows, WebKitGTK pe Linux) prin crate-ul `wry`; colecții ca fișiere.
- **Înlănțuirea declarativă** în `.http` (aproape aceeași sintaxă), fără JavaScript:
  `# @name login`, apoi `{{login.response.body.$.token}}` (JSONPath) — exact ce a cerut
  utilizatorul.
- **Colecțiile ca fișiere text în repo** (bune de comis), medii separate.

## Regula

1. **Formatul e al lumii, nu al nostru**: fișierele `.http` în sintaxa lor obișnuită (ce
   scriu alți clienți ai formatului merge aici și invers), mediile în `http-client.env.json` /
   `http-client.private.env.json`. Înlănțuirea, declarativă: `# @name` și `{{nume.response.body.$.cale}}` / `{{nume.response.headers.X}}`.
   Ce e personal (`*.private.env.json`, `*.local.env.json`, `.env.local`-ul cererilor) nu se
   comite: modulul îl pune în `.gitignore` la creare (întrebând), și îl marchează în arbore.
2. **Domeniul nu știe de rețea, de UI sau de alte module**: un crate de domeniu (`http-
   requests`: modelul, cititorul `.http`, mediile și variabilele, JSONPath, graful
   lanțului), un port (`HttpTransport`) implementat în infra (`http-transport`, cu `ureq`),
   modulele doar leagă. Modulele nu se cunosc: limbajul `.http` și rularea comunică prin
   contractele existente (bara editorului, Services, `DocumentAssist`) și, unde trebuie, o
   cerere nouă în `contracts`.
3. **Nimic nu există până nu e folosit**: nimic încărcat până nu se deschide un `.http` sau
   fereastra modulului; un răspuns mare stă pe disc (capat în memorie), arătat din fișier;
   motorul web pornește doar cât e deschisă previzualizarea și se închide odată cu ea.
4. **Rețeaua doar în fundal**, anulabilă (un Stop oprește cererea), cu limite (timpul,
   mărimea, redirecționările) din setări; firul UI doar desenează.
5. **Secretele nu apar în clar**: valorile din mediile private sau din keychain se arată
   mascate (hover-ul arată că există), nu ajung în log, nu intră în istoria comisă.

## Pași (fiecare cu teste, gate verde, un commit)

1. **77a. Domeniul (`crates/domain/http/http-requests`).** Modelul (cerere: metodă, URL, antete,
   corp — text, fișier `< ./body.json`, multipart —, opțiuni `# @no-redirect`,
   `# @timeout`, `# @name`); cititorul `.http` (cererile separate de `###`, comentariile `#`
   / `//`, variabilele de fișier `@host = …`, variabilele dinamice `{{$uuid}}`,
   `{{$timestamp}}`, `{{$randomInt}}`, `{{$isoTimestamp}}`, `{{$dotenv NUME}}`); mediile
   (public + privat + `.local`, cel privat câștigă; `$shared`); rezolvarea variabilelor cu
   erori clare (o variabilă nedefinită spusă, nu trimisă goală); exportul ca cURL și citirea
   unui cURL lipit. Teste pe fiecare formă, inclusiv fișiere scrise de alți clienți ai formatului.
2. **77b. Transportul (`crates/infra/http/http-transport`, portul în domeniu).** `ureq`: orice
   metodă, antete, corp în flux (fișiere mari fără a le ține în memorie), redirecționări la
   alegere, TLS (verificarea oprită doar la cerere, spus clar), proxy-ul sistemului, cookie-
   urile pe proiect (un fișier privat în `<project dir>/http/`), anulare, limitele.
   **Timpii**: total, până la primul octet, descărcarea; conectarea și TLS-ul separat dacă
   `ureq` 3 lasă (conectorul lui se poate înveli) — de verificat la implementare. Corpul mare
   pe disc. Teste pe un server local din test (fără rețea).
3. **77c. Limbajul `.http` (`crates/modules/languages/http`).** Gramatica (o gramatică
   tree-sitter existentă, vendorată dacă licența e bună, altfel un cititor al nostru — tot
   cel din 77a); culori (metoda, URL-ul, antetele, variabilele, separatoarele, corpul în
   limbajul lui: JSON / XML / GraphQL injectate); structura (cererile, cu `@name`); plierea;
   completarea: metode, antete standard și valorile lor (`Content-Type: application/json`),
   variabilele (de fișier, de mediu, dinamice), numele cererilor și, după `.response.body.`,
   **căile din răspunsul primit ultima dată** (77f); hover pe o variabilă: valoarea ei în
   mediul ales (secretele mascate).
4. **77d. Rularea (`crates/modules/tools/http-client`).** Bara editorului unui `.http`:
   `+` (o cerere nouă, din șabloane: GET, POST JSON, multipart, GraphQL),
   istoricul, **Run with: mediul** (lista mediilor, „Add Environment to Public File… /
   Private File…”), rulează tot fișierul, Stop. În gutter, ▷ pe fiecare cerere (Run; meniu:
   Run, Copy as cURL, Open in Browser). Răspunsul în **Services** (nodul „HTTP”, câte o
   rulare: `GET login · 200 · 550 ms`), cu taburile: **Body** (formatat după tip: JSON /
   XML / YAML / HTML ca document read-only al limbajului lui — culori, pliere, căutare;
   imaginile prin vizualizatorul de imagini; binarul: mărimea și „Save as…”), **Headers**,
   **Cookies**, **Timeline** (timpii, redirecționările), **Raw**. Istoria rulărilor pe proiect
   (`<project dir>/http/history/`, capată, niciodată comisă). Rulările în Task Manager și
   întrebate la închidere (cum face consola).
5. **77e. Mediile, din UI.** Fișierele lor create din bară (public / privat, privatul pus în
   `.gitignore`, întrebând); variabilele unui mediu editate într-un tabel (ca un `.env`);
   secretele ținute, la alegere, în keychain (`{{$secret.nume}}`), nu în fișier.
   **Făcut:** fișierele mediilor create din bară (cu un exemplu, deschise în editor), cel
   privat pus în `.gitignore`-ul proiectului (numele lui și al celui local, o dată; spus
   printr-o notificare); hover-ul unei `{{variabile}}`: valoarea ei în fișier și în fiecare
   mediu, cea privată ascunsă, una dinamică explicată, „definită nicăieri”. **Mutat la 77j:**
   tabelul de variabile (azi: JSON-ul editat în editor, cu culorile și verificarea lui) și
   secretele în keychain (`{{$secret.nume}}`: cer un dialog de introdus o valoare, pe care
   gazda nu-l are încă).
6. **77f. Înlănțuirea.** `# @name login` și `{{login.response.body.$.token}}` (JSONPath; XPath
   pentru XML; `headers.Nume`); o cerere care depinde de alta folosește răspunsul ei cel mai
   nou — sau, la alegere, o rulează întâi (cu dependențele ei, în ordine, ciclurile spuse).
   Un evaluator JSONPath al nostru (în domeniu): `$`, `.a`, `['a']`, `[0]`, `[*]`, `..a`,
   filtre simple `[?(@.x == 1)]`. **Autocompletarea căii**: din ultimul răspuns al cererii
   numite (cheile, indexurile), cu valoarea de acolo arătată lângă.
   **Făcut:** JSONPath-ul (`http_requests::jsonpath`: membri, `['a b']`, indexuri și
   negative, `[*]` / `.*`, `..a`, filtre cu `== != < <= > >=` și „există”), referințele
   (`chain`: `.response.body.<cale>` — `*` corpul întreg —, `.headers.<Nume>`, `.status`),
   dependențele și ordinea (o cerere rulează după cele pe care le folosește și care n-au
   răspuns încă; un ciclu, o cerere necunoscută spuse; un lanț și Run All într-o singură
   lucrare, pas cu pas — unul eșuat oprește restul, spus); completarea căii din cel mai nou
   răspuns (un asistent al modulului de rulare, nu al limbajului: el are răspunsurile).
   XPath pentru XML: amânat la 77j.
   **Aliniat cu documentația formatului („HTTP Client variables”, cerut 2026-11-11):** orice
   variabilă e o expresie JSONPath peste valorile mediului (`{{client.host.url}}`,
   `{{client.['host.url']}}`, `{{users[0].name}}`, `{{$.clients..id}}`; mai multe valori:
   tabloul lor JSON); prioritatea lor — mediul, apoi variabilele fișierului; variabilele
   dinamice `$random.float`, `$random.alphabetic`, `$random.hexadecimal`, `$random.email`,
   `$env.NUME`. **De făcut (77j / 77k):** iterarea pe colecții (`{{users[*].name}}` — o cerere
   pe element, `request.iteration()`), clasele Faker (`$random.name.fullName`…, cu
   localizare), „Select Environment Before Run”, notificarea pentru o variabilă nedefinită cu
   „creează / schimbă mediul”, `crypto` în scripturi.
7. **77g. Graful.** O vedere (un tab) cu cererile unui fișier sau ale unei colecții: nodurile
   (metoda, numele, ultimul status și timp), muchiile = variabilele care trec dintr-una în
   alta (numele căii pe muchie). **Play** rulează lanțul în ordine (topologic), **Stop** îl
   oprește; nodul care rulează pulsează, datele „curg” pe muchie (o animație care cere
   cadre doar cât rulează), fiecare nod își arată statusul; clic pe nod: cererea în editor,
   răspunsul în Services. Așezarea: pe straturi (ca un graf de dependențe), calculată o dată,
   în fundal.
   **Făcut:** vederea `view-http-chain` (`chain_view`), deschisă din bara fișierului
   (butonul Graf) sau comanda `http.graph`: cardurile (metoda, numele, starea ultimei rulări,
   linia cererii), muchiile în unghi drept prin golurile dintre carduri, capetele lor
   răspândite pe latura cardului (fără puncte și etichete comune), eticheta = calea
   variabilei lângă săgeată; Play (`run_graph`: toate, în ordine), Stop, Fit; clic pe un card:
   cererea în editor. Pulsul și punctul care curge cer cadre doar cât rulează ceva; vederea
   se redesenează doar când generația rulărilor se schimbă. Parsarea și așezarea rulează pe
   firul `http-graph` (`kernel::work`), niciodată pe cel al interfeței: graful vine printr-un
   `poll`, cel vechi rămâne până atunci; stările cererilor se citesc o dată pe schimbare a
   rulărilor (`Runs::states`, un singur lock), nu la fiecare cadru și card.
8. **77h. Previzualizarea HTML-ului.** Două căi, amândouă: (1) **motorul web al sistemului**
   în aplicație, prin `wry` (WKWebView / WebView2 / WebKitGTK), într-un tab al
   răspunsului: creat la „Preview”, distrus la închidere (memoria lui doar cât se vede);
   (2) **Open in Browser**: răspunsul salvat într-un fișier temporar, deschis cu browserul
   implicit. O vedere nativă stă **peste** suprafața noastră GPU: meniurile, popover-ele și
   modalele noastre nu se pot desena peste ea — e ascunsă cât e deschis ceva deasupra (o
   captură a ei rămâne în loc, ca să nu „sară”), și urmărește tabul (mutat, ascuns, altă
   fereastră). Capabilitate a gazdei (fereastra e a aplicației), implementată în
   `platform-winit`; pe un sistem fără motor (Linux fără WebKitGTK): doar calea (2).
   **Făcut:** meniul răspunsului HTML are „Preview” și „Deschide în browser”. Pagina e
   corpul cu un `<base href>` la adresa de unde a venit (stilurile și imaginile relative se
   găsesc acolo), făcută pe un fir separat, până la 2 MB (limita WebView2 pentru o pagină
   dată ca text).
   - **Deschide în browser**: pagina scrisă în `.lumen/http/pages/` (se păstrează cele mai
     noi 50), deschisă de `Window::open_in_browser`. Platforma deschide doar `.html` / `.htm`
     cu cale absolută.
   - **Preview**: un tab `view-http-preview` cu adresa și „Deschide în browser” deasupra.
     Vederea spune pagina ei prin `Viewer::web_page` (`WebPage`: id, HTML, zona). Aplicația
     (`app/web_views.rs`), după fiecare cadru desenat:
     - face webview-ul nativ prima dată când tabul se vede;
     - îl mută pe zona vederii;
     - îl ascunde cât tabul nu e activ sau e deschis un overlay (meniu, modal, dialog);
     - îl distruge odată cu tabul.
   - **Platforma** (`platform-winit/web_view.rs`, doar macOS și Windows) folosește `wry`
     0.57.1 (WKWebView, WebView2):
     - fără devtools și fără punte spre aplicație;
     - navigarea oprită: un link din pagină nu duce nicăieri, doar pagina dată se încarcă;
     - datele motorului stau în cache-ul aplicației (`web-engine`): WebView2 le-ar scrie
       altfel lângă program.
   - Pe Linux nu e motor: tabul arată textul lui, iar „Deschide în browser” merge.
   - Dependențele noi pe macOS: `objc2-web-kit`, `cookie`, `dpi`. Pe Windows: `webview2-com`,
     pe același `windows` 0.62.
   **Diferit de plan:** sub un overlay rămâne textul tabului, nu o captură a paginii (`wry`
   n-are captură pe toate sistemele). Notificările (toasts, care plutesc) nu ascund pagina:
   pot ajunge sub ea. **Netestat pe ecran** (doar teste și capturi): de verificat pe macOS și
   Windows.
9. **77i. Fereastra HTTP (colecțiile).** Un tool window: colecțiile = dosarele cu `.http`
   din proiect (găsite prin indexul de căi), cererile lor (cu `@name`), căutare, cerere nouă
   / colecție nouă, mutare prin tragere; ultimele rulări. **Import**: cURL, colecții Postman
   / Insomnia / Bruno (convertite în `.http` + medii). **Export**: cURL, `.http`.
   **Făcut:** fereastra „Client HTTP”, în dreapta (`window.rs`, comanda `view.toggle_http`):
   - **ce arată**: colecțiile (dosarele cu `.http` / `.rest`), fișierele, cererile (metoda
     colorată, numele), deschise / închise, căutate după nume, metodă sau fișier (Esc
     golește); un clic pe o cerere o deschide la linia ei, unul pe fișier îl deschide;
   - **de unde**: lista se citește pe firul `http-list` din lista de fișiere a proiectului
     (indexul de căi, fără să umble prin disc), cel mult 500 de fișiere, din nou doar când
     se schimbă generația fișierelor proiectului;
   - **`+`**:
     - fișier nou de cereri (`requests.http`, `requests-2.http`…);
     - import cURL din clipboard;
     - import de colecție. Domeniul (`http_requests::import`) citește Postman v2.x (dosarele
       ca titluri, auth Bearer / Basic, raw / urlencoded / GraphQL / fișier, variabilele
       colecției ca `$shared`, un mediu Postman), Insomnia v4 (grupuri, `{{ _.x }}`, mediile
       de bază și cele derivate) și Bruno (toată colecția de lângă `bruno.json`, `.bru` +
       `environments/`). Ce nu are echivalent (scripturi, alte autentificări) e spus într-un
       comentariu. Fiecare import devine un dosar `http/<nume>/` cu `requests.http` și
       `http-client.env.json`, niciodată peste unul existent.
   - Aplicația duce acum la capăt și munca unui panou care a primit un fișier ales
     (`picked-file`).
   **Adăugat (2026-11-11):** import **OpenAPI 3 / Swagger 2**, în JSON sau YAML
   (`yaml-rust2`):
   - fiecare operație devine o cerere, cu titlul după tag și rezumat și `@name` din
     `operationId`;
   - serverul devine `{{baseUrl}}` în mediu, iar `{id}` din cale devine `{{id}}`;
   - parametrii de query și header sunt scriși cu variabile;
   - corpul vine din exemplu sau e generat din schemă (`$ref` urmate, buclele tăiate);
   - securitatea (Bearer, Basic, API key) devine headerul ei, cu variabilele ei în mediu.
   **Lăsat:** mutarea prin tragere (arborele proiectului o face deja). Ultimele rulări stau
   în Services. Exportul e „Copy as cURL” din răspuns; `.http` e chiar formatul.
10. **77j. Mai departe (fiecare cântărit când îi vine rândul):** autentificarea ajutată
    (Basic, Bearer, OAuth 2 cu fluxul în browser), GraphQL (schema citită pentru
    completare), WebSocket / Server-Sent Events (fluxul arătat cum vine), gRPC; transformări
    pe răspuns (un filtru JSONPath / jq-lite pe Body); aserțiuni declarative după răspuns
    (`# @assert status == 200`), rulate și în lanț.
11. **77k. Scripturile JavaScript din `.http`** (cerut de utilizator): `< {% … %}` înainte
    de cerere și `> {% … %}` după răspuns (sau `> ./handler.js`), cu API-ul lor:
    `client.global.set/get`, `client.test(...)`, `client.assert(...)`, `client.log(...)`,
    `request.variables`, `response.status / headers / body / contentType`. Motorul: QuickJS
    (crate-ul `rquickjs`: mic, rapid, izolat — fără acces la disc sau rețea din script),
    încărcat doar când un fișier are scripturi, oprit după o vreme nefolosit; un script are
    o limită de timp și de memorie. Testele (`client.test`) arătate în tabul **Tests** al
    răspunsului; variabilele globale puse de script intră în înlănțuire (77f) și în
    completare.
    **Făcut:**
    - **portul**: `http_requests::script` (`ScriptEngine`, `ScriptRequest` /
      `ScriptResponse`, `ScriptRun`: globale, variabile, loguri, teste, eroare);
    - **adaptorul**: crate-ul infra `http-scripts` cu `rquickjs` 0.14.0:
      - un runtime pentru fiecare script, făcut și eliberat (nimic ținut între rulări);
      - 32 MB și 5 s (o buclă fără sfârșit e oprită);
      - API-ul scripturilor e JavaScript simplu peste datele date (`prelude.js`: `client.global`
        set / get / isEmpty / clear / clearAll, `client.test`, `client.assert`, `client.log`,
        `client.exit`, `request.method / url / body / headers / environment / variables /
        iteration`, `response.status / headers.valueOf / valuesOf / body` — JSON-ul dat
        parsat — `/ contentType`);
      - fără nimic nativ, deci scriptul nu ajunge la disc, rețea sau aplicație (testat);
    - **rularea**: în `job.rs`, pe firul cererii:
      - scriptul de dinainte îi pune variabilele cererii; dacă eșuează, cererea nu pleacă;
      - cel de după vede răspunsul, cu corpul până la 8 MB;
      - `> ./handler.js` se citește de lângă fișier;
      - ordinea variabilelor e: ce a pus scriptul, apoi răspunsurile, globalele, mediul și
        variabilele fișierului;
      - globalele sunt ale proiectului (`Runs`) cât e deschis;
    - **afișarea**: tabul **Tests** al răspunsului (✓ / ✗ cu motivul, apoi logul) apare doar
      la o cerere cu scripturi; un test picat marchează nodul ca eșuat în Services.
    **Lăsat:** completarea numelor din `client.global` după `{{`. Aplicația ia răspunsul
    unui singur asistent, iar al limbajului (mediul, fișierul) l-ar ascunde. Cere ca
    asistenții să se poată uni (un pas al aplicației, cântărit separat). Tot lăsate:
    `crypto` și `request.iteration()` real (odată cu iterarea pe colecții).

12. **77l. Editorul vizual de cereri și colecțiile** (cerut 2026-11-11, cu capturi de referință).
    O cerere se poate face și fără să scrii `.http`, ca într-un client HTTP vizual. Ce face editorul
    se scrie în `.http`, compatibil cu ceilalți clienți ai formatului: fișierul rămâne sursa, iar ce se scrie de
    mână se vede în editor.
    - **Bara de sus**: calea (proiect › colecție › METODĂ nume), metoda (listă: GET, POST, PUT,
      PATCH, DELETE, HEAD, OPTIONS, plus una scrisă de mână), URL-ul cu `{{variabile}}`
      colorate și completate din mediu, Send (cu meniu: Send, Run All, Graph), statusul
      ultimului răspuns.
    - **Params**:
      - URL Preview (cu variabilele puse, secretele ascunse) și copiere;
      - tabelul nume / valoare / activ / șterge, legat în ambele sensuri cu query-ul din URL;
      - Add, Delete all, Bulk Edit (text `nume=valoare` pe linii), Import from URL.
    - **Body**: No Body, Form Data (multipart, câmpuri text și fișiere), Form URL Encoded
      (tabel), GraphQL (interogarea și variabilele), JSON / XML / YAML / Plain Text / Other
      (editor de text colorat după tip), File (`< ./cale`). `Content-Type` urmează alegerea.
    - **Auth**: Inherit from parent (al colecției), None, API Key (header sau query), Basic,
      Digest, Bearer Token, OAuth 2.0 (configurația `Security.Auth` din mediu:
      `{{$auth.token("id")}}`). NTLM, OAuth 1.0, AWS IAM, Hawk, Atlassian ASAP și Netrc se
      cântăresc când le vine rândul (formatul `.http` nu le are).
    - **Headers**:
      - tabelul cu bifă de activare, ștergere, reordonare prin tragere;
      - cele puse de aplicație (Host calculat, User-Agent, Content-Length) arătate gri;
      - completare pentru numele și valorile cunoscute.
    - **Scripts**: înainte de cerere și după răspuns (editoare JavaScript, 77k).
    - **Docs**: descrierea cererii (Markdown, Write / Preview), scrisă în `.http` ca
      comentariu deasupra ei.
    - **În dreapta**: răspunsul (Preview / Body / Headers / Timeline / Raw / Tests, din 77d).
    - **Colecțiile**, în fereastra HTTP:
      - colecție nouă (dosar cu `requests.http` și `http-client.env.json`), cerere nouă în
        ea (deschisă în editor), redenumire, ștergere, duplicare, mutare prin tragere între
        colecții;
      - mediile colecției editabile ca tabel (variabilele, cele private ascunse), cu acces
        la JSON-ul lor;
      - un clic pe o cerere o deschide în editorul vizual, iar „Open in Editor” deschide
        textul.
    - **În `.http`**:
      - un header sau un parametru dezactivat e un comentariu `# @disabled Nume: valoare`
        (ceilalți clienți îl ignoră);
      - descrierea stă în comentarii `# ` sub titlul `###`;
      - auth-ul ajutat devine headerul lui (`Authorization: Basic …`, `Bearer {{token}}`)
        sau `{{$auth.token("id")}}`;
      - moștenirea de la colecție e un `# @auth inherit`, rezolvat din fișierul colecției.
    Pașii: (1) modelul editat ↔ `.http`, cu dus-întors testat, în domeniu; (2) vederea:
    bara, Params, Headers, Body, Auth, Scripts, Docs, răspunsul; (3) colecțiile și mediile ca
    tabel în fereastra HTTP.
    **Făcut (primul lot):**
    - **Modelul** (`http_requests::draft`): `Draft` ↔ blocul din `.http`, cu dus-întors
      testat pe toate felurile de corp și de auth; `block_of`, `write_into` (o cerere nouă la
      sfârșit), `remove_from`.
    - **Vederea** `view-http-request` (`editor/`):
      - fișierul e citit și scris pe firul `http-editor`, la 600 ms după ultima schimbare și
        înainte de Send, iar restul fișierului rămâne cum e;
      - bara: metoda (listă, colorată), URL-ul (cu ×), Send / Cancel cât rulează, cu loader;
      - **Params**: URL Preview cu copiere, tabelul legat de query, Add, Delete all, Bulk
        Edit;
      - **Body**: No Body / Form Data / URL Encoded / GraphQL (interogare și variabile) /
        JSON / XML / YAML / Text / Other / File;
      - **Auth**: None / API Key (header sau query) / Basic / Digest / Bearer / OAuth 2.0
        (configurația din mediu);
      - **Headers**: tabelul, plus cele puse de aplicație arătate gri;
      - **Scripts**: înainte și după; **Docs**: Write / Preview;
      - **Settings**: redirect-uri, cookie-uri, istoric, timeout de cerere și de conexiune
        (directivele formatului);
      - răspunsul în dreapta (Response / Headers / Timeline / Raw / Tests).
    - **Câmpurile**:
      - la `{{` se completează variabilele mediului (cu valoarea, cele private ascunse), ale
        fișierului, cele dinamice, cererile cu nume și căile JSONPath din răspunsuri
        (`login.response.body.$…`, cu valoarea), alese cu ↑ ↓ ↵;
      - × golește; Cmd+A, undo / redo; Tab trece la câmpul următor; Enter în URL trimite.
    - **Fereastra HTTP**:
      - un clic pe o cerere o deschide în editor;
      - meniul pe rând: Edit, Open as Text, New Request, Duplicate, Delete;
      - `+`: New Collection (`http/collection/`, cu mediul ei);
      - comanda `http.edit` deschide cererea de la cursor.
    - **Services**: rulările se șterg (una, sau toate cele încheiate).
    **Rămas pentru loturile următoare:**
    - completarea și culorile și în câmpurile de mai multe linii (corpul JSON);
    - „Inherit from parent” (auth-ul colecției);
    - mediile ca tabel editabil;
    - mutarea prin tragere între colecții;
    - previzualizarea Markdown completă la Docs.

## Deciziile utilizatorului (2026-11-11)

1. **Formatul**: `.http` în sintaxa obișnuită, mediile
   `http-client.env.json` / `http-client.private.env.json`.
2. **Înlănțuirea**: declarativă (`@name`, JSONPath) **și** scripturile JavaScript din
   `.http` (77k).
3. **Previzualizarea**: `wry` (motorul sistemului) + „Open in Browser”.
4. **Răspunsurile**: în Services.
5. **Ordinea**: tot planul, pas cu pas (77a → 77k), fără oprire după bază.

## Review după 77g (2026-11-11)

Un review al întregului lucru (domeniu, transport, limbaj, modul), făcut de un agent separat
de cel care l-a scris. **Reparat:**

- **Corectitudine / securitate**:
  - cookie-urile a două rulări simultane nu se mai calcă: se aplică doar ce a schimbat
    fiecare, sub lock, iar fișierul se scrie doar la o schimbare;
  - la un redirect spre alt host sau de pe https pe http cad `Authorization`,
    `Proxy-Authorization` și `Cookie`;
  - un `Location` relativ cu `://` în query rămâne relativ;
  - secretele sunt ascunse oriunde apar (URL, orice header, corp), nu doar în 4 headere;
  - răspunsurile salvate în aceeași secundă nu se mai suprascriu;
  - un corp vărsat pe disc e șters la orice eroare (un guard);
  - dosarul `.lumen/http` are propriul `.gitignore` (`*`);
  - `{{$.cale}}` citește mediul;
  - nicio depășire aritmetică pe valori de la server sau utilizator;
  - Play-ul grafului spune bucla în loc s-o ascundă și rulează textul curent din editor;
  - `curl -d @fișier` devine `< fișier`.
- **Firul interfeței și cadrele**:
  - vederea răspunsului citește și formatează pe firul `http-view`: un singur `String` cu
    începuturile liniilor, o linie desenată de cel mult 4 000 de caractere, Copy Body
    copiază corpul real;
  - completarea nu mai citește și nu mai parsează corpul la fiecare tastă: JSON-ul se
    parsează o dată, pe firul rulării (corpuri ≤ 1 MB), și se dă prin `Arc`;
  - bara parsează fișierul o dată pe versiune, iar linia cererii vine din parsare
    (`Request.line_range`);
  - mediile sunt comasate o dată la citire, nu clonate la fiecare variabilă;
  - textele cardurilor grafului se fac o dată, iar „Add environment” nu mai citește discul
    la clic.
- **Memorie**:
  - corpurile din memorie au un buget pe proiect (64 MB); cele mai vechi salvate se citesc
    din fișierul lor;
  - se păstrează cel mult 4 agenți HTTP.
- **Organizare**:
  - citirea mediilor e un crate infra separat (`http-environments`), deci limbajul nu mai
    depinde de transport;
  - `Pending<T>` e lucrul în fundal comun al vederilor modulului;
  - scanarea referințelor la răspunsuri e una singură (ordinea de rulare și graful nu mai
    pot diferi);
  - `HttpFile::index_at` / `index_of`;
  - `job.rs` are teste.

**Lăsat, cu motiv:**

- Limbajul (`lang-http`) parsează fișierul la hover și la completare: un `.http` are câteva
  zeci de cereri, iar parsarea lui e cât a unei linii de cod colorate.
- Un clic pe Run parsează o dată fișierul din editor: parsare, nu citire de pe disc.

## Al doilea review, după 77h–77k (2026-11-11)

**Reparat:**

- **Pagina previzualizată**:
  - fără JavaScript și incognito, deci nu poate trimite cereri din aplicație (nici spre
    rețeaua utilizatorului) și nu păstrează cookie-uri sau storage;
  - ascunsă, dă tastatura înapoi ferestrei (`focus_parent`), la fel la un clic pe fereastră;
  - una care n-a putut fi făcută (fără WebView2) nu se mai încearcă la fiecare cadru;
  - una ascunsă mai mult de un minut își eliberează motorul (zeci de MB) și se face din nou
    când tabul se vede;
  - `<base>` vine după doctype, nu înainte.
- **Fișierul ales**: un picker închis fără fișier e spus tuturor panourilor
  (`picked-file-cancelled`), deci cel care a cerut nu mai ia fișierul ales mai târziu
  pentru altul. Era la fel și la fereastra Database.
- **Scripturile**:
  - `client.global` aplică doar ce a schimbat fiecare script (două rulări simultane nu se
    mai calcă);
  - corpul trece o singură dată (parsat în JavaScript), iar înainte era dublat în program;
  - logurile și testele rămân și când scriptul aruncă o eroare;
  - promisiunile se duc la capăt înainte de citirea rezultatelor;
  - `charset` se citește indiferent de majuscule.
- **Importurile**:
  - un ciclu de grupuri în Insomnia nu mai blochează (resursele sunt indexate o dată);
  - o rupere de linie dintr-un nume, URL sau header nu mai poate adăuga linii (un script, un
    fișier trimis);
  - un corp pe care fișierul l-ar citi ca al lui stă într-un fișier separat
    (`bodies/<n>.txt`);
  - Bruno fără `bruno.json` importă doar fișierul ales, fără să urmeze legături simbolice,
    cu adâncimea și numărul de fișiere limitate.
- **Fereastra HTTP**:
  - fișierele unui dosar stau împreună;
  - „Fișier nou” și importul cURL își aleg numele în fundal, după disc (două clicuri fac
    două fișiere), fără lista proiectului pe firul interfeței;
  - lista se recitește doar cât fereastra se vede.
- **Organizare**: o singură `format::method_color`, un singur `page::open_in_browser`,
  codul mort scos.

**Lăsat:**

- Notificările plutitoare (toasts) încă pot ajunge sub pagină; e notat la 77h.
- Recitirea fereastrei la o generație nouă citește din nou toate fișierele `.http` (sub
  500): o ștampilă pe fișier ar fi următorul pas, dacă se vede în măsurători.
- Un `WebContext` pe pagină: pe Windows mediul WebView2 se face pentru fiecare pagină.
- `GRAPHQL` și `Authorization: Basic user parolă` (77j) se fac în domeniu (`prepare`).

## Tot ce suportă formatul `.http`, adus pe rând (2026-11-11, cerut: „să meargă tot ce suportă formatul”)

Referința e documentația publicată a formatului („HTTP request syntax”, „HTTP Client variables”, „HTTP Client
reference”, „HTTP Response reference”, „OAuth 2.0”).

**Lotul A (sintaxă și transport), făcut:**

- `@no-auto-encoding`; altfel URL-ul își codează ce nu poate ține așa (spațiu, litere din
  afara ASCII, `|`…). Pus și în Settings-ul editorului.
- `Authorization: Digest user parolă`: la un 401 cu provocare Digest (RFC 7616: MD5 /
  SHA-256, `-sess`, `qop=auth`), cererea e retrimisă o dată cu răspunsul calculat. Parola nu
  pleacă scrisă.
- `>> fișier` / `>>! fișier`: corpul răspunsului scris acolo, cu variabile în cale
  (`{{$projectRoot}}`, `{{$historyFolder}}`). `>>` scrie alături, sub `nume-1`, iar `>>!`
  scrie peste.
- `SSLConfiguration` din mediu: `verifyHostCertificate`, `clientCertificate` și
  `clientCertificateKey` (PEM), citite pe firul cererii, iar agentul HTTP e ținut separat
  pentru fiecare certificat. Lăsat: cheia cu parolă (`hasCertificatePassphrase`).
- `response.cookies()` și `response.cookiesByName(name)` în scripturi.
- „Copy as PHP (Guzzle)” în meniul răspunsului.
- Lăsat: `$exampleServer` (e serverul web din IDE-ul lor).
- **HTTP/2** — **făcut 2026-11-12:** `GET … HTTP/2` îl oferă prin TLS (ALPN) și rămâne pe
  HTTP/1.1 dacă serverul nu-l ia; `HTTP/2 (Prior Knowledge)` vorbește HTTP/2 de la primul
  octet, și pe `http://` (h2c). Pe `http://` simplu, `HTTP/2` rămâne HTTP/1.1 (upgrade-ul
  e învechit, RFC 9113), la fel printr-un proxy. Un schimb e trimis cu `h2` pe un runtime
  `tokio` cu un fir al lui (firul rulării), nimic păstrat între cereri; răspunsul e citit
  ca al lui ureq (`Answer`), deci redirect-urile, cookie-urile, Digest-ul, limitele corpului
  și fișierul lui rămân aceleași. Corpul pleacă după fereastra serverului (un fișier mare nu
  e ținut întreg). Headerele conexiunii (`Connection`, `Host`…) nu se trimit. Parserul ține
  acum `HTTP/2 (Prior Knowledge)` întreg (spațiile lui ajungeau în URL). Teste pe un server
  HTTP/2 al lor pe loopback (h2c, TLS cu ALPN, un corp mai mare decât fereastra).
  Măsurat (release, macOS): binarul 84,13 → 84,86 MB (+0,73 MB, +0,86%); build-ul de
  release după o schimbare în domeniu 6 min 11 s înainte, 6 min 2 s după (legarea domină;
  `h2`, `http`, `bytes`, `tokio-rustls` se compilează o dată).

**Lotul B (scripturile), făcut:**

- `crypto`: `md5` / `sha1` / `sha256` / `sha384` / `sha512`, `hmac.*` (plus `hmac.sha3(…)`),
  cu `updateWithText` / `updateWithHex` / `updateWithBase64` și `digest().toHex()` /
  `toBase64(urlSafe)`. Hash-urile se calculează nativ (domeniul `hash`); scriptul tot nu
  ajunge la nimic din afară.
- `jwt.sign` / `verify` / `decode` pentru HS256/384/512, verificat pe exemplul de pe jwt.io.
- `await sleep(ms)` și `setTimeout` / `clearTimeout`: timerele rulează în ordinea scadenței,
  în limita de 5 s a scriptului, pe firul lui.
- `client.variables` (`global`, `environment`, `file`, `request`, `get`) și
  `client.global.headers.set` / `clear`: headerele ajung în cererile următoare (cu un script
  de dinainte, și în cererea curentă), fără să le înlocuiască pe ale cererii.
- **Iterarea pe colecții**: o variabilă cu un tablou ca valoare (o listă din mediu,
  `{{users[*].name}}`) trimite câte o cerere pentru fiecare element, fiecare cu rularea ei,
  iar `request.iteration()` dă indexul.
- **JWT RS* / PS* / ES*** — **făcut 2026-11-12:** `jwt.sign(payload, cheiePEM,
  {algorithm: "RS256" | … | "PS512" | "ES256" | "ES384", keyid})` și `jwt.verify(token,
  cheiePublicăPEM, {algorithms})`, semnate și verificate nativ cu `ring` (deja în arbore prin
  TLS). Cheia privată PKCS#8 sau PKCS#1, cea publică SPKI sau PKCS#1; ES în forma `r ‖ s` a
  JWT-ului. Un token `HS*` verificat cu o cheie PEM e refuzat (confuzia de algoritm), iar
  `algorithms` limitează ce se primește. Lăsat: ES512 (P-521 nu e în `ring`), `crypto.subtle`.
  Teste: fiecare algoritm, o semnătură făcută de openssl, prin scripturi. (Streaming-ul
  `onEachLine` / `onEachMessage`, lăsat aici întâi, a venit cu lotul E.)

**Lotul C (XPath), făcut:** `{{login.response.body.//token}}`, `/a/b`, `[n]`, `[last()]`,
`[@id='7']`, `@atribut`, `text()`, `*` pe un răspuns XML (`roxmltree`, domeniul `xpath`).

**Lotul D (OAuth 2.0), făcut:** configurația lor, `Security.Auth.<id>` din mediu
(domeniul `oauth`: câmpurile cu variabilele puse, cererile fiecărui grant, răspunsurile,
PKCE S256 verificat pe RFC 7636), folosită ca `{{$auth.token("id")}}` /
`{{$auth.idToken("id")}}`. Tokenul se obține pe firul cererii (`auth.rs`):

- unul proaspăt se folosește așa cum e, unul expirat se reîmprospătează cu refresh
  token-ul;
- Client Credentials și Password se cer direct;
- Authorization Code (PKCE, `state` verificat): pagina de autentificare se deschide în
  browserul sistemului (cererea ajunge la bara fișierului, la editor sau la Services, oricare
  o ia primul), iar redirect-ul e prins pe portul din Redirect URL (`localhost`) sau pe unul
  al nostru;
- Device Authorization: pagina lui se deschide, iar endpoint-ul de token e întrebat până
  răspunde;
- Mock: tokenurile scrise;
- `Client Credentials` = `basic` / `in body` / `none`, `Use ID Token`, `Custom Request
  Parameters` (cu `Use`).

Tokenurile sunt ale proiectului, per mediu, și se golesc la „Clear and Rebuild Caches”. Un
token e secret oriunde se arată.

**Implicit — făcut 2026-11-12:** redirect-ul e prins ca la Authorization Code; tokenul vine
în fragmentul adresei, pe care browserul nu-l trimite, așa că prima cerere primește o pagină
a noastră care îl trimite înapoi ca query (`location.replace`, fără să rămână în istoria
browserului), iar tokenul se citește de acolo cu `state` verificat (fără PKCE: n-are cod de
schimbat). Test cap-coadă cu un „browser” al testului.

Lăsat: Revoke URL, jurnalul de autentificare ca fereastră separată (motivul unei
erori apare la rularea care a eșuat).

**Lotul E (streaming, WebSocket), făcut:**

- **Streaming**: corpul unui răspuns se vede cât vine (SSE, NDJSON, un corp lung). Transportul
  raportează fiecare bucată (`send_streaming`), iar `Runs` ține ultima parte (cel mult
  512 KB), dată la o parte când se termină; vederea e trezită cel mult o dată la 100 ms și
  urmărește capătul. În scripturi, un corp `text/event-stream` / NDJSON e un obiect cu
  `onEachLine(fn, onFinish)` și `onEachMessage(fn, onFinish)` (la SSE, câmpurile `data:`,
  parsate ca JSON când se poate; `unsubscribe` oprește).
- **WebSocket**: `WEBSOCKET ws://…` (sau `wss://`), cu headerele cererii în handshake. Corpul
  e despărțit de `===` în mesaje, iar `=== wait-for-server` așteaptă un mesaj al serverului
  înainte de următorul. `tungstenite` 0.30 rulează pe același rustls. Ce se trimite (→) și ce
  vine (←) se vede live, apoi transcrierea e răspunsul (101). Sesiunea ține până o închide
  serverul sau până la Stop, cel mult 30 de minute.
- Lăsat: **gRPC**. Cere descrierea serviciului (`.proto` sau reflection) și HTTP/2, adică
  un client gRPC întreg; de cântărit separat. Și GraphQL prin WebSocket (subscriptions).

**Lotul F (ce lipsea editorului vizual), făcut:**

- **Mediile ca tabel** (`view-http-env`; domeniul `env_edit`):
  - un tab pentru `$shared` și fiecare mediu, plus „+” pentru un mediu nou;
  - variabilele ca rânduri (nume, valoare, Privat); una privată e ascunsă, nu se copiază
    și stă în fișierul privat, pus în `.gitignore`;
  - `Security` și `SSLConfiguration` rămân neatinse, iar o valoare care nu era text e
    scrisă înapoi ca JSON;
  - „Deschide JSON-ul” (public și privat) le deschide ca text;
  - se scriu pe firul `http-editor`, la 600 ms după ultima schimbare;
  - se deschide din lista de medii a barei, din `+` al ferestrei HTTP și din meniul unui
    dosar sau fișier.
- **Completarea** la `{{` merge și în câmpurile cu mai multe linii (corpul, variabilele
  GraphQL, scripturile).
- **Mutarea prin tragere**: o cerere trasă pe alt fișier (sau pe dosarul lui) se mută acolo,
  pe un fir separat. Întâi e scrisă unde merge, apoi scoasă de unde era, ca să nu se piardă
  între ele. Un clic fără mișcare deschide ca până acum.
- Lăsat: **„Inherit from parent”** (auth-ul colecției). Formatul `.http` n-are o asemenea setare la
  nivel de colecție; ar fi un format al nostru, pe care fișierul `.http` nu l-ar mai citi la
  fel în ceilalți clienți. Echivalentul compatibil e o variabilă de mediu (`Authorization: Bearer
  {{token}}`) sau `client.global.headers`.

## Al treilea review, după loturile A–F (2026-11-11)

Un agent separat a citit codul loturilor A–F. Fiecare constatare a fost verificată, apoi
reparată (cu un test care pica înainte) sau cântărită mai jos.

**Făcut:**

- **Nimic scris peste o schimbare din altă parte.**
  - Editorul vizual își găsește blocul după text: întâi blocul citit, apoi numele, apoi
    poziția. Dacă blocul s-a schimbat între timp, nu-l scrie: îl recitește și spune
    „s-a schimbat în afara editorului”. Salvările care vin în timp ce una e în lucru se
    pun la coadă.
  - Fereastra HTTP ține amprenta fiecărui bloc (`draft::block_hash`, FNV-1a). Mutarea,
    duplicarea și ștergerea caută blocul după amprentă, nu după poziția din listă, care
    poate fi veche.
  - Mutarea și duplicarea copiază blocul exact cum e scris (`move_raw`, `duplicate_raw`).
    Nu se mai pierde nimic prin rescrierea lui.
  - Tabelul de medii scrie doar dacă fișierele de pe disc sunt cele citite. Altfel le
    recitește.
- **Drumul draft → text păstrează tot.** Ce nu arată editorul (`Kept`) e scris înapoi cum
  era: versiunea HTTP, `>>` / `>>!`, scripturile din fișiere și corpul multipart (dacă
  câmpurile lui n-au fost schimbate, cu headerele părților, numele fișierelor și
  boundary-ul). Comentariile din scriptul de dinainte nu mai devin descrierea cererii.
- **Mediile:**
  - un fișier care nu e JSON nu e scris peste (înainte devenea `{}`);
  - un nume și în public, și în privat rămâne două rânduri, fără să se piardă valoarea
    publică;
  - cheile de sus care nu sunt medii (`$schema`) rămân;
  - un mediu doar privat nu mai apare gol în fișierul public;
  - valorile JSON stau pe o linie, cum se editează;
  - rândurile se derulează, câte un rând întreg.
- **Digest:** o parolă cu `=` e tot forma scurtă, deci nu se trimite în clar. După un
  redirect pe alt host, datele Digest nu mai răspund nimănui (test cu un server care cere
  Digest după redirect).
- **OAuth:**
  - portul redirect-ului citește cel mult o linie de 8 KB de la fiecare client, în cel
    mult 2 s;
  - ascultă pe `::1` dacă așa e scris Redirect URL-ul;
  - răspunsul fără `state`-ul nostru primește 404 și sign-in-ul continuă; pagina de
    succes apare abia după verificarea lui;
  - o eroare a serverului e arătată ca text, nu ca HTML;
  - o configurație e obținută de o singură rulare odată, celelalte așteaptă tokenul ei
    (test cu 4 fire: o singură cerere la server).
- **Device flow:**
  - codul utilizatorului e arătat într-o notificare (`Runs::tell` → `take_asked`);
  - se deschide doar o pagină http(s), la fel pentru orice `ask_browser`;
  - intervalul e limitat la 60 s și durata la 30 min, iar așteptarea vede Stop la 100 ms.
- **OpenAPI:**
  - numele din descriere (parametri, headere, părți din cale) devin nume simple de
    variabile sau text simplu, deci o descriere nu poate injecta `{{$auth.token(…)}}`;
  - `{{` din exemple e despărțit;
  - eșantionul unui corp are cel mult 2 000 de valori;
  - YAML-ul e măsurat înainte de a fi construit: cel mult 1 000 000 de noduri cu
    aliasurile expandate și 256 de niveluri, ca să nu treacă un „billion laughs”.
- **Streaming:** coada live se taie la 2 × 512 KB, ca fiecare octet să fie mutat o singură
  dată (înainte era tăiată la fiecare bucată, O(n²)).
- **WebSocket:** conectarea are timp limită (10 s pe adresă, la fel handshake-ul) și vede
  Stop între adrese. Transcrierea păstrează cel mult 10 000 de mesaje și 16 MB; restul se
  vede live.
- **Iterația:** cel mult 1 000 de elemente. Mediile sunt citite o dată pentru tot lanțul.
- **XPath:** `//` din noduri imbricate dă fiecare nod o singură dată, în ordinea
  documentului.
- **`>>`** scrie doar în proiect (sau în dosarul fișierului dacă nu e proiect), cu `..` și
  legăturile rezolvate. Dacă nu poate, spune de ce în jurnalul scriptului.
- **Certificatul clientului** care nu poate fi citit oprește rularea cu motivul, în loc să
  trimită cererea fără el.
- `one_line` e într-un singur loc (`draft`), folosit și de import.

**Lăsat:**

- **Copy as cURL / Guzzle cu secrete.** Copiază ce s-ar trimite, deci și tokenul, cum fac
  și ceilalți clienți. Utilizatorul cere explicit copierea; un avertisment la fiecare copiere ar fi
  zgomot.
- **WebSocket și `SSLConfiguration`** — **făcut 2026-11-12:** portul primește opțiunile
  cererii (`SendOptions`), iar transportul face un `rustls::ClientConfig` al lui doar când
  mediul cere ceva (verificarea oprită, un certificat de client), cu același `ring` și
  aceleași rădăcini ca ureq (nicio dependență nouă în arbore). Cu verificarea oprită,
  semnăturile handshake-ului tot se verifică; doar cine a semnat certificatul nu. Test pe un
  server WSS al lui pe loopback (un certificat autosemnat făcut pentru test).
- **Redenumirea și ștergerea unui mediu din tabel** — **făcute 2026-11-12:** dublu-clic pe
  tabul mediului (sau meniul lui de clic dreapta) îl redenumește pe loc: Enter / Tab
  păstrează, Esc renunță, un nume gol, rezervat (`$…`, `Security`, `SSLConfiguration`) sau
  luat e spus și rămâne de scris. Setările mediului și valorile lui private îl urmează
  (`Table.was`). „Șterge mediul…” întreabă întâi (variabilele pleacă din ambele fișiere,
  tabelul n-are undo). `$shared` nu se redenumește și nu se șterge. Focusul mutat cu Tab / Enter
  derulează tabelul până la rândul lui (făcut tot 2026-11-12).

## Cerute după testare (2026-11-12)

**Făcut:**

- **Căutarea din liste** (metoda, tipul corpului, auth și orice picker): câmpul e mai scund
  (30, ca filtrul de branch-uri). Are × de golire odată ce s-a scris ceva. ⌘A, cut și undo
  merg în el, iar un clic pune cursorul.
- **Celulele tabelelor** (headere, parametri, medii) n-au ramă nici când se scrie în ele.
  Stau între liniile tabelului, iar cursorul arată unde se scrie.
- **Bara de derulare care se ascunde** apare doar la derulare, nu și când mouse-ul ajunge pe
  marginea ei. Pleacă după 600 ms în loc de 1,2 s și se stinge în 200 ms în loc de 300.
  Odată vizibilă, mouse-ul pe ea o ține.
- **Colecțiile sunt dosare de grupare.**
  - O colecție nouă se face cu numele scris în câmpul de sus (Enter salvează, Esc
    renunță). Din `+` se face sub `http/`, iar din meniul unui dosar se face în el.
  - Meniul unei colecții are: Cerere nouă, Fișier nou, Colecție nouă în ea, Redenumește.
  - Un fișier se poate redenumi la fel.
  - Redenumirea trece prin aplicație (`WorkbenchHost::rename_entry`, la fel ca în
    arbore), ca tab-urile să urmeze fișierul. Un nume care nu poate fi de fișier e cerut
    din nou, cu motivul.
- **Liniile editorului vizual** se întâlnesc: rândul de stare al răspunsului are înălțimea
  tab-urilor din stânga (`response_view::STATUS`, 38).
- **Numele scris pe rând, ca în arborele de proiect** (cerut după prima variantă):
  - o colecție nouă sau un fișier nou are un rând al lui acolo unde va fi (`collections::slots`);
  - o redenumire se scrie pe rândul intrării; la un fișier e selectat numele fără extensie;
  - Enter, un clic în altă parte sau tastatura mutată altundeva păstrează numele, iar Esc renunță;
  - fără nimic scris nu se face nimic; un nume care nu poate fi de fișier e marcat cu roșu;
  - căutarea rămâne doar căutare;
  - după ce se creează ceva, lista e citită din nou imediat (`host.relist`).
- **Tastatura:** fereastra primește tastele și după comanda unui meniu din header-ul ei, nu
  doar după clicul dreapta (aplicația, `run_panel_menu`). În listă:
  - ↑ ↓, PageUp/PageDown și Home/End mută selecția;
  - → deschide, ← închide sau sare la rândul părinte;
  - ↵ deschide rândul; ↑ ↓ ↵ merg și din căutare;
  - subsolul arată tastele.
- **Un clic pe o cerere deschidea nimic.** Aplicația arunca acțiunea pe care un panou o cere
  la eliberarea mouse-ului, în afară de mutare. Acum o face ca pe a unui clic.
- **Fundalul** ferestrei e al ferestrelor laterale (`sidebar_background`), ca la Database.

## Tabul HTTP Client (cerut 2026-11-12)

**Făcut:**

- **Butonul cu globul deschide un singur tab „HTTP Client”** (unul pe proiect), cum se
  deschide tabul unei cereri. Panoul lateral nu mai există (`ToolContent::Command`).
- **În stânga, arborele colecțiilor** (`request_tree.rs`, `collections::rows`):
  - dosare pe oricâte niveluri (sub-colecții), cererile direct sub dosarul lor;
    fișierul `.http` nu mai e un nivel;
  - colecțiile goale apar și ele: dosarul `http/` e citit de pe disc
    (`collections::walk`: cel mult 2 000 de intrări pe 8 niveluri, fără link-uri);
  - are căutare, tastele și subsolul lor;
  - patru butoane: cerere nouă, colecție nouă, medii, import;
  - meniul de clic dreapta: cerere nouă, sub-colecție, redenumire, medii, duplicare,
    ștergere, import din cURL.
- **„Colecție nouă” face doar dosarul**, cu numele scris pe rând. Nu se mai deschide
  nimic singur.
- **O cerere nouă** e scrisă la sfârșitul `requests.http` din dosarul ei (creat dacă nu
  există), apoi e selectată și deschisă în dreapta.
- **În dreapta, editorul vizual** al cererii alese. Între cele două, un separator care se
  trage. Și cererea are acum un separator față de răspunsul ei (minimum 320 px pe fiecare
  parte).
- **Lista de fișiere a proiectului** e un snapshot. Tabul o cere din nou aplicației când
  e arătat iar și după ce scrie ceva el însuși.
- **Preview** e un tab al răspunsului (doar pentru HTML, `Part::of`), cu pagina randată
  acolo. Merge în editor, în tabul HTTP și în Services. Ferestrele laterale și cea de jos
  pot arăta acum o pagină web (`ToolPanel::web_page`). Motorul e același pentru fiecare
  răspuns nou, iar pagina doar se reîncarcă.
- **Pickerele** (componenta comună) arată ca listele de filtre din Git:
  - rânduri de 28 px, fontul `ui -0.5`, suprafața `hover_background`;
  - rândul activ e evidențiat cu text la 6%;
  - ↵ și bifa rămân în dreapta, mai mici.
- **URL-ul** se oprește înainte de ×. Tab-urile cererii se strâng când nu încap și nu mai
  intră peste răspuns.

**Lăsat:**

- **Butonul de maximizare a zonei editorului** (toată lățimea, toată înălțimea sau
  ambele) e următorul pas, separat: e treaba aplicației, nu a modulului.
- **Zona editorului pe toată fereastra** (făcut după, tot 2026-11-12, în aplicație):
  - un buton la capătul din dreapta al barei de tab-uri (`maximize-2`, apoi `minimize-2`)
    ascunde ferestrele laterale și pe cea de jos;
  - al doilea clic le aduce înapoi pe cele ascunse de el (ce s-a deschis între timp rămâne);
  - din comenzi (paletă, meniul View, taste): toată fereastra (⌘⇧F12, ascunde toate
    ferestrele de unelte deodată), toată lățimea, toată înălțimea (`workbench/editor_full.rs`).
