# 75 — Comentariile: navigarea doar unde e scris cod

**Stare (2026-11-11): 75a–75e făcute; 75f–75h lăsate cu motiv (mai jos); pe branch-ul `index-members-on-demand`.**

Cerut 2026-11-10 („să acoperim tot ce se poate”; lista completă de tag-uri pe limbaje e în
`TODO.md`). Azi, într-un comentariu `/** … */`, un cuvânt obișnuit („runtime”) e luat drept
nume de clasă: hover-ul și ⌘ + click duc la `PHPUnit\Event\Runtime`.

## Ce avem (verificat în cod)

- Definiția, hover-ul și sublinierea de la ⌘ trec toate prin `code_intelligence::definition`
  (`engine.rs`), care întreabă limbajul `target(text, offset)`. PHP-ul (`context::target`)
  ia orice cuvânt drept `Target::Class`, iar la o clasă negăsită `by_short_name` caută după
  numele scurt: de aici „runtime” → `Runtime`.
- Hover-ul cade apoi pe `typed_hover` și `hover` (cele încorporate): un cuvânt ca „count”
  dintr-un comentariu poate primi cardul funcției `count()`.
- Adresele și căile din comentarii sunt deja navigabile, separat (`links.rs`, ale tuturor
  limbajelor): rămân cum sunt.
- PHP-ul citește deja PHPDoc-ul (`phpdoc::tags`, `types::split_doc_type`, `types::parse`).

## Regula

Fiecare modul de limbaj spune ce e un loc dintr-un comentariu al lui (`LanguageIntelligence::
comment_place`), într-o formă comună (`CommentPlace`):

- `Text`: text — fără subliniere, fără card, fără definiție;
- `Code(Target)`: un nume scris ca în cod (un tip, un membru, o funcție), rezolvat ca în cod;
- `Declared(offset)`: un nume declarat de comentariu sau de declarația lui (`T` din
  `@template T`, un alias `@phpstan-type`, parametrul unui `@param $x`).

`None`: nu e într-un comentariu (sau limbajul nu spune): ca azi. Motorul doar întreabă; nu
știe de niciun tag.

## Pași

1. **75a. Contractul și PHP-ul.** `CommentPlace` și `comment_place` în `code-intelligence`;
   `definition` și `hover` îl întreabă întâi. PHP: comentariile `//`, `#`, `/* */` sunt text;
   în `/** */`, tipurile după tag-urile cu tip (`@param`, `@return`, `@var`, `@throws`,
   `@property*`, `@method`, `@mixin`, `@extends` / `@implements` / `@use`, `@template … of`,
   variantele `@phpstan-*` / `@psalm-*`, aserțiunile, `@phpstan-type`), fiecare nume din forma
   tipului (generice, uniuni, `array{cheie: T}` — cheile sunt text —, `callable(T): U`,
   `Foo::CONST`), tipurile încorporate și pseudo-tipurile (`int`, `class-string`…) text;
   `T` / aliasurile la declararea lor; `$x` din `@param` la parametrul funcției;
   `@see` / `@uses` / `@used-by` / `@covers` / `@link` și `{@see …}` / `{@link …}` (`Clasa`,
   `Clasa::metoda()`, `Clasa::$prop`, `Clasa::CONST`, `funcție()`); `@dataProvider` /
   `@depends` la metoda clasei; adnotările Doctrine / Symfony (`@ORM\Entity`) la clasa lor.
   Teste pe fiecare formă.
   **Făcut.** `CommentPlace` / `comment_place` (`code-intelligence`), întrebat de
   `definition` și `hover` (în comentariu: fără tipul unei variabile, doar ce are limbajul
   încorporat); `php/src/comment_places.rs`, citit din textul de dinaintea locului (o căutare
   înapoi după `/*`, care nu e într-un șir al liniei lui). Teste: fiecare formă
   (`tests/unit/comment_places.rs`) și, prin motor, „runtime” din text fără definiție și
   fără card, `@param Runtime` cu ele, `$runtime` din `@param` la parametrul funcției.
2. **75b. PHP, restul:** `@inheritDoc` la părinte; `self` / `static` / `$this` la clasa
   curentă; `@covers ::metoda` cu `@coversDefaultClass`; `@phpstan-import-type` la aliasul
   din clasa de unde vine; argumentele adnotărilor Doctrine (`targetEntity=User::class`).
   **Făcut**, în afară de `@phpstan-import-type`: numele importat duce la declararea lui din
   comentariul de aici, clasa (`from Clasa`) la clasă; aliasul din clasa aceea ar cere o
   căutare nouă în index (amânat, până e cerut). `self` / `static` / `$this` / `parent` în
   corpul clasei sunt rezolvate de motor (clasa din jur); pe comentariul clasei, numele ei
   sau cel pe care îl extinde.
