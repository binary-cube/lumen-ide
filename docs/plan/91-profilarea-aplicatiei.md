# 91 — Profilarea aplicației: înregistrări cu acord, anonime, de dat mai departe

**Prioritate mare** (cerut 2026-11-20). Motivul imediat: pe Ubuntu, aplicația ține procesorul
la ~60% stând degeaba, la un coleg, și nu avem cum să vedem de la ce vine. Jurnalul spune ce
s-a întâmplat, dar nu ce consumă. Vrem un modul care, pornit de utilizator, înregistrează o
perioadă cât costă fiecare parte a aplicației și dă un fișier mic, de trimis.

## Ce vrem

- **O înregistrare pornită de utilizator, cu acordul lui, cerut de fiecare dată.**
  - Un modal (regulile de la `.claude/rules/modals.md`) spune ce se adună și ce nu:
    - se adună doar ce ține de aplicație și de sistemul de operare;
    - nimic despre proiecte, fișiere, cod, utilizator sau rețea;
    - totul rămâne pe calculator: nu se trimite nimic, nicăieri.
  - În modal, un dropdown (componenta noastră: câmpul și lista comună) cu durata după care
    se oprește singură: 1 min, 5 min (implicit), 15 min, 30 min, 1 h. Așa nu rămâne pornită
    din uitare.
  - Butoanele în bara de jos, cu tastele: „Anulează” și „Pornește înregistrarea” (principal,
    `↵`).
- **Cât înregistrează, se vede mereu.**
  - Un buton pe partea dreaptă a ferestrei, deasupra Task Manager-ului (un
    `ToolWindowProvider` cu ordinea înaintea lui), cu un punct roșu care pulsează încet (o
    animație care cere cadre doar cât pulsează, la ritmul ei, nu la fiecare cadru).
  - Clic pe el: oprește înregistrarea. Tooltip-ul spune cât a trecut și cât mai e.
  - Un rând în meniul aplicației, mereu, cât încă dezvoltăm: Help → „Record a Profile…” /
    „Stop Recording”.
- **Un tab cu înregistrările** (un vizualizator, ca al jurnalului), deschis la sfârșitul
  unei înregistrări și din meniu:
  - lista lor, cea mai nouă prima: când, cât a durat, sistemul, versiunea, mărimea;
  - fiecare se poate deschide, șterge, copia (fișierul, sau rezumatul ca text), arăta în
    dosar; lista se poate filtra;
  - vizualizarea unei înregistrări, ca uneltele de profilare ale unui browser:
    - o axă de timp cu CPU-ul pe fire, memoria, GPU-ul, cadrele desenate pe secundă și
      trezirile;
    - sub ea, un flame graph (componenta noastră `icicle`) cu ce a consumat timpul pe
      intervalul ales;
    - un rezumat: firele cele mai ocupate, cine cere cadre când nimic nu se mișcă, părțile
      cu cele mai multe desenări, vârfurile de memorie.
- **Formatul e JSON** (cerut): un singur fișier `.json` pe înregistrare, mic (ținta: sub
  2 MB pentru 5 minute; o limită tare, ce trece de ea e strâns pe intervale mai mari), de
  citit și cu alte unelte.

## Ce NU se adună (anonimizare, foarte important)

- Nicio cale, niciun nume de fișier, de proiect, de dosar, de ramură, de bază de date, de
  conexiune, de server; niciun conținut (cod, text, rezultate, cereri HTTP).
- Niciun nume de utilizator, de calculator, adresă, id de hardware, nimic din mediu
  (variabilele de mediu nu se citesc).
- Niciun mesaj din jurnal: jurnalul poate cita căi. Ce vine din aplicație e doar o listă
  închisă de nume ale noastre, scrise în cod: firele (`indexer`, `http-run`), modulele după
  id, părțile interfeței (tab, panou, overlay), comenzile după id.
- Numele programelor pornite de aplicație (shell, agent, server de limbaj): doar felul lor
  (`terminal`, `agent`, `server`), nu programul și nici argumentele.
- Despre sistem, doar ce explică performanța: sistemul și versiunea lui, arhitectura,
  numărul de nuclee, memoria totală, placa grafică și driverul (numele publicate), scala
  ecranului și numărul de ecrane, sesiunea grafică (Wayland / X11 / macOS / Windows: chiar
  ce ne trebuie pentru Ubuntu).
