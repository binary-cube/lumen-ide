# 35 — Teme noi și seturi noi de iconițe

Cinci teme noi, plus seturi noi de iconițe pentru interfață și pentru fișiere.

## Ce avem

- **22 de teme** în `themes/*.toml`, incluse de modulul `themes-builtin`. O temă nouă
  înseamnă doar un fișier nou, fără cod.
- **7 seturi de iconițe de interfață** (Lucide, Tabler, Phosphor, Material, Remix,
  Heroicons, Fluent) în modulul `icon-packs`.
  - Datele lor (`data/<pachet>.svgs`) vin din `cargo xtask icon-packs`, din pachete npm cu
    licență liberă.
  - Păstrăm doar desenele numelor noastre (tabela `NAMES`).
  - Un set se citește la primul desen și se eliberează când alegi altul.
- **Un singur set de iconițe pentru fișiere:** Material Icon Theme, cu reguli după
  extensie, nume, limbaj și folder (`material-files.map`) și desenele lor.

## 35a — cinci teme noi

Palete proprii, cu nume care nu sunt mărci, de la zero după contractul din
`07-teme.md` (paleta, apoi `[ui]` și `[syntax]` din ea):

| id | fel | ideea |
|---|---|---|
| `fjord` | întunecată | gri-albastru rece, accent albastru de gheață, contrast moale |
| `dusk` | întunecată | bleumarin adânc, accent violet, sintaxă cyan și roz |
| `slate` | întunecată | gri neutru, accent albastru clasic, pentru ore lungi |
| `linen` | luminoasă | alb cald ca hârtia, accent teracotă |
| `lavender` | luminoasă | alb rece, accent lavandă, sintaxă discretă |

- Cursorul rămâne cel implicit, ca la majoritatea temelor existente.
- Un test nou de contrast (WCAG) trece prin toate temele incluse, pe fundalul editorului:
  - codul: cel puțin 4,5;
  - textul estompat și cuvintele cheie, șirurile, funcțiile, numerele: cel puțin 2,8;
  - comentariile: cel puțin 2,2.

  Temele noi stau mult peste prag: codul are între 9 și 12.
- **Capturi:** configuratorul de teme arată 27 de teme. Temele noi le-am văzut în editor,
  dar nu sunt în suită (scriptul trece doar prin temele implicite).

## 35b — seturi noi de iconițe de interfață

Candidați, toți pe npm cu licențe libere (de verificat la generare, ca la cele 7):

| id | pachet | licență |
|---|---|---|
| `iconoir` | `iconoir` | MIT |
| `bootstrap` | `bootstrap-icons` | MIT |
| `octicons` | `@primer/octicons` | MIT |
| `ionicons` | `ionicons` (varianta outline) | MIT |

- `xtask icon-packs` primește încă patru coloane în `NAMES`, adică numele fiecărui set
  pentru fiecare din cele ~80 de iconițe ale noastre.
- Ce lipsește dintr-un set se desenează din setul nostru, ca acum.
- Fiecare set e un modul (pornit sau oprit din setări), cu fișierul de licență lângă
  date.
- **Teste:** fiecare set acoperă iconițele de bază (fișier, folder, căutare, setări,
  închidere…); datele se citesc doar la alegere și se eliberează.

## 35c — seturi noi de iconițe pentru fișiere

Un set de fișiere are nevoie și de regulile lui (extensie, nume, folder → iconiță), nu
doar de desene. Candidați, de ales după ce date publică și după licență:

- **vscode-icons** (MIT): cel mai cunoscut set colorat;
- **Catppuccin Icons** (MIT): pastelat, pe gustul temelor moi;
- **Symbols** (MIT): minimalist, monocrom cu accente.

Ce facem:
- `xtask icon-packs` scrie, pentru fiecare, `data/<set>.map` și `data/<set>.svgs` în
  formatul de azi, cu aceleași liste de extensii, nume și foldere.
- Fiecare set e un modul, ca `icons.files-material`, citit doar când e ales.
- Dacă unul din ei nu are regulile publicate într-o formă pe care o putem citi sau are o
  licență nepotrivită, îl înlocuim cu altul și scriu aici de ce.
- **Teste:** fiecare set dă iconițe pentru `composer.json`, `.php`, `.ts`, `src/`, `.git`
  și cele implicite.
- **Capturi:** arborele cu fiecare set de fișiere; setările de iconițe.

## Ordinea

1. 35a: temele, un commit.
2. 35b: seturile de interfață, un commit.
3. 35c: seturile pentru fișiere, un commit.

Fiecare pas se face după `check.sh` și după capturi.

## Stare

- **35a, gata.** `fjord`, `dusk`, `slate`, `linen`, `lavender` (palete în
  `themes/*.toml`, culorile derivate calculate din paletă), plus testul de contrast.
- **35b, gata.** Iconoir 7.12.1 (86 din 103 de iconițe), Bootstrap Icons 1.13.2 (92),
  Octicons 19.42.0 (85, desenele de 24 px) și Ionicons 8.1.0 (88, varianta outline).
  - Toate sunt MIT, cu licența lângă date.
  - Restul iconițelor se desenează din setul nostru.
  - Datele celor 7 seturi existente au ieșit identice la regenerare.
  - Testele verifică că fiecare desen al fiecărui set se randează și că fiecare set are
    iconițele de bază.
- **35c, gata.** Trei seturi noi de iconițe pentru fișiere, toate MIT:
  - Catppuccin 1.26.0, varianta Mocha;
  - Symbols 0.0.26, fără foldere deschise separate (rămân închise);
  - vscode-icons 13.0.0.
  - Sursa e extensia lor de pe Open VSX: tema de iconițe în formatul VS Code, același
    format pe care îl citim pentru Material.
  - `xtask icon-packs` are acum o listă de seturi de fișiere (`FILE_SETS`), din npm sau
    Open VSX, cu licența verificată. Regulile care trimit la un desen lipsă sunt sărite.
  - Datele Material au ieșit identice.
  - Fiecare set e un modul `icons.files-<id>`; Material își păstrează id-ul.
  - Testul trece prin toate seturile: fișierele uzuale (`index.php`, `app.ts`,
    `composer.json`), `src` deschis și închis, desenul fiecărei extensii randat color și
    eliberarea la alegerea altui set.
  - Mărimea datelor în binar: vscode-icons 489 KB, Symbols 168 KB, Catppuccin 112 KB.
    Se citesc doar când setul e ales.
