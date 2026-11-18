//! The index of the project's symbols: file → symbols, plus fast lookups by the FQN of
//! types and functions and by the members' class.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::symbols::{FileSymbols, Symbol, SymbolKind};

/// Where a symbol is declared.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub path: PathBuf,
    pub offset: usize,
    pub line: u32,
}

/// How a library file kept packed comes back: its symbols' codec (the indexer's). The index
/// doesn't know the format (plan 74).
pub trait Unpack: Send + Sync + std::fmt::Debug {
    fn unpack(&self, bytes: &[u8]) -> Option<FileSymbols>;
}

/// A library file's symbols kept packed (plan 74, [`kernel::Packed`]: unpacked once at the
/// first ask, what was given out valid while the file is in the index), how to unpack them,
/// its members' names (to match a name without unpacking), its types and functions.
///
/// The same for every index holding the same bytes with the same codec — every project
/// using that version of a library, whatever its language or its folder — so held once for
/// the whole process ([`LIBRARY`], plan 76b): what's unpacked of it, unpacked once for all.
/// Different bytes (another version, a file changed in one project) are another one.
#[derive(Debug)]
struct Packed {
    whole: kernel::Packed<FileSymbols>,
    codec: Arc<dyn Unpack>,
    /// Its types and functions (what the tables point into), all of its symbols but its
    /// members.
    slim: Arc<FileSymbols>,
    /// Its members' names, each followed by `\n`.
    names: Box<str>,
    members: u32,
}

impl Packed {
    /// `whole` (what `bytes` unpack to by `codec`), kept packed: its types and functions
    /// apart, its members' names.
    fn of(whole: &FileSymbols, bytes: Arc<[u8]>, codec: Arc<dyn Unpack>) -> Self {
        let (members, kept): (Vec<&Symbol>, Vec<&Symbol>) =
            whole.symbols.iter().partition(|s| s.kind.is_member());
        let mut names = String::new();
        for member in &members {
            names.push_str(&member.name);
            names.push('\n');
        }
        let slim = FileSymbols {
            language: Arc::clone(&whole.language),
            namespace: whole.namespace.clone(),
            uses: Vec::new(),
            symbols: kept.into_iter().cloned().collect(),
        };
        Self {
            whole: kernel::Packed::new(bytes),
            codec,
            slim: Arc::new(slim),
            names: names.into_boxed_str(),
            members: members.len() as u32,
        }
    }
}

/// Every index's library files, by their packed bytes (plan 76b).
static LIBRARY: std::sync::LazyLock<kernel::Interner<Packed>> =
    std::sync::LazyLock::new(kernel::Interner::default);

/// The library files held now, by every index (shared once each).
pub fn library_files_held() -> usize {
    LIBRARY.len()
}

/// A file of the index: its path and its symbols, once (every entry of its symbols points
/// to it). A library file kept packed has only its types and functions in `symbols`.
#[derive(Debug)]
struct Indexed {
    path: Arc<Path>,
    symbols: Arc<FileSymbols>,
    packed: Option<Arc<Packed>>,
}

impl Indexed {
    /// All its symbols: a packed one's unpacked once, at the first ask, then kept.
    fn whole(&self) -> &FileSymbols {
        let Some(packed) = &self.packed else { return &self.symbols };
        // Not what it packed (never, but): its types and functions at least.
        packed.whole.get(|bytes| packed.codec.unpack(bytes)).unwrap_or(&self.symbols)
    }

    /// All its symbols without keeping what's unpacked for it (a walk over every file).
    fn whole_once(&self) -> Cow<'_, FileSymbols> {
        match &self.packed {
            None => Cow::Borrowed(&*self.symbols),
            Some(packed) => packed
                .whole
                .once(|bytes| packed.codec.unpack(bytes))
                .unwrap_or(Cow::Borrowed(&*self.symbols)),
        }
    }

    /// One of its members' names is one `keep` accepts (a packed one's without unpacking it).
    fn has_member_named(&self, keep: &impl Fn(&str) -> bool) -> bool {
        match &self.packed {
            Some(packed) => packed.names.lines().any(keep),
            None => self.symbols.symbols.iter().any(|s| s.kind.is_member() && keep(&s.name)),
        }
    }
}

