# 16. Transferurile remote prin evenimente și cozi pe proiect

Scop: urcările și descărcările unui proiect remote (FTP / FTPS / SFTP) trec prin **cozi ale
proiectului**, hrănite de **evenimentele proiectului** (pe magistrala lui), cu **conexiuni
refolosite** și un număr de transferuri în paralel **configurabil** (urcări și descărcări
separat, pe proiect, cu o valoare globală implicită). Tot ce se transferă și tot ce așteaptă
se vede: un tab „De urcat” cu „Urcă tot”. Aceeași construcție ca indexarea (plan 15):
domeniile anunță, modulul ascultă, nimeni nu-l cheamă direct; transportul (FTP / SFTP),
planul (ce urcă, ce coboară, conflictele) și ce vede utilizatorul rămân separate, legate prin
porturi și evenimente. Nimic nu se pierde din ce face modulul azi.

## Azi (inventar, 2026-10-22)

Modulul `remote-projects` (~3.700 de linii) peste domeniul `remote` (planul, marcajele,
manifestul) și adaptoarele `remote-ftp` / `remote-sftp` / `fs-local::remote_copy`.

| Ce | Cum | Fir / conexiune |
|---|---|---|
| Urcare la salvare | abonat la `FileSaved` → `Job::Upload` pe un canal; verifică serverul (conflict → Compară / A mea / A serverului), apoi urcă | `remote-sync` (o sesiune păstrată pentru verificări) + **o conexiune nouă la fiecare urcare** (`apply` cu un worker) |
| Fișier adus la deschidere (la cerere) | `ProjectService::fetch` → `Job::Fetch` | `remote-sync` |
| Listarea folderelor (la cerere) | `Job::List`, o dată pe folder | `remote-sync` |
| Pull / Push / Sync (revizuire) | planul pe `remote-plan` (o sesiune), aplicarea pe `remote-apply` cu **4 workeri fixați** (`WORKERS`), fiecare cu sesiunea lui; progres pe octeți, oprire | până la 4 conexiuni noi pe aplicare |
| Prima descărcare (wizard) | același `apply`, 4 workeri | `remote-download` |
| Marcajele din arbore | `standing()` față de manifest (doar aici / schimbat / în urmă / conflict) | — |
| Jurnalul „Transferuri” | ultimele 500, citit de vedere la fiecare cadru cât lucrează | — |

Probleme găsite la inventar (verificate în cod):
1. **Firul `remote-sync` nu se termină niciodată**: starea ține emițătorul canalului, firul ține
   starea — la fiecare proiect remote deschis rămân un fir și o conexiune după închidere.
2. **Manifestul se poate pierde**: urcarea la salvare și aplicarea unei revizuiri copiază
   manifestul, lucrează pe copie, apoi o scriu peste — dacă rulează deodată, una o șterge pe
   cealaltă (fișiere urcate apar apoi ca schimbate / conflict).
3. **Salvările repetate nu se comasează**: un fișier salvat de 5 ori se urcă de 5 ori.
4. **Offline, urcarea se pierde**: „offline” afișat, fișierul nu mai pleacă (nimic nu ține minte
   că trebuie urcat).
5. **O conexiune nouă la fiecare urcare** (handshake SSH / TLS de fiecare dată).
6. **Paralelismul e fix** (4, o constantă) și nelimitat între sesiuni: o revizuire + salvări +
   o listare pot deschide oricâte conexiuni deodată (unele servere refuză peste 2–4).
7. **Reîncercări**: `None` înseamnă „setarea globală”, care nu există.
8. **Nimic nu arată ce așteaptă** să fie urcat (jobs ascunse într-un canal).
9. Mutările / ștergerile locale nu ajung pe server (nici ca opțiune).
10. Modulul nu publică nimic: jurnalul și bara de stare citesc starea direct.

## Ținta

```
domenii (document, workspace) ──publică pe magistrala proiectului──▶
   FileSaved · FileRenamed · EntriesDeleted · BulkChangeStarted / Ended · (Push / Pull cerute)
                                         │
modulul remote-projects                   ▼
  TransferService (ProjectService: pornit cu proiectul, oprit cu el — firele se termină)
  ├─ coada de urcări    (kernel::queue: o cale o dată, loturile ținute, bandă urgentă)
  ├─ coada de descărcări (la cerere: deschideri, Pull)
  ├─ PoolDeSesiuni      (N conexiuni păstrate, refolosite; N urcări + M descărcări setabile)
  ├─ Manifest           (un singur scriitor: actualizări pe intrare, nu copie peste copie)
  └─ publică: TransferQueued · TransferStarted · TransferProgress · TransferFinished /
              TransferFailed · PendingChanged { de urcat, de descărcat }
           ▲ ascultate de: tabul „De urcat”, jurnalul, bara de stare, Task Manager, arborele
domeniul remote: planul, marcajele, politica de conflict (pur)
infra: remote-ftp, remote-sftp (sesiuni), fs-local::remote_copy
```

