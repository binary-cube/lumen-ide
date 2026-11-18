# Plan 69 — cum comunică modulele: patru mecanisme, cu reguli

Cerut de utilizator (2026-11-08): modulele să nu se cunoască, peste tot unde e nevoie, „să nu
avem o ciorbă în n feluri”. Acesta e planul; **nimic din el nu e implementat încă**.

## Ce există azi (măsurat în cod, fără teste)

| # | Canal | Cât | Problema |
|---|---|---|---|
| 1 | Evenimente (`EventBus`) | 33 de tipuri | Bune. Unele par fără ascultător (`ConsoleRan`, `QualityRun*`, `Indexing*`): de confirmat, căutarea e după text |
| 2 | `ctx.contribute` | peste 60 de tipuri | Bun, e mecanismul principal |
| 3 | Comenzi (`CommandHandler`) | 67 de contribuții | Fără parametri și fără răspuns |
| 4 | `ViewAction` (comandă → vedere) | 14 | Bun, ține de taste |
| 5 | `ViewerAction` | 13 variante | Vederile n-au host, deci cer prin valori de retur |
| 6 | `AppRequest` | 20 de variante, 54 de folosiri | Dublează host-ul: `CopyText`, `Reveal`, `OpenSettings`, `FocusProject` |
| 7 | Metode pe host | 70 | `compare` e și pe host, și în `ViewerAction` |
| 8 | `set_data(&dyn Any)` | 61 de downcast-uri, 24 de module | Datele unei vederi nu sunt verificate la compilare |
| 9 | Registre globale în crate-uri comune | `services::PROVIDERS`, `decorations::PROVIDER`, `snippets::HIGHLIGHTER`, `value_editor::REGISTERED`, `drivers::*`, `env::*`, `calls::CALLS`, `servers::DATA`, `code_style::STYLES` | Ocolesc `contribute` |
| 10 | Tabele statice per proiect în module | din cele 88 de `static` din module, câte sunt de felul ăsta: de triat | Fiecare modul își face propria variantă |
| 11 | `presence` | 14 | Corect, dar separat de cine răspunde |
| 12 | Id-uri text (bloc de setări, fereastră) | câteva | Neverificate |

Exemplul cel mai clar: Relax răspunde la `CalmSceneWanted` cu un steguleț într-o tabelă statică,
o trezire și un `poll`, doar pentru că un eveniment n-are acces la host și nu poate răspunde. În
plus, fără proiect deschis nu merge deloc.

## Ținta: patru mecanisme

| Mecanism | Când | Exemplu |
|---|---|---|
| **Contribuție** (`ctx.contribute`) | „ofer ceva”, cunoscut la pornire; și listele | un limbaj, o scenă calmă, ce conține un limbaj |
| **Eveniment** | „s-a întâmplat”, 0..n ascultători, fără răspuns | `FileSaved` |
| **Cerere** (nou) | „fă / spune-mi”, 0..1 cine răspunde, DTO dus și întors | `CompareFiles`, `ShowCalmScene` |
| **Host** | ce poate doar aplicația (fereastra, clipboard, tab-uri) | `open_view` |

Comenzile rămân pentru ce pornește utilizatorul (meniu, taste). Dacă o comandă are nevoie de
altă funcție a altui modul, trimite o cerere.

### Cererea

```rust
// kernel
pub trait Request: 'static { type Answer: 'static; }

// crates/domain/core/contracts, pe domenii (compare, results, scenes…): doar DTO-uri
pub struct CompareFiles { pub input: CompareInput, pub title: String }
impl Request for CompareFiles { type Answer = (); }

// cine răspunde, la pornire
ctx.contribute(Answers::of(|q: &CompareFiles, host: &mut dyn WorkbenchHost| open(q, host)));

// cine întreabă
match host.ask(CompareFiles { input, title }) {
    Some(()) => {}
    None => open_as_text(host), // n-are cine: compilatorul obligă la tratare
}
host.can_ask::<CompareFiles>() // ascunde butonul dinainte; înlocuiește `presence`
```

Corecție față de ce am propus în discuție: cererea **nu** stă pe magistrală în `kernel` și nu are
`ask_all`.
- Handlerul primește host-ul: răspunde direct, fără steguleț și `poll`, merge și fără proiect și
  atinge doar fereastra care a întrebat.
- Listele sunt contribuții, deci `ask_all` nu e necesar.

Reguli: handlerul e scurt, pe firul UI (ce durează pornește `host.spawn` și anunță prin
eveniment); un singur răspuns per tip (două: eroare la pornire); doar la granița dintre module.

## Ce se mută unde