/// A packed file's one entry for a class in the members' table (not one for each member:
/// plan 74): it stands for the file's members, those of the class looked for kept by the
/// lookup (unpacked then).
const ALL_MEMBERS: u32 = u32::MAX;

/// A reference to a symbol: its file (held: reading a symbol doesn't look its file up by
/// path — hashing every component — once per candidate) and the position in its list. 16
/// bytes: the tables hold one per symbol (measured, plan 73: the path and the symbols held
/// apart made it 32).
#[derive(Debug, Clone)]
struct Entry {
    file: Arc<Indexed>,
    index: u32,
}

/// A lookup table: lowercase key → entries.
///
/// Almost every key has one entry: kept inline, no allocation of its own.
type Entries = smallvec::SmallVec<[Entry; 1]>;
type Table = Sharded<Key, Entries>;

/// How many shards a [`Sharded`] map is split in.
const SHARDS: usize = 64;

/// A map split in shards, each shared (copy on write): a copy of the index (a long job
/// reading it) costs a reference count per shard, and a change after it copies only the
/// shard it falls in — a 64th of the table, not all of it (a file indexed while the
/// canvas builds its graph, on the UI thread at times).
#[derive(Debug, Clone)]
struct Sharded<K, V> {
    shards: Vec<Arc<HashMap<K, V>>>,
}

impl<K, V> Default for Sharded<K, V> {
    fn default() -> Self {
        Self { shards: (0..SHARDS).map(|_| Arc::new(HashMap::new())).collect() }
    }
}

impl<K: Eq + std::hash::Hash + Clone, V: Clone> Sharded<K, V> {
    /// The shard `key` falls in (its hash, fixed: the same in every copy).
    fn shard_of<Q: std::hash::Hash + ?Sized>(key: &Q) -> usize {
        use std::hash::{DefaultHasher, Hasher};
        let mut hasher = DefaultHasher::new();
        key.hash(&mut hasher);
        (hasher.finish() % SHARDS as u64) as usize
    }

    fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.shards[Self::shard_of(key)].get(key)
    }

    fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        self.get(key).is_some()
    }

    /// The shard `key` falls in, to change (copied first if a copy of the index holds it).
    fn shard_mut<Q>(&mut self, key: &Q) -> &mut HashMap<K, V>
    where
        K: std::borrow::Borrow<Q>,
        Q: std::hash::Hash + Eq + ?Sized,
    {
        Arc::make_mut(&mut self.shards[Self::shard_of(key)])
    }

    fn iter(&self) -> impl Iterator<Item = (&K, &V)> {
        self.shards.iter().flat_map(|shard| shard.iter())
    }

    fn values(&self) -> impl Iterator<Item = &V> {
        self.shards.iter().flat_map(|shard| shard.values())
    }

    fn is_empty(&self) -> bool {
        self.shards.iter().all(|shard| shard.is_empty())
    }

    /// Every shard's spare capacity given back (`each` on its values first).
    fn shrink_to_fit(&mut self, mut each: impl FnMut(&mut V)) {
        for shard in &mut self.shards {
            let shard = Arc::make_mut(shard);
            shard.values_mut().for_each(&mut each);
            shard.shrink_to_fit();
        }
    }

    /// The heap the shards' tables take (bytes, an estimate).
    fn bytes(&self) -> u64 {
        self.shards.iter().map(|shard| kernel::memory::map_bytes(&**shard)).sum()
    }
}

/// A lookup key: the hash of the lowercase text, which the symbols already hold (each symbol
/// isn't kept twice for its table). Lookups check the entries' own text.
type Key = u64;

fn key(lowercase: &str) -> Key {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    lowercase.hash(&mut hasher);
    hasher.finish()
}

/// `text` is, ignoring case, what `lowercase` says.
fn same_text(text: &str, lowercase: &str) -> bool {
    text.chars().flat_map(char::to_lowercase).eq(lowercase.chars())
}

