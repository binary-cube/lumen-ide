# 17. Modalele ca un contract: înregistrate, nu scrise de mână în aplicație

Întrebarea: merită ca modalele (Setări, Căutare în proiect, Proiecte, About, dialogurile,
alegerile, pickerele, wizard-urile modulelor) să fie module care își înregistrează singure
nevoile, ca ferestrele de unelte (plan 14) — **fără nicio schimbare vizuală**?

**Răspunsul scurt: da, dar nu „fiecare modal un modul” orbește.** Câștigul real e altul decât
pare: (1) o singură regulă de rutare a input-ului în locul a șase lanțuri scrise de mână — unde
am găsit deja nepotriviri care pot fi bug-uri; (2) secțiunile de setări și modalele care țin de
o funcție (căutarea, proiectele, nivelul limbajului, culorile) aduse de modulele lor, ca
aplicația să nu mai depindă de modulele concrete. Aspectul rămâne exact (verificat pixel cu
pixel la fiecare pas).

## Azi (inventar, 2026-10-22)

**10 overlay-uri**, toate câmpuri ale `Workbench`: Setări, Căutare în proiect, Proiecte, About,
Dialog, alegeri (`ChoiceMenu`), meniul contextual, pickerul de nivel PHP, pickerul de culoare,
și vederea modală a unui modul (`ModalView`, folosită azi doar de wizard-ul remote).

**Un cadru comun există** (`ui-workbench/modal.rs`: `Modal`, `ModalSpec` — antet cu iconiță,
titlu, subtitlu, `esc`, secțiuni, subsol cu taste, fundal, animație) dar **doar 3 îl folosesc**
(Setări, Proiecte, vederea modală a modulelor); Căutarea, Dialogul, About, alegerile și
pickerele își desenează propriul fundal, `esc`, subsol și animație.

**Rutarea input-ului e scrisă de mână în 6 locuri**, fiecare cu ordinea lui:
- `Overlay::ALL` (prioritatea tastaturii), `key_overlays` (~290 de linii, un lanț de `if` în
  altă ordine), `type_text` (8 ramuri), `click_overlays` (~170 de linii), `hover_inner`
  (același bloc repetat de 8 ori), `scroll`, plus ordinea desenării (`paint.rs`) și verificările
  „e deschis” repetate în 5 locuri (tooltip, hover, taste, animație, viewport).
- **Nepotriviri găsite** (de confirmat cu teste în 17a):
  - About e **desenat sub** vederea modală și sub dialog, dar **primește tastele primul**.
  - Setările sunt desenate peste Proiecte, dar Proiectele primesc tastele primele.
  - `modal_open()` e adevărat doar pentru About, Dialog și vederea modală — **nu și pentru
    Setări, Căutare, Proiecte**: comenzile aplicației (de ex. ⌘W) par să ajungă la editorul din
    spate cât Setările sunt deschise, contrar regulii din `CLAUDE.md` („cât un modal e deschis,
    nimic din spate nu primește taste, comenzi…”).

**Duplicare**: `esc` tratat în 9 locuri; tripleta de taste „↑↓ alege · ↵ deschide · esc închide”
construită în 7 locuri și desenată pe 3 drumuri; editarea câmpurilor de text (←→, cuvânt,
Home/End) de două ori identic; `fold()` (fără diacritice) de două ori; animația de apariție
reimplementată în 5 componente; ↑↓ / PgUp / PgDn diferit de la un modal la altul.

**Setările conțin blocuri ale modulelor** (istoricul local, remote, framework-urile): aplicația
depinde de id-urile lor, valorile stau în aplicație (`history_retention`, `remote_defaults`),
`SettingsModel` are câmpuri tipizate pentru fiecare, `SettingsAction` 22 de variante tratate
într-un singur `match` din aplicație; secțiunile sunt un tablou fix de 8.

**Modale ale unor funcții, ținute de aplicație**: Căutarea în proiect (a indexării), Proiectele
(ale spațiului de lucru), pickerul de nivel (al limbajului PHP), pickerul de culoare (al
modulului de culori).

**Teste**: dialogul, setările (2), alegerile, meniul contextual, About blochează editorul; nimic
pentru cadrul `Modal`, Căutare, Proiecte, pickere.

## Ținta

