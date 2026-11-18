use database::Value;

use super::*;

/// A page of `n` rows of one text of `width` bytes.
fn page(n: usize, width: usize) -> Arc<Rows> {
    Arc::new(Rows { rows: vec![vec![Value::Text("x".repeat(width))]; n], ..Rows::default() })
}

fn key(offset: u64) -> PageKey {
    PageKey { offset, limit: 10 }
}

#[test]
fn pages_are_kept_and_found_again() {
    let mut cache = PageCache::new(DEFAULT_LIMIT);
    cache.insert(key(0), page(10, 10));
    cache.insert(key(10), page(10, 10));
    assert_eq!(cache.len(), 2);
    assert_eq!(cache.get(key(10)).map(|r| r.len()), Some(10));
    assert!(cache.get(key(20)).is_none());
    assert_eq!(cache.bytes(), page(10, 10).bytes() * 2);
}

#[test]
fn past_its_limit_the_pages_farthest_from_the_last_read_go_first() {
    let one = page(10, 1000).bytes();
    let mut cache = PageCache::new(one * 3);
    for offset in [0, 10, 20, 30] {
        cache.insert(key(offset), page(10, 1000));
    }
    assert!(cache.bytes() <= one * 3);
    assert!(cache.get(key(0)).is_none(), "the farthest from 30");
    assert!(cache.get(key(30)).is_some() && cache.get(key(20)).is_some());
    // Back near the start: the far end goes now.
    cache.insert(key(0), page(10, 1000));
    assert!(cache.get(key(30)).is_none());
    assert!(cache.get(key(0)).is_some());
}

#[test]
fn a_page_larger_than_the_limit_is_still_kept_alone() {
    let mut cache = PageCache::new(100);
    cache.insert(key(0), page(10, 10));
    cache.insert(key(10), page(10, 1000));
    assert_eq!(cache.len(), 1);
    assert!(cache.get(key(10)).is_some());
}

#[test]
fn clear_caches_lets_every_page_go() {
    let mut cache = PageCache::new(DEFAULT_LIMIT);
    cache.insert(key(0), page(10, 10));
    cache.purge();
    assert!(cache.is_empty());
    assert_eq!(cache.bytes(), 0);
    assert!(cache.get(key(0)).is_none());
}
