# 30 — Atributele PHP (`#[AsCommand(...)]`)

Atributele PHP 8 trebuie tratate ca ce sunt: o clasă instanțiată prin constructorul ei.
Asta înseamnă:
- go to definition;
- hover cu constructorul;
- completare;
- argumente cu nume;
- rename și utilizări;
- colorare.

Totul stă în modulul `lang-php`, plus un hook generic în domeniu.

## Ce merge azi și ce nu

- **Merge:**
  - `#[` nu e luat drept comentariu (`context.rs:77`);
  - `#[AsCommand]` fără paranteze duce la clasă;
  - căutarea utilizărilor găsește atributul;
  - importul automat există pentru clase.
- **Nu merge** când atributul are argumente, `#[AsCommand(`:
  - `target()` (`context.rs:246`) vede un nume urmat de `(` și îl ia drept apel de funcție;
  - de aceea cad go to definition, hover, signature help, rename și diagrama de
    dependențe.
- **Completarea** după `#[` oferă tot: funcții, constante, cuvinte cheie. Nu oferă doar
  clase.
- **Argumentele cu nume** (`name:`, `description:`) nu sunt oferite nicăieri, nici la
  apeluri obișnuite.
- **Docblock-ul** de deasupra unui atribut pe mai multe linii se pierde: `doc_before` sare
  doar peste o linie care începe cu `#[`.
- **Colorarea:** query-ul tree-sitter standard nu colorează numele atributului ca tip.
- **Move Class** (`namespaces.rs:629`) pierde al doilea atribut dintr-o listă
  (`#[A, B(...)]`).

## Pași

**30a — atributul e o clasă instanțiată (lang-php, plus un hook în domeniu)**
- În `context.rs`, un helper `in_attribute_name(text, start)` recunoaște numele de după
  `#[` sau de după `,` la nivelul listei de atribute, peste spații, linii noi și nume
  calificate (`ORM\Column`). Îl folosesc în două locuri:
  - `target()` dă `Target::Class` chiar dacă urmează `(`;
  - `context()` dă `ContextKind::TypeName`, deci completarea oferă doar clase, cu `use`
    adăugat automat.
- Un hook nou, `LanguageIntelligence::constructs_at(text, word_start)`: implicit îl citește
  din `Naming::construct` (`new`), iar PHP adaugă cazul atributelor. Îl folosește
  `describe`, așa că hover-ul și signature help-ul arată constructorul, adică parametrii
  `name`, `description`…
- `source_rules.before_type` primește `#[`, ca diagrama de dependențe să vadă o utilizare de
  tip, nu un apel.
- `namespaces.rs` `Parser::name` păstrează și atributele de după `,`.
- Teste: definiție, hover, completare, rename pe `#[AsCommand(` (pe o linie și pe mai multe)
  și pe `#[ORM\Column(...)]` cu alias.

**30b — argumente cu nume (domeniu generic, PHP îl folosește)**
- În lista de argumente a unui apel, a unui `new X(` sau a unui atribut, completarea
  oferă parametrii constructorului sau ai funcției, ca `name:` și `description:`.
- Parametrii deja scriși nu mai sunt oferiți.
- Ordinea e cea din semnătură, cu tipul ca detaliu.
- Signature help: parametrul activ e găsit după nume când argumentul are nume, nu doar după
  poziție.
- Hook-ul `LanguageIntelligence::named_arguments() -> Option<&str>`: PHP dă `":"`, iar
  limbajele fără argumente cu nume dau `None`.
- Teste: atribut pe mai multe linii, apel de funcție, `new`.

**30c — docblock și colorare**
- `doc_before` sare peste atributele pe mai multe linii, de la `#[` până la `]`-ul lor, ca
  hover-ul clasei să-și găsească docblock-ul.
- Un query `lang-php/queries/highlights.scm`, adăugat după cel standard, colorează:
  - numele atributului ca tip;
  - `#[` și `]` ca punctuație;
  - numele argumentelor (`name:`) ca parametru.

  Folosesc doar capturile pe care le cunoaște `syntax-treesitter`.
- Capturile `code` și `editor` cu un atribut pe mai multe linii, comparate în ambele teme.

**30d — opțional: atributele cunoscute de framework**
- `Symbol.attributes`: atributele declarației, ca FQN-uri rezolvate, extrase în
  `extract.rs`, ca să le poată folosi modulele de framework.
- În fw-symfony, comenzile consolei din `#[AsCommand(name: ...)]`, după modelul lui
  `#[Route]`: numele comenzii în Structure și go to definition din `bin/console app:…`.
- Îl fac doar dacă vrei; nu e necesar pentru 30a–30c.

## Ordinea

30a → 30b → 30c, câte un commit pe etapă, fiecare după `check.sh` și capturi (30c le
schimbă intenționat). 30d după, la cerere.

## Stare

- **30a, gata.**
  - `in_attribute_name` recunoaște numele unui atribut: după `#[`, după `,` într-o listă,
    nume calificate, pe mai multe linii.
  - `target()` dă clasa, iar `context()` dă `TypeName`.
  - Hook-ul `LanguageIntelligence::constructs_at` face ca hover-ul și informațiile despre
    parametri să arate constructorul.
  - `#[` e în `before_type`.
  - Move Class păstrează toate atributele dintr-o listă și nu mai ia numele argumentelor
    drept clase.
- **30b, gata.**
  - `LanguageIntelligence::named_argument` (PHP: `:`).
  - `signature::named_arguments` oferă parametrii încă nescriși, în ordinea din semnătură,
    deasupra celorlalte sugestii.
  - Informațiile despre parametri urmează argumentul cu nume.
- **30c, gata.**
  - `doc_before` sare peste atributele pe mai multe linii.
  - `lang-php/queries/highlights.scm`, pus înaintea query-ului standard, colorează numele
    atributului ca `type` și numele argumentului ca `variable.parameter`.
- **30d, gata.**
  - fw-symfony adună comenzile din `#[AsCommand]`: numele și descrierea, cu nume sau pe
    poziție.
  - În `->find('…')` și `'command' => '…'` oferă completare (cu descrierea), go to
    definition și hover.
- **Rămășițe, gata și ele:**
  - `Symbol.note` („ce e scris pe declarație”, salvat în indexul de pe disc, formatul 8):
    PHP îl umple din atributele claselor, metodelor și funcțiilor, de exemplu
    `AsCommand('app:auth:flush') Final`. Panoul Structure îl afișează estompat după nume, în
    spațiul rămas.
  - fw-symfony recunoaște comanda de după `console ` pe linie (`bin/console app:x` în
    scripturile din `composer.json`, în shell, în Makefile): completare, go to definition,
    hover.
  - lang-blade colorează și atributele și argumentele cu nume în stratul său PHP, cu
    query-ul din lang-php.