```
ui-components:  trait Overlay  (felul: modal | popover; deschis; animă)
                  key / text / click / hover / scroll / paint / cursor / tooltip
                ModalFrame     (cadrul de azi, pentru toți: antet, esc, subsol, fundal, animație)
                ListNav, TextField (↑↓ PgUp PgDn ↵ esc; editarea unui câmp — o singură dată)
aplicația:      OverlayStack   — o listă ordonată: desenarea și input-ul pe aceeași ordine
                (prin construcție), „modal” = blochează tot din spate (taste, text, comenzi,
                scroll, hover — regula din CLAUDE.md, verificată într-un singur loc)
module:         contribuie OverlayProvider (modale proprii) și SettingsSection (secțiuni de
                setări: id, iconiță, titlu, ordine, conținut, intrări în căutare, valori)
```

Reguli:
- **Nicio schimbare vizuală**: înainte de fiecare pas, snapshot-uri PNG ale fiecărui modal
  (variabilele `EDITOR_SNAPSHOT_*` există deja); după pas, aceleași snapshot-uri **identice
  pixel cu pixel** (un test le compară).
- O singură ordine pentru desenare și input; „modal” blochează tot din spate.
- Un modul își aduce modalele și secțiunea de setări; aplicația doar le descoperă și le pune pe
  stivă (ca ferestrele de unelte).

## Pașii (un commit fiecare; `check.sh`; „go” între ei)

- [x] **17a. Plasa de siguranță.** Snapshot-uri de referință pentru toate modalele (o comparație
      pixel cu pixel ca test); teste de input pentru fiecare (esc, ↑↓, ↵, text, clic în afară
      nu închide, nimic din spate nu primește taste / comenzi / scroll / hover); testele care
      confirmă sau infirmă nepotrivirile de mai sus — cele confirmate, reparate aici.
      *Făcut:* `scripts/modal-snapshots.sh record|compare <dir>` — 19 modale și popover-e
      (About, dialogurile, cele 8 secțiuni ale Setărilor și căutarea în ele, Proiectele,
      Căutarea, pickerele, meniurile, wizard-ul remote) în tema închisă și deschisă, 38 de
      capturi comparate octet cu octet (captura așteaptă acum finalul animațiilor de apariție,
      altfel meniurile ieșeau diferit de la o rulare la alta). Teste: `tests/unit/workbench/
      overlays.rs`. **Confirmate și reparate:** About primea tastele (și click-urile, hover-ul)
      înaintea dialogului desenat peste el; Proiectele înaintea Setărilor; Setările, Căutarea și
      Proiectele nu erau modale (comenzile, zoom-ul ajungeau în spate); panoul de jos cu focus
      (Git log) primea săgețile chiar cu un dialog / Setările deschise; vederea modală a unui
      modul lua click-urile și cu un meniu deschis peste ea. Acum `Overlay::ALL` e ordinea
      desenării, iar tastele, textul, click-ul, scroll-ul și hover-ul merg la overlay-ul de
      deasupra (`overlay_key`); clipboard-ul merge în câmpul oricărui modal
      (`overlay_has_input`). Capturile: identice după reparații.
- [x] **17b. Contractul și stiva.** `Overlay` + `ModalFrame` + `ListNav` / `TextField` în
      `ui-components`; `OverlayStack` în aplicație: cele 6 lanțuri înlocuite de o singură
      buclă. Overlay-urile de azi îmbrăcate în contract, fără să le schimb interiorul.
      *Făcut:* `workbench/overlay_stack.rs` — trăsătura `OverlaySlot` (felul: modal /
      popover, deschis, acoperă ecranul gol, tasta, textul, click-ul, hover-ul, scroll-ul,
      desenarea, animația, câmpul de text, `passes`: un popover poate da o tastă mai departe)
      cu **comportamentul comun ca implicit**; fiecare din cele 10 o implementează și
      suprascrie doar ce are diferit. Stiva = `Overlay::ALL` filtrată la ce e deschis (nimic
      de ținut în memorie sau de curățat: ce se închide iese singur din ea); tastele, textul,
      click-ul, hover-ul, scroll-ul, desenarea, animația, tooltip-ul, „e modal”, clipboard-ul
      — toate o singură buclă / întrebare pe ea. O tastă coboară prin overlay-urile deschise
      până la primul care o păstrează (un modal le păstrează pe toate), apoi pe lanțul
      focusului (panoul lateral, panoul de jos, vederea, ecranul gol, căutarea, editorul);
      focusul de dinainte de modal rămâne, deci după Esc tastele ajung iar acolo.
      *Abatere de la plan:* contractul stă în aplicație (overlay-urile de azi sunt câmpuri ale
      `Workbench` și ce fac atinge restul lui); partea pentru module (`ui-components`) vine
      cu 17e, când modulele își aduc modalele. `ModalFrame` / `ListNav` / `TextField` sunt
      ale lui 17c (unificarea interiorului). Capturile: identice.
