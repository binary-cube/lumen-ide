# 01 — Viziune și cerințe

## Viziune

Un IDE ușor pentru dezvoltare web (PHP în primul rând), care:
- pornește instant și rămâne fluid pe proiecte mari (Laravel, Symfony, WordPress, monorepo-uri JS);
- organizarea clasică de IDE (arbore în stânga, editor cu taburi, terminal jos), dar cu mai puțin „zgomot” vizual;
- are inteligență de cod reală pentru PHP (autocomplete, go to definition, find usages), nu doar colorare;
- poate fi extins cu limbaje noi fără să atingem nucleul.

## Cerințe funcționale

### F1. Editare text
- Deschidere, editare, salvare; encoding UTF-8 implicit, detecție pentru UTF-16 sau Latin-1 la deschidere.
- Line endings LF/CRLF păstrate per fișier.
- Multi-cursor, selecție pe coloane, undo/redo grupat pe tranzacții.
- Căutare și înlocuire în fișier (regex inclus); căutare în proiect.
- Indentare automată, închidere automată a parantezelor și ghilimelelor, comentare de linie sau bloc în funcție de limbaj.
- Folding pe baza arborelui sintactic.

### F2. Module de limbaj
- Module inițiale: **PHP, HTML, JavaScript, CSS, SCSS**.
- Detecție după extensie, nume de fișier (ex: `.php-cs-fixer.php`), shebang sau override manual.
- Fișierele `.php` sunt **multi-limbaj**: PHP + HTML + JS (în `<script>`) + CSS (în `<style>`), fiecare zonă cu inteligența ei.
- Modulele pot fi încărcate automat (toate cele disponibile) sau selectiv din config.

### F3. Language intelligence
- Evidențiere de sintaxă (syntax highlighting) incrementală.
- Formatare de cod (format document sau selecție), cu formatter nativ ori extern (php-cs-fixer, prettier).
- Autocomplete contextual.
- Go to definition / declaration.
- Find usages (cine folosește simbolul).
- Diagnostice (erori de sintaxă la început, apoi semantice).
- Outline / structura fișierului.

### F4. Workbench (UI)
- Panou stânga: arborele proiectului (respectă `.gitignore`, se actualizează live la schimbări pe disc).
- Zona centrală: editor cu taburi, split vertical și orizontal.
- Panou jos: terminal cu mai multe taburi.
- Status bar: limbaj, encoding, poziție cursor, branch git (ulterior).
- Command palette (Ctrl/Cmd+Shift+P) și Go to file (Ctrl/Cmd+P / Shift Shift).

### F5. Teme
- Teme light și dark incluse.
- Fișiere de temă externe, cu reîncărcare la cald (hot reload).
- Tema acoperă UI-ul, sintaxa și terminalul (paleta ANSI).

### F6. Terminal (CLI)
- Mai multe taburi de terminal, fiecare cu shell-ul lui (zsh/bash/pwsh/cmd).
- Randat tot pe GPU, cu aceeași temă ca restul aplicației.

## Cerințe nefuncționale

| Categorie | Țintă |
|-----------|-------|
| Pornire la rece (fără proiect) | < 300 ms până la primul frame |
| Latență tastă → pixel | < 8 ms la p95 (ideal un singur frame la 120 Hz) |
| Scroll | 120 FPS stabil pe un fișier de 100k linii |
| Fișiere mari | Deschidere fișier de 50 MB fără blocarea UI-ului |
| Indexare | Proiect Laravel tipic (~15k fișiere PHP cu vendor) în < 20 s pe fundal, UI-ul rămâne responsiv |
| Memorie | < 300 MB RAM pentru un proiect mediu deschis |
| Platforme | macOS (Metal), Windows (DX12, cu Vulkan ca fallback), Linux (Vulkan, cu GL ca fallback) |
| Stabilitate | Un modul care cade nu doboară editorul (izolare + `catch_unwind` la granițe; procese separate pentru LSP) |

## Eficiență (CPU / GPU / RAM): cerință de bază

Editorul trebuie să fie „invizibil” pentru resursele mașinii. Regulile de mai jos se aplică în fiecare fază și se verifică la fiecare livrare:

| Regulă | Cum o respectăm |
|--------|-----------------|
| **0% CPU la repaus** | Bucla de evenimente stă în `Wait`; se desenează un frame doar când s-a schimbat ceva (input, resize, rezultat de la un task). Fără redesenare continuă. Animațiile (ex: cursorul care clipește) cer frame-uri doar cât rulează și pe zona lor. |
| **GPU integrat, puțin lucru pe frame** | Se cere adaptorul `LowPower`. Maximum 2 draw call-uri pe strat (quad-uri + glife, instanțiate). Glifele se rasterizează o singură dată și stau în atlas. |
| **Zero alocări în regim stabil** | `Scene` se refolosește între frame-uri (`reset` păstrează capacitatea), bufferele de instanțe de pe GPU doar cresc, numerele de linie se formatează fără `String` nou. |
| **Doar ce e vizibil** | Shaping și highlight doar pentru liniile din viewport; liniile foarte lungi se trunchiază la lățimea ferestrei. Listele (arbore, rezultate) sunt virtualizate. |
| **Cache-uri limitate** | Fiecare cache are o limită explicită (ex: 2048 de linii shape-uite, atlas de 2048×2048). |
| **Muncă grea în afara thread-ului UI** | Încărcarea fonturilor, parsing-ul, indexarea și I/O rulează pe alte thread-uri; UI-ul primește snapshot-uri. |
| **Pornire în paralel** | Fonturile se încarcă în paralel cu fereastra și cu inițializarea GPU-ului. |

**Bugete măsurate** (macOS, Apple M2 Max, fișier PHP de 100k linii, faza 1):

| Măsurătoare | Buget | Măsurat |
|-------------|-------|---------|
| Pornire până la primul frame | < 300 ms | 205–248 ms |
| CPU la repaus | 0% | 0,0% |
| Memorie (fișier de 100k linii deschis) | < 150 MB | ~71 MB (top) / ~95 MB RSS cu bibliotecile Metal |

## Ce NU facem (cel puțin în prima versiune)

- Debugger (Xdebug): mai târziu, ca modul separat (DAP).
- Integrare git avansată (diff, blame, merge): în v1 doar branch-ul curent și markeri de modificare.
- Colaborare în timp real.
- Marketplace de pluginuri.
- Plugin-uri dinamice terțe (WASM): sunt planificate, dar după ce contractele interne se stabilizează (vezi faza 8 din roadmap).
