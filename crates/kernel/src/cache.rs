//! Shared cache primitives: all caches in the app are built from them, so the rules
//! (invalidation, size limit, clearing) are the same everywhere.
//!
//! - [`Memo`]: a single value computed for a key (e.g. the syntax colors of the visible
//!   area for one document version). The key describes everything that affects the
//!   result; a new key means recomputing.
//! - [`SharedMemo`]: a [`Memo`] shared between threads, made outside its lock: whoever
//!   asks for the value it holds never waits for another thread making a newer one.
//! - [`GenerationalCache`]: many values, with a bounded size: the current generation and
//!   the previous one. When the current one fills up, it becomes the previous one, and
//!   whatever hasn't been used since goes away — an approximate LRU, without linked lists
//!   and without clearing everything at once.
//! - [`Purge`]: what can be cleared at the user's request ("Clear and Rebuild Caches").
//! - [`OnDemand`]: a module's data (a dictionary, stubs) read on first use, let go when it
//!   hasn't been used for a while, read again when needed.
//! - [`Packed`]: data kept compressed in memory (cold: rarely read, many of them — a
//!   library file's members), unpacked once at the first ask; with [`compress`] /
//!   [`decompress`], the same deflate everywhere (plan 74).
//! - [`Interner`]: values the same for whoever makes them from the same content (a
//!   library's file, its symbols packed, in every project using that version of it), held
//!   once for the whole process while anyone holds them (plan 76b). Different content is a
//!   different value: nothing of one owner reaches another unless it is the very same.

use std::borrow::Borrow;
use std::collections::{HashMap, HashSet};
use std::hash::Hash;

/// Something that holds recomputable data and can release it (the "Clear and Rebuild
/// Caches" command, low memory). After clearing, everything must work the same, just
/// slower the first time.
pub trait Purge {
    fn purge(&mut self);
}

/// A value memoized for a key.
#[derive(Debug, Clone)]
pub struct Memo<K, V> {
    entry: Option<(K, V)>,
}

impl<K, V> Default for Memo<K, V> {
    fn default() -> Self {
        Self { entry: None }
    }
}

impl<K: PartialEq, V> Memo<K, V> {
    /// The value for `key`, computed with `compute` only if the key changed.
    pub fn get_or_update(&mut self, key: K, compute: impl FnOnce() -> V) -> &V {
        if self.entry.as_ref().is_some_and(|(k, _)| *k != key) {
            self.entry = None;
        }
        &self.entry.get_or_insert_with(|| (key, compute())).1
    }

    /// The value, if the memoized key satisfies `valid` (e.g. an area that contains the
    /// requested one).
    pub fn get_if(&self, valid: impl FnOnce(&K) -> bool) -> Option<&V> {
        self.entry.as_ref().filter(|(k, _)| valid(k)).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: K, value: V) -> &V {
        let (_, v) = self.entry.insert((key, value));
        v
    }

    /// The memoized value (whatever the key).
    pub fn value(&self) -> Option<&V> {
        self.entry.as_ref().map(|(_, v)| v)
    }

    pub fn key(&self) -> Option<&K> {
        self.entry.as_ref().map(|(k, _)| k)
    }
}

impl<K, V> Purge for Memo<K, V> {
    fn purge(&mut self) {
        self.entry = None;
    }
}

/// A value memoized for an ordered key (a generation), shared between threads: made
/// outside the lock, so a thread asking for the value held (the UI thread) never waits
/// while another makes the next one (a framework's model, from many files). A value made
/// for an older key than the one held is returned, not kept.
#[derive(Debug)]
pub struct SharedMemo<K, V> {
    entry: std::sync::Mutex<Option<(K, V)>>,
}

impl<K, V> Default for SharedMemo<K, V> {
    fn default() -> Self {
        Self { entry: std::sync::Mutex::new(None) }
    }
}

impl<K: PartialOrd + Copy, V: Clone> SharedMemo<K, V> {
    /// The value for `key`: the one held, else made with `make` (the lock not held
    /// meanwhile) and kept unless a newer one came.
    pub fn get_or_make(&self, key: K, make: impl FnOnce() -> V) -> V {
        if let Some((_, value)) = self.lock().as_ref().filter(|(k, _)| *k == key) {
            return value.clone();
        }
        let value = make();
        let mut entry = self.lock();
        if entry.as_ref().is_none_or(|(k, _)| *k < key) {
            *entry = Some((key, value.clone()));
        }
        value
    }