- Un test verifică fiecare câmp al formatului contra unei liste permise; un câmp nou care
  nu e pe listă pică testul. Un alt test face o înregistrare reală într-un proiect cu nume
  și căi „secrete” și caută în fișier orice bucată din ele.

## Ce se adună și cum (de măsurat întâi)

- **CPU pe fir**, la fiecare 100 ms: timpul consumat de fiecare fir al aplicației, numit
  după numele lui (al nostru), din sistem (`process-stats`: thread times pe macOS,
  `/proc/self/task/*/stat` pe Linux, `GetThreadTimes` pe Windows). Așa vedem imediat firul
  care mănâncă 60% pe Ubuntu.
- **Memoria** (cea proprie a procesului, cum o numără sistemul), **GPU-ul** (memoria
  texturilor, din renderer), **cadrele**: câte, cât a durat fiecare (pregătire, desenare,
  prezentare), cine a cerut cadrul următor (`frame_deadline`, `is_animating`, un waker,
  un eveniment al sistemului).
- **Trezirile buclei**: de câte ori se trezește aplicația și de ce. E cea mai probabilă
  cauză a unui CPU mare în repaus: o animație care nu se oprește, un poll care cere cadre,
  un watcher de fișiere care tot anunță.
- **Timpul pe părți** (pentru flame graph): span-uri `tracing` pe drumurile importante
  (cadru → overlay / tab / panou → view-ul unui modul; poll-urile; lucrul de pe fire),
  adunate în arbore pe intervale (nume, câte, timpul total), nu fiecare apel. Cât nu se
  înregistrează, span-urile costă cât un `debug!` sub nivel (~0,4 ns, măsurat în planul 89).
- **Sondele** sunt contoare globale ieftine (atomice) în `kernel` (de ex. `kernel::probe`),
  la care aplicația și modulele raportează. Modulul de profilare doar le citește, deci nu
  știe de niciun alt modul (`modules.md`). Un modul care nu e înregistrat nu adaugă nimic.
- De măsurat înainte de a alege: costul span-urilor pe cadru cu înregistrarea pornită, și
  mărimea fișierului pe 5 minute. Numerele merg în plan.

## Cum se împarte (regulile proiectului)

- `crates/domain/tools/profiling`: formatul JSON (versiune, antet cu sistemul, eșantioane,
  arborele pe intervale), lista câmpurilor permise, strângerea pe intervale, rezumatul.
  Pur, fără I/O, testat.
- `crates/infra/system/profiling-collect`: colectorul pe un fir al lui (`profiler`), cu
  eșantionarea sistemului; scrie fișierul la sfârșit (fișier temporar, apoi redenumit).
- `crates/modules/tools/profiler`: modulul. Modalul de acord (cu dropdown-ul), butonul din
  dreapta cu indicatorul, comanda din meniu, tab-ul cu lista și vizualizarea, setările lui
  (dosarul înregistrărilor, durata implicită). Contribuie prin `ctx.contribute`; dosarul
  vine prin gazdă.
- Înregistrările stau în dosarul de date al aplicației (`<data>/profiles/`), nu în proiect.
  Se șterg din tab; cele mai vechi de 30 de zile pleacă singure.
- Nimic pe firul interfeței: citirea, strângerea și scrierea sunt pe fire; vizualizarea
  citește fișierul pe un worker.

## Pași

1. **91a — formatul și anonimizarea:** crate-ul de domeniu, lista permisă, testele de
   anonimizare.
2. **91b — colectorul:** CPU pe fir, memorie, cadre, treziri; măsurat costul.
3. **91c — modulul:** modalul de acord, butonul cu indicatorul, oprirea automată, meniul.
4. **91d — tab-ul:** lista, ștergerea, copierea, filtrul.
5. **91e — vizualizarea:** axa de timp, flame graph-ul, rezumatul.
6. **91f — span-urile pe drumurile importante**, după măsurătoare.

Primul folos (pentru Ubuntu): 91a–91c dau deja fișierul cu CPU-ul pe fire și trezirile, de
cerut colegului.

## README

O secțiune „Privacy”: aplicația nu trimite telemetrie; jurnalul și înregistrările stau pe
calculator; profilarea pornește doar cu acordul utilizatorului, adună doar ce ține de
aplicație și de sistem, anonim, și se oprește singură.

## Făcut (2026-11-20): 91a, 91b, 91c, o parte din 91e

