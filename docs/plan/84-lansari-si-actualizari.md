# 84 — Lansări din GitHub și actualizări din aplicație

Codul stă pe GitHub (`binary-cube/lumen-ide`). Două lucruri urmează de aici:

1. **Lansările se fac singure.** Un tag `vX.Y.Z` pornește un workflow care face binarele pentru
   macOS și Windows (și Linux, fiindcă tot îl avem), le pune pe pagina release-ului, cu
   sumele lor de control și cu un manifest semnat.
2. **Aplicația își face singură actualizarea, dar doar când vrea omul.** Verifică din când în
   când dacă a apărut o versiune nouă, îl anunță pe utilizator și îi arată ce conține (descrierea
   release-ului). Dacă alege „Actualizează”, descarcă în fundal, închide aplicația cum se
   închide de obicei, un proces mic înlocuiește aplicația și o pornește din nou cu ferestrele de
   dinainte, ca la Chrome. Nimic nu se instalează fără un click al omului.

## Ce avem deja

- `scripts/build.sh` / `scripts/build.ps1`: build-urile de lansare (`release`: LTO, un singur
  codegen unit), împachetate în `dist/` ca `<id>-<ver>-macos-<arch>.zip`,
  `<id>-<ver>-windows-x64.zip`, `<id>-<ver>-linux-<arch>.tar.gz`. Pe macOS, semnătură ad-hoc.
- `.github/workflows/ci.yml`: fmt, clippy, teste și `check-deps` pe macOS, Windows și Ubuntu,
  la fiecare push.
- Versiunea e una singură, `[workspace.package] version` din `Cargo.toml`, citită în aplicație
  ca `kernel::brand::VERSION`.
- Sesiunea de dinainte (plan 78): ferestrele și tab-urile se redeschid la pornire, deci o
  repornire după actualizare poate lăsa totul cum era.
- Închiderea aplicației trece deja prin întrebările pentru fișierele nesalvate și prin
  programele încă pornite (plan 83).

## 84a — Workflow-ul de lansare

- `.github/workflows/release.yml`, pornit de un tag `v*` (și manual, `workflow_dispatch`, ca
  draft):
  - **macOS**: `macos-latest`, binar universal (`--universal`), `.app` în `.zip`;
  - **Windows**: `windows-latest`, `scripts/build.ps1`, `.zip` (mai târziu un instalator);
  - **Linux**: `ubuntu-latest`, `.tar.gz`.
- Versiunea din tag trebuie să fie egală cu cea din `Cargo.toml`; altfel workflow-ul se oprește
  (o lansare nu poate purta alt număr decât cel din aplicație).
- Release-ul e creat ca **draft**, cu fișierele, `SHA256SUMS` și `latest.json` (84b). Omul
  scrie sau verifică descrierea, apoi îl publică. Descrierea e ce va vedea aplicația la „Ce e
  nou”.
- Descrierea pornește de la commit-urile dintre tag-ul de dinainte și acesta (titlurile lor),
  ca punct de plecare pentru om.
- Cache-ul Rust (`Swatinem/rust-cache`), ca în CI.

**De hotărât de om (costă bani sau conturi):**

- **macOS**: fără un certificat Developer ID (cont Apple Developer, 99 $/an) și fără
  notarizare, Gatekeeper avertizează la prima deschidere a unei aplicații descărcate. Cu ele,
  workflow-ul semnează și notarizează (secretele în GitHub: certificatul `.p12`, parola,
  App Store Connect API key).
- **Windows**: fără un certificat de semnare a codului, SmartScreen avertizează. Cu unul (sau
  cu Azure Trusted Signing), workflow-ul semnează `.exe`-ul.
- Până atunci, binarele sunt nesemnate, iar README-ul spune ce avertismente apar și cum se trece
  de ele.

## 84b — Manifestul semnat

- `latest.json` pe release: versiunea, data, pentru fiecare platformă fișierul, mărimea și
  SHA-256 lui.
- Manifestul e semnat cu o cheie **Ed25519** a proiectului (`minisign` sau `ed25519-dalek` în
  `xtask`): cheia privată stă doar în secretele GitHub, cheia publică e în aplicație.
- Aplicația nu instalează nimic nesemnat sau a cărui sumă nu se potrivește, chiar dacă vine de
  la adresa așteptată: un cont GitHub compromis sau un fișier înlocuit nu ajung pe calculatorul
  utilizatorului.
- `cargo xtask release-manifest` face manifestul din fișierele din `dist/` (folosit și local,
  ca să se poată verifica înainte de tag).

