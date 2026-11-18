# 71 — Autocomplete contribuit de module: ce mai lipsește

Cerut 2026-11-09. Scopul: fiecare limbaj și framework își aduce sugestiile prin contribuții, în
editor și în serverul LSP propriu, fără cod special în aplicație și fără un al cincilea mecanism
de comunicare.

**Stare (2026-11-09): etapele 1–10 livrate.** Tabelul de mai jos arată punctul de plecare.

## Starea de azi (verificată în cod)

| Zonă | Sugestii azi | Ce lipsește |
|---|---|---|
| PHP, HTML, CSS, SCSS, JS, SQL, `.env` | `LanguageIntelligence` proprie | nimic în acest plan |
| Laravel, Symfony, Yii, Vue, React, HTMX | `Framework::complete` | vezi Twig, Blade, YAML |
| JSON | server LSP inclus, implicit, cu scheme locale (`composer.json`, `package.json`, `tsconfig.json`…) | doar verificare |
| YAML | server LSP inclus, implicit, cu scheme locale (GitHub Actions, Dependabot, Compose, pre-commit) | cheile framework-urilor |
| Twig | taguri HTML; HTMX; numele din șiruri, dacă regula Symfony prinde și în Twig (de verificat) | taguri, filtre, funcții, variabile |
| Blade | HTMX, `env()` din Laravel | directive, componente, variabile |
| Serverul LSP propriu | doar PHP, doar motorul limbajului | framework-uri, assist-uri, celelalte limbaje |

## Reguli

- Doar mecanismele existente: `LanguageIntelligence`, `Framework`, `DocumentAssist`.
- Ștergerea oricărui modul nu schimbă nimic în celelalte.
- Răspunsurile rămân sub bugetul framework-urilor pe firul UI; ce e greu se pregătește în fundal
  (`FrameworkSupport::prepare`), sub generația proprie.
- Datele limbajelor sunt ale noastre: nume și descrieri scrise de noi, niciodată copiate din
  fișiere ale altora.
- Un commit pe etapă, `./scripts/check.sh` verde, teste în `tests/unit/`.

## Etape

### 1. Twig: limbajul

`lang-twig` contribuie un `DocumentAssist` pentru `twig`, cu date în `data/twig.data`:

- în `{% … %}`: taguri, inclusiv perechile de închidere (`endif`, `endfor`);
- după `|`: filtre;
- în `{{ … }}` și în expresii: funcții, teste după `is`;
- cu descriere și snippet pentru parametri.

În interiorul delimitatorilor sugestiile înlocuiesc tagurile HTML (`merge: false`); în afara lor
HTML-ul rămâne neatins.

### 2. Twig: variabilele din fișier

Din text, fără index: `{% set %}`, `{% for %}` (plus `loop.*`), argumentele `{% macro %}`,
`{% import … as %}`.

### 3. Symfony în Twig

Întâi un test care stabilește ce merge deja (`path('…')`, `{% include '…' %}`, traduceri). Apoi:

- variabilele trimise din controller (`render('x.html.twig', [...])`), în fundal;
- `app.*`;
- funcțiile și filtrele pachetelor instalate (`form_*`, `asset`, `is_granted`).

### 4. Blade: directivele

`lang-blade` contribuie un `DocumentAssist`, cu date în `data/blade.data`: directive după `@`, cu
perechile de închidere (`@if` / `@endif`).

De verificat: dacă `DocumentAssist` poate declara caracterul `@` ca declanșator. Dacă nu, se
adaugă o metodă cu implementare implicită pe același contract.

### 5. Laravel în Blade

- variabilele din `view('x', [...])` și `compact(...)`, în fundal;
- `$loop` în `@foreach`, `$errors`, `$slot`;
- componentele `<x-…>` din `resources/views/components` și clasele lor;
- atributele din `@props`;
- numele din `@include`, `@extends`, `@section`, `route()`, `config()`.

### 6. YAML și JSON

- Symfony în `config/**/*.yaml`: chei de servicii, `@serviciu`, `%parametru%`, cu `merge: true`
  peste serverul YAML. Test că framework-ul și serverul se combină fără duplicate.
- JSON: nu facem motor propriu. Test pe `composer.json` și `package.json` cu serverul inclus; se
  repară doar ce pică.

### 7. Serverul LSP: aceeași îmbinare ca în editor

Partea din aplicație care adună limbajul, framework-urile și assist-urile se mută în
`code-intelligence` ca funcție pură. Editorul și serverul o folosesc pe aceeași, deci nu pot
ajunge să răspundă diferit. Această etapă nu schimbă comportamentul.

### 8. Serverul LSP: framework-uri și assist-uri

- Încarcă modulele framework și assist-urile limbajelor (fără consolă și bază de date, care țin
  de interfața aplicației).
- Detectare per proiect, cu cache-urile sub generații actualizate din `didChange` și
  `didChangeWatchedFiles`.
- Răspund la `completion`, `definition`, `hover`.
- De urmărit: mărimea binarului și pornirea, măsurate înainte și după.

### 9. Serverul LSP: restul limbajelor

Se adaugă în `Engines::modules()`: HTML, CSS, JS, SQL, Twig, Blade. Declanșatorii se anunță pe
limbaj. Test pe fiecare, printr-un client LSP real (`lsp-client`).

### 10. Închidere

Roadmap-ul (`10-roadmap.md`) bifat după ce s-a livrat; un test care scoate pe rând modulele noi
și verifică faptul că celelalte răspund la fel.

## Gata când

- Într-un `.twig` apar tagurile, filtrele, funcțiile și variabilele controllerului.
- Într-un `.blade.php` apar directivele, componentele `<x-…>` și variabilele din `view()`.
- `composer.json`, `package.json` și un `services.yaml` Symfony au sugestii.
- Un alt editor conectat la serverul nostru primește aceleași sugestii ca editorul nostru, pe
  aceleași fișiere.

## În afara planului

Formatarea (decizie separată), React și Vue dincolo de ce au deja, Yii în șabloane.