- **Sondele** (`kernel::probe`): cine cere un cadru și de ce (`poll`, `animation`,
  `deadline`, `input`, `wake`, `window`), cât ia fiecare parte pe firul interfeței (`paint`,
  `poll`), cadrele (câte, cât, cel mai lung). Numele sunt `&'static str`, deci doar din codul
  nostru. Măsurat cât nu se înregistrează: ~3,5 ns o sondă (o citire atomică).
- **Puse în aplicație:** poll-ul fiecărui view (după furnizorul lui: `log-viewer`,
  `terminal-panel`…) și al părților aplicației; desenarea view-urilor din tab-uri, din zona
  de jos și a panourilor laterale; termenele de cadru, fiecare cu numele lui (`caret`,
  `hover`, `autosave`, `toast`…), socotite doar când cer un cadru în următoarele 50 ms;
  animațiile și termenele fiecărui view (`probe_frame_askers`, citite doar cât se
  înregistrează); evenimentele ferestrei (intrare, trezire, fereastra); cadrele desenate.
- **Formatul** (`crates/domain/tools/profiling`): JSON, versiunea 1, cu sistemul, a cui e
  fiecare fir (`owners`) și eșantioanele. Peste 3000 de eșantioane, cele mai vechi se unesc
  câte două, fără să se piardă nimic din totaluri. Numele sunt curățate (ASCII, cel mult 48 de
  caractere). Rezumatul (`summary`) dă CPU-ul pe fire și pe module, cine cere cadre pe
  secundă, ce ocupă firul interfeței, cadrele și vârfurile.
- **Colectorul** (`crates/infra/system/profiling-collect`): firul `profiler`, un eșantion la
  100 ms, oprit la cerere sau la timpul ales, scris atomic. O singură înregistrare odată.
- **Modulul** (`crates/modules/tools/profiler`):
  - fereastra de acord cu ce se păstrează și ce nu, dropdown-ul nostru pentru durată (1, 5,
    15, 30 de minute, 1 oră), butoanele și tastele în bara de jos;
  - butonul din dreapta, deasupra Task Manager-ului, aprins cât înregistrează; semnul din
    bara de stare cu timpul rămas; Help → „Record a Profile…”; un clic pe oricare oprește;
  - la sfârșit: o notificare și fișierul deschis în tab-ul lui, cu cifrele, CPU-ul și cadrele
    în timp, și barele: modulele după CPU, cine cere cadre, ce ocupă interfața, firele;
  - a cui e fiecare fir vine din resursele aplicației: proprietarul și firele fiecărei părți,
    niciodată numele părții.
- **Teste:** lista câmpurilor permise; un proiect cu nume și căi „secrete” din care nimic nu
  ajunge în înregistrare; unirea eșantioanelor; rezumatul care arată modulul vinovat;
  colectorul real; fereastra de acord; vizualizarea.
- **Regula** `.claude/rules/privacy.md`, legată din cuprins.
- Mărimea: o înregistrare de probă de 1 minut, cu 4 fire active, are 340 KB.

## Lăsat

- **Tab-ul cu toate înregistrările** (91d): lista, ștergerea, copierea, filtrul. Acum se
  deschide doar cea tocmai scrisă; celelalte stau în `<date>/profiles/`.
- **Flame graph-ul** (91f): cere span-uri imbricate pe drumurile importante, de măsurat întâi.
- **Placa grafică, driverul și scala ecranului** în antet: le știe doar aplicația, deci e
  nevoie de o cerere către gazdă. Câmpurile există, acum sunt goale.
- **Trezirile pe modul:** un waker nu-și spune proprietarul; poll-ul de după arată deja cine
  a avut ceva nou.
- **Punctul roșu care pulsează** pe buton: acum butonul e doar aprins, cu semnul din bara de
  stare.
- **Mărimea pe 5 minute cu fire reale** (ținta: sub 2 MB): de măsurat pe o rulare adevărată.

## Făcut după (2026-11-20, a doua zi a pasului)

- **Rețeaua și discul:** fiecare parte își spune octeții trimiși, primiți, citiți și scriși
  (`ResourceUse`); înregistrarea îi ia o dată pe secundă, pe proprietar, plus discul
  întregului proces (`process_stats::disk_io`). Clientul HTTP își numără acum traficul
  (`NetIo`, pereche cu `DiskIo`), cererile și timpul lor (sondele), fără nimic din cerere.
