# 31 — Setări în fundal, parole șterse din memorie, monitorizare, mutări în module

Continuarea punctelor rămase din review-ul 29. Ce lăsăm pentru mai târziu e în
[TODO.md](TODO.md), ca să nu se piardă.

## Pași

**31a — setările recitite în fundal**
- La revenirea în fereastră, `settings.toml` nu mai e citit pe firul interfeței:
  - `SettingsService::refresh()` recitește, pe firul `settings-reload`, secțiunile citite
    până acum;
  - le înlocuiește doar pe cele schimbate și anunță `SettingChanged` pentru ele;
  - o scriere făcută între timp (`set` / `update`) are prioritate: rezultatul vechi e
    aruncat.
- App-ul pornește cel mult un fir odată și redesenează doar dacă s-a schimbat ceva.
- Teste: modificarea de mână e văzută și anunțată, iar o scriere concurentă nu e
  suprascrisă.

**31b — parolele șterse din memorie (`zeroize`)**
- `kernel::Secret` e un text care se golește la eliberare (`zeroize`). `Debug` nu-l
  afișează, iar textul se obține doar prin `expose()`.
- Îl folosesc toate parolele și passphrase-urile:
  - `SecretStore::get` (remote) și `database::Secret`;
  - credențialele SFTP / FTP;
  - conexiunea Postgres;
  - fluxul din fereastra de conectare;
  - cheia și textul decriptat din seiful proiectului.
- Câmpul de parolă (mascat) își golește textul la eliberare.

**31c — monitorizare**
- `warm()` din lang-php rulează pe firul `php-detect` al modulului, deci intră pe rândul
  PHP în Task Manager.
- Preîncărcarea modulelor: fiecare modul se încarcă pe firul `preload-<id>`, contat pe
  rândul lui (datele și gramaticile lui).
- Memoria:
  - git: textul din commit, liniile schimbate, blame-ul și adnotările fiecărui tab, pe
    rândul Git;
  - gramaticile: query-urile compilate, estimate, pe rândul modulului de limbaj.

**31d — mutări în module (DDD)**
- `css.rs` iese din domeniul `intelligence`. Regulile CSS ajung la app prin hook-urile
  limbajului, nu prin `is_css_kind`.
- Stub-ul de docblock la `/**` + Enter devine un hook al limbajului, păstrat și în
  `<script>` din HTML.
- `unclosed_tag` și `'$'` din `links.rs` ajung la limbajele care le cer.
- `LevelPicker` devine view-ul unui modul, deschis prin `host.open_overlay`.

**31e — curățenie**
- Helperii mici duplicați, adunați într-un singur loc fiecare:
  - numele fișierului dintr-o cale;
  - `is_name`;
  - `hex`;
  - `wrap_message`;
  - `short`.

## Ordinea

Totul într-un commit, după `check.sh` și după capturi.

## Stare

- **31a, gata.**
  - `SettingsService::refresh()` rulează pe firul `settings-reload`, cel mult unul
    odată.
  - O scriere concurentă câștigă; secțiunile schimbate sunt anunțate.
- **31b, gata.**
  - `kernel::Secret` (cu `zeroize`) e folosit de seif, SFTP, FTP, Postgres și fereastra de
    conectare.
  - Cheia și textul decriptat din seif sunt `Zeroizing`.
  - Câmpul de parolă își șterge textul când e înlocuit sau eliberat și își rezervă loc
    dinainte, ca să nu lase copii la tastare.
- **31c, gata.**
  - Fire noi: `php-detect`, pe rândul PHP, și `preload-<modul>`, pe rândul fiecărui
    modul.
  - Memoria git a fiecărui tab apare pe rândul Git.
  - Gramaticile compilate apar pe rândul modulului de limbaj. E o estimare de 20× sursa
    query-urilor: am măsurat între 13× și 31×.
- **31d, gata.**
  - Crate-ul `infra/stylesheet` citește selectorii.
  - Domeniul păstrează regulile de potrivire în spatele hook-ului
    `LanguageIntelligence::place_match` (`PlaceMatch`).
  - `SymbolKind::name_chars` înlocuiește `NameStyle::CSS` în app.
  - Hook-ul `doc_comment_opener` (PHP: `/**`).
  - `LanguageDescriptor::unclosed_element`.
  - `path_in_quotes` folosește sigilul limbajului, iar „linia N” vine din locale
    (`link.line`).
  - `LevelPicker` a trecut în `ui-components`. Modulul nou `language-levels` îl afișează
    ca popover, iar alegerea vine la app ca `AppRequest::SetLevel`.
  - `Overlay::LevelPicker`, slotul și `InputOwner::LevelPicker` au dispărut din app.
- **31e, gata.**
  - `kernel::paths::file_name` înlocuiește 15 copii (app, project-tree, refactoring,
    vcs-git, local-history, workspace).
  - `code_intelligence::{short_name, is_name_char}` sunt folosite de diagrams și
    structure.
  - `stylesheet::is_name` e folosit de lang-css.
  - `hex` (octeți / culoare) și `wrap_message` (dialogul cu evidențieri / mesajul din log-ul
    git, cu paragrafe) au rămas: au doar numele comun, nu fac același lucru.
