# 26. Badge-uri pe butoanele ferestrelor și listele lor (Remote, Git)

Cererea: pe butonul Remote un badge cu câte fișiere sunt de urcat și o zonă care le arată;
la Git la fel, un badge cu fișierele modificate și lista lor. Ceva comun, pe module, scris
bine și organizat vizual.

## Ce avem azi

- Butoanele de pe margini (`ToolWindowProvider`, `ToolStrip`) n-au niciun badge; niciun
  modul nu poate schimba ce arată un buton.
- **Remote** are deja tab-ul „De urcat” (`pending_view.rs`): ce n-a putut pleca (cu motivul),
  ce s-a schimbat aici și ce e nou. Se deschide doar din meniul butonului; butonul deschide
  jurnalul. Bara de jos arată „N de urcat” doar pentru ce n-a putut pleca.
- **Git** are lista fișierelor modificate doar în view-ul de commit (un tab în editor), citită
  când se deschide. Nimic nu ține starea repository-ului între timp; butonul deschide log-ul.

## Ce facem

### A. Comun (contractul și desenul)
- `workbench_model::ToolBadge { count, attention, label }`: câte, dacă cere atenție (ceva
  n-a mers: culoarea de avertizare, altfel accentul) și rândul din tooltip („3 fișiere
  modificate”).
- `WorkbenchHost::set_tool_badge(tool, Option<ToolBadge>)`: un modul își pune badge-ul pe
  butonul lui. Ținut în `ProjectData` (al proiectului: dispare cu el, nu trece în altul).
- `ToolStrip` desenează badge-ul: o pastilă mică în colțul de sus-dreapta al butonului, cu
  număr (peste 99: „99+”), conturată cu fundalul benzii ca să se despartă de iconiță.
  Tooltip-ul butonului adaugă rândul badge-ului.
- `ds::file_row`: rândul comun al unei liste de fișiere — marcajul stării (literă / iconiță
  colorată), numele, folderul estompat, eticheta din dreapta; tăiate cu „…”. Îl folosesc
  ambele liste, ca să arate la fel.

### B. Remote
- Lista „De urcat” e citită o singură dată pentru ambele (badge + tab): în `State`, în fundal
  (citește discul), refăcută când se mișcă transferurile, la salvări și la schimbările de pe
  disc; mai multe cereri apropiate se adună într-o singură citire.
- Badge-ul: câte sunt de urcat; „atenție” dacă vreunul n-a putut pleca.
- Butonul deschide „De urcat” (ce numără badge-ul); jurnalul rămâne în meniul butonului.

### C. Git
- Un serviciu de proiect al modulului ține starea working tree-ului (`status`), citită în
  fundal (un pool numit `git-status`, un fir, care se termină când n-are de lucru), refăcută la
  salvări, la schimbările de pe disc și la schimbarea HEAD-ului, cererile apropiate adunate.
- Badge-ul: câte fișiere sunt modificate.
- Tab nou „Modificări” în fereastra Git: fișierele cu starea lor (M / A / D / R / ?), un
  click deschide fișierul, butonul de pe rând îl compară cu ultimul commit; în antet numărul
  și „Commit…”. Butonul Git îl deschide pe el; log-ul rămâne în meniul butonului.

## Etape (fiecare: commit separat, `check.sh` verde, snapshot-urile comparate)

- [x] 26a. Contractul (`ToolBadge`, `set_tool_badge`), desenul pe bandă (`ds::count_badge`:
  alb cu cifra colorată pe butonul activ), tooltip-ul, `ds::file_row` (folosit și de commit:
  identic pixel cu pixel).
- [x] 26b. Remote: lista în `State` (`outgoing.rs`, pool-ul `remote-pending`), badge-ul,
  butonul pe „De urcat”, rândurile cu `file_row`.
- [x] 26c. Git: serviciul cu starea (`status.rs`, pool-ul `git-status`), badge-ul, tab-ul
  „Modificări” (snapshot `GIT=changes`).
- [x] `ProjectService::busy`: snapshot-urile așteaptă citirile modulelor (badge-ul apare
  mereu la fel). Toate celelalte snapshot-uri diferă doar în pătratul badge-ului Git (proiectul
  de probă are un fișier modificat).

Totul prin evenimentele proiectului: modulele își știu singure bucătăria (ce citesc, când),
app-ul doar desenează numărul pe care i-l dau.
