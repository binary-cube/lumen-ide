use super::*;

#[test]
fn stamps_differ_by_format() {
    assert_ne!(cache_stamp(1), cache_stamp(2));
    assert_eq!(cache_stamp(3), cache_stamp(3));
}

#[test]
fn memo_recomputes_only_on_new_key() {
    let mut memo = Memo::default();
    let mut runs = 0;
    for key in [1, 1, 2, 2, 1] {
        memo.get_or_update(key, || {
            runs += 1;
            key * 10
        });
    }
    assert_eq!(runs, 3);
    assert_eq!(memo.get_if(|k| *k == 1), Some(&10));
    memo.purge();
    assert_eq!(memo.key(), None);
}

#[test]
fn generations_keep_recently_used() {
    let mut cache: GenerationalCache<Box<str>, u32> = GenerationalCache::new(2);
    cache.insert("a".into(), 1);
    cache.insert("b".into(), 2);
    // "c" fills the generation: a, b move to the previous one.
    cache.insert("c".into(), 3);
    // "a" is used: it moves back to the current one.
    assert_eq!(cache.get("a"), Some(&1));
    // The current generation (c, a) fills up again: "b", unused, goes away.
    cache.insert("d".into(), 4);
    assert_eq!(cache.get("b"), None);
    assert_eq!(cache.get("a"), Some(&1));
    cache.purge();
    assert!(cache.is_empty());
}

#[test]
fn retain_filters_both_generations() {
    let mut cache: GenerationalCache<u32, u32> = GenerationalCache::new(2);
    for key in 1..=4 {
        cache.insert(key, key * 10);
    }
    // 1, 2 are in the previous generation, 3, 4 in the current one.
    cache.retain(|k, _| k % 2 == 0);
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(&1), None);
    assert_eq!(cache.get(&2), Some(&20));
    assert_eq!(cache.get(&4), Some(&40));
}

#[test]
fn on_demand_data_is_read_on_use_and_let_go_when_idle() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static READS: AtomicUsize = AtomicUsize::new(0);
    static DATA: OnDemand<Vec<u32>> = OnDemand::new(|| {
        READS.fetch_add(1, Ordering::Relaxed);
        vec![1, 2, 3]
    });
    assert!(!DATA.is_loaded());
    assert_eq!(DATA.get().len(), 3);
    let held = DATA.get();
    assert_eq!(READS.load(Ordering::Relaxed), 1, "read once");
    // Just used: kept.
    assert!(!DATA.release_if_idle(std::time::Duration::from_secs(60)));
    assert!(DATA.release_if_idle(std::time::Duration::ZERO));
    assert!(!DATA.is_loaded());
    assert_eq!(held.len(), 3, "who holds it keeps it");
    DATA.get();
    assert_eq!(READS.load(Ordering::Relaxed), 2, "read again when needed");
}

/// Data kept packed: unpacked once at the first ask and kept; asked "once", unpacked without
/// keeping it; bytes that aren't what the unpacking reads give nothing, and it's asked again
/// later; what's compressed comes back as it was, smaller when it repeats.
#[test]
fn packed_data_unpacks_once_and_keeps_it() {
    let text = "the same words again and again, ".repeat(40);
    let bytes: std::sync::Arc<[u8]> = compress(text.as_bytes()).into();
    assert!(bytes.len() < text.len() / 4, "repeated text compresses");
    assert_eq!(decompress(&bytes, 1 << 20).as_deref(), Some(text.as_bytes()));
    assert_eq!(decompress(&bytes, 10), None, "past the limit");
    assert_eq!(decompress(b"not deflate", 1 << 20), None);
    let unpacks = std::cell::Cell::new(0);
    let unpack = |b: &[u8]| {
        unpacks.set(unpacks.get() + 1);
        decompress(b, 1 << 20).and_then(|v| String::from_utf8(v).ok())
    };
    let packed = Packed::new(std::sync::Arc::clone(&bytes));
    assert_eq!(packed.once(unpack).as_deref(), Some(&text), "once: not kept");
    assert!(packed.unpacked().is_none());
    assert_eq!(packed.get(unpack), Some(&text));
    assert_eq!(packed.get(unpack), Some(&text));
    assert_eq!(unpacks.get(), 2, "once, then kept");
    assert!(matches!(packed.once(unpack), Some(std::borrow::Cow::Borrowed(_))), "the kept one");
    let broken = Packed::<String>::new(std::sync::Arc::from(&b"broken"[..]));
    assert_eq!(broken.get(unpack), None);
    assert!(broken.unpacked().is_none(), "nothing kept: asked again next time");
}

