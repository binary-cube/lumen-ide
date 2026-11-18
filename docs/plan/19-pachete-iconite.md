# 19. Pachete de iconițe

Cererea: mai multe pachete de iconițe — pe lângă stilurile setului nostru, pachete întregi, cu
alte desene, și seturi de iconițe pentru fișiere (pe tipuri și foldere speciale). Notată în
planul 11 („Pachete de iconițe”).

## Azi (inventar, 2026-10-22)

- `render/icons`: 79 de iconițe proprii (SVG 24×24, linie de 2 px, colțuri rotunde — în felul
  Lucide), încorporate; **5 stiluri** (Regular, Light, Bold, Duotone, Sharp) care sunt
  transformări ale acelorași desene (grosimea liniei, umplerea, capetele). Alegerea:
  Setări → Iconițe (cardurile cu previzualizare), setarea `[appearance] icons`.
- Modulul `file-icons`: 13 iconițe de fișiere (arhivă, bază de date, docker, git, imagine,
  markdown…), înregistrate peste cele încorporate; restul fișierelor — iconițele generice
  (`file-code`, `file-json`, `file-style`…) colorate după limbaj.
- Iconițele se desenează la cerere, în cache, pe mărimea și stilul cerute (GPU, nu imagini).

## Ținta

**Pachete de iconițe ale interfeței** — desene complet diferite, nu transformări:

| Pachet | Licență | Felul | Ce aduce |
|---|---|---|---|
| Lucide | ISC | linie (ca al nostru, mai multe variante) | aproape identic ca stil, desene mai fine |
| Tabler | MIT | linie, 1.5–2 px, mai geometric | aspect „curat”, multe variante |
| Phosphor | MIT | 6 greutăți (thin … fill, duotone) | rotunjit, prietenos |
| Material Symbols | Apache-2.0 | umplut / contur (Google) | aspectul Android / Google |
| Remix Icon | Apache-2.0 | linie / umplut, 24×24 | neutru, foarte complet |
| Heroicons | MIT | contur 1.5 px (Tailwind) | aspectul Tailwind / web |
| Fluent UI System Icons | MIT | regular / filled (Microsoft) | aspectul Windows 11 / Office |

**Seturi de iconițe pentru fișiere** — colorate, pe tip și pe nume:

| Set | Licență | Ce aduce |
|---|---|---|
| Material Icon Theme | MIT | sute de tipuri de fișiere și foldere speciale (`src`, `tests`, `config`, `public`, `vendor`, `node_modules`, `.github`…) |

Reguli:
- **Fiecare pachet e un modul** (`icons-lucide`, `icons-tabler`, `icons-phosphor`,
  `icons-material`, `file-icons-material`), cu datele lui (SVG-urile, licența) — **încărcat doar
  cât e ales**, eliberat când alegi altul.
- **Datele sunt ale noastre, generate** (ca stub-urile PHP): `cargo xtask icon-packs` ia
  pachetele publicate (npm, aceleași versiuni de fiecare dată), alege desenele de care avem
  nevoie după un tabel de corespondență (numele nostru → numele lor) și le scrie în
  `crates/modules/icons-*/data/` cu licența alături. În repo, deci utilizatorii nu au nevoie de
  rețea. Ce un pachet nu are, rămâne din setul nostru (niciodată o iconiță lipsă).
- **Contractul** (`render/icons` + `ui-components`): un pachet spune id-ul, numele, autorul /
  licența, felul (linie — stilurile noastre i se pot aplica — sau umplut) și dă desenul unui
  nume, la cerere. Aplicația doar îl descoperă (ca ferestrele de unelte).
- Setări → Iconițe: „Interfața” (stilurile noastre + pachetele, cu previzualizarea de acum) și
  „Fișierele” (implicite / Material Icon Theme), cu previzualizare pe un mic arbore.
- Nimic pe UI thread: datele pachetului citite în fundal la alegere; până atunci, cele de acum.
- **Doar ce folosim**: din fiecare pachet (Lucide are peste 1.500 de iconițe, Material peste
  3.000) generatorul ia **doar desenele numelor noastre** (~90) — restul nici nu ajunge în repo.
  Din pachetul ales, o iconiță e citită și desenată doar când apare (pe mărimea cerută); un
  pachet nealeș nu ține nimic în memorie, iar cel lăsat își eliberează datele și desenele.

## Pașii (un commit fiecare; `check.sh`; „go” între ei)

- [x] **19a. Contractul și registrul.** `IconPack` (id, nume, licență, fel, desenul unui nume),
      contribuit de module; `Icons` caută întâi în pachetul ales, apoi în setul nostru; setarea
      `[appearance] icons` primește și id-urile pachetelor (vechile id-uri merg mai departe);
      stilurile se aplică doar pachetelor „linie”. Teste (un pachet fals).
      *Făcut:* `icons::IconPack` (id, nume, autor și licență, desenul unui nume la cerere,
      `release`) contribuit de module; `IconLook` (un stil de-al nostru sau un pachet) — și în
      cheia glifei (două aspecte nu împart niciodată un desen); `Icons::register_pack`,
      `looks`, `look_id` / `look_from_id`, `set_look` (pachetul lăsat își eliberează datele);
      ce nu are pachetul se desenează din setul nostru. Setarea primește id-ul unui stil sau al
      unui pachet (unul dispărut → al nostru). Setările primesc lista aspectelor de la aplicație
      (nume, autor / licență). Capturile: identice.