    /// The value held, whatever its key.
    pub fn value(&self) -> Option<V> {
        self.lock().as_ref().map(|(_, v)| v.clone())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<(K, V)>> {
        self.entry.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

/// A key's hash (what a growing cache remembers of the keys it let go).
fn hash_of<K: Hash>(key: &K) -> u64 {
    use std::hash::Hasher as _;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}

/// Many values, with a bounded size (see the module documentation).
#[derive(Debug, Clone)]
pub struct GenerationalCache<K, V> {
    current: HashMap<K, V>,
    previous: HashMap<K, V>,
    /// How many entries a generation holds before it becomes the previous one.
    limit: usize,
    /// What the limit starts at and may grow to ([`Self::growing`]; the same: fixed).
    base: usize,
    most: usize,
    /// Entries of the previous generation used again since the last turn.
    promoted: usize,
    /// A growing one: the keys (their hashes) let go at the last two turns, and how many of
    /// them were asked for again since the last (more in use than both generations hold).
    gone: [HashSet<u64>; 2],
    back: usize,
}

impl<K: Hash + Eq, V> GenerationalCache<K, V> {
    pub fn new(limit: usize) -> Self {
        Self::growing(limit, limit)
    }

    /// [`Self::new`], its limit growing (up to `most`) while what's used again keeps coming
    /// back from the previous generation — more than a generation holds is in use at once (a
    /// big screen of a grid's cells): kept instead of made again at each turn —, and going
    /// back down when it no longer is.
    pub fn growing(limit: usize, most: usize) -> Self {
        let limit = limit.max(1);
        Self {
            current: HashMap::new(),
            previous: HashMap::new(),
            limit,
            base: limit,
            most: most.max(limit),
            promoted: 0,
            gone: [HashSet::new(), HashSet::new()],
            back: 0,
        }
    }

    /// How many entries a generation holds now.
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// The value for `key` (also looked up by a borrowed form, e.g. `&str` for `Box<str>`,
    /// without allocating). One from the previous generation moves back to the current one.
    pub fn get<Q>(&mut self, key: &Q) -> Option<&V>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized + ToOwnedKey<K>,
    {
        if !self.current.contains_key(key) {
            let value = self.previous.remove(key)?;
            self.promoted += 1;
            self.insert(key.to_owned_key(), value);
        }
        self.current.get(key)
    }

    pub fn insert(&mut self, key: K, value: V) {
        let growing = self.most > self.base;
        if growing && self.gone.iter().any(|g| !g.is_empty()) {
            let hash = hash_of(&key);
            if self.gone.iter().any(|g| g.contains(&hash)) {
                self.back += 1;
            }
        }
        if self.current.len() >= self.limit && !self.current.contains_key(&key) {
            // Half of what's in it came back (from the previous generation, or let go and
            // asked for again): what's in use at once is more than it holds (it would be made
            // again at each turn) — it holds more. Hardly any: less again.
            let reused = self.promoted + self.back;
            if reused >= self.limit / 2 && self.limit < self.most {
                self.limit = (self.limit * 2).min(self.most);
            } else if reused < self.limit / 8 && self.limit > self.base {
                self.limit = (self.limit / 2).max(self.base);
            }
            (self.promoted, self.back) = (0, 0);
            if self.current.len() >= self.limit {
                let dropped =
                    std::mem::replace(&mut self.previous, std::mem::take(&mut self.current));
                if growing {
                    self.gone.swap(0, 1);
                    self.gone[0] = dropped.keys().map(hash_of).collect();
                }
            }
        }
        self.current.insert(key, value);
    }

    /// How many entries there are in total (both generations).
    pub fn len(&self) -> usize {
        self.current.len() + self.previous.len()
    }

    pub fn is_empty(&self) -> bool {
        self.current.is_empty() && self.previous.is_empty()
    }

    /// Every value (both generations), without moving any.
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.current.values().chain(self.previous.values())
    }

    /// Keeps only the entries `keep` accepts (in both generations).
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &V) -> bool) {
        self.current.retain(|k, v| keep(k, v));
        self.previous.retain(|k, v| keep(k, v));
    }

    pub fn clear(&mut self) {
        self.current = HashMap::new();
        self.previous = HashMap::new();
    }
}

impl<K: Hash + Eq, V> Purge for GenerationalCache<K, V> {
    fn purge(&mut self) {
        self.clear();
    }
}

/// The owned key for a borrowed form (e.g. `&str` → `Box<str>`), used when an entry
/// moves up from the previous generation to the current one.
pub trait ToOwnedKey<K> {
    fn to_owned_key(&self) -> K;
}

