# Plan 65 — Modulele contribuie reguli la motorul de limbaj (început cu Symfony)

## Context (ce există)
- `crates/domain/code/intelligence/src/framework.rs`: `Framework` (`detect`, `activate` per proiect,
  familii exclusive) și `FrameworkSupport` (`complete`, `definition`, `hover`, toate implicit
  „nimic”), cu `FrameworkContext { generation, structure }` — cache-uri pe generația globală a
  proiectului (orice salvare le invalidează pe toate).
- Symfony (`crates/modules/frameworks/symfony`): rute (`#[Route]`, `config/routes*.yaml`) cu
  completare / definiție / hover; contribuții `.env` (`EnvSections`, `EnvReads`, `EnvFileOrder`).
- Planul 61 a adăugat reguli declarate ca date (`code_intelligence::env`), `CursorSymbols`,
  `AssistTarget::Choose`, `FoldProvider`, `Concealment`.
- Ideea, notată în `TODO.md`: motorul știe baza limbajului, fiecare modul vine cu regulile lui;
  modulele nu pornesc servere, doar își înregistrează regulile la motorul care rulează deja;
  versionare per modul, invalidare doar a ce depinde de el.

## Ce facem
- **65a — contractul extins și versionarea.**
  - `FrameworkSupport` primește (implicit „nimic”): `references` (Find Usages pentru textele
    magice), `rename` (editările), `diagnostics` (pe document, cu severitate), `types` (tipul
    unei expresii pe care framework-ul îl știe — containerul DI), `symbols` (pentru Structure /
    Go to Symbol), `actions` (acțiuni de context).
  - **Versionarea:** fiecare framework activ își ține un `FrameworkModel` construit în fundal
    (indexul lui: rute, servicii, parametri…) cu o **generație proprie** și lista fișierelor din
    care e făcut (tipare: `config/**`, `src/**/*.php` cu atribute, `var/cache/*/Container*.xml`);
    o schimbare pe disc (bus-ul proiectului, comasat cu `kernel::queue`) într-un fișier al lui
    reconstruiește incremental doar partea afectată și crește generația; rezultatele motorului
    (completări, diagnostice) memorează generațiile de care depind și se invalidează doar când
    una se schimbă. Un framework oprit își retrage modelul.
  - **Bugetul:** fiecare contribuție rulează pe firul de simboluri cu o limită (ex. 20 ms); peste
    limită e ignorată pentru cererea aceea și numărată (vizibilă în Task Manager).
  - **Combinarea:** motorul de bază primul, framework-urile adaugă (deduplicat după locație /
    etichetă); `merge` ca azi la completare; la definiție — framework-ul primul doar pentru textele
    lui (rută, serviciu), motorul în rest.
  - Aplicația rutează noile cereri într-un singur loc (unde se combină azi serverele / motorul).
- **65a — făcut așa** (2026-11-07):
  - Find Usages / Rename pentru numele unui framework nu au un contract al lor: trec prin
    `RenameContext`, ca la `.env` (`CursorSymbols`), deci previzualizarea, aplicarea și
    undo-ul sunt ale oricărei redenumiri. Framework-ul spune doar, pur:
    - `name_at`: numele de la cursor;
    - `names_in`: fișierele care îl pot scrie;
    - `name_places`: unde îl scrie un fișier;
    - `name_taken`: unde e declarat deja (conflictul).
    Adaptorul e în `workbench_model::framework_names`, iar aplicația îl întreabă după
    modulele cu `CursorSymbols`.
  - Diagnosticele (`diagnostics`) se calculează în fundal, pentru documentul activ, la
    fiecare schimbare a lui sau a modelului unui framework, și se anunță pe bus-ul
    proiectului (`DiagnosticsFound`). Le arată ce arată deja diagnosticele modulelor:
    sublinieri, bara de derulare, Structure, bara de jos.
  - Versionarea:
    - `watches` dă globurile fișierelor din care e făcut modelul
      (`kernel::paths::glob_match`, mutat din `workspace`);
    - `FrameworkContext::own` este generația proprie: crește doar pentru acele fișiere;
    - `prepare` face modelul în fundal (firul `frameworks`), câte unul pe rând, cu
      schimbările de între timp comasate.
    Symfony le folosește: rutele și comenzile nu se mai refac la orice salvare.
  - Bugetul: fiecare răspuns pe UI thread e cronometrat. Peste 20 ms e numărat și apare în
    Task Manager („Framework-uri”, cu răspunsurile lente ale fiecăruia).
  - `types` vine cu serviciile (65b), primul care îl folosește. `symbols` și `actions`
    rămân până le cere un framework (fără folositor, ar fi cod mort).