## 84a + 84b — Făcut / Lăsat

**Făcut** (2026-11-18, versiunea `1.0.0-beta.1`):

- `.github/workflows/release.yml`: un tag `v*` (sau pornit de mână) verifică întâi că tag-ul e
  versiunea din `Cargo.toml` (`cargo xtask release-version`), apoi construiește în paralel
  macOS pentru Apple Silicon (`build.sh mac`; Intel lăsat, la cererea omului: pe un Mac Intel
  se construiește din sursă), Windows (`build.ps1`) și Linux (`build.sh linux`), iar la sfârșit face un release **draft** (pre-release dacă versiunea are
  `-`) cu fișierele, `SHA256SUMS`, `latest.json` și `latest.json.sig`. Notele pornesc de la
  titlurile commit-urilor de la tag-ul de dinainte. Un draft al aceleiași versiuni e înlocuit la
  o nouă rulare; un release publicat, niciodată.
- `.github/workflows/release-cache.yml`: cache-ul build-urilor de release ținut pe `main`
  (un tag citește cache-urile ramurii implicite, niciodată pe ale altui tag), cu aceleași
  scripturi și chei (`shared-key: release-<OS>`, `xtask`); pornit când se schimbă
  dependențele, săptămânal (GitHub șterge un cache nefolosit 7 zile) și de mână. Release-ul
  doar îl citește (`save-if: false`). Încearcă și build-urile de release pe `main`, înainte să
  aibă nevoie de ele un tag.
- `cargo xtask release-key`: cheia Ed25519 a proiectului, făcută o singură dată
  (`ed25519-dalek`, deja în proiect prin `russh`; sămânța din `getrandom`). Partea publică în
  `packaging/release-key.pub` (în repo), cea secretă într-un fișier `0600` din `target/`, de
  pus în secretul `RELEASE_SIGNING_KEY` și apoi șters. O a doua cheie e refuzată (release-urile
  semnate cu prima n-ar mai putea fi verificate).
- `cargo xtask release-manifest [--dist <dir>] [--signed]`: `latest.json` (versiunea, data,
  pentru fiecare platformă numele, mărimea, SHA-256), `SHA256SUMS` (cum îl citește
  `sha256sum -c`) și semnătura, verificată cu cheia publică înainte de a fi scrisă; cu
  `--signed` și fără cheie, se oprește.
- `Info.plist` primește doar cifrele (`1.0.0` pentru `1.0.0-beta.1`): macOS nu acceptă altceva
  acolo.
- README: *Installing* (ce fișier pentru ce sistem, avertismentele binarelor nesemnate și cum
  se trece de ele), insigna release-ului.
- Teste: numele fișierelor → platforma, hex, semnătura (bună, alt text, altă cheie, nu e
  semnătură), manifestul dintr-un dosar (doar versiunea lui, semnătură cerută / lipsă / a altei
  chei), datele calendaristice.

**Lăsat:**

- Semnarea pentru macOS (Developer ID, notarizare) și Windows: binare nesemnate deocamdată,
  decizia omului.
- Un instalator pentru Windows (`.msi` / `.exe`) și pachete Linux (`.deb`, AppImage): `.zip` și
  `.tar.gz` ajung pentru început.
- Build-urile cu serverele de limbaj înăuntru (`--with-servers`): aplicația le descarcă oricum
  la prima nevoie.

## 84c — Verificarea versiunii (modulul `updates`)

- Un modul nou, `crates/modules/tools/updates`, cu domeniul pur în
  `crates/domain/tools/updates` (compararea versiunilor, citirea manifestului, alegerea
  fișierului pentru platformă) și transportul în infra (`http-transport` sau un client mic).
- Adresa repo-ului vine din `brand.toml` (o cheie nouă `repository`), nu e scrisă în cod.
- **Când verifică:** la 10 minute după pornire, apoi o dată la 12 ore cât timp aplicația e
  deschisă; și la cerere, din meniul aplicației („Caută actualizări…”). Totul pe un worker
  (`kernel::work`), niciodată pe firul UI.
- **Ce citește:** `GET https://api.github.com/repos/<repo>/releases/latest` (fără cont: 60 de
  cereri pe oră ajung din plin), cu `ETag` (un răspuns `304` nu consumă din limită), apoi
  `latest.json` de pe acel release. Fără niciun identificator al utilizatorului.
- **Ce compară:** versiunea release-ului cu `kernel::brand::VERSION` (semver). Pre-release-urile
  doar dacă utilizatorul a ales canalul „beta”.
