# ADR-0008: Un proces per fereastră

**Status:** Înlocuit de [ADR-0009](0009-un-proces-ferestre-izolate.md) (2026-10-19)

## Context
Utilizatorul vrea mai multe proiecte deschise simultan, în ferestre separate, și vrea să vadă consumul de resurse (CPU / RAM / GPU) al fiecărei ferestre.

## Decizie
Fiecare fereastră e un **proces separat**. „New Window” și „Open Project → Fereastră nouă” pornesc executabilul curent cu calea proiectului ca argument.

## Alternative
- **Toate ferestrele într-un proces** (winit permite): memorie comună (fonturi, gramatici), dar o problemă într-un proiect le afectează pe toate, iar consumul nu se mai poate atribui unei ferestre.

## Consecințe
+ Izolare completă: un crash sau un proiect uriaș nu afectează celelalte ferestre.
+ Consumul din status bar e exact consumul ferestrei.
+ Model simplu: fără stare partajată între ferestre.
− Fiecare fereastră își încarcă fonturile și gramaticile (~60–80 MB per fereastră goală).
− Până la împachetarea ca `.app` (faza 7), pe macOS fiecare fereastră apare ca icon separat în Dock. Ulterior: un proces mic „launcher” care coordonează ferestrele (meniu Window, Dock unic).
