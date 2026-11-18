# 38 — Notificări (toast)

## De ce

Confirmările de tipul „Data sources saved.” apar acum ca bandă în interiorul ferestrei
„Database”, împing rândurile în jos și rămân acolo. Vrem un singur mecanism, comun
aplicației și modulelor: notificări mici, plutitoare, în colț, care dispar singure.

## Ce face

- **Tipuri:** succes, informare, avertisment, eroare și „în lucru” (cu spinner, care trece în
  succes sau eroare când se termină operația).
- **Conținut:** iconița tipului într-un pătrat colorat, un titlu, un rând de detalii
  opțional, cel mult două acțiuni (de exemplu „Anulează”) și ×.
- **Timpul:** un inel în jurul lui × arată cât mai rămâne. Implicit 4 s la succes și
  informare, 8 s la avertisment. Erorile și cele cu acțiuni rămân până la ×.
  - Cât e mouse-ul deasupra stivei, timpul stă pe loc.
  - Ascunsă sau minimizată, fereastra nu consumă timp.
- **Poziția** se alege din setări (`Setting<ToastPosition>` în `crates/app/src/settings.rs`,
  secțiunea Aspect → Notificări). Controlul e segmentat: Jos dreapta, Jos centru,
  Sus centru (implicit), Sus dreapta.
  - Schimbarea se aplică pe loc, prin `SettingChanged`.
  - Jos: deasupra barei de jos, la `ds::POPOVER_GAP`, iar cea mai nouă e jos.
  - Sus: sub bara de taburi, iar cea mai nouă e sus.
  - Animația de intrare vine dinspre marginea aleasă.
- **Stiva:** sunt cel mult 4 vizibile; restul așteaptă.
  - Mesajele identice se adună într-unul singur, cu „×2”, în loc să umple stiva.
  - Fiecare intră cu o animație scurtă (alunecare și opacitate) și iese la fel.
- **Aspect „glass”:**
  - suprafață ușor translucidă (`popup_background` cu alfa aproximativ 0,92);
  - o strălucire discretă în culoarea tipului, în colțul stânga-sus, desenată ca gradient
    radial aproximat din câteva straturi, ca umbrele din `ds::surface`;
  - margine fină, umbră moale `Elevation`, colțuri `radius::LG`.

  Culorile vin din paletă, cu variante pentru dark și light și contrast verificat.

  Nu facem blur real al fundalului. Renderer-ul are doar quad-uri, iar blur-ul ar cere o
  trecere offscreen în wgpu, o variantă software și GPU consumat continuu. Se poate adăuga
  ulterior, ca opțiune.
- **Tastatură:** notificările nu iau focusul și nu blochează nimic. Nu sunt modale.
  - Comanda „Închide notificările” (Esc când stiva are focus, plus paletă).
  - Comanda „Arată ultimele notificări”: un istoric al ultimelor 50 pentru proiectul
    curent, pentru cine a ratat una.

## Arhitectură

- **Domeniu** (`workbench-model`):
  - tipul pur `Notification { id: Uuid v4, kind, title, detail, actions, timeout, key }`;
  - `NotificationKind`;
  - portul `Notifier`: `show`, `update` (pentru „în lucru”), `dismiss`.

  Regulile pure (adunarea duplicatelor, ordinea, limita vizibilă, timpii) sunt testate fără UI.
- **Pe proiect:** fiecare proiect are notificările lui, în `ProjectData`, și pleacă odată cu
  el.
  - Modulele le primesc prin `host.notify(...)` sau prin `ProjectContext`.
  - Acțiunile sunt comenzi, deci tastele merg ca peste tot.
  - Un modul nu desenează niciodată toast-ul el însuși.
- **ui-components:** `toast` desenează o notificare și stiva (componentă pură, cu
  layout-ul memorat per lățime și conținut, textele modelate o singură dată).
- **App:** stiva e un `OverlaySlot` de tip popover, fără backdrop.
  - Primește click și hover doar pe propriile carduri.
  - Cât e deschis un modal, rămâne deasupra lui, ca să se vadă confirmarea modalului.
- **Performanță:**
  - nimic pe cadru cât nu e nicio notificare;
  - cât rulează timpul, se cere redesenare doar pentru zona stivei (inelul), la 30 fps,
    nu la refresh-ul ecranului;
  - nicio alocare pe cadru.

## Migrare

- **Fereastra „Database”:** banda `notice` dispare.
  - „Data sources saved.” devine un toast de succes.
  - Erorile de salvare devin toast-uri de eroare.
  - „The password of {name} is now kept in the system keychain.” devine un toast de
    informare.
- Restul confirmărilor din aplicație și din module se caută (grep după benzi sau mesaje
  temporare) și se mută acolo unde e o confirmare trecătoare. Mesajele care țin de un
  formular rămân în formular.

## Teste

- **Domeniu:** duplicatele adunate, limita de 4 cu coadă, timpii pe tip, pauza la hover,
  „în lucru” trecut în succes, istoricul limitat.
- **ui-components:** layout-ul la lățimi diferite (texte lungi tăiate cu „…”, două
  acțiuni), inelul și toast-urile fără timp.
- **App:**
  - stiva peste un modal;
  - click pe × și pe o acțiune;
  - proiectele nu își văd notificările unul altuia.