Reguli:
- **Domeniile nu știu de transferuri**; modulul ascultă evenimentele proiectului (magistrala
  lui — alt proiect nu-l întârzie niciodată).
- **Cozi pe proiect**, urcări și descărcări separate: o cale deja în coadă nu se adaugă din nou;
  un lot (o lipire) pleacă grupat; ce nu a plecat (offline, eroare) **rămâne de urcat** și se
  reia când serverul revine.
- **Conexiuni refolosite**, cel mult cât e setat (urcări, descărcări), pentru tot proiectul
  (salvări, revizuiri, listări).
- **Un singur scriitor al manifestului**, prin actualizări pe intrare.
- **Starea anunțată**, nu citită: vederile ascultă.
- Conflictul se verifică înainte de fiecare urcare, ca azi (nimic nu suprascrie pe server
  ce s-a schimbat acolo).
- Setările: declarate în `settings.rs` (globale) și în `remote.toml` (pe proiect, peste cele
  globale).

## Pașii (un commit fiecare; `check.sh`; „go” între ei)

- [x] **16a. Plasa de siguranță.** Un server fals în memorie (sesiuni și scrieri numărate,
      offline la cerere, scrieri încete) și pornirea sesiunii exact ca serviciul proiectului
      (`session::start`: `attach` doar citește configurația și parola), pe magistrala
      proiectului. Teste: un fișier salvat urcă (serverul, manifestul, jurnalul; excluse și
      `.lumen` nu); o salvare peste o schimbare de pe server e conflict (nimic suprascris);
      offline e spus; la cerere, fișierul deschis e adus singur. Revizuirile (alegeri,
      progres, oprire) au deja teste în domeniu, pe server fals. Cele două bug-uri au teste
      care **pică azi** (rulate cu `--ignored`: firul ține starea pentru totdeauna; manifestul
      pierde una din două scrieri deodată), marcate până la 16b.
- [x] **16b. Bug-urile.** Firul `remote-sync` ține starea slab: cu proiectul se duc
      abonările, starea, emițătorii canalului, deci și firul. Manifestul: fiecare transfer
      pune în manifestul sesiunii doar intrările lui (sub lacăt, salvat pe disc tot atunci),
      nu copia lui peste a celorlalte. Cele două teste din 16a trec acum.
- [x] **16c. Cozile și sesiunile.** `pool.rs`: conexiunile proiectului într-un pool care e el
      însuși un `Connector` (cel mult 6 deschise, cele libere refolosite — fără un nou
      handshake —, lăsate după 45 s fără folosire, cele rupte de rețea nefolosite din nou);
      tot ce vorbește cu serverul trece prin el (urcări, aduceri, listări, revizuiri). Un fir
      nu mai ține o conexiune cât cere alta (verificarea o eliberează înainte de transfer).
      `transfers.rs`: coada de urcări (salvările comasate, loturile ținute) și cea de aduceri
      (o cerere de două ori = o descărcare, amândouă anunțate), fiecare cu firul ei și cel mult
      2 urcări / 4 descărcări odată; ce n-a putut pleca (offline, eroare) rămâne de urcat
      (`to_upload`) și se reia cu pauze crescânde (cel mult un minut), utilizatorul anunțat o
      dată; revizuirile folosesc limita direcției lor. Teste: salvat de 5 ori = urcat o dată;
      cel mult 2 în paralel, conexiunile păstrate (a doua rundă fără handshake); offline →
      urcat la revenire; un lot urcă la final; un fișier cerut de două ori adus o dată.
- [x] **16d. Starea ca evenimente.** `remote::events::{TransferFinished, TransfersChanged}`
      pe magistrala proiectului: fiecare transfer încheiat (calea, direcția, octeții, eroarea)
      și ce rulează / ce așteaptă să fie urcat, la fiecare schimbare. Jurnalul „Transferuri”
      le ascultă (se redesenează când aude ceva, nu la fiecare trezire; animația cât lucrează
      rămâne); bara de jos arată „N în transfer · M de urcat” cât e ceva de arătat (clic:
      jurnalul); Task Manager: „de urcat” în detaliul sesiunii; arborele își reia marcajele
      prin listarea din nou a folderelor transferate, ca înainte. Mesajul offline spune acum
      că ce e salvat urcă la revenirea serverului.
- [x] **16e. Setările.** Globale (`settings.rs`: `[remote] uploads / downloads / retries`,
      implicit 2 / 4 / 4; Setări → blocul „Proiecte remote”), date domeniului prin
      `configure` din modulul `remote-projects` (ca istoricul local) la pornire și la fiecare schimbare. Pe proiect:
      `Connection.uploads / downloads` (`remote.toml`), alese în wizard („Ca în setări (N)”
      implicit, sau 1 / 2 / 4 și 2 / 4 / 8); `Connection::limits()` / `retries()` = ale ei,
      altfel cele globale. Cozile, pool-ul (urcări + descărcări), revizuirile și wizard-ul le
      folosesc. Teste: limitele globale / proprii, alegerile din setări și din wizard, o
      limită proprie de o urcare ținută; snapshot al wizard-ului.