- [x] **17c. Cadrul comun pentru toți.** Căutarea, Dialogul, About, alegerile, pickerele pe
      `ModalFrame` / `ListNav` / `TextField` (duplicarea dispare), pixel cu pixel la fel.
      *Făcut (cu o abatere, vezi mai jos):* comune în contract, ca implicit — **Esc golește
      întâi căutarea scrisă** (`clear_query`: Setări, Proiecte, nivelul PHP; Căutarea în
      proiect își păstrează felul ei: Esc o închide direct) și **tastele de cursor în
      câmpul oricărei ferestre** (←→, cuvânt, Home / End: o singură funcție, `move_cursor`,
      în locul a 3 copii; Setările, Proiectele și nivelul PHP nu le aveau deloc).
      Animația de apariție: `anim::Appear` în locul a 5 copii (fiecare își păstrează durata).
      `fold()`: una singură (`ui_kit::fold`) în locul a două diferite — căutarea din Proiecte
      ignoră acum și ea diacriticele. Capturile: identice.
      *Abaterea:* Dialogul, About, Căutarea și meniurile **nu** trec pe cadrul `Modal` al
      Setărilor: au alte mărimi, antete, fundaluri și animații, deci ar fi însemnat fie o
      schimbare vizuală, fie un cadru cu câte o opțiune pentru fiecare diferență (mai complicat
      decât ce înlocuia). Comun e ce e cu adevărat la fel; restul rămâne particularitatea
      fiecăruia, prin contract. La fel, ↑↓ / PgUp / PgDn rămân ale fiecărei liste (Căutarea
      sare câte 8, Setările schimbă secțiunea), iar listele de taste din subsol diferă ca
      etichete (deja desenate de un singur `paint_key_hints`).
- [x] **17d. Secțiunile de setări ale modulelor.** `SettingsSection` contribuit de module:
      istoricul local și remote își aduc secțiunea, valorile și intrările în căutare;
      `SettingsModel` / `SettingsAction` pierd câmpurile lor; aplicația nu mai depinde de
      id-urile lor.
      *Făcut:* contractul `SettingsContribution` (`workbench-model::settings_page`, domeniu):
      unde intră blocul (o secțiune a aplicației sau o pagină proprie, așezată după una
      numită), ce arată (`SettingsBlock`: titlu, detaliu, rânduri — alegeri sau un card de
      stare cu buton distructiv), cuvintele pentru căutare, `choose` (salvează), `press`
      (întrebarea și ce rulează la „da”), `apply` (valorile salvate către domeniul lui, la
      pornire și după fiecare schimbare). Istoricul local (`local-history/settings.rs`: zilele,
      versiunile, ce ține proiectul, ștergerea) și Remote (`remote-projects/settings.rs`:
      urcări, descărcări, reîncercări, pagina „Remote”) își declară setările (aceleași chei
      în `settings.toml`: nimic salvat nu se pierde) și blocul. Aplicația: `history_retention.rs`,
      `remote_defaults.rs`, `Ask::ClearHistory` și cele 6 variante din `SettingsAction` au
      dispărut; un singur desenator de bloc în Setări (același aspect). Un modul oprit nu mai
      apare deloc (pagina „Remote” lipsește în loc de nota „modulul e oprit”). Reparat pe drum:
      ↑↓ imediat după deschiderea Setărilor (lista secțiunilor se știe din model, nu de la
      primul desen). Capturile: identice.
- [x] **17e. Modalele funcțiilor în modulele lor.** Căutarea în proiect (modulul indexării),
      pickerul de nivel (lang-php), pickerul de culoare (colors), Proiectele (dacă merită — vezi
      mai jos), prin contract.
      *Făcut:* **contractul pentru module** — o vedere a unui modul se deschide și fără cadrul
      aplicației (`host.open_overlay`: o desenează toată, peste fereastră), ca modal sau ca
      popover, pe lângă cea cu cadru (`open_modal`); stiva o tratează după felul ei. **Pickerul
      de culoare** e al modulului `colors` (`picker_view.rs`): editorul îl deschide lângă
      swatch, culoarea se scrie înapoi prin `AppRequest::ReplaceInActive` (un singur pas de
      undo, ca înainte), pipeta prin `SampleScreen`. **Căutarea în proiect** e a modulului de
      indexare (`search_panel.rs` + `search_view.rs`): caută prin interogările proiectului,
      modelul semantic vine prin gazdă (portul `SemanticSearch`, cu `SearchMode`, în domeniu),
      un folder se arată în arbore (`AppRequest::ShowFolder`), căutările recente se țin per
      proiect. *Abatere:* **pickerul de nivel rămâne al aplicației** (pe contract): nu e al PHP
      — e comun oricărui limbaj cu versiuni (`LanguageIntelligence::levels()`), iar versiunile
      le aduc deja modulele limbajelor; mutat în `lang-php` ar fi fost greșit. Capturile:
      identice.
