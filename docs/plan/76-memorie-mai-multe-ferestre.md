# 76 — Memoria și CPU-ul cu mai multe ferestre, proiecte și instanțe

**Stare (2026-11-11): făcute 76a, 76b, 76d, 76i (și fixul `b748c373`); 76g, 76h, 76j, 76k
cântărite și lăsate (motivele la fiecare); 76f de încercat cu utilizatorul pe ecran; 76c
câștigă puțin (căutarea nu intră în biblioteci).**

Cerut 2026-11-11: „o analiză cum mai putem reduce consumul de memorie dacă deschid mai multe
aplicații / mai multe proiecte; dacă se pot face sharing între anumite chestii, inclusiv
layerul de desenare… scopul e ca aplicația să se miște super repede și bine cu un consum de
memorie / CPU / GPU super redus, fără să pierdem nimic din ce avem deja”.

## Cum s-a măsurat

Build release, macOS (Apple Silicon), profil izolat, `footprint` (amprenta procesului, ca
Activity Monitor) și `ps` (timpul de CPU). Ferestrele de 1280×800 puncte (2560×1600 pixeli: o
imagine a suprafeței = 16,4 MB) și **fără focus** (altă aplicație în față). Proiectele: unul
mic, două copii ale aceluiași proiect Laravel (8.251 de fișiere, 8.189 în `vendor`). Al doilea
proiect e deschis printr-o a doua pornire, predată instanței care rulează (cum face
utilizatorul).

| scenariu | amprenta | IOSurface (imaginile ferestrelor) | heap (MALLOC_SMALL) | fire |
|---|---|---|---|---|
| 1 fereastră goală | 75 MB | 31 MB | 20 MB | 13 |
| 1 proiect mic | 77 MB | 31 MB | 21 MB | 15 |
| + Laravel (2 ferestre) | 142 MB | 63 MB | 51 MB | 21 |
| + a doua copie Laravel (3 ferestre) | 194 MB | 94 MB | 70 MB | 24 |
| același lucru, **după fix** | **154 MB** | **63 MB** | 61 MB | 23 |
| 3 ferestre goale | 145 MB | 94 MB | 20 MB | 14 |
| 3 ferestre goale, **după fix** | **112 MB** | **63 MB** | 21 MB | 14 |
| 2 **procese**, câte un Laravel | 107 + 111 = 218 MB | 31 + 31 MB | 52 + 52 MB | 14 + 13 |

CPU în repaus (3 ferestre goale, un minut): **0,73 s înainte, 0,26 s după fix** (ce rămâne e
cursorul care clipește în fereastra cu focus, cum trebuie).

## Găsit și reparat: o fereastră care stă desena la nesfârșit (`b748c373`)

O fereastră nemișcată desena un cadru întreg la ~1,5 s, pentru totdeauna, și ținea **două**
imagini ale suprafeței. Cadrul cerut ca să dea înapoi cadrul păstrat și imaginea de rezervă
desena în loc: fără cadru păstrat, urmăritorul schimbărilor nu știe nimic, deci cadrul e
întreg; e arătat din nou, iar așteptarea imaginii de rezervă reîncepe. Acum, când doar asta e
scadent, aplicația le dă înapoi fără să deseneze (ce e pe ecran rămâne). **~31 → ~21 MB pe
fereastră, −21% pe 3 proiecte, CPU-ul în repaus ÷3.** Pe Windows bucla nu exista (imaginea de
rezervă e doar a macOS-ului), dar calea e aceeași.

## Ce e deja comun (verificat în cod)

Un singur proces pentru toate ferestrele (a doua pornire își predă căile celei care rulează,
`app/src/instance.rs`). În el, o singură dată:

- **GPU:** dispozitivul, coada, pipeline-urile (pe format), atlasul de glife (1024² → 4096²,
  ~1,3–20 MB), texturile imaginilor (`render-wgpu`, `GpuContext`, slot `Weak`);