impl ToOwnedKey<Box<str>> for str {
    fn to_owned_key(&self) -> Box<str> {
        self.into()
    }
}

impl ToOwnedKey<std::sync::Arc<str>> for str {
    fn to_owned_key(&self) -> std::sync::Arc<str> {
        self.into()
    }
}

impl ToOwnedKey<std::path::PathBuf> for std::path::Path {
    fn to_owned_key(&self) -> std::path::PathBuf {
        self.to_path_buf()
    }
}

impl<K: Clone> ToOwnedKey<K> for K {
    fn to_owned_key(&self) -> K {
        self.clone()
    }
}

/// The stamp written in the header of an on-disk cache: the app version combined with
/// the cache's own `format` version. A cache with another stamp (older / newer app, or a
/// format change) is ignored and rebuilt — a new release never reads data it might
/// misinterpret.
pub fn cache_stamp(format: u32) -> u64 {
    // FNV-1a over the version text and the format number.
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in crate::brand::version().bytes().chain(format.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash
}

/// Data read on first use (`load`), shared (`Arc`: whoever holds it keeps it), and let go
/// when unused for a while ([`release_if_idle`](Self::release_if_idle)) or on request —
/// read again on the next use. Usable in a `static`.
pub struct OnDemand<T> {
    slot: std::sync::RwLock<Option<std::sync::Arc<T>>>,
    /// When it was last used (milliseconds since [`epoch`]).
    used: std::sync::atomic::AtomicU64,
    load: fn() -> T,
}

/// The time base of [`OnDemand`]'s use stamps.
fn epoch() -> std::time::Instant {
    static EPOCH: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    *EPOCH.get_or_init(std::time::Instant::now)
}

fn now_ms() -> u64 {
    epoch().elapsed().as_millis() as u64
}

impl<T> OnDemand<T> {
    pub const fn new(load: fn() -> T) -> Self {
        Self {
            slot: std::sync::RwLock::new(None),
            used: std::sync::atomic::AtomicU64::new(0),
            load,
        }
    }

    /// The data, read now if it isn't in memory.
    pub fn get(&self) -> std::sync::Arc<T> {
        self.used.store(now_ms(), std::sync::atomic::Ordering::Relaxed);
        if let Ok(slot) = self.slot.read()
            && let Some(data) = slot.as_ref()
        {
            return std::sync::Arc::clone(data);
        }
        let mut slot = match self.slot.write() {
            Ok(slot) => slot,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Another thread may have read it meanwhile.
        std::sync::Arc::clone(slot.get_or_insert_with(|| std::sync::Arc::new((self.load)())))
    }

    pub fn is_loaded(&self) -> bool {
        self.slot.read().is_ok_and(|slot| slot.is_some())
    }

    /// `look` at the data if it's in memory, without reading it nor counting as a use (the
    /// Task Manager weighing it).
    pub fn peek<R>(&self, look: impl FnOnce(&T) -> R) -> Option<R> {
        self.slot.read().ok().and_then(|slot| slot.as_deref().map(look))
    }

    /// Lets the data go (it's read again on the next use). `true` if it was in memory.
    pub fn release(&self) -> bool {
        self.slot.write().is_ok_and(|mut slot| slot.take().is_some())
    }

    /// Lets the data go if it wasn't used for `idle`. `true` if it was released.
    pub fn release_if_idle(&self, idle: std::time::Duration) -> bool {
        let used = self.used.load(std::sync::atomic::Ordering::Relaxed);
        let unused = now_ms().saturating_sub(used);
        unused >= idle.as_millis() as u64 && self.release()
    }
}

impl<T> std::fmt::Debug for OnDemand<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OnDemand").field("loaded", &self.is_loaded()).finish()
    }
}

/// Data kept packed (its bytes, in the format its owner gives: usually [`compress`]ed),
/// unpacked once at the first ask, then kept: what's handed out stays valid as long as the
/// `Packed` does. Measured on a library file's symbols (plan 74): a third of their encoded
/// form, unpacked in microseconds.
#[derive(Debug)]
pub struct Packed<T> {
    bytes: std::sync::Arc<[u8]>,
    unpacked: std::sync::OnceLock<std::sync::Arc<T>>,
}

impl<T> Packed<T> {
    pub fn new(bytes: std::sync::Arc<[u8]>) -> Self {
        Self { bytes, unpacked: std::sync::OnceLock::new() }
    }