3. **75c. Rename și Find Usages prin comentarii (PHP):** o clasă folosită doar într-un
   comentariu e tot o folosire; redenumirea ei schimbă și comentariile (doar locurile `Code`).
   **Făcut.** Rename-ul întreabă deja `definition` pentru fiecare loc: un tip scris într-un
   tag e sigur (bifat), un cuvânt din text cu același nume e arătat, nebifat (alegerea
   utilizatorului). „Find Usages” sare peste textul comentariilor: `SourceRules.comments`
   (`CommentReader`, funcția limbajului), întrebat doar pentru potrivirile găsite. Teste:
   `a_class_renamed_in_the_comments_that_write_it_as_code` (app), `a_comment_s_text_is_no_usage`.
4. **75d. JavaScript / TypeScript (JSDoc, TSDoc):** tag-urile și formele din `TODO.md`.
   **Făcut.** Părțile comune ale limbajelor în `code_intelligence::comments` (comentariul
   `/* */` din jurul unui loc, linia fără margine, cuvântul din jur), folosite de PHP și JS.
   JS (`javascript/src/comment_places.rs`): `//` și `/* */` text; în `/** */`, numele din
   `{tipuri}` după tag-urile cu tip (`@param`, `@returns`, `@type`, `@typedef`, `@property`,
   `@throws`, `@this`, `@enum`, `@extends` / `@augments` / `@implements`, `@template {B} T`,
   `@satisfies`…), cu formele lor (`?T`, `T=`, `...T`, `T[]`, generice, `{a: T}` — cheile
   text —, `function(T): U`, `import('./x').T`); `@typedef` / `@callback` / `@template` la
   declararea lor; `@param name` / `[name=implicit]` / `name.prop` la parametrul funcției;
   `@see` și `{@link}` / `{@linkcode}` / `{@linkplain}` (`Nume`, `Nume#membru`,
   `Nume.static`; adresele și `module:` text). „Find Usages” îl folosește la fel.
5. **75e. SCSS (SassDoc) și CSS** (`sourceMappingURL` e deja o legătură).
   **Făcut.** CSS: `/* */` text (azi `/* .btn */` ducea la clasă); SCSS: `//` text, iar
   SassDoc (`///`) `@see` / `@require` duc la mixin / funcție sau la `$variabilă`,
   `@param {Tip} $x` la parametrul mixin-ului de după comentariu, restul text. HTML:
   `<!-- … -->` e text (ce e comentat nu duce nicăieri). „Find Usages” le citește la fel.
6. **75f. Twig și Blade:** `{# @var nume \App\Clasa #}`, PHPDoc-ul din `@php … @endphp` și
   `{{-- @var … --}}`.
   **Verificat, lăsat:** în Twig nu duce azi nimic dintr-un comentariu (Twig n-are motor de
   limbaj, doar completarea modulului), deci greșeala nu există acolo; `@php /** @var … */
   @endphp` e o regiune PHP: merge prin 75a. Ca `{# @var #}` / `{{-- @var --}}` să ducă la
   clasă, modulul Twig / Blade ar cere locul unei clase din indexul PHP — o cerere nouă
   între module (`contracts`). De făcut dacă e cerut.
   **Verificat 2026-11-12, de hotărât cum:**
   - o asistență (`DocumentAssist`) nu ajunge azi nici la cereri (`host.ask` cere
     `WorkbenchHost`), nici la indexul de simboluri;
   - `code-intelligence` (unde e `AssistContext`) nu poate depinde de `workbench-model`.

   Variante:
   - **(a)** `AssistContext` primește indexul de simboluri al proiectului (doar citire),
     ca asistențele să caute o clasă după nume. E simplu, dar o asistență ar ști de
     simboluri.
   - **(b)** o asistență a modulului PHP, înregistrată și pentru `twig` / `blade`, care
     citește doar comentariile `@var` și caută în index. N-ar fi o cerere între module,
     dar cere tot indexul (ca la a).
   **Făcut 2026-11-12 (varianta b, fără să dea indexul asistenței):** asistența modulului
   PHP pentru `twig` / `blade` (`php/src/template_vars.rs`) citește comentariul din jurul
   cursorului (`{# … #}`, `{{-- … --}}`, o căutare înapoi de cel mult 4 KB) și spune doar
   ce nume PHP e acolo: `AssistTarget::Code { language: "php", target }`. Motorul îl caută
   în index ca pe codul PHP (`definition_of`, `hover_of`, despărțite din `definition`).
   Ambele ordini: `@var user \App\User` (Twig) și `@var \App\User $user` (Blade, PHPDoc);
   tipul citit ca în PHPDoc (`User[]`, `User|null`, scalarii text). ⌘-click, sublinierea și
   cardul de hover merg. Teste: formele, și prin modulele reale clasa găsită din Twig și Blade.