- **text:** fonturile (mmap), cache-ul de linii modelate, al glifelor (`TextSystem` în
  `Shared`);
- **sintaxa:** gramaticile, interogările compilate (o dată, eliberate după 10 min nefolosite);
- **datele limbajelor:** stub-urile PHP, MDN (CSS/HTML/JS), SQL — `OnDemand`, eliberate după
  10 min nefolosite; modelul semantic (mmap);
- iconițele, temele, monitorul de resurse.

## Unde se duce memoria

- **Pe fereastră:** ~21 MB imaginea suprafeței (minimul cât fereastra e văzută: ce e pe ecran),
  ~24 MB cadrul păstrat cât se lucrează în ea (dat înapoi după 2 s de liniște), blur-uri mici,
  tampoane de instanțe (cresc, nu scad), un fir `syntax` permanent, un fir `session`.
- **Pe proiect:** indexul de simboluri (Laravel: 9,1 MB), restul proiectului indexat (8,7 MB —
  în mare indexul de căutare text), arborele, Git, firul `indexer`, firul `index-queue`,
  watcher-ul, serverele de limbaj (procese separate, sute de MB fiecare la intelephense /
  tsserver, oprite după 2 min fără folosire).
- **Pe proces în plus:** ~55 MB (heap-ul de bază, al doilea dispozitiv GPU, cache-urile) —
  măsurat: două procese 218 MB față de ~160 MB aceleași două proiecte într-unul.

## Propuneri, în ordinea câștigului

Regula pentru toate: ce vede utilizatorul nu se schimbă; fiecare pas măsurat înainte / după,
cu cifrele în commit.

1. **76a. Proiectele care nu sunt în față își lasă indexul — și când aplicația e în spate.**
   Azi indexul unui proiect se odihnește (salvat pe disc, scos din memorie) doar la 2 minute
   după ce **fereastra altui proiect primește focusul** (`ProjectBroughtForward`). Nu se
   întâmplă dacă aplicația toată e în spatele altor aplicații, dacă o fereastră e minimizată,
   sau dacă proiectul a fost deschis în fundal (măsurat: după 3 minute, nimic odihnit). De
   făcut: odihna și pentru o fereastră minimizată / acoperită de mult, și pentru toate
   proiectele în afară de ultimul cu focus când aplicația stă în spate de N minute. Câștig:
   ~18 MB pe un proiect Laravel aici; la utilizator (3 proiecte, ~70.000 de fișiere) zeci de
   MB pe proiect. Cost la revenire: citirea cache-ului (măsurat 69 ms pe Laravel) în fundal.
   Risc mic: mecanismul există, doar e pornit mai des.
   **Făcut:** evenimentul nou `ProjectLeft` (fereastra proiectului pierde tastatura: alt
   program în față, altă fereastră, minimizată), publicat pe busul proiectului. Indexul se
   odihnește după 10 minute de la el (`LEFT_AFTER`, mai lung decât cele 2 de „în spatele
   altui proiect”: poate fi cel la care utilizatorul revine); un proiect deschis fără să vină
   în față (aplicația în spate) e „părăsit” de la început. Venit în față: înapoi. Două
   motive pentru aceeași odihnă: o face primul scadent (`kernel::work::Later::sooner`).
   Câștigul e al mecanismului existent (măsurat mai sus: ~18 MB pe un Laravel).
