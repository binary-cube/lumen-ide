//! Messages on a stream: each one its length (4 bytes, little-endian) then its `postcard`
//! bytes. [`Reader`] takes whatever bytes arrive and gives back whole messages; a length
//! past [`MAX_MESSAGE`] stops it for good (the other end is broken or hostile: the
//! connection is let go), never allocated.

use serde::Serialize;
use serde::de::DeserializeOwned;

/// The longest message (bytes): an HTML page or an app's message, the largest things sent
/// (the app refuses a page's message past 64 MB).
pub const MAX_MESSAGE: usize = 96 << 20;

/// The length's bytes before each message.
const LENGTH: usize = 4;

/// The bytes a reused buffer keeps once drained (a reader's, a writer's): room for the
/// usual messages, so one large message (an HTML page) doesn't hold its size for good.
pub const KEPT_CAPACITY: usize = 1 << 20;
/// Past this capacity, a drained buffer is brought back to [`KEPT_CAPACITY`] (below it, the
/// room is kept: shrinking at every message would allocate at every message).
const SHRINK_PAST: usize = 4 * KEPT_CAPACITY;

/// A drained buffer (`buffer` empty, or about to be reused) brought back to
/// [`KEPT_CAPACITY`] when one large message grew it far past that.
pub fn release_excess(buffer: &mut Vec<u8>) {
    if buffer.capacity() > SHRINK_PAST {
        buffer.clear();
        buffer.shrink_to(KEPT_CAPACITY);
    }
}

/// `out` appended to by the encoder, never taken from its owner (an encoding that fails
/// leaves it as it was, its room kept).
struct Appended<'a>(&'a mut Vec<u8>);

impl Extend<u8> for Appended<'_> {
    fn extend<I: IntoIterator<Item = u8>>(&mut self, bytes: I) {
        self.0.extend(bytes);
    }
}

/// Why a stream can't be read any more.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramingError {
    /// A message said to be longer than [`MAX_MESSAGE`].
    TooLong(usize),
    /// A message that isn't one of ours (another protocol, broken bytes).
    Malformed,
}

impl std::fmt::Display for FramingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLong(n) => write!(f, "a message of {n} bytes, past the limit"),
            Self::Malformed => f.write_str("a message that can't be read"),
        }
    }
}

impl std::error::Error for FramingError {}

/// `message` framed, appended to `out` (a buffer reused from one message to the next).
pub fn write<T: Serialize>(message: &T, out: &mut Vec<u8>) -> Result<(), FramingError> {
    let start = out.len();
    out.extend_from_slice(&[0; LENGTH]);
    if postcard::to_extend(message, Appended(out)).is_err() {
        out.truncate(start);
        return Err(FramingError::Malformed);
    }
    let length = out.len() - start - LENGTH;
    let header = u32::try_from(length).ok().filter(|_| length <= MAX_MESSAGE);
    let Some(header) = header else {
        out.truncate(start);
        return Err(FramingError::TooLong(length));
    };
    out[start..start + LENGTH].copy_from_slice(&header.to_le_bytes());
    Ok(())
}

/// Whole messages out of the bytes as they arrive.
#[derive(Debug, Default)]
pub struct Reader {
    /// What arrived and isn't a whole message yet (the part already read dropped at each
    /// [`Self::message`] that finds one).
    pending: Vec<u8>,
    /// Where the next message starts in `pending`.
    at: usize,
    broken: Option<FramingError>,
}

impl Reader {
    /// Bytes that arrived.
    pub fn push(&mut self, bytes: &[u8]) {
        // What's read is dropped before the buffer grows (no copy per message).
        if self.at > 0 && self.at == self.pending.len() {
            self.pending.clear();
            self.at = 0;
        }
        self.pending.extend_from_slice(bytes);
    }

    /// The next whole message (`Ok(None)`: more bytes are needed). An error stays: the
    /// stream can't be read further.
    pub fn message<T: DeserializeOwned>(&mut self) -> Result<Option<T>, FramingError> {
        if let Some(broken) = &self.broken {
            return Err(broken.clone());
        }
        let rest = &self.pending[self.at..];
        let Some(header) = rest.get(..LENGTH) else { return Ok(None) };
        let mut length = [0; LENGTH];
        length.copy_from_slice(header);
        let length = u32::from_le_bytes(length) as usize;
        if length > MAX_MESSAGE {
            return Err(self.fail(FramingError::TooLong(length)));
        }
        let Some(body) = rest.get(LENGTH..LENGTH + length) else { return Ok(None) };
        let message = postcard::from_bytes(body).map_err(|_| FramingError::Malformed);
        self.at += LENGTH + length;
        if self.at == self.pending.len() {
            // All of it read: emptied, and a large message's room let go.
            self.pending.clear();
            self.at = 0;
            release_excess(&mut self.pending);
        } else if self.at > self.pending.len() / 2 {
            // Most of the buffer read: what's left moved to its start, so it doesn't grow.
            self.pending.drain(..self.at);
            self.at = 0;
        }
        match message {
            Ok(message) => Ok(Some(message)),
            Err(e) => Err(self.fail(e)),
        }
    }

    fn fail(&mut self, error: FramingError) -> FramingError {
        self.broken = Some(error.clone());
        self.pending = Vec::new();
        self.at = 0;
        error
    }
}

#[cfg(test)]
#[path = "../tests/unit/framing.rs"]
mod tests;