/// The tables are shared (copy on write): a copy of the index (a long job reading it
/// without holding the lock) costs a few reference counts, and the first change after it
/// copies only the tables that change.
#[derive(Debug, Default, Clone)]
pub struct SymbolIndex {
    files: Sharded<Arc<Path>, Arc<Indexed>>,
    /// FQN (lowercase) → types and functions.
    by_fqn: Table,
    /// The class FQN (lowercase) → its directly declared members.
    members: Table,
    /// Short name (lowercase) → any symbol (for context-free lookups).
    by_name: Table,
    /// Names a framework knows the class of, by table (`symfony.services`: a service's id →
    /// its class), from the project's own files: what a call naming one gives
    /// ([`crate::calls::ClassArgumentCall::names`]). Kept when the symbols are rebuilt.
    named: Arc<HashMap<&'static str, Arc<HashMap<String, String>>>>,
    /// Changes with each change of it (never the same for two different contents in the
    /// process): what's found in it can be kept while it stays.
    version: u64,
    /// The packed library files' member names, each once with a file having it: made at the
    /// first `->` on an unknown type, made again only when a library file changes (not at
    /// each save of the project's).
    library_names: Arc<std::sync::OnceLock<LibraryNames>>,
    /// The packed library files' types and functions, one after another, with whether each
    /// file is a dependency's: made at the first name completed, made again only when a
    /// library file changes (a flat list: not every table's buckets walked, nor each path
    /// looked at, at each key).
    library_top: Arc<std::sync::OnceLock<Vec<(Entry, bool)>>>,
    /// Changes when the libraries' lists are made again ([`Self::library_version`]).
    library_version: u64,
}

/// The library files' distinct member names, each with one file declaring it.
#[derive(Debug, Default)]
struct LibraryNames {
    names: Vec<(Box<str>, Arc<Indexed>)>,
}