- **Explorer:** salvarea surselor produce un toast, nu o bandă.
- **Snapshot-uri noi:** `toasts` (toate tipurile, în ambele teme) și `toast-over-modal`.
  Snapshot-urile ferestrei „Database” care arătau banda se schimbă.

## Stare

Implementat.

- **Domeniu** (`workbench-model::notifications`): `Notification` (id UUID v4, tip, titlu,
  detaliu, cel mult două acțiuni ca comenzi, `Timeout`, cheie), `NotificationKind`,
  portul `Notifier` (`show`, `update`, `dismiss`; `NoNotifier` pentru teste), `Notifications`
  cu regulile pure (adunarea duplicatelor cu „×N”, cel mult 4 vizibile și coada, timpii pe
  tip, pauza la hover, intrarea și ieșirea, istoricul ultimelor 50) și `ToastPosition`
  (citit și scris ca setare). Generația stivei e unică între proiecte, ca layout-ul memorat
  să nu fie luat drept al altui proiect.
- **Port:** `WorkbenchHost::notify` primește acum o `Notification` (în loc de textul din
  bara de jos), plus `update_notification` și `dismiss_notification`; `ProjectContext`
  are `notifier`, folosit de pe orice fir. Vederile (`Viewer::take_notifications`) spun ce
  au de spus, iar aplicația le arată.
- **ui-components** (`toast`): cardul „glass” (suprafață cu alfa 0,92, strălucire în culoarea
  tipului din colțul stânga-sus, din straturi care nu ies din colțurile rotunjite, margine,
  umbră `Medium`, `radius::LG`), iconița în pătrat colorat (spinner pentru „în lucru”),
  titlul pe cel mult două rânduri, detaliul pe cel mult două (tăiate cu „…”), „×2”, două
  acțiuni, × cu inelul timpului rămas. Layout-ul e memorat per generație și lățime, textele
  modelate o dată; nicio alocare pe cadru, nimic desenat cât stiva e goală. Iconițe noi:
  `info`, `circle-alert`, `bell`.
- **App:** stiva e `Overlay::Toasts`, deasupra tuturor (și a modalelor), dar „plutitoare”:
  nu e niciodată overlay-ul activ, nu ia tastele, primește click, hover și scroll doar pe
  propriile carduri. Esc cu mouse-ul peste stivă le închide; comenzile „Închide
  notificările” și „Arată ultimele notificări” (lista istoricului proiectului, un rând ales
  arată notificarea din nou). Notificările stau în `ProjectData` și pleacă odată cu
  proiectul. Timpul trece doar când se desenează cadre (fereastra ascunsă sau minimizată nu
  consumă timp; un pas e de cel mult 100 ms) și stă pe loc sub mouse. Setarea
  `TOAST_POSITION` (`[appearance] notifications`) apare în Aspect → Notificări ca control
  segmentat (rând nou `SettingsRow::Segmented`) și se aplică pe loc.
- **Migrare:** banda `notice` din fereastra „Database” a dispărut (salvarea surselor e un
  toast de succes, mutarea parolei în keychain unul de informare, erorile — și cea a
  salvării parolelor — toast-uri de eroare; „deja o sursă”, „șters”, structura schimbată,
  diagrama indisponibilă la fel). Banda din vederea de date (submit, export, copiere,
  structură, erori) e înlocuită de toast-uri; nota fișierului CSV nesalvat rămâne (e o stare,
  nu o confirmare). Mesajele din bara de jos ale aplicației și modulelor (git, arbore,
  refactorizări, calitate, proiecte la distanță, istoricul local, compararea) sunt acum
  toast-uri cu tipul potrivit; „Fetch / Pull / Push” și aducerea unui fișier la distanță
  sunt toast-uri „în lucru” care trec în succes sau eroare. Mesajele de validare din
  formulare (modalul surselor, structura) rămân în formular.
- **Teste:** domeniul (duplicate, limita și coada, timpii, pauza, „în lucru” → succes,
  închiderea, istoricul, înghețarea, pozițiile), ui-components (lățimi, texte tăiate, două
  acțiuni și unde se apasă, inelul, fără timp, marginea aleasă, layout-ul păstrat), app
  (peste un modal, × și o acțiune, Esc, firul unui modul, proiectele separate, poziția,
  pauza la hover, setarea), explorer (salvarea surselor și „deja o sursă” sunt notificări).
- **Snapshot-uri noi:** `toasts`, `toasts-top-right`, `toasts-bottom-right`,
  `toasts-bottom-center`, `toast-over-modal` (toate tipurile, între ele), `settings-notifications`.
  Singurul schimbat: `data-exported` (banda „exportat” e acum un toast); ferestrei „Database”
  nu i se schimbă niciun snapshot (niciunul nu arăta banda). În snapshot-uri timpul
  notificărilor e oprit la o treime, ca imaginea să fie aceeași la fiecare rulare.

Ce nu s-a făcut:

- Redesenarea doar a zonei stivei: renderer-ul redesenează tot cadrul. În schimb, un cadru
  se cere doar când se schimbă inelul (un punct din 44) sau spinner-ul, cel mult ~30 pe
  secundă, și niciunul cât nu numără nimic.
- Blur real al fundalului (vezi mai sus).
- Combinațiile de taste afișate pe acțiuni: acțiunile sunt comenzi, deci tastele lor merg,
  dar nu sunt scrise pe buton.