- Fără rețea sau cu GitHub căzut: nimic nu se arată, se încearcă la următoarea verificare.
- Setări (`src/settings.rs` al modulului, `SettingsContribution`): verificarea automată
  (pornită implicit), canalul (stabil / beta). O versiune „sărită” e ținută, ca să nu mai fie
  anunțată.

## 84d — Anunțul și „Ce e nou”

- O versiune nouă găsită: un toast („Versiunea 0.9.0 e disponibilă”) cu butoanele
  **Ce e nou**, **Actualizează** și **Mai târziu**, și un element în bara de jos cât timp
  actualizarea e disponibilă (prioritate mică: `spare`).
- **Ce e nou**: un modal (`host.open_modal`) cu descrierea release-ului, randată ca Markdown
  (modulul de Markdown, prin `ViewerAction::Ask` sau un contract în `contracts`, fără a-l numi),
  versiunea curentă și cea nouă, mărimea descărcării, plus butoanele **Actualizează**,
  **Sari peste versiunea asta** și **Închide**.
- Nimic nu se descarcă înainte de **Actualizează**: un anunț ignorat nu costă nici disc, nici
  rețea.

## 84e — Descărcarea

- **Actualizează**: descărcarea pornește în fundal, cu progres în anunț și în Task Manager
  (o activitate a modulului, se poate opri).
- Fișierul ajunge într-un dosar al aplicației (`<date>/updates/<versiune>/`), e verificat
  (SHA-256 din manifestul semnat) și despachetat acolo, alături de aplicația curentă, nu peste
  ea.
- Gata: „Actualizarea e pregătită” cu **Repornește acum** și **La următoarea închidere**.

## 84f — Aplicarea și repornirea

- **Repornește acum** trece prin închiderea obișnuită: întrebările pentru fișierele nesalvate,
  pentru programele pornite (plan 83), sesiunea salvată. Dacă utilizatorul anulează o
  întrebare, nu se întâmplă nimic: actualizarea rămâne pregătită.
- Înainte să iasă, aplicația pornește **ajutorul de actualizare**: același binar copiat în
  dosarul temporar și pornit cu `--apply-update <dosar> --pid <pid> --relaunch`, ca să nu
  depindă de fișierele pe care le înlocuiește.
- Ajutorul:
  1. așteaptă să se încheie procesul aplicației (cu o limită de timp);
  2. mută aplicația veche deoparte (`.app` pe macOS, dosarul cu `.exe` pe Windows), mută cea
     nouă în locul ei (mutări în același volum: nimic pe jumătate scris);
  3. pe macOS, scoate atributul de carantină de pe aplicația nouă și verifică semnătura;
  4. pornește aplicația nouă, care își redeschide ferestrele (plan 78);
  5. dacă ceva nu merge, pune aplicația veche înapoi, o pornește și lasă un mesaj pentru ea
     („Actualizarea n-a reușit: …”);
  6. șterge aplicația veche abia la următoarea pornire reușită a celei noi.
- Fără drepturi de scriere în dosarul aplicației (de exemplu `/Applications` al altui
  utilizator, `Program Files`): aplicația spune asta înainte de descărcare și oferă doar
  descărcarea manuală, cu un link spre release.
- **La următoarea închidere**: la închiderea obișnuită, același ajutor face înlocuirea fără să
  repornească.

## 84c–84f — Făcut / Lăsat

**Făcut** (2026-11-18):

- **Domeniul** `app-updates` (fără I/O): versiunile comparate ca SemVer (pre-release-urile
  înaintea lansării), lista de release-uri a GitHub-ului citită (drafturile și tag-urile care nu
  sunt versiuni lăsate deoparte), cel mai nou de oferit (beta doar cui le ia, niciodată cel
  sărit), manifestul (un fișier cu cale în nume sau cu o sumă care nu e una, refuzat), fișierul
  pentru sistemul acesta (Mac: arhitectura lui, altfel cel universal), notele ca rânduri
  (titluri, puncte, paragrafe, marcajele scoase) și partea lor pentru aplicație: între
  `<!-- app -->` și `<!-- /app -->` dacă există, altfel tot fără secțiunea de descărcare.