- **65b — Symfony: rute și servicii.**
  - Rute: Find Usages / Rename pentru numele rutei (`path('x')`, `url('x')` în Twig,
    `redirectToRoute('x')`, `generateUrl('x')`, `#[Route(name: 'x')]`, YAML), diagnostic „rută
    inexistentă”.
  - Servicii: modelul din `config/services.yaml` + autowiring (clasele din `src/`), sau containerul
    compilat (`var/cache/dev/App_KernelDevDebugContainer.xml`) când există — mai precis; completare
    și definiție pentru `@service` în YAML, `service('id')`, id-uri în `$container->get('id')`,
    tipul deductibil (`$this->container->get(Foo::class)` → `Foo`) contribuit motorului PHP prin
    `types`; diagnostic „serviciu inexistent”.
- **65b — făcut așa** (2026-11-07):
  - Rute:
    - Find Usages / Rename prin contractul din 65a, de la o folosire sau de la declarație.
      Folosirile sunt `path()`, `url()`, `redirectToRoute()`, `generateUrl()`,
      `->generate()`, `logout_path()`, în PHP și în Twig. Declarațiile sunt
      `#[Route(name:)]` și cheile din `config/routes*.yaml`.
    - O rută numită prin prefixul clasei (`#[Route(name: 'blog_')]` pe clasă) nu se
      redenumește. Numele ei întreg nu e scris nicăieri, deci declarația ar rămâne
      neschimbată.
    - Diagnosticul „rută inexistentă” apare doar când proiectul are rute cunoscute.
  - Servicii:
    - Modelul vine din containerul compilat (`var/cache/dev/*Container.xml`), cu toate
      serviciile, inclusiv ale bundle-urilor. Altfel vine din `config/services.yaml`: id-urile
      lui și clasele încărcate din `src/` (`App\: resource:`, autowiring).
    - Completare, definiție (la fișierul clasei, când e a proiectului) și hover (clasa)
      pentru `'@id'`, `service('id')`, `#[Autowire(service: 'id')]` și
      `$container->get('id')`.
    - Diagnosticul „serviciu inexistent” apare doar cu containerul compilat: fără el,
      serviciile bundle-urilor nu sunt știute.
  - Tipuri:
    - Ca date, nu ca apel per proiect. `code_intelligence::calls::ClassArgumentCall` spune
      că un apel `get(X::class)` pe `Psr\Container\ContainerInterface` sau pe
      `ContainerInterface` din Symfony dă `X`.
    - Modulul o contribuie, aplicația o înregistrează, iar inferența PHP o citește la
      fiecare apel. Merge și pentru clasele care extind containerul.
    - `$container->get('id')` → clasa serviciului: făcut (2026-11-08). Regula își numește
      tabela (`ClassArgumentCall::names`, `symfony.services`). Framework-ul își pune numele în
      indexul proiectului (`FrameworkSupport::named_types`, după `prepare`, în fundal;
      `SymbolIndex::set_named`), iar inferența le caută acolo. Tabelele rămân la o
      reindexare (`replace_symbols`) și pleacă odată cu proiectul.
  - Containerul compilat nu se mai recitește la orice salvare (2026-11-08): un catalog se
    reface doar când fișierele lui se schimbă (`catalog::Print`: căile și datele lor, plus
    lista fișierelor proiectului).