2. **76b. Bibliotecile comune între proiecte.** Două proiecte cu același `vendor` (aceeași
   versiune Laravel / Symfony) țin de două ori aceleași blocuri împachetate, aceleași
   tipuri decodate, aceleași texte. Măsurat: a doua copie Laravel adaugă 19 MB de heap. De
   făcut: un depozit al procesului, după conținut (hash-ul blocului + versiunea extractorilor
   + regula de vizibilitate), din care proiectele iau `Arc`-uri (`Weak` în depozit: pleacă
   odată cu ultimul proiect). Câștig: cea mai mare parte din cei 19 MB pe fiecare proiect cu
   aceleași biblioteci. Risc mediu: marcajele „Library / Normal” diferă pe proiect (intră în
   cheie); un fișier schimbat într-un proiect nu-l atinge pe al celuilalt (alt hash).
   **Făcut:** `kernel::cache::Interner` (valori după conținut, ținute slab: pleacă odată cu
   ultimul care le ține) și, în indexul de simboluri (care nu știe de limbaje), fișierele de
   bibliotecă împachetate ținute o dată pe proces, după octeții blocului și codecul lor —
   pentru orice limbaj și orice dosar de biblioteci (`vendor`, `node_modules`, ce declară un
   modul). Ce se împarte: octeții, tipurile și funcțiile decodate, numele membrilor, membrii
   decodați (o dată pentru toți). Ce rămâne al proiectului: căile și tabelele (ce găsește
   proiectul): un pachet pe care doar alt proiect îl are nu apare niciodată la completare,
   definiții, hover (test); o altă versiune are alți octeți, deci e alt fișier. Măsurat
   (două copii ale aceluiași Laravel, în același proces, heap-ul după indexare):
   al doilea proiect **+17,9 MB → +9,2 MB** (7.036 de fișiere de bibliotecă ținute o dată).
   Apoi o singură valoare goală pentru simbolurile pe care un fișier de bibliotecă nu le mai
   ține (în loc de câte una pe fișier): **+9,2 → +8,5 MB**.
   **Ce rămâne pe proiect (măsurat prin eliminare):** o fereastră cu un proiect mic, după ce
   datele comune sunt încărcate, costă +0,2 MB; cei 8,5 MB ai unui Laravel sunt aproape toți
   indexul lui (se duc la odihnă): căile fiecărui fișier, tabelele de căutare, tabelul
   indexerului (amprenta fiecărui fișier). Căutarea text nu intră în `vendor`, deci 76c
   câștigă puțin aici. Pasul următor, dacă merită: căile relative și comune între indexer și
   index (azi absolută în index, relativă în indexer), tabelele mai compacte.
3. **76c. Indexul de căutare text, mai mic.** `Chunk.path` e un `String` pe fiecare bucată
   (aceeași cale repetată pentru toate bucățile unui fișier), `preview` un `String`, vectorii
   semantici `Vec<f32>`. De măsurat întâi (cei 8,7 MB „restul proiectului”), apoi: calea
   comună pe fișier (`Arc<str>`), vectorii cuantizați (i8: de 4× mai mici) doar dacă
   rezultatele rămân aceleași (test pe interogări).
4. **76d. Serverele de limbaj în spate.** Fiecare proiect pornește propriile servere (procese
   separate, sute de MB). Se parchează după 2 minute în spate — tot doar la focusul altui
   proiect. De făcut: aceeași regulă ca la 76a (aplicația în spate, fereastra minimizată).
   Un server comun mai multor proiecte (spațiu de lucru cu mai multe rădăcini) ar câștiga
   cel mai mult, dar riscul e mare (diagnosticele și editările amestecate): de cântărit
   separat, pe server.
   **Făcut odată cu 76a:** `ProjectLeft` parchează și serverele, după timpul de inactivitate
   din setări, dar cel puțin 10 minute (repornirea unui server costă secunde); fără timp de
   inactivitate în setări, niciodată, ca până acum.
5. **76e. O singură instanță.** Un proces în plus costă ~55 MB. Aplicația deschide deja
   totul într-unul; `EDITOR_NEW_INSTANCE` rămâne pentru teste. De verificat pe Windows că a
   doua pornire (din Explorer, din jump list, din terminal) ajunge mereu la instanța care
   rulează.
6. **76f. Fereastra nevăzută.** Minimizată sau acoperită de tot: de verificat (măsurat) că
   imaginea suprafeței pleacă (`set_visible(false)` dă înapoi cadrul păstrat; imaginea?).
   Câștig până la ~21 MB pe fereastră ascunsă.