- [x] **19b. Generatorul.** `cargo xtask icon-packs` (descărcarea versiunilor fixate, tabelul de
      corespondență pentru cele ~80 de nume ale noastre, scrierea datelor și a licențelor);
      `--check` spune ce lipsește din fiecare pachet.
      *Făcut:* `cargo xtask icon-packs [--check]`: versiunile fixate (lucide-static 1.61.0,
      @tabler/icons 3.49.0, @phosphor-icons/core 2.1.1, @material-symbols/svg-400 0.48.1,
      remixicon 4.9.1, heroicons 2.2.0, @fluentui/svg-icons 1.1.344), licențele verificate;
      tabelul pentru cele 101 nume ale noastre (ale setului și ale modulelor); câte are fiecare:
      Lucide 95, Tabler 95, Phosphor 94, Material 92, Remix 92, Fluent 89, Heroicons 68 (restul
      — ale noastre). Datele: `crates/modules/appearance/icon-packs/data/<pachet>.svgs` (30–45 KB fiecare,
      un SVG pe rând) și `<pachet>.LICENSE` (Fluent nu publică fișierul: nota din package.json).
- [x] **19c. Modulele celor 7 pachete** ale interfeței, încărcate la nevoie; Setări → Iconițe
      cu cardurile lor (previzualizarea pe aceleași 12 iconițe).
      *Făcut:* crate-ul `icon-packs`, cu câte un modul pe pachet (`icons.lucide`…; pornit /
      oprit în Setări → Module ca oricare); datele în binar, neatinse până la alegere; indexul
      citit la primul desen și eliberat când alegi alt pachet (`release`). Setări → Iconițe:
      cardurile celor 7 pachete după stilurile noastre (numele, autorul și licența), cu
      previzualizarea pe aceleași iconițe. Testat: fiecare din cele ~625 de desene se desenează;
      interfața întreagă verificată cu Material și Fluent (capturi).
- [x] **19d. Iconițele fișierelor.** Contractul pentru seturile de fișiere (după extensie, nume
      de fișier, nume de folder; deschis / închis), modulul `file-icons-material`, arborele,
      taburile, căutările îl folosesc; Setări → Iconițe → „Fișierele”.
      *Făcut:* `icons::FileIconSet` (fișierul după nume și limbaj, folderul după nume, deschis
      sau nu; desenele în culorile lor) și **un singur loc care decide iconița** — registrul:
      `Icons::file(nume, a noastră, limbaj)` / `Icons::folder(nume, deschis)` dau iconița și
      felul culorii (`Tint`: limbaj / folder / proprie); aplicația, arborele, căutările doar
      numesc fișierul. Glifele color respectă acum transparența (rândurile estompate, modalele
      care apar). Generatorul ia din Material Icon Theme 5.39.0 doar regulile pentru fișierele
      și folderele obișnuite (329 de reguli, 199 de desene, ~125 KB — nu 8.000 / 1.251 / 5 MB);
      modulul `icons.files-material`; Setări → Iconițe → „Iconițele fișierelor” (ale noastre /
      Material, cu previzualizare); setarea `[appearance] file_icons`. Capturi: arborele cu
      ale noastre și cu Material.
- [x] **19e. Lista de verificare**, capturi (fiecare pachet, tema închisă și deschisă), memoria
      (un pachet ales nu ține în memorie pe celelalte), Windows.
      *Făcut:* capturi cu fiecare din cele 8 aspecte (ale noastre + 7 pachete), tema închisă și
      cea deschisă, cu iconițele fișierelor Material; testul de memorie
      (`only_the_chosen_pack_holds_anything`: cu toate înregistrate, doar cel ales își citește
      datele, cel lăsat le eliberează, ale noastre alese — niciunul nu ține nimic); Windows:
      clippy (`check.sh`), nerulat acolo.

## Hotărât (2026-10-22)

1. Pachetele interfeței: **Lucide, Tabler, Phosphor, Material Symbols, Remix Icon, Heroicons,
   Fluent**; fișierele: **Material Icon Theme**. Încărcate doar cât sunt alese.
2. Alegerea: **globală**.
3. Generatorul descarcă de pe npm (doar la noi); datele rămân în repo.

## De hotărât (răspuns mai sus)

1. Pachetele: propun **Lucide, Tabler, Phosphor, Material Symbols** pentru interfață și
   **Material Icon Theme** pentru fișiere (toate cu licențe permisive, cunoscute). Vrei și
   altele (de ex. Remix Icon — Apache-2.0, Heroicons — MIT, Fluent — MIT)?
2. Alegerea **per proiect** (un proiect cu alt pachet decât restul)? Propun: globală acum,
   per proiect mai târziu, dacă o vrei.
3. Generatorul descarcă pachetele de pe npm la rularea lui (doar la noi, ca să le actualizăm);
   datele rămân în repo. E ok?
