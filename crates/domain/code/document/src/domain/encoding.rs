//! Detecting and converting the encoding and the line terminator.

/// The file's encoding on disk. Internally, text is always UTF-8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Encoding {
    #[default]
    Utf8,
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    /// ISO-8859-1: used when the file is not valid UTF-8. Each byte becomes a character.
    Latin1,
}

impl Encoding {
    /// Every encoding a file can be read or written in, as the chooser lists them.
    pub const ALL: [Encoding; 5] =
        [Encoding::Utf8, Encoding::Utf8Bom, Encoding::Utf16Le, Encoding::Utf16Be, Encoding::Latin1];

    pub fn label(&self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Latin1 => "ISO-8859-1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    #[default]
    Lf,
    CrLf,
}

impl LineEnding {
    pub fn label(&self) -> &'static str {
        match self {
            LineEnding::Lf => "LF",
            LineEnding::CrLf => "CRLF",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    /// Text with lines separated only by `\n`.
    pub text: String,
    pub encoding: Encoding,
    pub line_ending: LineEnding,
}

/// `bytes` read as `encoding` (the user says what the file is: a Latin-1 file taken for
/// UTF-8, a UTF-16 one without its mark); its mark skipped if it has its own. UTF-8 is
/// told with or without its mark as the file has it (asked either way: a save never adds
/// or drops one unasked). Invalid sequences become U+FFFD (a UTF-16 file's odd last byte
/// too: seen, never lost unseen); the line terminator found as by [`decode`].
pub fn decode_as(bytes: &[u8], encoding: Encoding) -> Decoded {
    let mut encoding = encoding;
    let text = match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => {
            let rest = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]);
            encoding = if rest.is_some() { Encoding::Utf8Bom } else { Encoding::Utf8 };
            String::from_utf8_lossy(rest.unwrap_or(bytes)).into_owned()
        }
        Encoding::Utf16Le => {
            decode_utf16(bytes.strip_prefix(&[0xFF, 0xFE]).unwrap_or(bytes), u16::from_le_bytes)
        }
        Encoding::Utf16Be => {
            decode_utf16(bytes.strip_prefix(&[0xFE, 0xFF]).unwrap_or(bytes), u16::from_be_bytes)
        }
        Encoding::Latin1 => bytes.iter().map(|&b| b as char).collect(),
    };
    let (text, line_ending) = split_lines(text);
    Decoded { text, encoding, line_ending }
}

/// The text with `\n` only, and the terminator it had most of.
fn split_lines(text: String) -> (String, LineEnding) {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    let line_ending = if crlf > lf { LineEnding::CrLf } else { LineEnding::Lf };
    let text = if crlf > 0 { text.replace("\r\n", "\n") } else { text };
    (text, line_ending)
}

pub fn decode(bytes: &[u8]) -> Decoded {
    let (text, encoding) = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        (String::from_utf8_lossy(rest).into_owned(), Encoding::Utf8Bom)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        (decode_utf16(rest, u16::from_le_bytes), Encoding::Utf16Le)
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        (decode_utf16(rest, u16::from_be_bytes), Encoding::Utf16Be)
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => (s.to_owned(), Encoding::Utf8),
            Err(_) => (bytes.iter().map(|&b| b as char).collect(), Encoding::Latin1),
        }
    };

    // The majority terminator decides how we save; internally we normalize to `\n`.
    let (text, line_ending) = split_lines(text);
    Decoded { text, encoding, line_ending }
}

/// Whether every character of `text` can be written in `encoding` (Latin-1 has only the
/// first 256).
pub fn represents(text: &str, encoding: Encoding) -> bool {
    represents_chunks([text], encoding)
}

/// [`represents`] over a text in pieces (a buffer's chunks: no copy of it whole); only a
/// Latin-1 file has anything to look at.
pub fn represents_chunks<'a>(
    chunks: impl IntoIterator<Item = &'a str>,
    encoding: Encoding,
) -> bool {
    encoding != Encoding::Latin1
        || chunks.into_iter().all(|chunk| chunk.chars().all(|c| u32::from(c) <= 0xFF))
}