- **65c — Symfony: parametri, `%env()%`, traduceri.**
  - `%param%` în YAML / `#[Autowire('%param%')]`: completare, definiție, hover cu valoarea.
  - `%env(X)%`: deja prin `.env` — mutat pe noul contract (aceeași logică, fără cod dublu).
  - Traduceri: `trans('key')` / `|trans` în Twig ↔ `translations/*.yaml|xlf`: completare,
    definiție, hover cu textul, diagnostic „cheie lipsă”.
- **65c — făcut așa** (2026-11-07):
  - Serviciile, parametrii și cheile de traducere au aceeași formă: un `Catalog` de nume
    declarate (`catalog.rs`). Completarea, definiția, hover-ul și diagnosticele sunt scrise
    o singură dată pentru toate trei. Fiecare fel spune doar cum se adună și cum îl numește
    codul.
  - Parametri:
    - Din containerul compilat (`<parameter key>`, toți), altfel din `parameters:` în
      `config/services.yaml` și `config/packages/*.yaml`.
    - Numiți ca `%name%` (fără `%%` și `%env()%`), `#[Autowire(param:)]` și
      `getParameter()`.
    - Hover-ul arată valoarea. „Parametru inexistent” apare doar cu containerul compilat.
  - Traduceri:
    - Din `translations/<domeniu>.<limbă>.yaml|xlf` (cheile YAML imbricate, unite cu `.`;
      XLIFF 1.2 și 2.0).
    - Numite de `trans()`, `->trans()`, `t()` și `|trans` din Twig.
    - Hover-ul arată textul în fiecare limbă. „Traducere lipsă” apare doar la cheile scrise
      ca chei: o propoziție este propriul ei text.
  - `%env()%` rămâne cum e: Symfony contribuie deja ca date la modulul `.env`
    (`EnvReads`). Un alt contract ar fi dublat logica.
  - Un cititor YAML mic, comun (`yaml.rs`), pentru parametri și traduceri.
- **65d — măsurat și testat:** probe noi în `tools/lsp-compare` pentru Symfony (pe un proiect
  Symfony real creat în scratchpad cu Composer), cu și fără modul; latențe; invalidarea (o rută
  adăugată apare fără reindexarea proiectului).

- **65d — făcut așa** (2026-11-07):
  - 13 sonde noi în `tools/lsp-compare`, pe scheletul Symfony (`run.sh` îi adaugă
    `symfony/translation`, o traducere și un fișier cu nume necunoscute, apoi compilează
    containerul cu `cache:warmup`). Ele acoperă:
    - completarea rutelor, serviciilor, parametrilor și traducerilor;
    - tipul lui `get(RequestStack::class)->`;
    - definiția, Find Usages, Rename și hover-ul unei rute;
    - rută, serviciu și traducere lipsă.
  - Rezultat: 133 / 133 (motorul nostru).
  - Latențe pe UI thread, de la al doilea apel:
    - completare: 0,04 ms p50 / 0,45 ms p95;
    - Find Usages: 0,26 / 0,38 ms;
    - Rename: 0,20 / 0,46 ms.
  - Prima rulare a găsit o problemă: un nume încă gol (`get('|')`) nu se completa. Acum
    se completează, dar nu e verificat ca folosire.
  - Invalidarea are un test în aplicație. O rută adăugată apare imediat la completare, iar
    generația Symfony crește doar pentru fișierele lui, nu pentru un `.css`, fără să se
    reindexeze proiectul.

## Reguli
Domeniul pur (contracte, versionare, combinare — testate unitar), modulele fără dependențe între
ele, aplicația doar leagă; nimic per cadru; fără servere noi; ro / en; Windows.

## Stare
- [x] 65a
- [x] 65b
- [x] 65c
- [x] 65d
