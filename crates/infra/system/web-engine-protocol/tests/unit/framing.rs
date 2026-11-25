use super::*;

/// A length past the limit stops the stream for good, nothing allocated for it; broken
/// bytes too; a message cut short waits for the rest.
#[test]
fn a_hostile_stream_is_refused() {
    let mut reader = Reader::default();
    let length = u32::try_from(MAX_MESSAGE + 1).unwrap();
    reader.push(&length.to_le_bytes());
    assert_eq!(reader.message::<u32>(), Err(FramingError::TooLong(MAX_MESSAGE + 1)));
    reader.push(&[0; 8]);
    assert!(reader.message::<u32>().is_err(), "stays broken");

    let mut reader = Reader::default();
    reader.push(&3u32.to_le_bytes());
    reader.push(&[0xff, 0xff]);
    assert_eq!(reader.message::<String>(), Ok(None), "cut short: waits");
    reader.push(&[0xff]);
    assert_eq!(reader.message::<String>(), Err(FramingError::Malformed));
}

/// Many messages read one after another keep the buffer small (what's read is dropped).
#[test]
fn the_buffer_doesnt_grow_with_what_was_read() {
    let mut reader = Reader::default();
    let mut bytes = Vec::new();
    for n in 0..10_000u32 {
        bytes.clear();
        write(&n, &mut bytes).unwrap();
        reader.push(&bytes);
        assert_eq!(reader.message::<u32>(), Ok(Some(n)));
    }
    assert!(reader.pending.capacity() < 1024, "{}", reader.pending.capacity());
}

/// One large message read: once drained, the buffer gives back what it grew to.
#[test]
fn a_large_message_doesnt_hold_its_room() {
    let mut reader = Reader::default();
    let mut bytes = Vec::new();
    let large = vec![7u8; 8 * KEPT_CAPACITY];
    write(&large, &mut bytes).unwrap();
    reader.push(&bytes);
    assert_eq!(reader.message::<Vec<u8>>(), Ok(Some(large)));
    assert!(reader.pending.capacity() <= KEPT_CAPACITY, "{}", reader.pending.capacity());
    // A writer's buffer the same.
    release_excess(&mut bytes);
    assert!(bytes.capacity() <= KEPT_CAPACITY && bytes.is_empty());
    // A small one keeps its room (no allocation at every message).
    let mut small = Vec::with_capacity(KEPT_CAPACITY);
    small.push(1);
    release_excess(&mut small);
    assert_eq!(small, [1]);
}

/// What can't be encoded.
struct Unwritable;

impl serde::Serialize for Unwritable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("refused"))
    }
}

/// A message that can't be encoded leaves the buffer as it was: what came before it, its
/// room.
#[test]
fn a_failed_encoding_keeps_the_buffer() {
    let mut out = Vec::with_capacity(4096);
    write(&1u32, &mut out).unwrap();
    let before = out.clone();
    assert_eq!(write(&Unwritable, &mut out), Err(FramingError::Malformed));
    assert_eq!(out, before);
    assert!(out.capacity() >= 4096);
}
