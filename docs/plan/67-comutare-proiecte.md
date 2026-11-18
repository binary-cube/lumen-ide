# Plan 67 — Comutare rapidă între proiectele deschise

## Context
Cerut de utilizator (2026-11-07): cu mai multe proiecte deschise (câte o fereastră fiecare),
trecerea de la unul la altul cere acum clic dreapta, Mission Control sau ⇧⇥. Vrea:
- o combinație de taste care deschide un modal cu proiectele deschise: le filtrez, aleg unul și
  fereastra lui vine în față; fiecare cu o captură a ferestrei;
- fiecare proiect deschis cu numărul lui și o scurtătură (proiectul 1, 2, …), numărul și
  scurtătura afișate și în bara de sus, după ramura git;
- un modul bine scris, cu API, după structura proiectului.

Decis cu utilizatorul: proiectele pe **⌘⌥1…9** (Ctrl+Alt+cifră pe Windows / Linux; ⌘1 / ⌘7 /
⌘8 rămân ale panourilor), modalul pe **⌘⌥P** (Ctrl+Alt+P).

## Ce facem

- **67a — proiectele deschise, ca API, cu numerele lor.**
  - `workbench-model`: `OpenProject { window, name, path, number, current }` și pe host
    (`SystemHost`): `open_projects()` (toate ferestrele cu proiect, în ordinea numerelor) și
    `focus_project(window)` (fereastra lui în față).
  - Aplicația dă numerele: fiecare fereastră cu proiect primește cel mai mic număr liber (1–9)
    și îl păstrează cât e deschisă; închisă, numărul se eliberează. Ținute în `Shared`, ca
    proiectele deschise.
  - Punct de extensie nou `TopBarBadge`: ce scrie în bara de sus după ramura git pentru
    proiectul ferestrei (aici: scurtătura lui). Aplicația nu știe de modul.
  - Modulul `project-switcher` (`crates/modules/project/switcher`): comenzile
    `projects.go_1` … `projects.go_9` (⌘⌥1…9, schimbabile în Scurtături), scurtătura
    proiectului în bara de sus (cea din keymap, cum o are utilizatorul).
- **67b — modalul.** `projects.switcher` (⌘⌥P): un modal al modulului cu proiectele deschise —
  câmpul de filtrare (nume și cale), cardurile lor (numele, calea scurtată la mijloc, numărul cu
  scurtătura, cel curent marcat), ↑ ↓ ← → / mouse alege, ↵ sau clic aduce fereastra în față,
  Esc închide (niciodată clic în afară). Fără nimic deschis în altă fereastră: spune asta.
- **67c — capturile.** Renderer-ul învață să dea o imagine mică a ultimului cadru
  (`Renderer::capture`: wgpu copiază cadrul și îl citește, micșorat pe un fir de fundal;
  software: din bufferul lui). Aplicația ține o captură per fereastră, făcută când fereastra
  pierde focusul și când se deschide modalul (a ei: `SystemHost::capture_window`, luată
  din cadrul de pe ecran, înaintea modalului); câteva sute de KB fiecare, eliberate când
  proiectul ferestrei se închide. Ținute în `workbench_model::pictures` (oricare modul le
  citește; `taken` spune când s-a schimbat una — lista o redesenează). Modulul le arată în
  carduri, întregi, centrate; fără captură încă: un card simplu, cu inițialele.

- **67d — numărul ales de utilizator, insigna și butonul** (cerut, 2026-11-07).
  - Numărul e al utilizatorului: „Schimbă numărul…” (lista 1–9: al cui e fiecare, sau
    liber) — proiectul îl ia, cel care îl avea primește numărul vechi (schimb). Ținut cu
    proiectul (`[project] number` în setările lui): la deschidere îl ia dacă e liber.
  - Insigna din bara de sus devine un element complet (`TopBadge`: text, tooltip, ce rulează
    un clic, meniul unui clic dreapta): clic → numărul; clic dreapta → meniul modulului
    (Proiecte deschise…, Schimbă numărul…).
  - Un buton în bara din dreapta, deasupra lui „Ține treaz”: clic → lista; clic dreapta →
    același meniu.

## Stare
- [x] 67a
- [x] 67b
- [x] 67c
- [x] 67d
