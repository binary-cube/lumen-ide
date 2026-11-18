# 70 — Marcaje pe foldere și fișiere: ce se indexează

Cerut 2026-11-09. Utilizatorul decide ce intră în index („Mark Directory As”),
vede starea în arbore și are o pagină cu tot ce e marcat. Motivul imediat: `api.acme.test` are
27 GB în `var/` (24 GB `var/old-wp`), iar din `var/` e exclus doar `var/cache`.

## Decis

- **Stări:** Exclus, Bibliotecă, Generat, Normal (scoate efectul regulilor și al părintelui).
  Fără „Sources / Test / Resource Root”: n-ar schimba nimic la noi.
- Marcajul utilizatorului are prioritate față de regulile modulelor (`ProjectDirs`); copiii
  moștenesc de la cel mai apropiat părinte marcat.
- **Exclus:** rămâne vizibil în arbore, estompat; nu apare în căutarea după nume, în index, în
  căutarea text; se poate deschide normal, fără cunoașterea proiectului.
- **În git** (ales de utilizator, ca să le aibă și colegii): `<project dir>/path-marks`, text,
  un rând pe marcaj, excepție în `.gitignore`-ul folderului nostru.
- Aplicare fără repornire: indexul se uită din nou după fingerprint (doar ce s-a schimbat se
  parsează).

## Arhitectură

- `workspace::marks` (domeniu, pur): `PathMark`, `PathMarks`, parsare / scriere, `ProjectMarks`
  (partajat, citit de oricine), evenimentul `PathMarksChanged` pe bus-ul proiectului.
- Aplicația îl citește la deschidere și îl dă indexului (`IndexContext`) și interogărilor
  (`ProjectQueries`). Fără marcaje totul merge ca înainte.
- Un modul nou deține meniul, salvarea, pagina și sugestiile; scos, nu se schimbă nimic.

## Pași

- [x] 70a — domeniul; indexul, căutarea text și căutarea după nume țin cont de marcaje
- [x] 70b — modulul: meniul „Mark As”, salvarea în fișier, `.gitignore`
- [x] 70c — arborele: culoare, nume estompat, eticheta de la capătul rândului
- [x] 70d — pagina „Ce se indexează”: marcajele, motivul, fișiere și memorie
- [x] 70e — sugestii: folderele foarte mari ce merită excluse
- [x] 70f — „Bibliotecă” în ordonarea sugestiilor și la rename (azi merge după numele folderului)
