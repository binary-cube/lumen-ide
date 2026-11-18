# 11 — Convenții și calitate

## 1. Cod

- **Ediția 2024**, toolchain stable fixat în `rust-toolchain.toml` (1.99).
- `#![deny(unsafe_code)]` în toate crate-urile de domeniu. `unsafe` e permis doar în driverele de randare și PTY, cu comentariu `// SAFETY:`.
- Erori: `thiserror` în librării (erori tipizate per context) și `anyhow` doar în `app`.
- Fără `unwrap()` / `expect()` în cod de producție, cu excepția invarianților documentați.
- Logging: `tracing` cu span-uri per subsistem; log-ul se poate vedea din aplicație (Help → Show Log).
- Denumiri: crate-uri `lumen-<context>`, module Rust pe concepte de domeniu (nu `utils`, `helpers`).
- Tipurile de domeniu sunt **newtypes** (`struct LineNumber(u32)`, `struct ByteOffset(usize)`), ca să nu se confunde linia cu coloana sau byte-ul cu caracterul.
- Pozițiile: intern în **byte offset UTF-8**. Conversia în UTF-16 (pentru LSP) și în coloane vizuale (tab-uri, caractere wide) se face doar la margine.

## 2. Reguli de dependență (verificate automat)

Stratul fiecărui crate se deduce din calea lui. Dependențele interne permise:

| Strat (cale) | Poate depinde de | Externe permise |
|--------------|------------------|-----------------|
| `kernel` | — | listă albă: serde, thiserror, smallvec, uuid (identitățile, ex. `ProjectId`: UUID v4, făcute într-un singur loc) |
| `domain/*` | kernel, domain | listă albă: ropey, smallvec, semver, serde, thiserror, async-trait, futures, unicode-segmentation, regex |
| `render/render-api` | kernel | listă albă: serde, thiserror, smallvec, bytemuck, raw-window-handle |
| `platform/platform-api` | kernel | listă albă: serde, thiserror, raw-window-handle |
| `render/text`, `render/icons` | kernel, render-api | orice |
| `render/*` (drivere) | kernel, render-api, text | orice |
| `platform/*` (implementări) | kernel, platform-api | orice |
| `ui/*` | kernel, domain, render-api, platform-api, text, ui | orice |
| `infra/process` (unealtă comună: rularea programelor) | kernel | orice |
| `infra/*` | kernel, domain, infra/process | orice |
| `modules/*` | kernel, domain, render-api, text, ui, infra | orice |
| `app`, `xtask` | orice | orice |

Dev-dependencies (ex: `proptest`) nu sunt restricționate.

Verificarea: `cargo xtask check-deps` parsează `cargo metadata` și pică dacă o regulă e încălcată (rulează în CI). Un crate nou într-o cale neclasificată face verificarea să pice, ca să fie adăugat explicit în `xtask/src/main.rs`.

## 3. Testare

| Nivel | Unde | Unelte |
|-------|------|--------|
| Unit (domeniu) | fiecare crate `domain/*` | `cargo test`, `proptest` pentru buffer, tranzacții, selecții |
| Contract tests | `render-api`, `language`, `intelligence` | aceeași suită rulată pe fiecare implementare (ex: orice `Renderer` trebuie să treacă testele de bază) |
| Snapshot vizual | UI + randare | `render-software` → PNG, `insta` pentru diff |
| Integrare limbaj | `modules/languages/*` | fixtures: fișier sursă + poziție → completări, definiții, referințe așteptate |
| Performanță | `benches/` | `criterion`; CI semnalează regresii de peste 10% |
| End-to-end | `app` | mod headless care rulează un script de comenzi (deschide, tastează, salvează) |

## 4. ADR (Architecture Decision Records)

Fiecare decizie importantă se scrie în `docs/plan/adr/NNNN-titlu.md`: **Context**, **Decizie**, **Alternative considerate**, **Consecințe**. Un ADR nu se șterge niciodată. Dacă decizia se schimbă, se scrie unul nou cu statusul „Superseded by”.

## 5. Git și workflow

- Branch-uri `feat/…`, `fix/…`, `refactor/…`. PR-uri mici, câte unul per livrabil din roadmap.
- Conventional commits (`feat(lang-php): …`).
- Changelog generat automat.