| Azi | Devine |
|---|---|
| `CalmSceneWanted` + tabela din Relax | `ShowCalmScene { id }` + scene contribuite; Keep Awake le poate lista |
| `host.compare`, `ViewerAction::Compare*`, `diff::COMPARE_VIEW`, `MergeInput` | `CompareFiles`, `MergeFile` |
| Consola → Services (`TOGGLE_SERVICES`, `SERVICES_TOOL`) | `ShowResults`; la `None`, rezultate într-un tab propriu |
| `SetSettingText/Choice { block: "decorations" }` | cereri `SetDecorationLayers`, `SetDecorationPace` |
| `presence::*` | `host.can_ask`; contribuțiile se văd direct |
| `SHOW_RESOURCES`, `OPEN_REMOTE_PROJECT`, `CHOOSE_ENGINE`, `SHOW_TYPE_DIAGRAM`, `FIND_USAGES` | rămân comenzi, ascunse când nu răspunde nimeni |
| Registrele globale (#9) | contribuții, distribuite de aplicație |
| Tabelele statice per proiect (#10) | stare în `ProjectService`, un singur tipar |
| `twig`, `vue`, `blade` în liste | `LanguageDescriptor.embeds`, se întreabă după capacitate |
| Copiile din Blade | crate comun de date (interogările HTML, PHP) |
| `AppRequest`, `ViewerAction` care dublează host-ul | o singură cale (vezi decizia 1) |
| `set_data(&dyn Any)` | rămâne la deschidere; între module, doar DTO-uri din `contracts` |

## Pași (fiecare e un commit, cu `check.sh` verde)

1. `Request`, `Answers`, `host.ask`, `host.can_ask`, crate-ul `contracts`; prima cerere:
   `ShowCalmScene`, care scoate tabela din Relax.
2. **Testul de scoatere:** aplicația pornește cu fiecare modul scos pe rând.
3. Compare pe cereri; Local History și Git au variantă de rezervă.
4. Services pe cereri, cu rezultatele în tab când lipsește.
5. Theme Studio → Decorations.
6. `presence` scos.
7. Registrele globale trecute pe contribuții, câte unul pe commit.
8. Tabelele statice triate, cele per proiect trecute pe tiparul comun.
9. Capacitățile limbajelor; crate-ul comun pentru Blade.
10. Curățarea `ViewerAction` / `AppRequest`.
11. Evenimentele fără ascultător: confirmate, apoi scoase.
12. `check-deps`: refuză id-uri de ferestre și blocuri de setări străine, registre globale noi.
    `CLAUDE.md`: tabelul cu cele patru mecanisme.

## Stare și corecții din implementare

Făcute: pașii 1, 2 și 5 (decizia 1 aplicată: `ViewerAction::Ask`).

- **Non-blocant:** răspunsul rulează pe firul UI; ce durează merge prin `host.spawn`. Un răspuns
  de peste 16 ms e semnalat în log.
- **`presence` rămâne** (pasul 6 se schimbă): vederile n-au host, iar modulele sunt aceleași în tot
  procesul. E singurul răspuns la „e cineva?”, inclusiv pentru cereri (`presence::request`);
  `can_ask` a fost scos, ca să nu fie două.
- **Compare rămâne pe host** (pasul 3 se schimbă): deschiderea unei comparații înseamnă a alege
  tab-ul și grupul, ce poate doar aplicația; contractul (`CompareInput`, `COMPARE_VIEW`) e deja
  neutru. O cerere n-ar aduce nimic. Rămâne varianta de rezervă din Local History.

Făcute apoi: comparațiile citite în fundal (`Deferred`), Local History fără Compare, rezultatele
consolei într-un tab fără Services, pașii 6 (Blade), 8 (`PerProject`), 11, 12.

Rezultate ale analizei, care schimbă planul:

- **Pasul 7 scos.** Registrele (#9) nu ocolesc `contribute`: modulele contribuie, iar aplicația le
  duce acolo de unde le poate citi codul de domeniu, care n-are host. Afirmația din tabel era
  greșită.
- **Pasul 8.** Cele ~170 de `static` din module sunt stare internă, nu comunicare. S-a unificat
  doar tabela „proiect → stare” (`PerProject`: Git, Quality, LSP). Rămân două tipare de privit
  separat, care nu cuplează module: `PENDING` (date duse de la comandă la `ViewerProvider::open`).
  Scoase ulterior (`OpenView::modal`): `PENDING` din Markdown, Preview și Schema. Rămân cele cu
  logică în plus: `sources`, `diagram`, `PENDING_OPEN`, `edit`, `profile`.
- **Pasul 9, capacitățile limbajelor: nefăcut, cu motiv.** Listele sunt în majoritate tipuri de
  fișiere (`svelte`, `xml`, `svg`, `vue`), valabile și fără vreun modul pentru ele. O capacitate pe
  `LanguageDescriptor` nu le-ar acoperi.
- **Pasul 10, `AppRequest`: rămâne.** Vederile n-au host, iar variantele sunt singura lor cale spre
  aplicație; `ViewerAction::Ask` e calea spre alte module. Nu sunt două feluri pentru același lucru.

## Decizii

1. **Vederile n-au host.** Recomandat: `ViewerAction` rămâne un set mic (`Close`, `Open*`) plus o
   variantă `Ask(cerere)`; ce dublează host-ul dispare. Alternativa, un `FnOnce(&mut host)` în loc
   de tot enum-ul, e mai simplă, dar acțiunile n-ar mai putea fi comparate în teste (zeci de teste
   de rescris).
2. **Ordinea.** Recomandat: pașii 1–6 întâi, apoi o verificare; 7–12 sunt curățare, fără efect
   vizibil pentru utilizator.

## Riscuri

- Un tip de cerere nou pentru fiecare interacțiune: `contracts` trebuie ținut pe domenii.
- O cerere e mai greu de urmărit decât un apel direct: numai la granițe.
- Pașii 7, 8 și 10 ating multe fișiere; se fac separat, ca să poată fi anulați.