#[test]
fn the_same_content_is_held_once_while_someone_holds_it() {
    let interner: Interner<String> = Interner::default();
    let make = |text: &'static str| move || text.to_owned();
    let one = interner.get_or_make(1, |v| v == "vendor/a.php", make("vendor/a.php"));
    let two = interner.get_or_make(1, |v| v == "vendor/a.php", make("vendor/a.php"));
    assert!(std::sync::Arc::ptr_eq(&one, &two), "the same content: one value");
    // The same hash, another content: another value, both kept.
    let other = interner.get_or_make(1, |v| v == "vendor/b.php", make("vendor/b.php"));
    assert!(!std::sync::Arc::ptr_eq(&one, &other));
    assert_eq!(interner.len(), 2);
    drop((one, two, other));
    assert!(interner.is_empty(), "gone with its last holder");
    let again = interner.get_or_make(1, |v| v == "vendor/a.php", make("vendor/a.php"));
    assert_eq!(*again, "vendor/a.php", "made again");
}

/// Made outside the lock: the value held is had while another thread makes the next one;
/// one made for an older key is returned, not kept over a newer one.
#[test]
fn a_shared_memo_is_read_while_the_next_value_is_made() {
    use std::sync::{Arc, mpsc};
    let memo = Arc::new(SharedMemo::<u64, &'static str>::default());
    assert_eq!(memo.get_or_make(1, || "one"), "one");
    let (started, wait) = mpsc::channel();
    let (go, release) = mpsc::channel::<()>();
    let making = {
        let memo = Arc::clone(&memo);
        std::thread::spawn(move || {
            memo.get_or_make(2, || {
                started.send(()).unwrap();
                release.recv().unwrap();
                "two"
            })
        })
    };
    wait.recv().unwrap();
    assert_eq!(memo.get_or_make(1, || unreachable!()), "one", "not waiting for the next");
    go.send(()).unwrap();
    assert_eq!(making.join().unwrap(), "two");
    assert_eq!(memo.get_or_make(2, || unreachable!()), "two");
    assert_eq!(memo.get_or_make(1, || "old"), "old", "an older key: made, returned");
    assert_eq!(memo.get_or_make(2, || unreachable!()), "two", "not kept over the newer");
}

/// More in use at once than a generation holds (each turn bringing the rest back): it grows,
/// up to its most, and keeps them all; used less again, it goes back down.
#[test]
fn a_growing_cache_keeps_what_is_used_at_once() {
    let mut cache: GenerationalCache<u32, u32> = GenerationalCache::growing(100, 400);
    let mut misses = 0;
    for _ in 0..10 {
        for k in 0..300 {
            if cache.get(&k).is_none() {
                misses += 1;
                cache.insert(k, k);
            }
        }
    }
    assert!(cache.limit() >= 300, "{}", cache.limit());
    let before = misses;
    for k in 0..300 {
        if cache.get(&k).is_none() {
            misses += 1;
            cache.insert(k, k);
        }
    }
    assert_eq!(misses, before, "all kept now");
    // New keys only, nothing used again: back down to where it started.
    for k in 1_000..20_000 {
        cache.insert(k, k);
    }
    assert_eq!(cache.limit(), 100);
    let fixed: GenerationalCache<u32, u32> = GenerationalCache::new(100);
    assert_eq!(fixed.limit(), 100);
}