7. **76g. Terminalul.** Istoria (5.000 de linii implicit, până la 100.000) ține celulele
   întregi; un terminal plin are zeci de MB. De măsurat la utilizator, apoi liniile vechi
   împachetate (`kernel::Packed`, deja făcut în planul 74) și decodate doar când sunt
   derulate.
   **Cântărit, lăsat:** grila e a lui `alacritty_terminal` (o celulă ~24 de octeți: 5.000 de
   linii × 150 de coloane ≈ 18 MB la un terminal plin); liniile vechi împachetate ar cere să
   înlocuim istoria lui (căutarea, selecția, derularea o citesc) — risc mare de a strica ce
   merge. De reluat doar dacă terminalele pline se văd mari la utilizator.
8. **76h. Firele.** Pe fereastră un fir `syntax` permanent și unul `session`; pe proiect
   `indexer`, `index-queue`, watcher-ul; câte un fir pentru fiecare așteptare `Later`
   (`index-rest`, `lsp-behind`). Puțină memorie, dar treziri: de mutat în pool-urile
   existente (ies după 30 s fără lucru) și un singur fir de cronometre pentru `Later`.
   **Cântărit, lăsat:** niciun fir nu se trezește periodic cât aplicația stă (watchdog-ul
   doarme până la primul eveniment, indexerul o oră, `module-release` o dată pe minut);
   `Later` are doi folositori (odihna indexului, parcarea serverelor), câte un fir care
   doarme — un fir comun n-ar câștiga aproape nimic.
9. **76i. Task Manager-ul numără GPU-ul de N ori.** Fiecare fereastră își adaugă atlasul și
   texturile comune (`gpu_memory_bytes`). De numărat partea comună o singură dată — înainte
   de orice măsurătoare cu mai multe ferestre în aplicație.
   **Făcut (`c7700734`):** `render_api::GpuMemory { shared, window }`; în vederea tuturor
   proiectelor, partea comună se numără o dată, iar rândul „Randare” adună suprafețele
   tuturor ferestrelor (înainte o arăta doar pe a primei).
10. **76j. Mărunțișuri (sub 1–3 MB fiecare):** cheile stub-urilor PHP copiate în `String`-uri
    (pot fi împrumutate din binar); sursele SVG ale iconițelor copiate; pozele ferestrelor
    din comutator copiate de fiecare listă (N² cât e deschisă); tampoanele de instanțe care
    nu scad după `release_idle`.
    **Tampoanele, cântărite și lăsate:** pornesc de la 64 KB și cresc rar; micșorate după 2 s
    de liniște, prima clipire a cursorului (un cadru întreg) le-ar crește la loc.
    **Compresia blocurilor (cerut: Snappy?):** măsurat pe cele 8.251 de blocuri ale unui
    Laravel — deflate 1: 3.373 KB, deflate 6: 3.016 KB, Snappy: 4.186 KB, LZ4: 4.323 KB;
    despachetat: deflate ~6,5 µs / bloc, Snappy 0,8, LZ4 0,5. Un bloc se despachetează o dată,
    apoi e ținut: memoria contează, nu cele câteva µs — **făcut: deflate 6** (`4c339d80`).
11. **76k. CPU: clipirea cursorului.** După ce cadrul păstrat pleacă, fiecare clipire
    desenează fereastra întreagă (puțin, dar de două ori pe secundă cât fereastra are focus).
    Cursorul într-un strat al lui (deja în TODO) ar desena doar cursorul; cere verificare pe
    ecran.
    **Cântărit, lăsat:** un cadru de clipire costă ~2 ms de CPU (0,26 s pe minut cu fereastra
    în față); a desena doar cursorul cere un strat al lui, de verificat pe ecran.

**Ordinea propusă:** 76i (ca măsurătorile din aplicație să fie corecte), 76a, 76f, 76b, 76c,
76d, apoi restul. 76a + 76b sunt câștigul mare la utilizator (mai multe proiecte cu aceleași
biblioteci); 76f și fixul de azi, la mai multe ferestre.
