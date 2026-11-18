# 33 — Încrederea în depozite, limbajele scoase din codul generic, fișierele private pe Windows

Trei puncte luate din [TODO.md](TODO.md).

## 33a — încrederea într-un depozit git

Un proiect dintr-o arhivă sau dintr-un clone poate avea un `.git/config` care rulează
programe la comenzi obișnuite (`git status`, `git blame`):
- filtre (`filter.<x>.clean` / `smudge` / `process`);
- `core.fsmonitor`, `core.hooksPath`, `core.sshCommand`, `core.pager`, `core.editor`;
- drivere `diff` / `merge` (`command`, `textconv`, `driver`);
- `credential.helper`, `gpg.program`;
- `include` / `includeIf`, care aduc config din alt fișier.

Soluția:
- **Infra `git`:** `trust::risky(root)` citește `.git/config`-ul depozitului, nu pe al
  utilizatorului, și dă liniile care rulează programe.
  - Un depozit fără așa ceva rulează ca până acum, fără nicio întrebare.
  - Unul cu așa ceva rulează doar după aprobare. Aprobarea se ține în
    `~/.config/<app>/trusted-repositories`, cu amprenta acelor linii, deci un config
    schimbat cere din nou aprobare.
  - Până atunci, orice `git` rulat în el dă `VcsError::Untrusted`.
- **Modulul vcs-git:** la `Untrusted` întreabă o dată, cu liniile găsite. La „da”
  aprobă și pornește din nou.
- Teste: un config cu filtru e refuzat până la aprobare, un config curat trece, iar un
  config schimbat după aprobare e refuzat din nou.

## 33b — fișierele private pe Windows

- `files::write_private`: pe Windows, fișierul primește ACL doar pentru utilizatorul
  curent, fără moștenire (`icacls /inheritance:r /grant:r`). E ce face `0600` pe Unix.
- Îl folosesc seiful, aprobările (remote, depozite) și aprobările quality.
- Nu se poate testa aici; clippy pentru Windows îl compilează.

## 33c — limbajele scoase din codul generic

- **`DocRules`** (opener, closer, atribute, decoratori) vine din limbaj
  (`LanguageIntelligence::doc_rules`), iar `doc_before` îl primește. PHP: `/**`, `*/`,
  `#[`, fără decoratori. JS: `/**`, `*/`, decoratori `@`.
- **`code_tokens`:**
  - cuvintele cheie și sigilul vin de la apelant (`CodeStyle`), adică din limbaj;
  - scope-ul `variable.php` devine `variable.sigil`, iar temele salvate cu numele vechi
    rămân valabile (alias).
- **Hover-ul unui nume CSS:** hook-ul `declaration_code` al limbajului care declară numele.
  - Motorul primește lista inteligențelor, ca hover-ul să ajungă la limbajul fișierului
    declarant.
  - `css: bool` din `rename.rs` devine `SymbolKind::name_chars`.
- **Mai mici:**
  - semnătura implicită din `engine.rs`;
  - separatorii din `fuzzy.rs`, din `Naming` sau scoși dacă nu se folosesc.

## Ordinea

Un commit pentru toate trei, după `check.sh` și capturi.

## Stare

- **33a, gata.**
  - `git::trust`: `risky` / `check` / `trust`, folosite pentru orice folder din depozit și
    pentru `.git`-ul ca fișier (worktree, submodul).
  - `VcsError::Untrusted`.
  - vcs-git întreabă o dată pe deschidere, cu stil de pericol.
- **33b, gata.**
  - `files::write_private` rulează `icacls` pe Windows.
  - `restrict()` a ieșit din seif.
  - Aprobările quality se scriu tot privat.
- **33c, gata.**
  - `DocRules` (`LanguageIntelligence::doc_rules`) înlocuiește `doc_comment_opener`.
  - Hover-ul folosește limbajul fișierului declarant (`Project::languages`,
    `intelligence_for`) pentru docblock, semnătură și `declaration_code`. CSS dă regula
    prin `stylesheet::rule_at`.
  - `rename::name_at` primește caracterele numelui, nu `css: bool`.
  - `code_intelligence::code_style` e un registru de stiluri (cuvinte cheie, sigil,
    comentarii) pe extensie, înregistrat de app din `LanguageIntelligence::code_style`.
    `CodeLine.extension` vine din canvas, comparații și hover.
  - Lăsate, cu motiv:
    - scope-ul `variable.php`: e captura gramaticii PHP, pe care temele o folosesc;
    - separatorii din `fuzzy`: punctuație generică la marginea cuvintelor;
    - `signature_plainly`: rămâne doar rezervă, pentru un limbaj pe care proiectul nu-l
      cunoaște.
