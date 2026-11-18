# ADR-0006: Module statice acum, WASM mai târziu

**Status:** Acceptat (2026-10-19)

## Context
Vrem module plug-and-play, iar pe viitor și pluginuri terțe. Rust nu are ABI stabil.

## Decizie
**E1:** modulele sunt crate-uri compilate în binar, activate sau dezactivate din setări (`modules.mode = auto | explicit`) și opțional prin feature flags Cargo.
**E2:** pluginuri terțe în WASM (wasmtime + component model / WIT), cu sandbox.
Contractele E1 se proiectează de la început „serializabile” (DTO-uri, fără referințe cu lifetime), ca trecerea la E2 să fie doar un strat de adaptare.

## Alternative
- `libloading` cu `.so`/`.dll`: ABI instabil, fără izolare, iar un crash dărâmă editorul.
- Procese separate cu RPC pentru toate modulele: overhead prea mare pentru highlighting.

## Consecințe
+ Modulele interne sunt rapide (apeluri directe) și sigure.
+ Calea spre pluginuri terțe rămâne deschisă, fără rescriere.