7. **75g. YAML:** linia de schemă (`# yaml-language-server: $schema=…`).
   **Verificat:** o schemă dată prin adresă e deja o legătură (`links.rs`).
   **Făcut 2026-11-12:** și calea fără ghilimele (`$schema=./schema.json`) e o legătură,
   dacă fișierul există în proiect (`links::schema`, cu test).
8. **75h. Colorarea** tag-urilor și tipurilor din comentarii ca în cod (unde nu e deja).
   **Făcut 2026-11-12:** un contract nou al sintaxei, `syntax_ts::DocComments` (contribuit
   ca `FoldProvider`): limbajul spune unde sunt într-un comentariu doc tag-urile, tipul de
   după un tag cu tip și numele documentat; colorarea le pune peste culoarea comentariului,
   ca scope-uri ale lor (`comment.doc.tag`, `.type`, `.variable`), doar când se refac
   culorile (o versiune, o parsare, o temă). O temă care nu le numește le ia din culorile
   codului: tag-ul în culoarea comentariului, bold; tipul ca tipurile; numele ca parametrii;
   toate italice ca și comentariul (`ResolvedTheme::doc_syntax`). Împărțirea e una
   (`code_intelligence::comments::doc_parts`); PHPDoc (tipul citit ca la navigare,
   `split_doc_type`; doar `$x` e un nume), JSDoc / TSDoc (`{tip}`), SassDoc (`///`) dau doar
   lista lor de tag-uri. Un comentariu simplu (`//`, `/* */`) rămâne dintr-o culoare.
   Verificat pe ecran: PHP și JS, pe `default-dark` și `default-light`. Teste: împărțirea,
   tema, și prin modulele reale scope-urile din PHP, JS și SCSS.

**Hotărât (utilizatorul, 2026-11-12):**
- `@var` în Twig / Blade: **varianta (b)**, o asistență a modulului PHP pentru `twig` /
  `blade`, care citește doar comentariile `@var`;
- **codul dintre backtick-uri** din comentarii duce la ce numește (o clasă, o funcție),
  ca în cod;
  **Făcut 2026-11-12:** în orice comentariu PHP și JS/TS (`//`, `#`, `/* */`, `/** */`),
  codul dintre backtick-uri (și între mai multe, ``` ``a `b` c`` ```) e cod: citit ca o
  referință `@see` (`Clasa`, `Clasa::metoda()` / `Clasa#metoda`, `funcție()` cu
  argumentele lăsate deoparte, `new Clasa`); o variabilă (`$x`, `this.x`) rămâne text.
  Părțile comune în `code_intelligence::comments` (`backticked`, `code_word`). Rename-ul și
  „Find Usages” le văd ca pe codul din tag-uri (aceeași regulă).
- **completarea în comentarii** și pentru celelalte limbaje (ca la PHP: tag-urile după
  `@`).
  **Făcut 2026-11-12:** după `@` la începutul textului unei linii de comentariu-doc (sau
  după `{`, `{@link`), tag-urile limbajului, cu descrierea lor (ro / en) și un spațiu după
  cele urmate de ceva: JSDoc / TSDoc în `/** */` la JS, TS, TSX (34 de tag-uri), SassDoc în
  `///` la SCSS (20). Un `@` dintr-un cuvânt (o adresă) nu deschide nimic. Părțile comune:
  `comments::tag_at`, `comments::tag_items`. **Lăsat:** CSS n-are tag-uri de documentație
  folosite. **Tipurile din `{…}` la JSDoc — făcut tot 2026-11-12:** în acoladele unui tag
  (`@param {string|Us…}`), partea scrisă după `|`, `<`, `,`… se completează cu tipurile
  proiectului (din index) și cu cele ale JSDoc / TypeScript (`string`, `Promise`…); `{@link`
  și acoladele din text nu.

## Reguli

- Nimic pe firul UI în plus: locul se află din textul comentariului (o căutare înapoi după
  `/*`, linia lui), fără parsare.
- Ce nu e cunoscut rămâne ca azi (`None`): un limbaj fără `comment_place` nu se schimbă.
- Un commit pe pas, `./scripts/check.sh` verde.
