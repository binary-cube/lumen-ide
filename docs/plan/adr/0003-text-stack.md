# ADR-0003: Stack de text

**Status:** Acceptat (2026-10-19)

## Context
Textul este 90% din ce desenăm. Contează calitatea (AA, hinting, ligaturi, fallback, emoji) și viteza (cache).

## Decizie
Definim trait-urile `TextShaper`, `GlyphRasterizer` și `FontSource` în `text-layout`. **Începem cu `cosmic-text`** (shaping + fallback + swash) ca implementare rapidă. Atlasul de glife și pipeline-ul GPU sunt ale noastre. Dacă apar limitări, înlocuim implementarea cu rustybuzz + swash + fontdb direct, fără să afectăm restul codului.

## Alternative
- glyphon (wgpu + cosmic-text): bun pentru prototip, dar ne leagă de wgpu în stratul de text.
- Rasterizare prin API-urile OS (CoreText / DirectWrite): text identic cu nativul, dar trei implementări diferite.

## Consecințe
+ Pornim repede în faza 1.
− Poate fi nevoie de o rescriere a implementării (nu și a contractului) mai târziu.
