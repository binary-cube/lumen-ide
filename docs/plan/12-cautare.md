# 12 — Căutarea în proiect (lexicală + semantică)

## 1. Ce face

- **⌘⇧F (Ctrl+Shift+F):** panoul „Caută în proiect”: un nume, câteva cuvinte sau o
  întrebare („unde se trimite emailul”). Rezultatele arată fișierul, linia potrivită și
  fragmentul; Enter / click deschide la linie.
- **Lexical (BM25)** pe termeni, cu identificatorii sparți (`getUserName` → `get`, `user`,
  `name`); funcționează mereu, fără model.
- **Semantic** (opțional): embeddings statice model2vec (`minishlab/potion-code-16M`, MIT,
  64 MB), descărcate o singură dată în `<config>/lumen/models/`, la cerere din panou.
  Fără GPU, fără runtime ML: tokenizare WordPiece + medie de vectori, matricea mapată în
  memorie (`memmap2`). Merge identic pe macOS, Windows, Linux.
- Clasamentele se combină prin Reciprocal Rank Fusion (k = 60).

## 2. Arhitectură

```
text-search (domeniu)      Chunk, Hit, SearchIndex (BM25 + cosinus + RRF)
                            port Embedder { embed, dims }
                            API  CodeSearch { search(&SearchQuery), related(path, line), is_semantic }
                            SearchQuery { text, limit, paths, extensions, strategy: Hybrid|Lexical|Semantic }
embed (infra)         Model2Vec: Embedder; download(dir, progress)
indexer (infra)       SearchState: bucăți de 40 de linii, cache `.lumen/index/search.bin`,
                            actualizat incremental de watcher (doar fișierele schimbate)
                            ProjectIndexer::search_api() -> Arc<dyn CodeSearch>
app                         semantic.rs: un singur model pe proces (partajat de proiecte)
```

## 3. API-ul pentru alți consumatori (ex: un agent AI)

`ProjectIndexer::search_api()` întoarce un `Arc<dyn CodeSearch>` ieftin, `Send + Sync`,
care citește indexul ținut la zi pe fundal. Un agent (sau o comandă, un plugin) îl
primește și face căutări rapide, în memorie:

```rust
let search = indexer.search_api();
let mut q = SearchQuery::new("unde se validează parola", 10);
q.paths = vec!["src/".into()];
q.extensions = vec!["php".into()];
for hit in search.search(&q) { /* hit.path, hit.start_line, hit.preview */ }
let similar = search.related("src/Auth/Login.php", 42, 5);
```