- **Infra** `self-update`: verificarea (doar gazdele GitHub-ului, nimic despre utilizator;
  manifestul crezut doar semnat cu cheia proiectului și doar pentru versiunea release-ului lui),
  descărcarea în dosarul de date (reluată, verificată cu SHA-256 și mărimea din manifest,
  despachetată de codul nostru: încercat pe zip-ul adevărat de macOS, semnătura ad-hoc rămâne
  validă), înlocuirea: aplicația veche pusă deoparte, cea nouă în locul ei (mutare, altfel
  copiere), cea veche dată înapoi la eșec; ajutorul e o copie a programului în dosarul temporar,
  care așteaptă să se închidă conducta ținută de aplicație (aceeași pe toate sistemele, fără
  cod `unsafe`), apoi repornește aplicația cu `--reopen-session` și lasă `result.toml`.
- **Aplicația**: `SystemHost::restart_to_update` → `AppRequest::RestartToUpdate`: închiderea
  ca la Quit (lucrul în curs și fișierele nesalvate întrebate întâi; Cancel nu închide nimic,
  iar un Quit al utilizatorului uită actualizarea), `AppWindows` predă actualizarea ajutorului
  la `exiting()`, pornită din nou redeschide sesiunea de dinainte (planul 78).
- **Modulul** `updates`: verificarea la 30 s după primul proiect, apoi la 12 ore (setarea
  `[updates] check`), Help › Check for Updates… oricând; notificarea, una singură actualizată pe
  parcurs (Ce e nou / Actualizează → progresul → Repornește acum); modalul „Ce e nou” (notele
  aranjate o dată pe lățime, nu la fiecare cadru); Sari peste această versiune; pagina de setări
  (verificare automată, beta-uri, versiunea curentă și cea sărită). Totul pe firele modulului:
  pe firul UI doar starea din memorie și notificările.
- **Release-ul**: notele generate pun lista de noutăți între comentariile pentru aplicație; pasul
  de draft folosește `RELEASE_TOKEN` dacă există (token-ul workflow-ului e refuzat de politica
  organizației cu 403).

**Lăsat:**

- Un element în bara de jos cât timp o actualizare e disponibilă (notificarea și meniul ajung
  deocamdată).
- Oprirea descărcării din Task Manager (se oprește doar la ieșire).
- Încercarea cap-coadă pe un release adevărat (`1.0.0-beta.2`): după ce ajunge pe GitHub.

**Revizuirea** (un agent separat, după 84c–84f): nicio cale prin care să se instaleze ceva
nesemnat. Reparate: un „Repornește acum” anulat într-o fereastră nu mai e pus în loc de un Quit
dat din alta (doar fereastra care iese contează; orice Cancel îl uită); Skip în timpul
descărcării o oprește, iar versiunea sărită nu mai devine „gata”; stările se schimbă atomic (o
verificare nu mai strică o descărcare începută între timp, două descărcări nu mai pot porni);
ajutorul pornește detașat (un Ctrl-C în terminalul aplicației nu-l mai oprește la mijloc); un
`.old` rămas nu mai blochează actualizarea următoare (alt nume, curățat la pornire); fără 10 s
de reîncercări pe alt volum; dosarele versiunilor vechi șterse; arhiva deja descărcată
verificată pe bucăți, nu încărcată în memorie; căile `C:x` refuzate în `staged.toml` și
manifest. **Lăsat:** pe Windows și Linux se înlocuiește doar programul (un build
`--with-servers` și-ar păstra serverele vechi: release-ul nu face așa ceva); dacă utilizatorul
pornește aplicația chiar cât durează înlocuirea, cea repornită i se predă și nu-și redeschide
sesiunea; un ajutor omorât exact între cele două mutări lasă doar `.old` (detașat, e foarte rar).

## 84g — Teste și verificări

- Domeniul: compararea versiunilor (semver, pre-release, `v` în tag), manifestul (semnătură
  bună, greșită, lipsă; sumă greșită), alegerea fișierului pentru platformă și arhitectură.
- Transportul: un server propriu pe loopback, care servește un release, un manifest și o
  arhivă (cum fac testele HTTP).
- Ajutorul: înlocuirea și revenirea într-un dosar temporar (fișiere false, un „proces” care se
  încheie), fără să atingă aplicația adevărată.
- Snapshot-uri: toast-ul, modalul „Ce e nou”, elementul din bara de jos.
- Un release de probă (`v0.0.0-test`, pre-release, draft) pe repo, ca workflow-ul să fie
  încercat cap-coadă înainte de prima lansare adevărată.

## Ordinea

84a → 84b → 84c → 84d → 84e → 84f → 84g. Primele două pot merge fără aplicație (lansările
manuale de pe GitHub sunt deja utile). Restul vine după ce există cel puțin un release cu
manifest.
