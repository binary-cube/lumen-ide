# 06 — Randare accelerată pe GPU

## 1. Obiective

- Tot ce e pe ecran (text, UI, terminal) se desenează pe GPU, într-un singur pipeline.
- Driverul grafic e **înlocuibil**: contractele sunt separate de implementare.
- Funcționează pe macOS (Metal), Windows (DX12 / Vulkan), Linux (Vulkan / GL).
- Există un backend **software (CPU)** pentru teste headless (snapshot tests în CI fără GPU) și ca fallback de urgență.

## 2. Straturile de randare

```
┌───────────────────────────────────────────────────────────────┐
│ UI (ui-kit / ui-workbench): construiește o Scene declarativă  │
├───────────────────────────────────────────────────────────────┤
│ render-api: CONTRACTE                                   │
│   Scene, Primitive (Quad, Glyph, Path, Image, Shadow), Layer, │
│   trait Renderer, trait RenderSurface, trait GlyphRasterizer  │
├───────────────────────────────────────────────────────────────┤
│ text-layout: shaping (rustybuzz) + fallback de fonturi +       │
│   rasterizare (swash) + atlas de glife (bin packing, etagere) │
├───────────────────────────────────────────────────────────────┤
│ DRIVERE (fiecare un crate separat, ales la runtime)           │
│  render-wgpu      → Metal / DX12 / Vulkan / GL (implicit)     │
│  render-software  → tiny-skia (CPU): teste și fallback        │
│  (opțional, viitor) render-metal / render-d3d12 nativ, dacă   │
│   profilarea arată că wgpu e un bottleneck                    │
└───────────────────────────────────────────────────────────────┘
```

## 3. Contracte (`render-api`)

```rust
/// Scena descrie un frame complet, independent de backend.
pub struct Scene {
    pub layers: Vec<Layer>,               // ordine z; fiecare layer are clip rect
}

// Implementare: în loc de un singur `Vec<Primitive>`, fiecare `Layer` are câte un
// vector per tip (`quads`, `glyphs`), ca driverul să le trimită direct ca instanțe,
// fără sortare pe frame. Tipurile Shadow / Image / Path / Underline se adaugă când e nevoie.
pub enum Primitive {
    Quad   { rect: Rect, fill: Fill, corner_radius: Corners, border: Option<Border> },
    Shadow { rect: Rect, blur: f32, color: Rgba, corner_radius: Corners },
    Glyph  { glyph: GlyphKey, origin: Point, color: Rgba },   // GlyphKey = font + glyph id + size + subpixel offset
    Image  { image: ImageId, rect: Rect },
    Path   { path: PathId, fill: Fill },                      // squiggles, icoane vectoriale
    Underline { start: Point, width: f32, thickness: f32, color: Rgba, wavy: bool },
}

pub trait Renderer {
    fn capabilities(&self) -> RendererCapabilities;           // nume backend, MSAA, limite atlas
    fn resize(&mut self, size: PhysicalSize, scale_factor: f32);
    fn render(&mut self, scene: &Scene) -> Result<FrameStats, RenderError>;
    fn glyph_cache(&mut self) -> &mut dyn GlyphCache;
}

pub trait RendererFactory {
    fn name(&self) -> &'static str;                           // "wgpu", "software"
    fn is_supported(&self, platform: &PlatformInfo) -> bool;
    fn create(&self, surface: SurfaceHandle, opts: &RenderOptions) -> Result<Box<dyn Renderer>, RenderError>;
}
```

`SurfaceHandle` conține `raw-window-handle`, așa că driverul nu depinde de `winit`.

### Alegerea driverului

```toml
[render]
backend = "auto"          # auto | wgpu | software
wgpu.preferred_api = "auto"  # auto | metal | dx12 | vulkan | gl
vsync = true
```

`auto` încearcă driverii în ordinea priorității, iar primul care se creează cu succes câștigă. Se păstrează și un log despre ce s-a ales (vizibil în „About / Diagnostics”).

## 4. Pipeline-ul `render-wgpu`