    /// Its bytes (to write elsewhere as they are, to share).
    pub fn bytes(&self) -> &std::sync::Arc<[u8]> {
        &self.bytes
    }

    /// Its value: unpacked by `unpack` the first time, kept after (`None`: they weren't
    /// what `unpack` reads — asked again next time).
    pub fn get(&self, unpack: impl FnOnce(&[u8]) -> Option<T>) -> Option<&T> {
        if let Some(value) = self.unpacked.get() {
            return Some(value);
        }
        let value = std::sync::Arc::new(unpack(&self.bytes)?);
        Some(self.unpacked.get_or_init(|| value))
    }

    /// Its value if it was unpacked already.
    pub fn unpacked(&self) -> Option<&T> {
        self.unpacked.get().map(|v| &**v)
    }

    /// Its value for this once: the one kept, or unpacked now and not kept (a walk over
    /// many: it doesn't bring them all back into memory).
    pub fn once(&self, unpack: impl FnOnce(&[u8]) -> Option<T>) -> Option<std::borrow::Cow<'_, T>>
    where
        T: Clone,
    {
        match self.unpacked() {
            Some(value) => Some(std::borrow::Cow::Borrowed(value)),
            None => unpack(&self.bytes).map(std::borrow::Cow::Owned),
        }
    }
}

/// Values made from their content, held once for the whole process while anyone holds them
/// (weakly here: the last holder gone, so is the value). Found by a hash of their content,
/// then compared whole (`same`): two contents with the same hash are two values.
#[derive(Debug)]
pub struct Interner<V> {
    buckets: std::sync::Mutex<HashMap<u64, Vec<std::sync::Weak<V>>>>,
    /// Added since the buckets were last swept of the values gone.
    added: std::sync::atomic::AtomicUsize,
}

impl<V> Default for Interner<V> {
    fn default() -> Self {
        Self { buckets: std::sync::Mutex::default(), added: std::sync::atomic::AtomicUsize::new(0) }
    }
}

/// A stable hash of `bytes` (64-bit FNV-1a: the same in every run): what an [`Interner`]
/// finds a content by.
pub fn content_hash(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0xcbf2_9ce4_8422_2325, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3))
}

/// How many values are added between two sweeps of the ones gone.
const SWEEP_EVERY: usize = 4096;

impl<V> Interner<V> {
    /// The value of the content hashed `hash`: the one held already if `same` says it is
    /// its, else `make`'s (held from now on).
    pub fn get_or_make(
        &self,
        hash: u64,
        same: impl Fn(&V) -> bool,
        make: impl FnOnce() -> V,
    ) -> std::sync::Arc<V> {
        let mut buckets = self.buckets.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let bucket = buckets.entry(hash).or_default();
        bucket.retain(|v| v.strong_count() > 0);
        if let Some(held) = bucket.iter().filter_map(std::sync::Weak::upgrade).find(|v| same(v)) {
            return held;
        }
        let value = std::sync::Arc::new(make());
        bucket.push(std::sync::Arc::downgrade(&value));
        let added = self.added.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        if added.is_multiple_of(SWEEP_EVERY) {
            buckets.retain(|_, bucket| {
                bucket.retain(|v| v.strong_count() > 0);
                !bucket.is_empty()
            });
        }
        value
    }

    /// The values held now (someone holds them).
    pub fn len(&self) -> usize {
        let buckets = self.buckets.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        buckets.values().flatten().filter(|v| v.strong_count() > 0).count()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Deflate's default level: unpacking is as fast at every level (what's packed is unpacked
/// when first asked for, someone waiting), packing is done in the background. Measured on
/// a Laravel project's 8,251 library blocks (plan 76): level 1 3,373 KB, level 6 3,016 KB
/// (−11%), both unpacked in ~6.5 µs a block; Snappy 4,186 KB, LZ4 4,323 KB (unpacked in
/// under 1 µs, but kept larger: memory is what this saves).
const LEVEL: u8 = 6;

/// `bytes` compressed (deflate, [`LEVEL`]): [`decompress`] gives them back.
pub fn compress(bytes: &[u8]) -> Vec<u8> {
    miniz_oxide::deflate::compress_to_vec(bytes, LEVEL)
}

/// What [`compress`] made, back (`None`: it isn't, or it unpacks past `limit` bytes).
pub fn decompress(bytes: &[u8], limit: usize) -> Option<Vec<u8>> {
    miniz_oxide::inflate::decompress_to_vec_with_limit(bytes, limit).ok()
}

#[cfg(test)]
#[path = "../tests/unit/cache.rs"]
mod tests;