- **Sonde generice:** `count` și `level`, pe lângă `time` și `trigger`. Un modul care vrea
  să fie măsurat adaugă o linie, iar înregistrarea și vizualizarea le preiau singure.
- **Contractul `ProfileSource`** (`contracts::profiling`): datele bogate ale unui modul,
  anunțat la început și la sfârșit, citit o dată pe secundă din memorie, cu citiri tipizate
  și nume statice. Le înregistrează aplicația, iar profilerul nu numește niciun modul.
  Primul: clientul HTTP (rulările ținute, cererile în curs).
- **Concluziile** (`Finding`): aplicația ocupată fără să faci nimic, firele cele mai ocupate,
  cine cere cadre, părțile lente, cadrele lungi, rețeaua și discul mari; fiecare cu gravitatea
  ei.
- **Vizualizarea refăcută** din componentele noastre: antetul cu „Copiază rezumatul”
  (Markdown) și „Arată în dosar”, concluziile cu iconița gravității, cardurile de metrici cu
  grafic, axa de timp (CPU, cadre, rețea, disc) cu valorile sub mouse, tabelele cu aspectul
  comun (subtitluri, rânduri alternate, bare în celule).
- **Textul de citit se copiază** peste tot unde e folosit (`ReadText::menu`): notele unei
  actualizări au acum Copiază / Copiază tot la clic dreapta.

## Făcut, finalizarea (2026-11-20)

- **Concluziile spun unde și ce e de făcut:** sub fiecare, un sfat după felul ei și după
  numele părții din cod (un poll care spune mereu „schimbat”: compară o generație; o
  animație care nu se oprește: `is_animating`; un termen pus mereu: `frame_deadline`; un fir
  care nu așteaptă; o desenare lentă: păstrează ce construiește sau mută pe un worker). Sunt
  și în rezumatul copiat.
- **Flame graph-ul:** `kernel::probe::time` ține acum drumul imbricat pe fiecare fir (cel mult
  8 pași, fără alocări pe drum). Aplicația deschide `app·frame` și `app·poll`, iar desenările
  și poll-urile view-urilor se așază sub ele. Desenat cu `icicle`: clic pentru a intra,
  tastele lui, detalii sub mouse.
- **Tab-ul cu înregistrările** (Help → „Profile Recordings…” și meniul butonului): toate,
  cele mai noi întâi, cu filtru; clic deschide; meniul are Deschide, Copiază rezumatul, Arată
  în dosar, Șterge (cu întrebare, la coș). Lista se citește din nou după o înregistrare sau o
  ștergere.
- **Antetul complet:** placa grafică, driverul și scala ecranului vin din resursele aplicației
  (`ResourceSnapshot.renderer`, `scale_percent`).
- **Insigna de pe buton:** minutele rămase, în roșu, cât înregistrează; colectorul trezește
  fereastra o dată pe secundă (insigna și resursele se țin la zi).
- **Clicurile ferestrei de acord:** o fereastră de modal trimite `press`, un tab
  `pointer_down`, ambele ajung la `click`. View-urile profilerului implementează `click`, iar
  testul apasă exact ca modalul.
- **Profilerul nu se numără pe sine:** firele lui (`profiler`, `profiler-read`) nu apar, iar
  memoria ținută de înregistrare e scăzută din memoria raportată. La oprire, tabelul sondelor
  e eliberat, înregistrarea pleacă odată cu firul ei, sursele modulelor își eliberează ce
  țineau, iar vizualizarea ține datele doar cât e deschisă.

## Lăsat

- Un fir din afara aplicației noastre (al sistemului, al unei biblioteci) apare cu numele lui
  curățat (`tokio-*`), fără modul.

## Făcut, după code review (2026-11-20)

Trei recenzii separate (nucleul, modulul, integrările); ce s-a confirmat a fost reparat:

- **Sondele:** timpul interfeței e doar al firului ei (`mark_interface_thread`) și doar partea
  proprie a fiecărei lucrări (fără cele imbricate), deci nimic nu e numărat de două ori. Un
  `Timer` nu poate fi trimis pe alt fir. Drumurile mai adânci de 8 pași rămân în părintele lor.
