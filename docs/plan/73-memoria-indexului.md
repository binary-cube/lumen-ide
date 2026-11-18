# 73 — Memoria indexului proiectului activ

**Stare (2026-11-11): pașii 1 și 2a făcuți (33,7 → 30,6 MB pe copia Laravel, −9%); 2b măsurat și lăsat; pasul 5 făcut ca planul 74 (comprimate în RAM): 30,6 → 23,6 MB, −30% față de început.**

Cerut 2026-11-10 („ce mai avem de făcut să fim optimi?”, apoi „da, go”). Din TODO
(Performanță): sesiunea reală cu 3 proiecte avea 415 MB de heap, aproape tot indexul;
proiectele din spate își lasă deja indexul pe disc, cel activ îl ține întreg.

## Măsurat

Unealta: `EDITOR_MEASURE_PROJECT=<copie> cargo test -p app --release --bin lumen
project_index_memory -- --ignored --nocapture` (heap-ul viu, numărat de alocatorul testelor;
proiectul deschis, indexat, apoi indexul de simboluri golit, apoi proiectul lăsat).

Copie a unui proiect Laravel (8.251 fișiere cu simboluri, 8.189 în `vendor`), indexat în 6,3 s:

| | heap |
|---|---|
| proiectul indexat | +33,7 MB (~4 KB pe fișier) |
| indexul de simboluri (estimarea lui) | 22,7 MB, din care `vendor` 22,6 MB |
| — simbolurile fișierelor | 16,5 MB |
| — tabela după nume | 3,5 MB |
| — membrii după clasă | 2,0 MB |
| — tabela după FQN | 0,7 MB |

Simbolurile: 67.169, din care 54.817 membri (metode, proprietăți, constante).

| din ce sunt făcute | heap |
|---|---|
| înregistrările `Symbol` (~151 B fiecare, fără texte) | 9,7 MB |
| semnăturile (`detail`) | 1,2 MB |
| importurile (`UseImport`) | 1,1 MB |
| numele | 0,8 MB |
| FQN-ul membrilor | 0,6 MB |
| tipurile din doc (`types`) | 0,5 MB |
| FQN-ul tipurilor, părinții, notele | 0,7 MB |

Neadunat mai sus: costul alocatorului pe fiecare text mic (câteva texte pe simbol).

**Concluzii:**
- Căutarea text sare peste `vendor` (`searchable`), deci în proiecte ca ale utilizatorului
  memoria indexului e aproape toată simbolurile dependențelor.
- Golirea indexului eliberează doar 8,7 MB: worker-ul indexerului ține aceleași
  `Arc<FileSymbols>` în harta lui (`CachedFile`, pentru cache-ul de pe disc). Nu e o copie
  (același `Arc`), dar o măsurătoare prin eliminare trebuie să țină cont de asta.
- Nu e o risipă mare într-un singur loc: e un cost mic înmulțit cu multe înregistrări.
- La ~70.000 de fișiere PHP (cele 3 proiecte ale utilizatorului) asta face ~290 MB.

## Pași (după câștig și risc)

Fiecare: măsurat înainte / după cu aceeași unealtă, cifrele în commit, `./scripts/check.sh`
verde, un commit pe pas.

1. **Intrările tabelelor, mai mici** (doar `domain/intelligence/src/index.rs`). O intrare
   ține azi calea (`Arc<Path>`, 16 B), fișierul (`Arc<FileSymbols>`, 8 B) și poziția (8 B):
   32 B, plus slotul tabelei. Calea și fișierul se pot ține o dată pe fișier (o valoare
   comună a hărții), iar intrarea doar referința la ea și poziția pe 32 de biți: 16 B.
   Câștig estimat: ~2 MB din 6,2 (tabele), fără nicio schimbare în afara indexului.
   **Făcut:** `Indexed { path, symbols }` o dată pe fișier, `Entry { file, index: u32 }`.
   Măsurat: heap-ul proiectului indexat 33,7 → 31,3 MB; tabelele 6,2 → 3,7 MB (după nume
   3,5 → 2,1, membrii 2,0 → 1,1, după FQN 0,7 → 0,5).