- [x] **17f. Curățenia** și lista de verificare. (Cheia `settings.remote_off`, rămasă fără
      folosință, ștearsă; `CLAUDE.md`: regula modalelor spune cum își aduc modulele ferestrele.)

## Lista de verificare (nimic nu se pierde)

- [x] Fiecare modal arată pixel cu pixel ca înainte (tema închisă și deschisă) —
      `scripts/modal-snapshots.sh compare`, după fiecare pas (singura schimbare voită:
      slider-ele paginii Remote, referința înregistrată din nou atunci).
- [x] Esc / × / butoanele închid; un clic în afară nu închide; nimic din spate nu primește
      taste, text, comenzi, scroll sau hover (`tests/unit/workbench/overlays.rs`: 10 teste).
- [x] Tastele fiecăruia (↑↓, PgUp / PgDn, ↵, Tab, ⌘A / C / X / V în câmpuri), căutarea din
      Setări (cu deschiderea rezultatului), secțiunile cu ↑↓, filtrul temelor — tastele de
      cursor sunt acum aceleași în câmpul oricărei ferestre; testele Setărilor, ale căutării
      (`indexing`), ale pickerului (`colors`).
- [x] Dialogurile cu câmp (redenumire, „Go to line”), confirmările modulelor (`host.confirm`,
      inclusiv ștergerea istoricului din Setări), alegerile (`host.choose`), meniul contextual
      cu submeniuri — testele existente trec.
- [x] Animațiile de apariție (captura le așteaptă finalul), tooltip-urile suprimate cât e
      deschis un modal (orice modal, prin stivă).
- [x] Ecranul gol (home) care folosește același picker de proiecte (verificat vizual).
- [x] Windows: clippy (`check.sh`); tastele trec prin aceleași căi (Ctrl în loc de ⌘ e al
      keymap-ului, neatins).

## Ce câștigăm, ce costă (sincer)

- **Câștig**: corectitudine (o singură ordine, regula „modal” verificată într-un loc — azi
  pare încălcată pentru Setări / Căutare / Proiecte); ~600–900 de linii de duplicare mai puțin;
  un modal nou = un tip care implementează contractul, nu ramuri în 6 fișiere; modulele își
  aduc setările și modalele (aplicația nu mai știe de remote / istoric / PHP / culori).
- **Cost**: ~3.000–4.000 de linii atinse, mult în rutarea input-ului (zona cea mai sensibilă);
  6 pași. Riscul vizual e ținut sub control de comparația pixel cu pixel; riscul de input, de
  testele din 17a.
- **Ce n-aș face**: Proiectele și About ca module separate — țin de aplicație (deschiderea
  ferestrei, ecranul gol); ajunge să fie pe contract. Meniul contextual rămâne al aplicației
  (e deja generic: modulele contribuie rânduri prin `MenuContribution`).

## Hotărât (2026-10-22)

1. **Tot planul**, în ordinea 17a → 17b → 17c → 17d → 17e → 17f.
2. Nepotrivirile confirmate în 17a se repară acolo (sunt bug-uri).
3. **Customizabil**: fiecare modal are particularitățile lui (About se închide și la ↵,
   Setările cu ↑↓ schimbă secțiunea sau rezultatul, pickerele sunt popover-e care se închid
   la un clic în afară, dialogul cu butoane distructive la stânga, vederea modulului cu
   câmpurile ei…). Contractul dă comportamentul comun ca **implicit** și fiecare îl poate
   înlocui pe bucăți: felul (modal / popover), tastele, ce închide, cadrul (antet, subsol,
   fundal, animație), lățimea, ce face Esc când are o căutare scrisă. Nimic nu e forțat în
   tiparul comun dacă nu se potrivește.
