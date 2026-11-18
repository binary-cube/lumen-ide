# Plan 66 — Ținut treaz: calculatorul nu adoarme cât lucrează ceva

## Context
Cerut de utilizator (2026-11-06 / 07, intrarea din `TODO.md`): pornesc un AI în terminal (sau
transferuri, o indexare lungă) și calculatorul nu trebuie să intre în sleep și să oprească
treaba. Cât e ținut treaz și nimeni nu atinge nimic, ecranul nu trebuie să rămână ore cu aceeași
imagine (burn-in) și se poate întuneca după un timp. Totul configurabil, dintr-o singură sursă.

## Principii (cerute de utilizator, 2026-11-07)
- **Utilizatorul controlează.** Totul e oprit implicit; nimic nu ține calculatorul treaz
  până nu alege el (butonul sau Setările). Modul automat („doar cât rulează ceva”) e și el o
  alegere a lui, nu implicit.
- **Mereu știe.** Cât calculatorul e ținut treaz, se vede: butonul aprins și un indicator în
  bara de jos („Ținut treaz”, clic îl oprește, tooltip-ul spune de ce: pornit de el, sau ce
  rulează în modul automat). În modul automat, o notificare când începe și când se oprește.

## Ce facem

- **66a — portul și implementările pe sisteme.**
  - Domeniu nou, pur: `crates/domain/tools/power` (`power`).
    - Portul `SleepBlocker`: `hold(reason) -> Result<Box<dyn SleepHold>, PowerError>`. Ținerea
      e eliberată când `SleepHold` e aruncat (și când procesul moare: sistemul o eliberează).
    - Politica (funcții pure, testate): **când** se ține treaz (`Mode`: oprit / mereu / doar cât
      rulează ceva) și **ce face ecranul** după un timp fără activitate (`ScreenCare`: dim după N
      minute, cât de întunecat, ceva care se mișcă împotriva burn-in-ului).
  - Infra nou: `crates/infra/system/power` (`power-system`), câte o implementare pe sistem:
    - macOS: `IOPMAssertionCreateWithName(kIOPMAssertPreventUserIdleSystemSleep)`, motivul
      vizibil în `pmset -g assertions`; eliberată cu `IOPMAssertionRelease`.
    - Windows: `PowerCreateRequest` + `PowerSetRequest(PowerRequestSystemRequired)` (legată de
      un handle, nu de un fir), eliberată cu `PowerClearRequest` / `CloseHandle`.
    - Linux: `systemd-inhibit --what=sleep:idle --mode=block … sleep infinity` ca proces copil,
      oprit la eliberare (fără systemd: eroare clară, „nesuportat”).
- **66b — modulul.** `crates/modules/tools/keep-awake` (`keep-awake`):
  - Butonul lui în bara din dreapta, **deasupra lui „Mod concentrare”** (înregistrat ca
    `ToolWindowProvider`, comandă, aprins cât e pornit), comanda `keep_awake.toggle` (scurtătura
    aleasă în keymap).
  - Setările `[keep_awake]` declarate o dată în `src/settings.rs` (`Setting<T>`), pagina lui
    în Setări → Interface (`SettingsContribution`): pornit, când (mereu / doar cât rulează ceva),
    dim (după câte minute, cât de întunecat), mișcare împotriva burn-in-ului.
  - Ținerea luată și eliberată în fundal (niciodată pe UI thread), o singură dată pe proces
    (toate ferestrele o împart), eliberată la oprire și la închiderea aplicației. O eroare (un
    sistem fără suport) e spusă o dată, ca notificare.
- **66c — ecranul protejat.** Cât e ținut treaz și nu atinge nimeni nimic N minute
  (`IdleAction`, ca la Relax): o vedere peste toată fereastra (`host.open_overlay`) se întunecă
  treptat până la nivelul ales; dacă e cerut, un ceas mic plutește încet (poziția se schimbă din
  când în când, nimic nu stă pe loc). Câteva cadre pe secundă, nimic altceva. Orice mișcare de
  mouse, click sau tastă o închide.
- **66d — opțiuni directe și „doar cât rulează ceva”.**
  - Click dreapta pe buton: meniul lui (`MenuContribution` cu același id): pornit / oprit, „doar
    cât rulează ceva”, „întunecă ecranul acum”, **Setări…** (deschide pagina lui). Ca orice
    meniu de buton, din comenzi — fără cod dublat.
  - API nou, folosibil de orice modul: `open_settings_page(page)` pe host (aplicația deschide
    Setările la pagina dată).
  - Indicatorul din bara de jos cât e ținut treaz (vezi „Principii”).
  - „Doar cât rulează ceva” (ales de utilizator, nu implicit): ținut treaz cât aplicația are de lucru (ce întreabă și la închidere:
    `running_work` — terminale cu procese, transferuri, interogări), verificat rar (la câteva
    secunde, în fundal), eliberat când nu mai rulează nimic.
- **66e — în Task Manager.** Rândul modulului spune dacă ține treaz și de când.

- **66f — și ecranul aprins** (cerut, 2026-11-07, după ce ecranul s-a stins și s-a blocat cât
  calculatorul era ținut treaz): opțiunea „Ține și ecranul aprins” (`[keep_awake] screen`,
  oprită implicit), în Setări și în meniul butonului (`keep_awake.screen`).
  - Portul primește ce să țină (`power::Keep { screen }`):
    - macOS: `PreventUserIdleDisplaySleep`;
    - Windows: și `PowerRequestDisplayRequired`;
    - Linux: `--what=sleep:idle` (fără ecran: doar `sleep`).
  - Ținut deja: luat din nou pe noua alegere, înainte ca vechiul să plece. Nu există niciun
    moment fără, iar „de când” rămâne același.

## Stare
- [x] 66a
- [x] 66b
- [x] 66c
- [x] 66d
- [x] 66e
- [x] 66f

## 66f. Semnul din afara ferestrelor (2026-11-11)

Cerut: „dacă keep awake e on, să apară și un icon lângă ceas, atât pe Mac cât și pe Windows,
și pe iconul aplicației, un identificator că keep awake e ok și rulează”. **Făcut:**

- contribuția `workbench_model::SystemIndicator` (`SystemSign`: iconița, tooltip-ul, insigna,
  acțiunea meniului); modulul o dă cât ține calculatorul treaz;
- aplicația o trimite platformei doar când se schimbă (`app/src/app/system_sign.rs`, una
  pentru toată aplicația), cu iconița desenată o dată (macOS: template, în culoarea barei;
  Windows: accentul temei);
- `platform_api::Window::set_system_sign`: pe macOS un `NSStatusItem` în bara de meniu (cu
  meniul: „Oprește «Ține treaz»”, „Arată {app}”) și insigna ☕ pe iconița din Dock; pe Windows
  iconița din zona de notificări (`Shell_NotifyIconW`, meniul la clic, refăcută dacă
  Explorer repornește, ștearsă la ieșire) și semnul pe butoanele din bara de activități
  (`ITaskbarList3::SetOverlayIcon`, și pe ferestrele deschise după).
- Verificat pe macOS (instanță izolată): insigna ☕ în Dock (`lsappinfo`), iconița în bara de
  meniu (19 → 18 la oprirea instanței), hold-ul eliberat la ieșire. Windows: compilat și
  verificat de clippy; de probat pe un Windows real.
