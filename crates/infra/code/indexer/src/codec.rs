//! The binary format of the cache in `<project dir>/index/symbols.bin`: compact and fast to read,
//! with no dependencies. A header with the app + format stamp and the extractor versions
//! invalidates the cache when any of them changes. Each file's symbols are a block of
//! their own, its length before it: their encoded form, compressed ([`pack`]) — the same
//! bytes a library file is kept as in memory, read alone later ([`read_block`],
//! [`unpack`]; plan 74).

use code_intelligence::{FileSymbols, Symbol, SymbolKind, UseImport, Visibility};
use std::sync::Arc;

const MAGIC: &[u8; 4] = b"LMNI";
/// The layout of this file; bump it on any change. Together with the app version it
/// forms the header stamp (`kernel::cache_stamp`).
/// 8: a symbol's note (PHP's attributes). 9: a symbol's doc types (PHPDoc). 10: each file's
/// symbols a block, its length before it. 11: the block compressed. 12: compressed at
/// deflate's level 6 (every project's blocks alike again: the same library's are shared).
const FORMAT: u32 = 12;

/// The most a block unpacks to (a file's symbols: far less; past it, it isn't one).
const MAX_UNPACKED: usize = 64 << 20;

/// A cached file: relative path, fingerprint (size, date, hash) and symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedFile {
    /// Relative to the project (shared with the worker's table: one copy per file).
    pub path: Arc<str>,
    pub len: u64,
    pub modified: u64,
    pub hash: u64,
    /// Shared with the [`SymbolIndex`](code_intelligence::SymbolIndex) (one copy); a
    /// library file kept packed holds none here (its block is what's kept: `packed`).
    pub symbols: Arc<FileSymbols>,
    /// Its symbols packed (its block's bytes, [`pack`]): a library file's, kept so instead
    /// of `symbols` (the index unpacks them when asked, plan 74); written to the cache as
    /// they are.
    pub packed: Option<Arc<[u8]>>,
    /// Where its symbols' block is in the cache file it was read from or written to
    /// (`None`: extracted since, not written yet).
    pub block: Option<Block>,
}

/// A file's block in the cache file: where it starts and how long it is (bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Block {
    pub at: u64,
    pub len: u32,
}

/// 64-bit FNV-1a: enough to know whether a file has changed.
pub fn hash(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

struct Writer(Vec<u8>);

impl Writer {
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn str(&mut self, s: &str) {
        self.u32(s.len() as u32);
        self.0.extend_from_slice(s.as_bytes());
    }
    fn opt(&mut self, s: Option<&str>) {
        match s {
            Some(s) => {
                self.0.push(1);
                self.str(s);
            }
            None => self.0.push(0),
        }
    }
}

struct Reader<'a>(&'a [u8]);

/// One copy of each repeated text (a class's name in every member's `container`, the
/// imports, the language): the whole index shares them.
#[derive(Debug, Default)]
pub struct Pool(std::collections::HashSet<Arc<str>>);

impl Pool {
    pub fn get(&mut self, text: &str) -> Arc<str> {
        if let Some(shared) = self.0.get(text) {
            return Arc::clone(shared);
        }
        let shared: Arc<str> = Arc::from(text);
        self.0.insert(Arc::clone(&shared));
        shared
    }

    /// A file's repeated texts (extracted just now) shared with the rest of the index.
    pub fn share(&mut self, file: &mut FileSymbols) {
        file.language = self.get(&file.language);
        if let Some(ns) = &file.namespace {
            file.namespace = Some(self.get(ns));
        }
        for import in &mut file.uses {
            import.alias = self.get(&import.alias);
            import.fqn = self.get(&import.fqn);
        }
        for symbol in &mut file.symbols {
            if let Some(container) = &symbol.container {
                symbol.container = Some(self.get(container));
            }
            for parent in &mut symbol.parents {
                *parent = self.get(parent);
            }
        }
    }
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        if self.0.len() < n {
            return None;
        }
        let (head, rest) = self.0.split_at(n);
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        self.take(1).map(|b| b[0])
    }
    fn u32(&mut self) -> Option<u32> {
        self.take(4)?.try_into().ok().map(u32::from_le_bytes)
    }
    fn u64(&mut self) -> Option<u64> {
        self.take(8)?.try_into().ok().map(u64::from_le_bytes)
    }
    fn str(&mut self) -> Option<String> {
        let len = self.u32()? as usize;
        String::from_utf8(self.take(len)?.to_vec()).ok()
    }
    fn opt(&mut self) -> Option<Option<String>> {
        match self.u8()? {
            0 => Some(None),
            _ => self.str().map(Some),
        }
    }
    fn raw(&mut self) -> Option<&'a str> {
        let len = self.u32()? as usize;
        std::str::from_utf8(self.take(len)?).ok()
    }
    /// A text read once into `pool` (later ones share it).
    fn shared(&mut self, pool: &mut Pool) -> Option<Arc<str>> {
        Some(pool.get(self.raw()?))
    }
    fn shared_opt(&mut self, pool: &mut Pool) -> Option<Option<Arc<str>>> {
        match self.u8()? {
            0 => Some(None),
            _ => self.shared(pool).map(Some),
        }
    }
}

