use super::*;

#[test]
fn round_trips_every_encoding() {
    for encoding in [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be] {
        for line_ending in [LineEnding::Lf, LineEnding::CrLf] {
            let bytes = encode("ăîș\nțâ\n", encoding, line_ending);
            let d = decode(&bytes);
            assert_eq!(d, Decoded { text: "ăîș\nțâ\n".into(), encoding, line_ending });
        }
    }
}

#[test]
fn invalid_utf8_falls_back_to_latin1() {
    let d = decode(&[b'c', 0xE9]);
    assert_eq!(d.text, "cé");
    assert_eq!(d.encoding, Encoding::Latin1);
    assert_eq!(encode(&d.text, d.encoding, d.line_ending), vec![b'c', 0xE9]);
}

#[test]
fn majority_line_ending_wins() {
    assert_eq!(decode(b"a\r\nb\r\nc\n").line_ending, LineEnding::CrLf);
    assert_eq!(decode(b"a\nb\nc\r\n").line_ending, LineEnding::Lf);
}

#[test]
fn text_binary_and_too_large() {
    assert_eq!(check_text(5, b"<?php"), TextCheck::Text);
    assert_eq!(check_text(8, b"\0\0\0\x18ftyp"), TextCheck::Binary);
    // UTF-16 has NUL bytes but is text.
    assert_eq!(check_text(6, &[0xFF, 0xFE, b'a', 0, b'b', 0]), TextCheck::Text);
    assert_eq!(check_text(MAX_TEXT_LEN + 1, b"a,b,c\n"), TextCheck::TooLarge);
}

/// Encoding a text in pieces gives what encoding it whole gives, line terminators split by
/// the pieces' edges included.
#[test]
fn encoding_in_pieces_is_encoding_whole() {
    let text = "a\nșb\n\nc€\n";
    let pieces = ["a", "\nș", "b\n", "\nc€", "\n"];
    for encoding in
        [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be, Encoding::Latin1]
    {
        for ending in [LineEnding::Lf, LineEnding::CrLf] {
            let whole = encode(text, encoding, ending);
            assert_eq!(encode_chunks(pieces, text.len(), encoding, ending), whole);
        }
    }
    assert_eq!(encode("a\nb", Encoding::Utf8, LineEnding::CrLf), b"a\r\nb");
    assert!(represents_chunks(["é", "x"], Encoding::Latin1));
    assert!(!represents_chunks(["é", "ș"], Encoding::Latin1));
    assert!(represents_chunks(["ș"], Encoding::Utf8));
}

/// Read as the encoding the user says: a Latin-1 file's bytes, UTF-16 without its mark, a
/// mark of its own skipped; UTF-8 told with or without its mark as the file has it (a save
/// never adds or drops one unasked: converting does); a UTF-16 file's odd last byte shown;
/// the terminator found as when opened.
#[test]
fn bytes_read_as_the_encoding_chosen() {
    let latin = decode_as(&[b'c', 0xE2, b'\r', b'\n'], Encoding::Latin1);
    assert_eq!((latin.text.as_str(), latin.line_ending), ("câ\n", LineEnding::CrLf));
    let utf16 = decode_as(&[b'h', 0, b'i', 0], Encoding::Utf16Le);
    assert_eq!(utf16.text, "hi");
    let marked = decode_as(&[0xFE, 0xFF, 0, b'o'], Encoding::Utf16Be);
    assert_eq!((marked.text.as_str(), marked.encoding), ("o", Encoding::Utf16Be));
    let utf8 = decode_as(&[0xEF, 0xBB, 0xBF, b'x'], Encoding::Utf8);
    assert_eq!((utf8.text.as_str(), utf8.encoding), ("x", Encoding::Utf8Bom));
    assert_eq!(decode_as(b"x", Encoding::Utf8Bom).encoding, Encoding::Utf8, "no mark to keep");
    assert_eq!(decode_as(&[b'h', 0, b'i'], Encoding::Utf16Le).text, "h\u{FFFD}");
    assert_eq!(decode_as(&[0xFF], Encoding::Utf8).text, "\u{FFFD}");
}