pub fn encode(text: &str, encoding: Encoding, line_ending: LineEnding) -> Vec<u8> {
    encode_chunks([text], text.len(), encoding, line_ending)
}

/// [`encode`] of a text in pieces (a buffer's chunks, `len` bytes in all), written straight
/// into the result: no copy of the text whole, none for the line terminators.
pub fn encode_chunks<'a>(
    chunks: impl IntoIterator<Item = &'a str>,
    len: usize,
    encoding: Encoding,
    line_ending: LineEnding,
) -> Vec<u8> {
    let wide = matches!(encoding, Encoding::Utf16Le | Encoding::Utf16Be);
    let mut out = Vec::with_capacity(if wide { len * 2 + 2 } else { len + 3 });
    match encoding {
        Encoding::Utf8Bom => out.extend_from_slice(&[0xEF, 0xBB, 0xBF]),
        Encoding::Utf16Le => out.extend_from_slice(&[0xFF, 0xFE]),
        Encoding::Utf16Be => out.extend_from_slice(&[0xFE, 0xFF]),
        Encoding::Utf8 | Encoding::Latin1 => {}
    }
    let push = |out: &mut Vec<u8>, text: &str| match encoding {
        Encoding::Utf8 | Encoding::Utf8Bom => out.extend_from_slice(text.as_bytes()),
        Encoding::Utf16Le => out.extend(text.encode_utf16().flat_map(u16::to_le_bytes)),
        Encoding::Utf16Be => out.extend(text.encode_utf16().flat_map(u16::to_be_bytes)),
        // Characters outside Latin-1 have no representation; we replace them with `?`.
        Encoding::Latin1 => {
            out.extend(text.chars().map(|c| u8::try_from(u32::from(c)).unwrap_or(b'?')));
        }
    };
    for chunk in chunks {
        match line_ending {
            LineEnding::Lf => push(&mut out, chunk),
            LineEnding::CrLf => {
                for (i, line) in chunk.split('\n').enumerate() {
                    if i > 0 {
                        push(&mut out, "\r\n");
                    }
                    push(&mut out, line);
                }
            }
        }
    }
    out
}

fn decode_utf16(bytes: &[u8], from_bytes: fn([u8; 2]) -> u16) -> String {
    let (pairs, odd) = bytes.as_chunks::<2>();
    let units = pairs.iter().map(|&c| from_bytes(c));
    let mut text: String =
        char::decode_utf16(units).map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER)).collect();
    // A byte left alone isn't half of anything: shown as such.
    if !odd.is_empty() {
        text.push(char::REPLACEMENT_CHARACTER);
    }
    text
}

/// How many bytes from a file's start decide whether it's text.
pub const SNIFF_LEN: usize = 8 * 1024;
/// Larger files aren't opened as text (a whole log or dump would take gigabytes as a
/// document: lines, syntax tree, undo).
pub const MAX_TEXT_LEN: u64 = 64 * 1024 * 1024;

/// Whether a file can be opened as text, from its size and its first bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextCheck {
    Text,
    /// Not text (a video, an archive, an executable…): a NUL byte near its start.
    Binary,
    /// Text perhaps, but too large to edit.
    TooLarge,
}

/// Decides from `len` (the file's size) and `head` (its first [`SNIFF_LEN`] bytes) — like
/// git: a NUL byte means binary, except in UTF-16 text (it has a BOM).
pub fn check_text(len: u64, head: &[u8]) -> TextCheck {
    let utf16 = head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]);
    if !utf16 && head.iter().take(SNIFF_LEN).any(|&b| b == 0) {
        TextCheck::Binary
    } else if len > MAX_TEXT_LEN {
        TextCheck::TooLarge
    } else {
        TextCheck::Text
    }
}

#[cfg(test)]
#[path = "../../tests/unit/domain/encoding.rs"]
mod tests;