- **Instancing:** câte un draw call per tip de primitivă per layer (quads, glyphs, shadows). Datele instanțelor stau într-un buffer reutilizat între frame-uri.
- **Atlas de glife:** texturi `R8` pentru glife monocrome și `RGBA8` pentru emoji color. LRU cu evacuare pe pagini. Subpixel positioning (3–4 variante pe orizontală) pentru text clar.
- **Anti-aliasing text:** grayscale AA, cu gamma corectă. Subpixel LCD AA eventual opțional (pe Windows contează mai mult).
- **Shaders WGSL:** `quad.wgsl` (SDF pentru colțuri rotunjite și border), `glyph.wgsl`, `shadow.wgsl` (blur analitic gaussian).
- **Damage tracking (redesenare parțială):** `render_api::DamageTracker` ține o evidență compactă a ultimului cadru desenat (straturile cu clip, quad-urile, imaginile ca id + loc + opacitate, glifele; fără pixeli) și compară scena următoare primitivă cu primitivă: zonele afectate (`Damage`, cel mult 8 dreptunghiuri disjuncte în pixeli întregi, unite cele care se ating; peste jumătate din suprafață = cadru întreg) sunt amprentele vechi și noi ale primitivelor schimbate, tăiate de clipul stratului. Cadru întreg la primul cadru, altă mărime, alt fundal, alte colțuri ale ferestrei, chei de glife reemise (`glyph_keys_reset`, la „Clear and Rebuild Caches”) sau o glifă a cărei cutie nu se știe. `render-wgpu` desenează într-o textură păstrată (una, de mărimea suprafeței, `kept-frame`) doar în zonele afectate (`LoadOp::Load`, fiecare zonă umplută cu fundalul, apoi doar instanțele care o ating, cu scissor) și o copiază în imaginea suprafeței; fără nicio schimbare nu se prezintă nimic. Fallback la cadru întreg direct în suprafață: suprafața nu acceptă copii (`COPY_DST`), în timpul redimensionării (macOS), `EDITOR_FULL_REDRAW=1`. Textura se eliberează când fereastra e ascunsă / minimizată (`Renderer::set_visible`). `render-software` desenează doar zonele afectate în bufferul păstrat. Teste: secvențe aleatoare de cadre, parțial față de întreg, pixel cu pixel (software; GPU fără fereastră unde există un adaptor) și `EDITOR_SNAPSHOT_PARTIAL=1` pe toate capturile. **Măsurat** (release, Apple M2 Max, HOME izolat, timpul GPU al procesului din IOKit): clipirea cursorului 0,26–0,28 % → 0,15–0,16 % GPU, decorațiunile de Crăciun animate 1,7–1,8 % → 1,0 %, un toast numărând invers 0,61–0,63 % → 0,18–0,20 %; CPU-ul procesului rămâne practic același (clipire ~0,55 %, decorațiuni 3,2–3,7 %): ~2 ms de CPU per cadru prezentat (nextDrawable, codarea și trimiterea comenzilor, firele Metal / Core Animation, kernelul) nu depind de câți pixeli se desenează — fără prezentare (build de test) aceleași scenarii costă 0,13 % și 0,45 %. Doar mai puține cadre prezentate l-ar scădea.
- **Prezentare:** `PresentMode::Fifo` (vsync) implicit; opțional `Mailbox` pentru latență mică.

## 5. Text: de la rope la pixeli

```
Document (rope) → linii vizibile (viewport) → segmente cu stil (highlight + selecție + diagnostice)
  → shaping per linie (rustybuzz, cache pe (text, font, stil)) → poziții de glife
  → Primitive::Glyph în Scene → atlas → GPU
```

- **Cache de linii shape-uite**, indexat după hash-ul conținutului liniei plus stil. La scroll, liniile nu se re-shape-uiesc.
- **Fonturi:** detectarea fonturilor de sistem prin `font-kit` sau `fontdb`. Fallback chain (ex: JetBrains Mono → Symbols Nerd Font → Noto Emoji). Ligaturi opționale.
- **Monospace fast path:** pentru ASCII în fonturi monospace se sare peste shaping complet (doar lookup în cmap).
- **Alternativă evaluată:** `cosmic-text` (shaping + layout + swash integrat). Poate fi folosit ca implementare inițială rapidă în spatele trait-ului `TextShaper` și înlocuit ulterior (vezi ADR-0003).

## 6. Testare

- **Snapshot tests** cu `render-software`: Scene → PNG → comparare cu referința (toleranță la diferențe de AA).
- **Benchmark-uri** (`criterion`): timp de construire a scenei, timp de shaping, rata de hit în atlas.
- Overlay de debug (F12): FPS, timp de frame CPU/GPU, nr. de draw calls, ocuparea atlasului.
