use super::*;

/// The published FNV-1a values: the same in every run, so what's kept under them is found.
#[test]
fn hashes_are_the_published_ones() {
    assert_eq!(fnv64(""), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv64("a"), 0xaf63_dc4c_8601_ec8c);
    assert_eq!(fnv32(""), 2_166_136_261);
    assert_eq!(fnv32("a"), 0xe40c_292c);
}
