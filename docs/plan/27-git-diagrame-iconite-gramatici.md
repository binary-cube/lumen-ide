# 27. Git în editor, diagrama în popover, iconițe pe foldere, DDD, gramatici eliberate

Cererea: punctele 5, 6, 7, 2, 3 din lista rămasă.

## Ce avem deja (verificat în cod și în capturi)

- **5. Git în editor**: barele din gutter (adăugat / modificat / șters, calculate pe un fir
  separat, `gutter_diff.rs`), click pe bară → revert / compare, adnotările blame (date + autor,
  estompate după vechime, citite în fundal pe pool-ul `git-blame`), tooltip cu commit-ul
  (hash, autor, dată, mesaj). Bifa din roadmap rămăsese goală. Lipsește: click pe adnotare →
  commit-ul în Git Log.
- **7. Pachete de iconițe**: Lucide, Tabler, Phosphor, Material, Remix, Heroicons, Fluent
  (modulul `icon-packs`); iconițe pe fișiere după nume / extensie (`file-icons`). Lipsește:
  iconițe pentru folderele speciale (`src`, `tests`, `config`, `public`, `vendor`…).

## Etape (fiecare: commit separat, `check.sh` verde, capturile comparate)

- [x] **27a. Gramaticile compilate eliberate (3).** Azi query-urile unei gramatici se compilează
  o dată (`OnceLock`) și rămân până la închiderea aplicației (PHP ~6 MB, Blade ~6,7 MB).
  Devin `Arc<Queries>` ținute de straturile arborilor (`Layer`): cât e deschis un fișier al
  limbajului (sau unul care îl înglobează), query-urile sunt în uz. Intrarea gramaticii le ține
  și ea, cu momentul ultimei folosiri; bucla `module-release` le lasă să plece când doar ea le
  mai ține și au trecut `IDLE` (10 min) — mai puțin cele bifate „La pornire” sau modul
  „Toate la pornire”. Recompilate la nevoie, pe firul `syntax`.
- [x] **27b. Click pe adnotarea blame (5)** → Git Log deschis, cu commit-ul selectat (detaliile
  lui în dreapta).
- [-] **27c. „Show Diagram Popup” (6)**: făcut, apoi scos la cererea utilizatorului (diagrama
  se deschide în tab, ajunge). Rămâne reparația generală: ce decide o vedere la eliberarea
  mouse-ului (click pe un membru → codul lui) se face imediat, nu la următoarea tastă.
- [x] **27d. Iconițe pentru folderele speciale (7)**: reguli pe numele folderului contribuite de
  `file-icons` (ca regulile pe fișiere), cu iconițe colorate din temă: `src`, `app`, `lib`,
  `tests` / `test`, `config`, `public`, `resources` / `assets`, `vendor`, `node_modules`,
  `docs`, `.git`, `.github`, `migrations`, `views` / `templates`, `routes`. Deschis / închis.
- [x] **27e. Resturile de DDD (2)**: `FileSymbols::resolve_class` (`\`, `self` / `static` /
  `parent`, alias din `use`) și euristicile din `graph.rs` (separatorul `\`, `require` /
  `include`, familia de extensii `php`) trec în regulile limbajului (`Naming` / hook-uri în
  `LanguageIntelligence`), cu implicit neutru pentru celelalte limbaje.

  Făcut: `resolve_class(name, at, naming)` (separatorul, `self` / `static` / `parent` din
  `Naming::own_type` / `parent_type`), tipurile parametrilor constructorului prin hook-ul
  `LanguageIntelligence::parameter_types` (parsarea PHP mutată în `lang-php/src/parameters.rs`),
  graful canvas-ului scanat după `SourceRules` date de fiecare limbaj (PHP, JS, CSS / HTML
  simple), sigiliul proprietăților statice din `Naming`.
- [x] **27f. Coada DDD-ului**: `docs.rs` citește numele din `@param` după sigiliul limbajului
  (`Naming::sigil`, `doc_before(text, offset, sigil)`); „Find Usages” caută după `SourceRules`
  ale limbajului (accesul la membri `->` / `.`, cel static `::`, sigiliul, cuvântul care declară
  o funcție) — în JS o proprietate e găsită acum după orice `.`, nu doar după `this.`, iar
  `Type::$prop` e găsit și el; mutarea PSR-4 scrie namespace-urile cu separatorul limbajului
  (`NameSeparator`, dat de `NamespaceRules::separator` și ținut de `NamespaceRoots`). Rămâne
  presupunerea că numele claselor nu țin cont de majuscule la mutare (ca în PHP).
- [x] **27g. Previzualizările jos, în split**: Find Usages, Rename, Move și Change Signature
  rămân taburi, dar se deschid într-un grup de editor sub cel curent (`open_view_below`, 60%
  din înălțime prima dată); următoarele intră în același grup cât e deschis. Codul deschis din
  ele (un click pe un loc) merge în grupul de sus. Formularele se decupează deasupra barei cu
  taste (Move și Change Signature desenau peste ea), lista nu mai iese din zona ei.