// New kinds go last: the files cached before them read the same.
const KINDS: [SymbolKind; 16] = [
    SymbolKind::Class,
    SymbolKind::Interface,
    SymbolKind::Trait,
    SymbolKind::Enum,
    SymbolKind::Function,
    SymbolKind::Method,
    SymbolKind::Property,
    SymbolKind::Constant,
    SymbolKind::ClassConstant,
    SymbolKind::EnumCase,
    SymbolKind::CssClass,
    SymbolKind::CssId,
    SymbolKind::CssVariable,
    SymbolKind::Mixin,
    SymbolKind::Variable,
    SymbolKind::Section,
];

/// The cache's bytes, with where each file's block is in them (in `files`' order).
pub fn encode_with_blocks(extractors: u32, files: &[CachedFile]) -> (Vec<u8>, Vec<Block>) {
    let mut w = Writer(Vec::with_capacity(files.len() * 256));
    w.0.extend_from_slice(MAGIC);
    w.0.extend_from_slice(&kernel::cache_stamp(FORMAT).to_le_bytes());
    w.u32(extractors);
    w.u32(files.len() as u32);
    let mut blocks = Vec::with_capacity(files.len());
    for f in files {
        w.str(&f.path);
        w.u64(f.len);
        w.u64(f.modified);
        w.u64(f.hash);
        // The block's length, written once it's known.
        let length_at = w.0.len();
        w.u32(0);
        let at = w.0.len();
        match &f.packed {
            Some(packed) => w.0.extend_from_slice(packed),
            None => w.0.extend_from_slice(&pack(&f.symbols)),
        }
        let len = (w.0.len() - at) as u32;
        w.0[length_at..length_at + 4].copy_from_slice(&len.to_le_bytes());
        blocks.push(Block { at: at as u64, len });
    }
    (w.0, blocks)
}

/// A file's symbols: its block's content.
fn encode_symbols(w: &mut Writer, symbols: &FileSymbols) {
    w.str(&symbols.language);
    w.opt(symbols.namespace.as_deref());
    w.u32(symbols.uses.len() as u32);
    for u in &symbols.uses {
        w.str(&u.alias);
        w.str(&u.fqn);
    }
    w.u32(symbols.symbols.len() as u32);
    for s in &symbols.symbols {
        w.str(&s.name);
        w.0.push(KINDS.iter().position(|k| *k == s.kind).unwrap_or(0) as u8);
        w.str(&s.fqn);
        w.opt(s.container.as_deref());
        w.u32(s.parents.len() as u32);
        for p in &s.parents {
            w.str(p);
        }
        w.u64(s.range.start as u64);
        w.u64(s.range.end as u64);
        w.u64(s.offset as u64);
        w.u32(s.line);
        w.opt(s.detail.as_deref());
        w.0.push(u8::from(s.is_static));
        w.0.push(match s.visibility {
            Visibility::Public => 0,
            Visibility::Protected => 1,
            Visibility::Private => 2,
        });
        w.opt(s.note.as_deref());
        w.opt(s.types.as_deref());
    }
}

/// `None` if the file is invalid or written by another version (the cache is rebuilt).
pub fn decode(extractors: u32, bytes: &[u8], pool: &mut Pool) -> Option<Vec<CachedFile>> {
    let mut r = Reader(bytes);
    let stamp = kernel::cache_stamp(FORMAT).to_le_bytes();
    if r.take(4)? != MAGIC || r.take(8)? != stamp || r.u32()? != extractors {
        return None;
    }
    let count = r.u32()? as usize;
    let mut files = Vec::with_capacity(count.min(1 << 20));
    for _ in 0..count {
        let path = r.str()?;
        let (len, modified, hash) = (r.u64()?, r.u64()?, r.u64()?);
        let block_len = r.u32()?;
        let at = (bytes.len() - r.0.len()) as u64;
        let block = r.take(block_len as usize)?;
        let symbols = unpack(block, pool)?;
        files.push(CachedFile {
            path: path.into(),
            len,
            modified,
            hash,
            symbols: Arc::new(symbols),
            // Kept for a library file (packed already: not packed again); the others drop it.
            packed: Some(Arc::from(block)),
            block: Some(Block { at, len: block_len }),
        });
    }
    Some(files)
}

