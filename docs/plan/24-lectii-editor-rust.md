# 24. Ce putem lua de la un editor Rust matur (memorie, CPU) — doar plan

Cererea: să ne uităm la `Cargo.toml`-ul unui editor de cod open-source matur, scris în Rust,
și să vedem ce ne-ar ajuta
la consumul de memorie / CPU / GPU. **Doar plan, fără cod.** Nimic nu se schimbă fără „go”.

## Ce face editorul de referință (2026-10-24) vs noi

| | Editorul de referință | Noi |
|---|---|---|
| Alocator | sistemul (niciun mimalloc / jemalloc) | sistemul pe macOS / Linux; `mimalloc` doar pe Windows, cu `mi_collect` după eliberări mari (`app/src/main.rs`) |
| Release | `lto = "thin"`, `codegen-units = 1`, `debug = "limited"`; binarul principal cu `codegen-units = 16` (compilare) | `opt-level = 3`, `lto = "thin"`, `codegen-units = 1`, fără debug info |
| Profil de profilare | `release-fast` (release fără LTO, debug complet) | — |
| Dev | `incremental`, `split-debuginfo = "unpacked"`, `debug = "limited"`; tree-sitter, resvg, serde_json… cu `opt-level = 3` | `debug = "line-tables-only"`; toate dependențele `opt-level = 2` |
| Lint-uri | `redundant_clone = deny`, `disallowed_methods = deny`, `declare_interior_mutable_const = deny`, `dbg_macro` / `todo = deny` | `unwrap_used` / `expect_used` / `dbg_macro` / `todo = warn` |
| Hash-uri | `rustc-hash` (FxHash) prin crate-ul lor `collections` | `std` (SipHash) peste tot (~134 de `HashMap` / `HashSet`) |
| Colecții mici | `smallvec` (`union`, `const_new`) | — |
| Lock-uri | `parking_lot` | `std::sync` (ne bazăm pe otrăvire: `clear_poison`, `PoisonError::into_inner`) |
| Date citite des, scrise rar | `arc-swap` | `RwLock` / `Memo` |
| Text | rope propriu (`sum_tree`) | `ropey` |
| Stocare pe disc | `heed` (LMDB, mapat în memorie) | fișiere binare proprii, încărcate întregi (`search.bin`, indexul de simboluri) |

## Ce merită (în ordinea câștig / risc)

### 1. `clippy::redundant_clone` (CPU, alocări) — risc mic
- Probă (2026-10-24): **18 clonări inutile** doar în `app`, `code-intelligence`, `document`,
  `indexer`. Fiecare e o alocare + o copiere în plus (adesea `String` / `Vec` pe căi des
  rulate).
- Pas: lint-ul pe workspace (`warn`, apoi corectat tot; `check.sh` îl face eroare prin
  `-D warnings`).

### 2. `disallowed_methods` ca pază de arhitectură — risc mic
- Editorul de referință îl folosește ca să interzică metode anume. La noi: un `clippy.toml` **în crate-urile
  de domeniu** care interzice `std::thread::spawn`, `std::thread::sleep`, `std::fs::*`,
  `std::env::var` — exact ce am corectat în review (H2, M3, M4), ca să nu mai revină.
- De verificat: clippy citește `clippy.toml` din directorul crate-ului; testele
  (`tests/unit`) pot avea nevoie de `#[allow]` local.

### 3. `rustc-hash` (FxHash) în structurile interne fierbinți — CPU
- SipHash e făcut pentru chei venite din afară (atacuri prin coliziuni); la noi cheile sunt
  ale noastre (căi, nume de simboluri, id-uri). FxHash e de câteva ori mai rapid pe chei
  scurte.
- Unde contează: `SymbolIndex` (tabelele pe nume / FQN, `members_of`, `remove_file`),
  vocabularul căutării (BM25), cache-ul de linii al textului, `GenerationalCache`.
- **Nu** pentru amprente (`text_fingerprint`, `refactoring::fingerprint` rămân pe
  `DefaultHasher`) și nu pentru ce se salvează pe disc (rezultatul FxHash nu e garantat
  stabil între versiuni).
- Pas: un tip `kernel::FastMap` / `FastSet`, introdus pe rând, cu măsurare înainte / după
  (indexarea proiectului sintetic din scratchpad, timpul și RSS-ul).