- **Cine cere cadre:** un termen se numără o singură dată, când vine cadrul pentru el (nu
  „curând” la fiecare cadru). Termenul comun al view-urilor nu se mai numără peste ale
  fiecăruia. Trezirile nu se mai numără (le spune poll-ul care urmează). Evenimentele
  ferestrei sunt clasificate explicit (intrare / fereastră).
- **Colectorul:** o eroare în timpul înregistrării oprește sondele și eliberează locul
  (`catch_unwind`). Numele firelor sunt anonimizate (`prefix-*`). Vârfurile memoriei și GPU-ului
  se păstrează la comasare. Sumele sunt saturate. Un fișier existent primește sufixul `-n`.
- **Modulul:**
  - fiecare fereastră își are trezirea, eliberată la închidere (nu „a ultimei ferestre”);
  - o a doua pornire e refuzată înainte să fie anunțate sursele;
  - resursele se citesc fără lacăt ținut;
  - vizualizarea ia doar fișierele din dosarul înregistrărilor;
  - meniul listei ține fișierul, nu poziția rândului;
  - ora e afișată în fusul utilizatorului;
  - mărimile se formatează cu `i18n::bytes`.
- **Vizualizarea:**
  - pagina (texte, rânduri, serii, nodurile flame graph-ului, rezumatul de copiat) se face o
    dată, pe firul de citire (`view/prepared.rs`), iar înregistrarea e eliberată după;
  - un cadru doar desenează, graficele se refac doar când se schimbă lățimea;
  - axa timpului nu mai cade la lățimi mici;
  - flame graph-ul e tăiat la marginea paginii (`IciclePaint::clip`);
  - tabelele spun „încă N rânduri”;
  - tastele (↑ ↓ Page Up/Down Home End; ale flame graph-ului sub mouse) derulează;
  - cursorul săgeată, iar mișcarea care nu schimbă nimic nu redesenează.
- **Integrările:**
  - cronometrele desenării cuprind doar desenarea view-ului;
  - figurile clientului HTTP se numără fără copierea rulărilor;
  - detaliile jurnalului folosesc meniul comun al textului (Copiază, Copiază tot, cu puncte
    și rânduri goale), plus mesajul și linia;
  - cursorul din setări peste dropdown-uri e verificat doar în conținut;
  - documentația pusă greșit a fost mutată la locul ei.

## Lăsat, după code review

- **CPU-ul procesului** e suma firelor văzute la fiecare eșantion: un fir care se termină între
  două eșantioane își pierde ultima bucată. Diferența e mică la 100 ms; un total al procesului
  citit de la sistem ar cere încă un cititor pe fiecare platformă.
- **Comasarea eșantioanelor** caută după nume în liste mici (zeci de intrări). Rămâne liniară
  pe intrare, sub o milisecundă la 3000 de eșantioane, deci nu cere hărți.
- **Hasher-ul sondelor** e cel implicit: o sondă ia lacătul doar cât înregistrează, iar
  măsurat e sub 1 µs; unul mai rapid nu se vede.
- **Timpul interfeței și drumurile flame graph-ului** se țin separat, deși vin din aceleași
  cronometre: unul e „cine”, celălalt „unde”, iar un tabel derivat din drumuri ar pierde
  apelurile și cel mai lung apel.

## Făcut, după primul profil real (2026-11-21)

- **Fazele unui cadru cronometrate:**
  - fiecare fază a desenării e cronometrată sub `app·frame` (editoarele, panourile, bara
    de stare, overlay-urile…, numite de marcajul care le încheie: `Timer::set_what`);
  - la fel randarea pe GPU (`app·render`);
  - flame graph-ul spune acum care parte face un cadru lent; înainte, 95% din timpul unui
    cadru nu era atribuit nimănui.
- **Firele fără nume:** eșantionate cu `sample` pe aplicația pornită izolat, sunt
  worker-ele sistemului (pe macOS cozile GCD):
  - Metal predă cadrele driver-ului (`com.Metal.CommandQueueDispatch`) și le primește
    înapoi (`com.Metal.CompletionQueueDispatch`);
  - ritmul cadrelor (`FramePacing`).

  Aplicația își numește toate firele ei (niciun `thread::spawn` fără nume, runtime-urile
  tokio pe un singur fir). În înregistrări apar acum ca `system-workers`, iar profilul le
  arată „ale sistemului (driver GPU, cadre)”; timpul lor crește cu cadrele desenate. Un
  `unnamed` dintr-o înregistrare veche se citește la fel.

