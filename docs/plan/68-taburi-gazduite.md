# Plan 68 — Taburi care găzduiesc alte aplicații — ABANDONAT

> **Abandonat (2026-11-08), la decizia utilizatorului.** Codul a fost scos cu totul: domeniul
> `hosted`, infra `hosting` (cu `wry`), modulul `hosted-tabs`, legăturile din aplicație și
> feature flag-ul. De ce:
> - **Aplicațiile oarecare** (Chrome, Firefox, orice program): încercate de utilizator, nu
>   se simt bine. Pe macOS fereastra nu poate fi pusă în a noastră, doar andocată peste tab,
>   iar o fereastră separată nu se comportă ca un tab.
> - **Webview-ul integrat**: nu merge la fel pe toate sistemele, fără griji de dependențe.
>   - WebView2 există doar pe Windows.
>   - Pe Linux, WebKitGTK e un pachet de sistem legat la compilare: fără el, aplicația nu
>     pornește deloc.
>   - Sub Wayland, webview-ul nu poate sta în fereastra noastră.
>
> Dacă o idee de felul ăsta revine (documentație randată ca web, un helper), pornește de la
> zero, cu condiția de mai sus: să meargă pe toate sistemele, fără dependențe de sistem.
> Textul de mai jos e planul de atunci, păstrat ca istoric.


## Context
Cerut de utilizator (2026-11-07, intrarea din `TODO.md`): un tab ca oricare altul (comutat,
mutat, pus în split lângă cod) în care rulează altă aplicație — întâi un browser, ca să nu mai
comute între editor și browser — cu taburi rapide configurabile, mereu în proces separat, izolat
de editor, ca modul cu API, arătat în Task Manager.

## Ce am verificat (2026-11-07)
- **Pagini web.** `wry` (crate-ul lui Tauri, v0.56, iulie 2026) pune un webview nativ ca fereastră
  copil în fereastra noastră, pe orice `raw-window-handle` (avem `winit` 0.30):
  - macOS — WKWebView; Windows — WebView2; Linux — WebKitGTK;
  - copil direct al ferestrei: macOS, Windows, Linux **doar X11**. Sub Wayland cere un container
    GTK (`build_gtk`), pe care `winit` nu îl are: alt drum, mai târziu;
  - motoarele rulează deja conținutul în procese separate (WebContent / browser process /
    WebKitWebProcess), accelerat de GPU. În procesul nostru rămâne doar „mânerul” lor (o vedere
    nativă, apelată de pe firul UI, cum cer sistemele: subțire, fără randare sau JavaScript);
  - căderea procesului de conținut e anunțată (WebView2 `ProcessFailed`, WKWebView
    `webContentProcessDidTerminate`, WebKitGTK `web-process-terminated`): tabul arată „s-a oprit”,
    cu „Pornește din nou”, editorul merge mai departe.
- **O aplicație oarecare, încorporată.** Windows: `SetParent` (merge, cu ciudățeniile ei);
  Linux: `XReparentWindow`, doar X11; macOS: nu se poate. Altfel: o captură în direct
  (ScreenCaptureKit / Windows Graphics Capture / PipeWire), cu clicurile și tastele trimise mai
  departe, cu permisiunea de înregistrare a ecranului și cu mică întârziere.
- **De știut pentru interfață.** Webview-ul e o vedere nativă *deasupra* suprafeței noastre (GPU):
  ce desenăm noi peste ea (meniuri, popover-e, modale, tooltip-uri) n-ar apărea. Așa că, cât un
  overlay acoperă zona tabului, vederea nativă se ascunde și în locul ei arătăm ultima ei captură
  (o imagine, ca la comutatorul de proiecte), apoi revine.

## Decis cu utilizatorul (2026-11-07)
- **Și aplicațiile oarecare acum**, nu doar paginile web: 68e intră în plan, după webview.
- **Linux: X11 acum, Wayland după.** Sub Wayland, tabul spune clar că nu e încă suportat și
  oferă „Deschide în browser”.

## Ce facem
- **68a — portul și webview-ul.**
  - Domeniu nou, pur: `crates/domain/hosted` (`hosted`): `HostedKind` (pagină web: URL; mai
    târziu aplicație: comandă și argumente), portul `Host` (deschide în fereastra dată, mută /
    redimensionează, arată / ascunde, focus, reîncarcă, înapoi / înainte, închide) și ce anunță
    (titlu, URL, se încarcă, s-a oprit, nu răspunde).
  - Infra: `crates/infra/hosting-web` (`wry`), câte un drum pe sistem; Linux X11 întâi.
- **68b — modulul.** `crates/modules/tools/hosted-tabs`:
  - tabul ca viewer (`view-hosted`), cu bara lui (înapoi, înainte, reîncarcă, adresa), mutat și
    pus în split ca orice tab; vederea nativă urmează dreptunghiul tabului, ascunsă cât nu e
    vizibil (alt tab, fereastra ascunsă) sau cât un overlay îl acoperă (atunci: captura ei);
  - comenzi: „Deschide pagina în tab…”, „Deschide în browser”; API pe host pentru alte module
    („deschide pagina în tab” din alt loc);
  - închis curat cu tabul, cu proiectul și cu aplicația.
- **68c — taburile rapide.** Lista de „ce pornește în tab” (nume, URL), declarată o dată în
  `src/settings.rs`, în Setări și în meniul butonului lui din bara din dreapta; fiecare cu
  scurtătura lui.
- **68d — în Task Manager.** Fiecare tab găzduit pe rândul lui (procesul lui unde sistemul îl
  dă: WebView2 îl spune; WKWebView nu, public — atunci: tabul, fără cifrele procesului), cu
  Oprește / Pornește din nou.
- **68e — aplicații oarecare (după)**, unde se pot face bine: Windows și Linux X11 prin
  încorporare; macOS prin captură în direct.

## Stare
- [ ] 68a
- [ ] 68b
- [ ] 68c
- [ ] 68d
- [ ] 68e
