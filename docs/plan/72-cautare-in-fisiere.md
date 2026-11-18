# 72 — Căutarea în fișiere: folder, opțiuni, înlocuire

**Stare (2026-11-10): toate etapele livrate** (`effa41a6`, `a5bfc773`, `8e951292`, iar masca de
fișiere din etapa 4); excluderile suplimentare și istoricul folderelor rămân la cerere.

Cerut 2026-11-10. Scopul: tot ce face „Find in Files” / „Replace in Files” dintr-un IDE, într-o
singură zonă de căutare (cea de acum, ⌘⇧F), fără un al doilea modal. Opțiunile stau ascunse
și apar când e nevoie: un iconiță mică în câmp, la click zona ei.

## Ce avem (verificat în cod)

- Căutarea de azi întreabă indexul (`ProjectQueries::text`): rezultate ordonate după relevanță
  (BM25, opțional semantic), bucăți de 40 de linii, primele 50. Nu găsește toate aparițiile.
- `text_search::SearchQuery.paths` filtrează deja după prefixul căii, dar nu-l setează nimeni.
- Majuscule, cuvânt întreg, regex: doar în căutarea dintr-un document (`document::domain::search`,
  `Matcher` pe `regex`), nu peste fișiere.
- Înlocuirea peste fișiere există ca plan de refactorizare (`refactor::Plan`,
  `ViewerAction::ApplyRefactoring`): fișierele deschise se schimbă în documente, celelalte pe
  disc, totul se poate anula („Undo Last Refactoring”), ce s-a schimbat între timp e sărit.
- Meniul arborelui primește rânduri prin `ContextMenuItem` (`MenuPlace::TreeFolder`); comanda
  află folderul din `host.context().path()`.
- Nu există autocomplete pentru căi; `Picker::open_suggestions` e o listă de sugestii sub un câmp,
  iar folderele vin din `ProjectQueries::paths` (potrivire fuzzy pe nume).

## Reguli

- O singură zonă de căutare. Ce nu e folosit nu se vede: iconițele din câmp, rândul folderului
  apare la click (sau când e setat), rândul înlocuirii doar în modul înlocuire.
- Căutarea exactă (toate aparițiile) rulează în fundal, sub o generație: tastarea nu așteaptă
  după disc, un rezultat vechi nu ajunge peste unul nou.
- Lista fișierelor respectă `.gitignore` și excluderile (`ProjectQueries::files`), citirea are
  limita ei de mărime.
- Indexul rămâne calea implicită (rapidă, cu relevanță); căutarea exactă pornește doar când o
  cer opțiunile sau înlocuirea.
- Un commit pe etapă, `./scripts/check.sh` verde, teste în `tests/unit/`.

## Etape

### 1. Folderul

- În câmp, înainte de `esc`: iconiță de folder. Click: apare sub câmp rândul „În:” cu un câmp
  pentru cale (relativă la proiect; gol = tot proiectul). Setat, iconița e aprinsă și rândul
  rămâne.
- Sugestii de foldere sub câmp cât scrii (din `ProjectQueries::paths`), ↑↓ și Tab / Enter
  aleg. Tab mută între câmpul de căutare și cel al folderului.
- Rezultatele (conținut, fișiere și foldere după nume) se limitează la folder.
- În arbore, click dreapta pe un folder: „Find in Files…” deschide căutarea cu folderul pus.

### 2. Căutarea exactă și opțiunile

- În câmp: `Cc` (majuscule), `W` (cuvânt întreg), `.*` (regex). Oricare aprinsă: căutare
  exactă în fundal peste fișierele din folder, fiecare apariție un rând (fișier, linie, textul
  liniei cu potrivirea evidențiată), cu o limită și numărul total.
- Regex greșit: spus sub câmp, fără rezultate vechi amestecate.
- Motorul e în domeniu (pe `&str`, refolosind `Matcher`), testat separat.

### 3. Înlocuirea

- „Replace in Files…” (⌘⇧R) și în meniul folderului: deschide aceeași zonă cu rândul
  înlocuirii. Rezultatele arată ce se schimbă; „Replace All” aplică un `refactor::Plan`.
- Se poate anula ca orice refactorizare; fișierele blocate sunt refuzate și spuse.

### 4. Opțional, la cerere

Masca de fișiere (`*.php`), excluderi suplimentare, istoricul folderelor folosite.

## Gata când

- Click dreapta pe `migrations` → „Find in Files…” caută doar acolo.
- Folderul se poate scrie de mână, cu sugestii, și în căutarea obișnuită.
- `Cc`, `W`, `.*` dau toate aparițiile; „Replace All” se poate anula.