/// The index's way back from a block ([`code_intelligence::Unpack`]): a library file's
/// members unpacked when first asked for (plan 74).
#[derive(Debug, Default)]
pub struct Codec;

/// No symbols: what a library file kept packed holds instead of its own (one for all of
/// them, not an empty one each).
pub fn no_symbols() -> Arc<FileSymbols> {
    static NONE: std::sync::LazyLock<Arc<FileSymbols>> = std::sync::LazyLock::new(Arc::default);
    Arc::clone(&NONE)
}

impl Codec {
    /// The one codec of the process: the same blocks of two projects (a library's file
    /// they both use) are known to unpack to the same symbols, so they're shared (plan 76b).
    pub fn shared() -> Arc<dyn code_intelligence::Unpack> {
        static CODEC: std::sync::LazyLock<Arc<dyn code_intelligence::Unpack>> =
            std::sync::LazyLock::new(|| Arc::new(Codec));
        Arc::clone(&CODEC)
    }
}

impl code_intelligence::Unpack for Codec {
    fn unpack(&self, bytes: &[u8]) -> Option<FileSymbols> {
        unpack(bytes, &mut Pool::default())
    }
}

/// A file's symbols packed: their encoded form, compressed — a block's bytes.
pub fn pack(symbols: &FileSymbols) -> Vec<u8> {
    let mut w = Writer(Vec::with_capacity(symbols.symbols.len() * 64));
    encode_symbols(&mut w, symbols);
    // Measured on a Laravel copy: a file's block comes to ~35% of its encoded form.
    kernel::cache::compress(&w.0)
}

/// A file's symbols from its block's bytes ([`pack`]); `None` if they aren't one.
pub fn unpack(block: &[u8], pool: &mut Pool) -> Option<FileSymbols> {
    let encoded = kernel::cache::decompress(block, MAX_UNPACKED)?;
    let mut r = Reader(&encoded);
    let symbols = decode_symbols(&mut r, pool)?;
    r.0.is_empty().then_some(symbols)
}

/// A file's block read from the cache file at `path` (where [`decode`] or
/// [`encode_with_blocks`] said it is): only its bytes, the rest of the file not read.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "a library file's members are read from their block (plan 74c)")
)]
pub fn read_block(path: &std::path::Path, block: Block) -> std::io::Result<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    file.seek(SeekFrom::Start(block.at))?;
    let mut bytes = vec![0; block.len as usize];
    file.read_exact(&mut bytes)?;
    Ok(bytes)
}

fn decode_symbols(r: &mut Reader<'_>, pool: &mut Pool) -> Option<FileSymbols> {
    let language = r.shared(pool)?;
    let namespace = r.shared_opt(pool)?;
    let uses = (0..r.u32()?)
        .map(|_| Some(UseImport { alias: r.shared(pool)?, fqn: r.shared(pool)? }))
        .collect::<Option<Vec<_>>>()?;
    // Exactly as many as stored (growing by doubling wasted up to half of each list).
    let symbol_count = r.u32()? as usize;
    let mut symbols = Vec::with_capacity(symbol_count.min(1 << 16));
    for _ in 0..symbol_count {
        let name = r.str()?;
        let kind = *KINDS.get(r.u8()? as usize)?;
        let fqn = r.str()?;
        let container = r.shared_opt(pool)?;
        let parents = (0..r.u32()?).map(|_| r.shared(pool)).collect::<Option<Vec<_>>>()?;
        let range = r.u64()? as usize..r.u64()? as usize;
        let offset = r.u64()? as usize;
        let line = r.u32()?;
        let detail = r.opt()?;
        let is_static = r.u8()? != 0;
        let visibility = match r.u8()? {
            1 => Visibility::Protected,
            2 => Visibility::Private,
            _ => Visibility::Public,
        };
        let note = r.opt()?;
        let types = r.opt()?.map(String::into_boxed_str);
        symbols.push(Symbol {
            name: name.into_boxed_str(),
            kind,
            fqn: fqn.into_boxed_str(),
            container,
            parents: parents.into(),
            range,
            offset,
            line,
            detail: detail.map(String::into_boxed_str),
            is_static,
            visibility,
            note: note.map(String::into_boxed_str),
            types,
        });
    }
    Some(FileSymbols { language, namespace, uses, symbols })
}

#[cfg(test)]
#[path = "../tests/unit/codec.rs"]
mod tests;