2. **Înregistrarea `Symbol`, mai compactă** (atinge modulele de limbaj: `range`, `offset`,
   `detail` sunt folosite în sute de locuri). `range` / `offset` pe 32 de biți, `parents` ca
   felie (`Box<[Arc<str>]>`), `note` + `types` (rare) împreună într-o singură cutie
   opțională. ~151 → ~104 B: ~3 MB aici. De făcut cu accesori, ca schimbarea să nu se
   repete în fiecare modul.
   - **2a făcut:** `parents` ca felie fixă. Măsurat: înregistrările 9,7 → 9,2 MB, proiectul
     indexat 31,3 → 30,6 MB.
   - **2b reevaluat (calculat, nefăcut):** `range` / `offset` pe 32 de biți câștigă doar 8 B
     pe simbol din cauza alinierii (144 → 136 B): ~0,5 MB aici (<2%), pentru sute de locuri
     atinse în modulele de limbaj — nu merită riscul. `note` + `types` într-o singură cutie:
     24 B pe simbol, ~1,6 MB (~5%), ~115 locuri.
   - **Măsurat:** costul alocatorului pe textele mici. 221.301 alocări (~3,3 pe simbol);
     rotunjirea lor la 16 B adaugă 1,5 MB, peste heap-ul numărat (care vede doar ce se
     cere). Un singur text per fișier (simbolurile ținând început + lungime) ar scoate cele
     ~205.000 de alocări și 40 B din fiecare înregistrare: ~4 MB (~13%), dar fiecare modul
     citește azi `s.name`, `s.fqn`, `s.detail` direct — o schimbare peste tot.

**Concluzia după măsurători:** compactările rămase aduc fiecare 5–13% și ating multe module.
Pârghia mare e alta: **82% din simboluri sunt membrii claselor din dependențe** (54.817 din
67.169, aproape toate în `vendor/`), folosiți doar la `->` / `::` pe o astfel de clasă.

5. **(Propus) Membrii dependențelor citiți la cerere.** Tipurile și funcțiile din `vendor`
   rămân în memorie (completarea numelor, definițiile); membrii lor stau doar în cache-ul de
   pe disc al indexerului (există deja: `codec`) și se citesc pe clasă când sunt ceruți,
   ținuți într-un cache mic (LRU, sub `kernel::cache`). Câștig estimat: cea mai mare parte
   din ~25 MB aici și din ~250 MB la utilizator. Cere: prima completare / hover pe o clasă
   din `vendor` așteaptă o citire în fundal (o dată pe clasă), niciodată pe firul UI; restul
   intelligence-ului (moștenirea, `members_of`) trece prin aceeași cerere. Plan separat:
   `docs/plan/74-membrii-dependentelor-la-cerere.md`.
   **Făcut altfel (planul 74):** membrii stau comprimați în RAM, nu pe disc, și se decodează
   sincron la prima cerere (microsecunde): fără așteptare, fără citire în fundal. Proiectul
   indexat 30,6 → 23,6 MB; indexul de simboluri ~22,5 → 9,1 MB.
3. **FQN-ul membrilor nu mai e ținut** (se face din clasă și nume când e cerut): 0,6 MB de
   text plus un text alocat mai puțin pe membru.
4. **De cântărit după 1–3:** importurile fișierelor din biblioteci (1,1 MB) sunt necesare
   doar ca să se rezolve numele din semnăturile lor; dacă se rezolvă la extragere, nu mai
   trebuie ținute.

Țintă: o treime din indexul de simboluri (~7–8 MB aici, ~100 MB la utilizator), fără nicio
schimbare în ce vede utilizatorul (completare, definiții, hover).