### 4. Alocatorul — deja măsurat pe macOS; Windows de măsurat
- **macOS** (măsurat anterior, release, proiect PHP de ~15.000 de fișiere indexat):
  alocatorul sistemului ~184 MB stabilizat (vârf ~110 MB footprint) vs mimalloc ~215 MB
  (~160 MB); mimalloc indexa ~10% mai repede, dar ține pagini libere pe thread → pe macOS /
  Linux rămânem pe al sistemului.
- **Windows**: mimalloc, pentru că heap-ul Windows returnează greu memoria și fragmentează
  cu multe alocări mici; plus `mi_collect` după eliberări mari.
- Comparația pe Windows (de făcut de tine): `cargo build --release -p app` vs
  `cargo build --release -p app --features system-allocator` (alocatorul sistemului și pe
  Windows), același proiect deschis și indexat, memoria din Task Manager („Memory”, nu
  „Commit size”) la final și după câteva minute.

### 5. `smallvec` pe căi fierbinți — alocări
- Candidați: selecțiile (aproape mereu un cursor), `Symbol.parents` (0–2), rezultatele
  per linie din gutter / diagnostice. Fiecare `Vec` mic e o alocare pe heap.
- Pas: doar unde un profil (`release-fast`, mai jos) arată alocările; nu peste tot.

### 6. Profil `release-fast` pentru profilare — fără efect în produs
- `inherits = "release"`, `lto = false`, `codegen-units = 16`, `debug = "full"`: build
  rapid cu simboluri, pentru Instruments / `perf` / ETW. Ne ajută să găsim restul (5, 7).
- `debug = "limited"` în release doar dacă vrei backtrace-uri cu linii în
  rapoartele de crash — mărește binarul, nu memoria la rulare.

### 7. Indexul mapat în memorie (`heed` / LMDB sau `memmap2`) — memorie, mare
- Azi indexul de căutare (BM25 + vectori) și simbolurile se încarcă întregi în RAM. Pe un
  proiect mare, vectorii sunt partea grea. Mapate în memorie, paginile neatinse nu ocupă
  RAM rezident.
- E o schimbare mare (formatul `search.bin`, `kernel::cache_stamp`); întâi măsurat cât din
  RSS e indexul, pe proiectul sintetic. Plan separat dacă merită.

### 8. Dev: `opt-level = 3` pe tree-sitter — doar dezvoltare
- Testele de parsare / snapshot-urile mai rapide; un rând în `[profile.dev.package]`.

## Ce nu luăm

- **`parking_lot`**: lock-uri mai mici, dar fără otrăvire — noi ne bazăm pe ea ca un panic
  într-un worker să nu lase datele pe jumătate (A1 din review). Câștigul de memorie e
  neglijabil la câte lock-uri avem.
- **Rope-ul / `sum_tree` propriu**: `ropey` ne ajunge; o rescriere fără câștig măsurat.
- **`dashmap`, `moka`, `lru`**: avem `kernel::cache` (`Memo`, `GenerationalCache`, `Purge`),
  care se golește la „Clear and Rebuild Caches”.
- **Patch-urile lor** (`[patch.crates-io]`): sunt pentru dependențele lor (audio, WebRTC,
  EGL), nu ale noastre.
- **`style = allow` pe clippy**: noi păstrăm stilul verificat.

## Mărunt, văzut în trecere

- `Cargo.toml` rădăcină, `[workspace.lints.rust]`: comentariul de la `unsafe_code` e încă în
  română („driverele pot permite local…”) → de tradus (regula „English in the code”).

## Stare (2026-10-24)

- [x] 1. `redundant_clone` pe workspace (27 de clonări scoase).
- [x] 2. `crates/domain/clippy.toml`: firele, așteptarea, discul și mediul interzise în
  domeniu (testele care pregătesc fișiere / fire le au permise explicit).
- [x] 4. Alocatorul: macOS / Linux pe al sistemului (măsurat), mimalloc doar pe Windows;
  comutatorul `system-allocator` pentru comparația pe Windows.
- [x] 8. `opt-level = 3` pentru tree-sitter în dev.

## Ordinea propusă

1 → 2 → 8 (mici, sigure), apoi 6 (unealta de măsurare), apoi 3 și 4 cu cifre, 5 unde arată
profilul, 7 doar dacă măsurătoarea o justifică. Fiecare pas: commit separat, `check.sh`
verde, snapshot-urile identice.