/// The versions given (one counter for every index: two never share one).
static VERSIONS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl SymbolIndex {
    /// Changes with each change of the index (see the field).
    pub fn version(&self) -> u64 {
        self.version
    }

    /// A library file changed: its lists made again when next asked for.
    fn library_changed(&mut self) {
        self.library_names = Arc::default();
        self.library_top = Arc::default();
        self.library_version = VERSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn changed(&mut self) {
        self.version = VERSIONS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    /// The library files' types and functions (see the field).
    fn library_top_entries(&self) -> &[(Entry, bool)] {
        self.library_top.get_or_init(|| {
            let mut out = Vec::new();
            for file in self.files.values().filter(|f| f.packed.is_some()) {
                let dependency = kernel::in_dependencies(&file.path);
                let top =
                    file.symbols.symbols.iter().enumerate().filter(|(_, s)| !s.kind.is_member());
                for (i, _) in top.take_while(|(i, _)| *i < ALL_MEMBERS as usize) {
                    out.push((Entry { file: Arc::clone(file), index: i as u32 }, dependency));
                }
            }
            out
        })
    }

    /// Table `table`'s names (replacing what it had; empty: the table goes).
    pub fn set_named(&mut self, table: &'static str, names: HashMap<String, String>) {
        self.changed();
        let tables = Arc::make_mut(&mut self.named);
        if names.is_empty() {
            tables.remove(table);
        } else {
            tables.insert(table, Arc::new(names));
        }
    }

    /// The class table `table` has for `name`, if any.
    pub fn named_type(&self, table: &str, name: &str) -> Option<&str> {
        self.named.get(table)?.get(name).map(String::as_str)
    }

    /// The symbols of `rebuilt` in place of these (a new build of the whole index), the named
    /// tables kept: they come from the frameworks, not from the files' symbols.
    pub fn replace_symbols(&mut self, rebuilt: SymbolIndex) {
        let named = std::mem::take(&mut self.named);
        *self = rebuilt;
        self.named = named;
        self.changed();
        self.library_changed();
    }

    /// Gives back the spare capacity of the lookup tables (after building the whole index:
    /// they grew one entry at a time).
    pub fn shrink_to_fit(&mut self) {
        for map in [&mut self.by_fqn, &mut self.members, &mut self.by_name] {
            map.shrink_to_fit(|entries| entries.shrink_to_fit());
        }
        self.files.shrink_to_fit(|_| {});
    }

    pub fn len(&self) -> usize {
        self.files
            .values()
            .map(|f| f.symbols.symbols.len() + f.packed.as_ref().map_or(0, |p| p.members as usize))
            .sum()
    }

    /// The heap it takes (bytes, an estimate: its tables, its files' symbols and their text;
    /// what's shared is counted once per holder). For the Task Manager, once a second at most.
    pub fn memory(&self) -> u64 {
        self.memory_parts().iter().map(|(_, bytes)| bytes).sum()
    }

    /// [`Self::memory`] by part: the files (their paths and symbols), then each lookup table
    /// (its slots and the entries past the one kept inline).
    pub fn memory_parts(&self) -> [(&'static str, u64); 4] {
        let table = |t: &Table| {
            let spilled = |v: &Entries| {
                if v.spilled() { (v.capacity() * std::mem::size_of::<Entry>()) as u64 } else { 0 }
            };
            t.bytes() + t.values().map(spilled).sum::<u64>()
        };
        let files: u64 = self
            .files
            .iter()
            .map(|(path, file)| {
                // Its record (and the `Arc`'s two counts), its path's text, its symbols (a
                // packed one's: its bytes, its names, what's unpacked of it).
                let record = std::mem::size_of::<Indexed>() + 2 * std::mem::size_of::<usize>();
                let packed = file.packed.as_ref().map_or(0, |p| {
                    (p.whole.bytes().len() + p.names.len()) as u64
                        + p.whole.unpacked().map_or(0, FileSymbols::memory)
                });
                record as u64 + path.as_os_str().len() as u64 + file.symbols.memory() + packed
            })
            .sum();
        [
            ("files", self.files.bytes() + files),
            ("by FQN", table(&self.by_fqn)),
            ("members by class", table(&self.members)),
            ("by name", table(&self.by_name)),
        ]
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// How much of [`Self::memory`] each folder under `root` accounts for (relative, with `/`,
    /// down to `depth` levels): its files' symbols, the tables shared out in proportion.
    pub fn memory_by_folder(&self, root: &Path, depth: usize) -> HashMap<String, u64> {
        let sizes: Vec<(&Arc<Path>, u64)> = self
            .files
            .iter()
            .map(|(path, file)| {
                let packed = file.packed.as_ref().map_or(0, |p| p.whole.bytes().len() as u64);
                (path, path.as_os_str().len() as u64 + file.symbols.memory() + packed)
            })
            .collect();
        let held = sizes.iter().map(|(_, size)| *size).sum::<u64>().max(1);
        let total = self.memory();
        let mut folders: HashMap<String, u64> = HashMap::new();
        for (path, size) in sizes {
            let Ok(relative) = path.strip_prefix(root) else { continue };
            let relative = relative.to_string_lossy().replace('\\', "/");
            let share = u64::try_from(u128::from(size) * u128::from(total) / u128::from(held))
                .unwrap_or(u64::MAX);
            for (at, _) in relative.match_indices('/').take(depth) {
                *folders.entry(relative[..at].to_owned()).or_default() += share;
            }
        }
        folders
    }

    /// Replaces a file's symbols.
    pub fn set_file(&mut self, path: &Path, symbols: FileSymbols) {
        self.changed();
        self.set_file_shared(path, Arc::new(symbols));
    }

    /// Replaces a file's symbols with shared ones (e.g. the indexer's cache entry, so the
    /// symbols exist once in memory).
    pub fn set_file_shared(&mut self, path: &Path, symbols: Arc<FileSymbols>) {
        self.changed();
        self.remove_file(path);
        let path: Arc<Path> = Arc::from(path);
        let file = Arc::new(Indexed { path: Arc::clone(&path), symbols, packed: None });
        let positions = (0..file.symbols.symbols.len()).map(|i| i as u32);
        self.add_entries(&file, &file.symbols.symbols, positions);
        self.files.shard_mut(&*path).insert(path, file);
    }

    /// A library file kept packed (plan 74): its types and functions decoded (completion of
    /// names, definitions, the walks over every file unpack nothing), its members in
    /// `bytes`, unpacked by `codec` at the first ask for them. `whole`: its symbols, all of
    /// them (what `bytes` unpacks to), looked at now for the tables, not kept. The same bytes
    /// with the same codec in another index (another project with that library): what it
    /// holds is shared (plan 76b) — those bytes are returned, for the caller to keep instead
    /// of its own copy.
    pub fn set_file_packed(
        &mut self,
        path: &Path,
        whole: &FileSymbols,
        bytes: Arc<[u8]>,
        codec: Arc<dyn Unpack>,
    ) -> Arc<[u8]> {
        self.remove_file(path);
        self.library_changed();
        let path: Arc<Path> = Arc::from(path);
        let hash = kernel::cache::content_hash(&bytes);
        let packed = LIBRARY.get_or_make(
            hash,
            |held| Arc::ptr_eq(&held.codec, &codec) && **held.whole.bytes() == *bytes,
            || Packed::of(whole, Arc::clone(&bytes), Arc::clone(&codec)),
        );
        let file = Arc::new(Indexed {
            path: Arc::clone(&path),
            symbols: Arc::clone(&packed.slim),
            packed: Some(Arc::clone(&packed)),
        });
        let members = whole.symbols.iter().filter(|s| s.kind.is_member());
        // Its types and functions in every table, where they're kept; its members by their
        // class only, one entry for each class (by name, the file's names find them).
        let kept = Arc::clone(&file.symbols);
        self.add_entries(&file, &kept.symbols, 0..kept.symbols.len() as u32);
        let mut classes: Vec<String> =
            members.filter_map(|m| m.container.as_deref()).map(str::to_lowercase).collect();
        classes.sort_unstable();
        classes.dedup();
        for class in classes {
            let key = key(&class);
            let entry = Entry { file: Arc::clone(&file), index: ALL_MEMBERS };
            self.members.shard_mut(&key).entry(key).or_default().push(entry);
        }
        self.files.shard_mut(&*path).insert(path, file);
        Arc::clone(packed.whole.bytes())
    }

    /// The tables' entries for `symbols` (a file's, all of them), each at its position.
    fn add_entries(
        &mut self,
        file: &Arc<Indexed>,
        symbols: &[Symbol],
        positions: impl Iterator<Item = u32>,
    ) {
        // Each table's shards touched only (copied first if a copy of the index holds them).
        let push = |table: &mut Table, key: Key, entry: Entry| {
            table.shard_mut(&key).entry(key).or_default().push(entry);
        };
        for (s, index) in symbols.iter().zip(positions) {
            // More symbols than fit in a position aren't in one file: past it, none is listed.
            if index == ALL_MEMBERS {
                break;
            }
            let entry = Entry { file: Arc::clone(file), index };
            if s.kind.is_member() {
                if let Some(container) = &s.container {
                    push(&mut self.members, key(&container.to_lowercase()), entry.clone());
                }
            } else {
                push(&mut self.by_fqn, key(&s.fqn.to_lowercase()), entry.clone());
            }
            push(&mut self.by_name, key(&s.name.to_lowercase()), entry);
        }
    }

    pub fn remove_file(&mut self, path: &Path) {
        self.changed();
        if !self.files.contains_key(path) {
            return;
        }
        // Its entries hold the very file removed (made once in `set_file_shared`): they are
        // told apart by pointer, not by comparing paths component by component (the
        // short-name buckets are as long as the project: `__construct`, `get`…).
        let Some(held) = self.files.shard_mut(path).remove(path) else { return };
        if held.packed.is_some() {
            self.library_changed();
        }
        // All its symbols (a packed one's unpacked for this, not kept): their keys.
        let old = held.whole_once();
        // Each key once: a file with many `get`s doesn't walk the (large) `get` bucket
        // once per symbol.
        let mut members = HashSet::new();
        let mut fqns = HashSet::new();
        let mut names = HashSet::new();
        for s in &old.symbols {
            if s.kind.is_member() {
                if let Some(container) = &s.container {
                    members.insert(key(&container.to_lowercase()));
                }
            } else {
                fqns.insert(key(&s.fqn.to_lowercase()));
            }
            names.insert(key(&s.name.to_lowercase()));
        }
        let tables =
            [(&mut self.members, members), (&mut self.by_fqn, fqns), (&mut self.by_name, names)];
        for (table, keys) in tables {
            for key in keys {
                // Only a shard holding the key is copied (a key in none touches nothing).
                if table.contains_key(&key)
                    && let Some(entries) = table.shard_mut(&key).get_mut(&key)
                {
                    entries.retain(|e| !Arc::ptr_eq(&e.file, &held));
                }
            }
        }
    }

    /// A file's symbols, all of them (a library file kept packed: unpacked at the first ask,
    /// then kept).
    pub fn file(&self, path: &Path) -> Option<&FileSymbols> {
        self.files.get(path).map(|f| f.whole())
    }

    /// A file's language (`""`: none), without unpacking anything.
    pub fn language_of(&self, path: &Path) -> Option<&str> {
        self.files.get(path).map(|f| &*f.symbols.language)
    }

    fn symbol(entry: &Entry) -> Option<(&Path, &Symbol)> {
        let file = &entry.file;
        Some((&*file.path, file.symbols.symbols.get(entry.index as usize)?))
    }

    /// What an entry stands for: its symbol, or (a packed file's class, in the members'
    /// table) every member of its file — unpacked now, the lookup keeping its class's.
    fn symbols_of(entry: &Entry) -> Box<dyn Iterator<Item = (&Path, &Symbol)> + '_> {
        if entry.index != ALL_MEMBERS {
            return Box::new(Self::symbol(entry).into_iter());
        }
        let file = &entry.file;
        let members = file.whole().symbols.iter().filter(|s| s.kind.is_member());
        Box::new(members.map(move |s| (&*file.path, s)))
    }

    /// The symbols `table` has for `lowercase`, `text` saying what each is found by.
    fn resolve(
        table: &Table,
        lowercase: String,
        text: fn(&Symbol) -> &str,
    ) -> impl Iterator<Item = (&Path, &Symbol)> {
        let entries = table.get(&key(&lowercase)).into_iter().flatten();
        entries
            .flat_map(Self::symbols_of)
            .filter(move |(_, symbol)| same_text(text(symbol), &lowercase))
    }

    fn found_by_fqn(symbol: &Symbol) -> &str {
        &symbol.fqn
    }

    fn found_by_name(symbol: &Symbol) -> &str {
        &symbol.name
    }

    fn found_by_container(symbol: &Symbol) -> &str {
        symbol.container.as_deref().unwrap_or_default()
    }

    /// The type (class, interface, trait, enum) with the given FQN.
    pub fn type_by_fqn(&self, fqn: &str) -> Option<(&Path, &Symbol)> {
        Self::resolve(&self.by_fqn, fqn.to_lowercase(), Self::found_by_fqn)
            .find(|(_, s)| s.kind.is_type())
    }

    pub fn function_by_fqn(&self, fqn: &str) -> Option<(&Path, &Symbol)> {
        Self::resolve(&self.by_fqn, fqn.to_lowercase(), Self::found_by_fqn)
            .find(|(_, s)| s.kind == SymbolKind::Function)
    }

    /// The class's members, including inherited ones (parents, interfaces, traits), with those
    /// declared in the class first. Each name appears only once.
    pub fn members_of(&self, class: &str) -> Vec<(&Path, &Symbol)> {
        let mut out: Vec<(&Path, &Symbol)> = Vec::new();
        let mut names: HashSet<(&str, SymbolKind)> = HashSet::new();
        let mut queue = vec![class.to_owned()];
        let mut seen: Vec<String> = Vec::new();
        while let Some(current) = queue.pop() {
            let key = current.to_lowercase();
            if seen.contains(&key) || seen.len() > 32 {
                continue;
            }
            seen.push(key.clone());
            for (path, s) in Self::resolve(&self.members, key.clone(), Self::found_by_container) {
                if names.insert((&s.name, s.kind)) {
                    out.push((path, s));
                }
            }
            if let Some((_, ty)) = self.type_by_fqn(&current) {
                queue.extend(ty.parents.iter().rev().map(|p| p.to_string()));
            }
        }
        out
    }

    /// The members declared in the class itself (not the inherited ones).
    pub fn declared_members(&self, class: &str) -> impl Iterator<Item = (&Path, &Symbol)> {
        Self::resolve(&self.members, class.to_lowercase(), Self::found_by_container)
    }

    /// The symbols with the given short name (case-insensitive): the tables' (types,
    /// functions, the project's members), and the members of the packed files whose names
    /// have it (their names read first; those files unpacked, then kept).
    pub fn by_name(&self, name: &str) -> Vec<(&Path, &Symbol)> {
        let lowercase = name.to_lowercase();
        let mut found: Vec<(&Path, &Symbol)> =
            Self::resolve(&self.by_name, lowercase.clone(), Self::found_by_name).collect();
        let named = |n: &str| same_text(n, &lowercase);
        for file in self.files.values().filter(|f| f.packed.is_some()) {
            if file.has_member_named(&named) {
                let members = file.whole().symbols.iter();
                found.extend(
                    members
                        .filter(|s| s.kind.is_member() && named(&s.name))
                        .map(|s| (&*file.path, s)),
                );
            }
        }
        found
    }

    /// Every indexed file and its declarations — a library file kept packed: its types and
    /// functions only (nothing unpacked; [`Self::file_once`] for its members too).
    pub fn files(&self) -> impl Iterator<Item = (&Path, &FileSymbols)> {
        self.files.iter().map(|(p, f)| (&**p, &*f.symbols))
    }

    /// A file's symbols, all of them, a packed one's unpacked for this only (not kept: a walk
    /// over every library file doesn't bring them all back into memory).
    pub fn file_once(&self, path: &Path) -> Option<Cow<'_, FileSymbols>> {
        self.files.get(path).map(|f| f.whole_once())
    }

    /// All types and functions (for name completion).
    pub fn top_level(&self) -> impl Iterator<Item = (&Path, &Symbol)> {
        self.by_fqn.values().flatten().filter_map(Self::symbol)
    }

    /// The types and functions of the files `keep` accepts (a completion goes through the
    /// whole project's).
    pub fn top_level_where(
        &self,
        keep: impl Fn(&FileSymbols) -> bool,
    ) -> impl Iterator<Item = (&Path, &Symbol)> {
        self.by_fqn
            .values()
            .flatten()
            .filter(move |e| keep(&e.file.symbols))
            .filter_map(Self::symbol)
    }

    /// All members in the index (a library file kept packed: unpacked, then kept).
    pub fn all_members(&self) -> impl Iterator<Item = (&Path, &Symbol)> {
        // A packed file once, whatever classes its entries stand for.
        let mut seen = HashSet::new();
        self.members
            .values()
            .flatten()
            .filter(move |e| e.index != ALL_MEMBERS || seen.insert(Arc::as_ptr(&e.file)))
            .flat_map(Self::symbols_of)
    }

    /// The members `score` scores, given to `each`: every one of the project's files (their
    /// symbols are there), and of the library files kept packed only those whose name is
    /// among the `names` best — scored from the libraries' distinct member names (read
    /// without unpacking), only the files of the best unpacked (for this walk): a short word
    /// scores most of a library's names, its files aren't all unpacked at each key (17,700 of
    /// them: ~140 ms). `within`: the library names to score (the places [`Self::library_version`]
    /// gave for a word this one goes on from: `ge` then `get`), all of them if `None`.
    /// Returns the places of the library names `score` took.
    pub fn for_each_best_member(
        &self,
        score: &dyn Fn(&str) -> Option<i32>,
        names: usize,
        within: Option<&[u32]>,
        mut each: impl FnMut(&Path, &Symbol),
    ) -> Vec<u32> {
        for file in self.files.values().filter(|f| f.packed.is_none()) {
            let members = file.symbols.symbols.iter().filter(|s| s.kind.is_member());
            for symbol in members.filter(|s| score(&s.name).is_some()) {
                each(&file.path, symbol);
            }
        }
        // The libraries' names, each once: the best ones, then their files unpacked.
        let library = self.library_names.get_or_init(|| self.make_library_names());
        let scored = |i: usize| Some((score(&library.names.get(i)?.0)?, i));
        let mut best: Vec<(i32, usize)> = match within {
            Some(places) => places.iter().filter_map(|&i| scored(i as usize)).collect(),
            None => (0..library.names.len()).filter_map(scored).collect(),
        };
        let taken: Vec<u32> = best.iter().map(|&(_, i)| i as u32).collect();
        if best.len() > names {
            best.select_nth_unstable_by(names, |a, b| b.0.cmp(&a.0));
            best.truncate(names);
        }
        let chosen: HashSet<&str> = best.iter().map(|&(_, i)| &*library.names[i].0).collect();
        let mut files: Vec<&Arc<Indexed>> =
            best.iter().map(|&(_, i)| &library.names[i].1).collect();
        files.sort_unstable_by_key(|f| Arc::as_ptr(f) as usize);
        files.dedup_by_key(|f| Arc::as_ptr(f) as usize);
        for file in files {
            let whole = file.whole_once();
            let members = whole.symbols.iter().filter(|s| s.kind.is_member());
            for symbol in members.filter(|s| chosen.contains(&*s.name)) {
                each(&file.path, symbol);
            }
        }
        taken
    }

    /// [`LibraryNames`] made from the packed files' names.
    fn make_library_names(&self) -> LibraryNames {
        let mut seen: HashSet<&str> = HashSet::new();
        let mut names = Vec::new();
        for file in self.files.values() {
            let Some(packed) = &file.packed else { continue };
            for name in packed.names.lines() {
                if seen.insert(name) {
                    names.push((Box::from(name), Arc::clone(file)));
                }
            }
        }
        LibraryNames { names }
    }

    /// Changes only when a library file does (the libraries' lists made again): places in
    /// them a caller kept ([`Self::for_each_best_member`], [`Self::library_top_at`]) stay
    /// right while it stays.
    pub fn library_version(&self) -> u64 {
        self.library_version
    }

    /// How many types and functions the libraries have ([`Self::library_top_at`]'s places).
    pub fn library_top_count(&self) -> usize {
        self.library_top_entries().len()
    }

    /// The library type or function at `place`: its file's path, itself, whether that file
    /// is a dependency's, the file's symbols (its language).
    pub fn library_top_at(&self, place: u32) -> Option<(&Path, &Symbol, bool, &FileSymbols)> {
        let (entry, dependency) = self.library_top_entries().get(place as usize)?;
        let (path, symbol) = Self::symbol(entry)?;
        Some((path, symbol, *dependency, &entry.file.symbols))
    }

    /// The project's (not the libraries') types and functions of the files `keep` accepts.
    pub fn project_top_level(
        &self,
        keep: impl Fn(&FileSymbols) -> bool,
    ) -> impl Iterator<Item = (&Path, &Symbol)> {
        self.files
            .values()
            .filter(|f| f.packed.is_none())
            .filter(move |f| keep(&f.symbols))
            .flat_map(|f| {
                let top = f.symbols.symbols.iter().take(ALL_MEMBERS as usize);
                top.filter(|s| !s.kind.is_member()).map(|s| (&*f.path, s))
            })
    }

    /// Each member whose name `keep` accepts, given to `each`: a library file kept packed is
    /// looked at only if one of its members' names is one (its names read first), and
    /// unpacked for this walk only.
    pub fn for_each_member_named(
        &self,
        keep: &dyn Fn(&str) -> bool,
        mut each: impl FnMut(&Path, &Symbol),
    ) {
        for file in self.files.values().filter(|f| f.has_member_named(&keep)) {
            let whole = file.whole_once();
            for symbol in whole.symbols.iter().filter(|s| s.kind.is_member() && keep(&s.name)) {
                each(&file.path, symbol);
            }
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/index.rs"]
mod tests;
