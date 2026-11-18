# Plan 64 — Proiecte remote: proprietăți și drepturi, ca în WinSCP

## Context
Proiectele remote (`crates/modules/project/remote`, transporturile `crates/infra/remote/remote-sftp`,
`crates/infra/remote/remote-ftp`) nu arată azi drepturile, owner-ul sau grupul fișierelor de pe server
și nu le pot schimba. Referința: dialogul „Properties” din WinSCP (tab-urile Common / Checksum).

## Ce facem
- **64a — citirea și modalul.**
  - Portul domeniului (`remote`): `Metadata { size, modified, permissions: u32 (mode), owner:
    Option<Principal { name, id }>, group: Option<Principal> }` și
    `capabilities { chmod, chown, chgrp, checksum: Vec<Algo>, server_hash }` pe transport.
    SFTP: `stat` / `lstat` (mode, uid, gid), numele din `ls -ln` / `getent` dacă e shell, altfel
    doar id-urile; FTP: `MLST` / `MLSD` (`unix.mode`, `unix.owner`, `unix.group`, `perm`) sau
    `LIST` parsat (`drwxr-xr-x user group`).
  - Click dreapta pe un fișier / folder (arborele proiectului remote, „De urcat”, fereastra Remote)
    → **Proprietăți…**: modal (al modulului, `host.open_modal`), tab-urile **General** și
    **Checksum**. General: iconița și numele, locația pe server, mărimea (la foldere
    „Necunoscută” + **Calculează**, în fundal, anulabil, în Task Manager), data modificării,
    **Owner** / **Group** (liste cu `nume [id]`), grila **Owner / Group / Others × R W X** +
    **Set UID / Set GID / Sticky**, câmpul **Octal** (`0755`) sincronizat în ambele sensuri cu
    bifele (validat: 3–4 cifre octale).
  - Selecție multiplă: valorile comune afișate, celelalte „mixte” (bifă cu liniuță), numărul de
    elemente în titlu.
- **64b — aplicarea.**
  - SFTP: `setstat` (mode), `chown` / `chgrp` (dacă serverul permite: altfel câmpurile dezactivate,
    cu explicația); FTP: `SITE CHMOD` dacă e acceptat (testat o dată per server, ținut minte),
    owner / grup doar afișate.
  - **Aplică recursiv** (cu „Adaugă X la foldere” — X doar pe foldere, ca în WinSCP): numărul de
    elemente afișat, confirmare, rulat în fundal pe pool-ul prudent, ca operație în Task Manager și
    în „ce rulează”, anulabil; erorile pe rând, într-o listă la final.
  - OK aplică doar ce s-a schimbat; dezactivat dacă nimic nu s-a schimbat.
- **64c — Checksum.**
  - Algoritmul (MD5 / SHA-1 / SHA-256 / SHA-512); calculat pe server dacă se poate (SFTP extensia
    `check-file`, sau `md5sum` / `sha256sum` printr-un canal exec dacă serverul dă shell; FTP
    `HASH` / `XMD5` / `XSHA1` / `XSHA256`), altfel descărcat în fundal și calculat local;
    comparat cu copia locală (la fel / diferit), copiere în clipboard.
- **64d — vizibil în arbore (opțional, setare):** o coloană / un tooltip cu drepturile și
  owner-ul pe rândurile remote; avertisment când un fișier urcat nu e citibil de grup / ceilalți
  (frecvent la site-uri web).

## Reguli
Modul remote, ui-components, domeniu pur (modelul de drepturi, octal ↔ bife, „mixte”, recursiv —
testate unitar); transporturi testate pe servere false și containere proprii `editor-test-*`
(SFTP + FTP cu `MLSD` / fără); niciodată serverele sau containerele utilizatorului; ro / en;
Windows.

