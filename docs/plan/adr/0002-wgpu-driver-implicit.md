# ADR-0002: `wgpu` ca driver implicit în spatele contractului `Renderer`

**Status:** Acceptat (2026-10-19)

## Context
Avem nevoie de randare GPU pe macOS, Windows și Linux. Fiecare platformă are API-ul ei preferat: Metal, DX12 sau Vulkan.

## Decizie
Definim contractele în `render-api` (Scene, Primitive, `Renderer`, `RendererFactory`). Primul driver este `render-wgpu`, care acoperă Metal, DX12, Vulkan și GL printr-un singur cod de shaders (WGSL). Adăugăm și `render-software` (tiny-skia) pentru teste și fallback. Drivere native (Metal sau D3D12 direct) se adaugă doar dacă profilarea arată o problemă reală.

## Alternative
- Metal + DX12 + Vulkan scrise direct: control maxim, dar triplul de muncă.
- OpenGL: învechit pe macOS (deprecated), cu performanță inconsistentă.
- Skia (skia-safe): dependență C++ grea, build complicat.

## Consecințe
+ Un singur set de shaders, o singură implementare de întreținut.
+ Driverul se poate schimba din setări fără să atingem UI-ul.
− Un strat de abstracție în plus (overhead mic, măsurabil și acceptabil pentru 2D).
