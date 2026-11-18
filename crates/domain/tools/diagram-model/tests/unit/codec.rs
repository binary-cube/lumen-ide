use super::*;

/// Base64 and URI encoding as draw.io's compression uses them.
#[test]
fn base64_and_uri_encoding_are_read() {
    assert_eq!(base64("aGVsbG8=").unwrap(), b"hello");
    assert_eq!(base64("aGVs\nbG8").unwrap(), b"hello", "whitespace and no padding");
    assert!(base64("a$b").is_none());
    assert_eq!(uri_decode("%C3%9Cn%20%26").unwrap(), "Ün &");
    assert!(uri_decode("%C3").is_none(), "not UTF-8");
    assert!(uri_decode("%G1").is_none());
}

/// A compressed page larger than allowed once inflated is refused, not inflated whole.
#[test]
fn a_page_inflating_past_its_limit_is_refused() {
    let big = vec![b'a'; 10_000];
    let packed = miniz_oxide::deflate::compress_to_vec(&big, 6);
    let text = encode(&packed);
    assert_eq!(decompress(&text, 1_000), Err(Error::TooLarge));
    assert_eq!(decompress(&text, 20_000).unwrap().len(), 10_000);
}

/// Base64 for the test's own data.
fn encode(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, b)| n | u32::from(*b) << (16 - 8 * i));
        for i in 0..=chunk.len() {
            out.push(ABC[(n >> (18 - 6 * i) & 63) as usize] as char);
        }
    }
    out
}