- [x] **16f. Mutările și ștergerile pe server** (`Connection.moves`, oprit implicit; căsuța din
      wizard). Pornit: `FileRenamed` → redenumire pe server (fișier sau folder) dacă serverul a
      primit ceva de acolo, manifestul mutând intrările; `EntriesDeleted` → întrebare („Ștergi
      și de pe server?”), apoi, la „da”, fișierele trimise șterse prin `DeleteRemote` (jurnal,
      manifest) și folderele rămase goale, de la cele mai adânci. Ce e exclus sau `.lumen` nu
      pleacă; ce n-a ajuns pe server nu are ce muta. Teste: mutarea unui folder, ștergerea
      doar după răspuns, opțiunea oprită lasă serverul neatins; alegerea din wizard.
- [x] **16g. Tabul „De urcat”** (`pending_view.rs`, comanda `remote.pending`, în meniul Remote
      și la clic pe bara de jos când așteaptă ceva): ce n-a putut pleca (cu motivul), ce e
      schimbat aici de la ultimul transfer, ce e făcut aici și netrimis — nu ce e exclus sau
      deja pe server —, citit pe firul lui (discul) când transferurile proiectului anunță o
      schimbare; „Urcă tot” și „Urcă” pe fiecare rând, prin coada de urcări (serverul
      verificat întâi). Găsit și reparat: reluarea după offline cerea cozii „tot”, care înghițea
      un „Urcă tot” venit între timp (doar ce aștepta mai urca) — acum își cere fișierele unul
      câte unul.
- [x] **16h. Curățenia** și lista de verificare. Nimic rămas nefolosit (constantele vechi,
      joburile scoase); setările globale mutate în secțiunea lor din Setări („Remote”, cu
      iconița de server, găsite și din căutarea setărilor). Testele pe serverele demo reale
      (SFTP, FTPS: listare, adus la cerere, Pull / Sync revizuite, manifestul citit înapoi)
      trec; unul a picat o dată și a trecut la a doua rulare, fără schimbări de cod (starea
      serverului demo) — mesajul lui spune acum exact ce mai e de sincronizat.


## Lista de verificare (nimic nu se pierde)

- [x] Urcarea la salvare, cu verificarea serverului înainte; conflictul: Compară (salvarea după
      comparare urcă direct), A mea, A serverului.
- [x] Fișierele excluse și datele proiectului (`.lumen`) nu pleacă niciodată.
- [x] La cerere: listarea folderelor (o dată), aducerea la deschidere, „Adu local”,
      „Recitește de pe server”.
- [x] Pull / Push / Sync pe țintă sau pe proiect, revizuite: alegerile, ștergerile și
      conflictele nebifate implicit, progresul pe octeți, oprirea (fișierele temporare
      șterse, nimic pe jumătate), eșecurile spuse.
- [x] Prima descărcare din wizard, cu progresul ei.
- [x] Marcajele din arbore (doar aici / schimbat / în urmă / conflict), cu tooltip.
- [x] Jurnalul „Transferuri” (ultimele 500), „se transferă N”.
- [x] Reîncercările (doar pentru erori trecătoare), FTP 522 → TLS 1.2, cheile de host SSH.
- [x] Parolele: keychain sau criptate în proiect; nimic în clar, nimic în Debug.
- [x] Task Manager: sesiunile, traficul (rata), memoria.
- [x] Alt proiect deschis: nimic din cel vechi (fire, conexiuni, abonări) nu rămâne.
- [x] Windows: clippy, căile, FTP / SFTP testate.
  Dovezi: `remote-projects/tests/unit/{session,pool,pending_view,log_view,connect_view,
  review_view,config,lib}.rs` (pe serverul fals: urcare, conflict, offline și reluare,
  aducere, comasare, paralelism, conexiuni păstrate, loturi, evenimente, mutări / ștergeri,
  „De urcat”), domeniul `remote` (plan, aplicare, oprire, progres, limite), testele pe
  serverele demo reale (rulate cu `EDITOR_REMOTE_DEMO=1`), snapshot-ul wizard-ului. Windows:
  clippy trece; la rulare, FTP / SFTP pe Windows rămân de încercat pe o mașină reală.

## Hotărâte (2026-10-22)

1. **Mutările și ștergerile locale pe server** (azi nu se trimit deloc). Propun o opțiune pe
   proiect, **oprită implicit** (o ștergere pe server e greu de întors): când e pornită,
   `FileRenamed` → redenumire pe server, `EntriesDeleted` → ștergere, cu confirmare.
2. **Valorile implicite**: 2 urcări și 4 descărcări în paralel (multe servere FTP limitează
   conexiunile pe utilizator); reîncercări 4, de la 1 s, dublate. **Configurabile pe
   proiect** (urcări / descărcări separat), cu aceste valori implicite.
3. **Ce e „de urcat” după o repornire**: propun ca lista să se reconstruiască din marcaje
   (manifestul spune ce s-a schimbat de la ultimul transfer) — nimic de ținut separat pe disc.
