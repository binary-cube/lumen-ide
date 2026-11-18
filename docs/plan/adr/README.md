# Architecture Decision Records

| # | Decizie | Status |
|---|---------|--------|
| [0001](0001-ddd-hexagonal-workspace.md) | DDD + arhitectură hexagonală, un crate per bounded context | Acceptat |
| [0002](0002-wgpu-driver-implicit.md) | `wgpu` ca driver implicit în spatele contractului `Renderer` | Acceptat |
| [0003](0003-text-stack.md) | Stack de text: rustybuzz + swash + atlas propriu (cosmic-text ca start rapid) | Acceptat |
| [0004](0004-ropey-in-domain.md) | `ropey` acceptat în domeniul Document, în spatele trait-ului `TextBuffer` | Acceptat |
| [0005](0005-ui-kit-propriu.md) | UI kit propriu peste `render-api` (nu egui/iced/Slint) | Acceptat |
| [0006](0006-module-statice-apoi-wasm.md) | Module statice acum, WASM (wasmtime + WIT) pentru pluginuri dinamice | Acceptat |
| [0007](0007-tree-sitter-si-lsp.md) | tree-sitter pentru sintaxă; indexator nativ PHP + LSP pentru restul | Acceptat |
| [0008](0008-un-proces-per-fereastra.md) | Un proces per fereastră (izolare, consum per fereastră) | Înlocuit de 0009 |
| [0009](0009-un-proces-ferestre-izolate.md) | Un singur proces: resurse comune, ferestre izolate, `catch_unwind` per fereastră | Acceptat |