## Stare
- [x] 64a — făcut (2026-11-06). Modelul în `remote::properties` (domeniu pur, testat); portul
  `properties` / `accounts` / `capabilities` / `measure`. SFTP: `stat` + numele din long names
  (un canal SFTP brut, o listare per folder) + `/etc/passwd` / `/etc/group` dacă se pot citi;
  FTP: `MLST` pe conexiunea de control, altfel un singur `LIST` per folder. Testat pe
  `editor-test-sftp` (OpenSSH), `editor-test-pureftpd` (MLST), `editor-test-vsftpd` (LIST).
  Modalul are tab-ul General; tab-ul Checksum vine cu 64c, butoanele OK / Anulează (aplicarea)
  cu 64b — până atunci „Închide”. Componente comune noi în ui-components: `picker::Picker`
  (lista de ales cu căutare, avatar, detaliu, bifă, ↵, derulare, tastele jos — în stratul ei,
  după temă), `ds::dropdown` (câmpul; lista simplă mutată din sursele de date, acum în stratul
  ei și încăpând unde e loc), `ds::initials`, `tag_colors::for_key`; `ds::field_style`, eticheta / iconița operațiilor (`ActivityKind::key/icon`).
  Snapshot: `REMOTE_PROPS=file|several|folder|list`.
- [x] 64b — făcut (2026-11-06). `PropertiesChange` / `SetProperties` + cazul de utilizare
  `apply_properties` (recursiv, folder cu folder; X pe foldere unde se poate citi; biții mixți
  păstrați — modurile citite per folder doar atunci; un refuz notat, restul continuă; o
  conexiune pierdută oprește). SFTP: `setstat` (mod; uid + gid împreună, celălalt păstrat).
  FTP: `SITE CHMOD`, un refuz (500/502/504) ținut minte pe conexiune — grila devine doar de
  citit; owner / grup doar afișate. Modalul: OK / Anulează (OK doar dacă s-a schimbat ceva),
  recursiv: numărat (măsura), întrebat („N elemente se schimbă”), rulat în fundal ca operație
  `Permissions` în Task Manager / „ce rulează”, oprit de buton sau de acolo; la final se
  închide cu o notificare, sau listează refuzurile. Set UID / GID / Sticky lângă grilă.
  Snapshot: `REMOTE_PROPS=…|confirm|failed`.
- [x] 64c — făcut (2026-11-06). Tab-ul Checksum (control segmentat General | Checksum în
  antet, doar pentru fișiere): algoritmul (SHA-512, SHA-384, SHA-256, SHA-224, SHA-1, MD5 —
  lista comună `Picker`, ultimul ales ținut minte), Calculează / Oprește, apoi fiecare fișier:
  digest-ul, unde s-a calculat, comparat cu copia locală (la fel / diferit / nu e local),
  copiat în clipboard. Pe server: SFTP prin uneltele lui (`sha256sum`…, altfel `shasum -a`,
  pe un canal exec — un cont doar SFTP n-are: calculat aici); FTP prin `HASH` (`OPTS HASH`) sau
  `XSHA512` / `XSHA256` / `XSHA1` / `XMD5`, după `FEAT` (citit o dată pe conexiune). Altfel
  descărcat și calculat local (RustCrypto, deja adus de SSH). Ton nou `ds::Tone::Success`.
  Testat pe `editor-test-ssh` (cu shell: calculat pe server) și pe cele fără shell / `HASH`.
  Snapshot: `REMOTE_PROPS=checksum`.
- [x] 64d — făcut (2026-11-06). Setarea „Arată drepturile în arbore” (oprită implicit): pe
  rândurile unui proiect remote, estompat în dreapta, drepturile și proprietarul
  (`rw-r--r-- www-data`, tot textul în tooltip), citite o dată pe folder în fundal
  (`list_properties`: SFTP o listare brută cu atributele și long names, FTP `MLSD` sau
  `LIST`), uitate la o urcare sau după ce se aplică drepturi. În culoarea de avertisment
  când serverul web nu le poate citi (nici grupul, nici ceilalți; un folder: nici intra).
  Setarea „Avertizează la fișierele urcate necitibile” (pornită implicit): o notificare,
  o dată pe fișier. Generic în domeniu: `workspace::Note` pe `DirEntry` (orice modul poate
  spune ceva despre un rând), arătată de arbore.
