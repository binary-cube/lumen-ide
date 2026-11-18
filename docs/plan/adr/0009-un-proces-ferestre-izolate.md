# ADR-0009: Un singur proces, resurse comune, ferestre izolate

**Status:** Acceptat (2026-10-19). Înlocuiește [ADR-0008](0008-un-proces-per-fereastra.md).

## Context
Cu un proces per fereastră, fiecare proiect nou încărca din nou fonturile, gramaticile,
GPU-ul (device, atlas) și modelul semantic (~100 MB per fereastră). Utilizatorul vrea
proiecte în ferestre separate, dar cu resursele comune partajate și cu izolare pe proiect.

## Decizie
Toate ferestrele rulează într-un singur proces, pe thread-ul UI:

| Comun (o dată pe proces) | Izolat (per fereastră / proiect) |
|---|---|
| fonturile și sistemul de text (shaping, cache-uri) | workbench-ul: taburi, documente, selecții |
| `LanguageSupport`: gramatici, injecții, inteligența limbajelor | indexerul și `.lumen/` al proiectului |
| temele (o schimbare se propagă în toate ferestrele) | terminalele, watcher-ele, thread-ul de parsare |
| GPU: instanța, adaptorul, device-ul, coada, atlasul de glife, texturile | suprafața, bufferele de instanțe, pipeline-urile |
| modelul semantic (embeddings) | panourile, starea UI |

- **Platforma** (`platform-api`): evenimentele vin cu `WindowId`; `Window::open_window`
  cere o fereastră nouă; `Control::Close` închide doar fereastra, `Control::Exit` aplicația.
- **GPU** (`render-wgpu`): `GpuContext` ținut printr-un `Weak` la nivel de proces: ferestrele
  noi îl refolosesc dacă adaptorul suportă suprafața lor; după ultima fereastră se eliberează.
- **Aplicația**: `Lumen` ține ferestrele (`EditorApp`) și `Shared`; schimbările comune (temă,
  limbă) au versiuni pe care fiecare fereastră le urmează.
- **Izolare la erori:** fiecare eveniment al unei ferestre rulează sub `catch_unwind`; un
  panic închide doar fereastra lui (și se loghează), celelalte continuă.

## Alternative
- **Proces per fereastră** (ADR-0008): izolare totală, dar memorie dublată și pornire lentă.
- **Proces launcher + procese-fereastră cu memorie partajată**: complexitate mare (IPC,
  sincronizare) pentru un câștig mic față de `catch_unwind` + izolarea datelor.

## Consecințe
+ A doua fereastră costă doar starea ei (măsurat: 2 ferestre ≈ memoria uneia).
+ Fereastră nouă instantanee: fără reîncărcarea fonturilor, gramaticilor, GPU-ului.
+ Tema și limba se schimbă peste tot dintr-o singură alegere.
− Un blocaj (nu un panic) pe thread-ul UI afectează toate ferestrele: munca grea rămâne pe
  thread-uri de fundal (parsare, indexare, căutare, terminal), ca până acum.
− Consumul din bara de stare e al procesului, nu al ferestrei.
