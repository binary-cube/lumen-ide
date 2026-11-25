//! The package's archive, written by our own code: a ustar tarball (GNU long names for
//! paths past 100 bytes) compressed as xz, the format the app's unpacking reads.
//!
//! Deterministic and bare: entries sorted by path, every one owned by 0:0 at a fixed time,
//! folders 0755, files 0755 when executable and 0644 otherwise. Only the files' bytes go
//! in: no extended attributes, no AppleDouble `._*` companions, no Finder files — what a
//! system `tar` adds on macOS and what breaks a signed bundle's check once unpacked.

use std::io::{self, Read, Write};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};

/// The time every entry carries (the epoch): an archive of the same files is the same bytes.
const MTIME: u64 = 0;

/// A tar block's bytes.
const BLOCK: usize = 512;

/// The longest name a ustar header holds by itself; longer ones get a GNU long-name entry.
const NAME_LEN: usize = 100;

/// The largest entry an 11-digit octal size says (8 GiB).
const MAX_ENTRY: u64 = 0o77_777_777_777;

/// The xz preset: `tar -J`'s default, the size the packages were measured at.
const XZ_PRESET: u32 = 6;

/// The bytes each xz block holds, compressed on its own (in parallel): three times preset 6's
/// 8 MiB dictionary, so splitting costs little of the ratio. Fixed, so the output doesn't
/// depend on the machine's cores.
const XZ_BLOCK: u64 = 24 * 1024 * 1024;

/// The most threads compressing at once (each holds a block and its encoder, ~100 MB).
const MAX_WORKERS: u32 = 8;

/// What an entry holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Folder,
    /// A file on disk, read as it's written.
    File {
        source: PathBuf,
        exec: bool,
    },
    /// Bytes made here (`engine.json`).
    Bytes(Vec<u8>),
}

/// One entry: its path in the archive (`/`-separated, relative) and what it holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: String,
    pub body: Body,
}

/// What was written: the archive's bytes and its SHA-256 (hex).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Packed {
    pub size: u64,
    pub sha256: String,
}

/// Names never packed: the system's own files beside a bundle's (AppleDouble companions,
/// the Finder's folder settings).
pub fn system_file(name: &str) -> bool {
    name.starts_with("._") || name == ".DS_Store"
}

/// Whether the file at `path` is executable (any `x` bit; on Windows none is kept).
#[cfg(unix)]
pub fn executable(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::PermissionsExt;
    Ok(std::fs::metadata(path)?.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub fn executable(_path: &Path) -> io::Result<bool> {
    Ok(false)
}

/// A writer that counts and hashes what passes through it.
struct Hashing<W> {
    inner: W,
    hash: ring::digest::Context,
    size: u64,
}

impl<W: Write> Write for Hashing<W> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(bytes)?;
        self.hash.update(&bytes[..n]);
        self.size += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// `entries` (sorted here by path) packed as a `.tar.xz` at `archive`.
pub fn pack(entries: &mut [Entry], archive: &Path) -> Result<Packed, String> {
    entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let file = std::fs::File::create(archive).map_err(|e| format!("{}: {e}", archive.display()))?;
    let hashing = Hashing {
        inner: io::BufWriter::new(file),
        hash: ring::digest::Context::new(&ring::digest::SHA256),
        size: 0,
    };
    let mut options = lzma_rust2::XzOptions::with_preset(XZ_PRESET);
    options.set_block_size(NonZeroU64::new(XZ_BLOCK));
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| u32::try_from(n.get()).unwrap_or(MAX_WORKERS))
        .clamp(1, MAX_WORKERS);
    let mut xz =
        lzma_rust2::XzWriterMt::new(hashing, options, workers).map_err(|e| format!("xz: {e}"))?;
    write_tar(&mut xz, entries)?;
    let mut hashing = xz.finish().map_err(|e| format!("xz: {e}"))?;
    hashing.flush().map_err(|e| format!("{}: {e}", archive.display()))?;
    Ok(Packed { size: hashing.size, sha256: crate::release::hex(hashing.hash.finish().as_ref()) })
}

/// The tarball of `entries`, in their order, ended by its two empty blocks.
pub fn write_tar(out: &mut impl Write, entries: &[Entry]) -> Result<(), String> {
    for entry in entries {
        write_entry(out, entry).map_err(|e| format!("{}: {e}", entry.path))?;
    }
    out.write_all(&[0u8; BLOCK * 2]).map_err(|e| e.to_string())
}

fn write_entry(out: &mut impl Write, entry: &Entry) -> io::Result<()> {
    let name = match entry.body {
        Body::Folder => format!("{}/", entry.path.trim_end_matches('/')),
        _ => entry.path.clone(),
    };
    if name.len() > NAME_LEN {
        let mut long = name.clone().into_bytes();
        long.push(0);
        out.write_all(&header(&Header {
            name: "././@LongLink",
            mode: 0o644,
            size: long.len() as u64,
            kind: b'L',
        })?)?;
        out.write_all(&long)?;
        pad(out, long.len() as u64)?;
    }
    let short = cut(&name, NAME_LEN);
    match &entry.body {
        Body::Folder => {
            out.write_all(&header(&Header { name: short, mode: 0o755, size: 0, kind: b'5' })?)
        }
        Body::Bytes(bytes) => {
            let size = bytes.len() as u64;
            out.write_all(&header(&Header { name: short, mode: 0o644, size, kind: b'0' })?)?;
            out.write_all(bytes)?;
            pad(out, size)
        }
        Body::File { source, exec } => {
            let mut file = std::fs::File::open(source)?;
            let size = file.metadata()?.len();
            let mode = if *exec { 0o755 } else { 0o644 };
            out.write_all(&header(&Header { name: short, mode, size, kind: b'0' })?)?;
            let copied = io::copy(&mut (&mut file).take(size), out)?;
            if copied != size {
                return Err(io::Error::other("the file changed while it was packed"));
            }
            pad(out, size)
        }
    }
}

/// `text` cut to at most `max` bytes, never inside a character.
fn cut(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Zeros to the end of the block an entry's `size` bytes ended in.
fn pad(out: &mut impl Write, size: u64) -> io::Result<()> {
    let rest = (BLOCK as u64 - size % BLOCK as u64) % BLOCK as u64;
    out.write_all(&[0u8; BLOCK][..rest as usize])
}

/// A header's own fields; the rest are fixed (owner, time, ustar's mark).
struct Header<'a> {
    name: &'a str,
    mode: u32,
    size: u64,
    kind: u8,
}

/// A ustar header block.
fn header(fields: &Header<'_>) -> io::Result<[u8; BLOCK]> {
    if fields.size > MAX_ENTRY {
        return Err(io::Error::other("an entry too large for a tar header"));
    }
    let mut block = [0u8; BLOCK];
    let name = fields.name.as_bytes();
    block[..name.len()].copy_from_slice(name);
    octal(&mut block[100..108], u64::from(fields.mode));
    octal(&mut block[108..116], 0);
    octal(&mut block[116..124], 0);
    octal(&mut block[124..136], fields.size);
    octal(&mut block[136..148], MTIME);
    block[156] = fields.kind;
    block[257..263].copy_from_slice(b"ustar\0");
    block[263..265].copy_from_slice(b"00");
    octal(&mut block[329..337], 0);
    octal(&mut block[337..345], 0);
    // The checksum: the header's bytes summed with its own field as spaces.
    block[148..156].copy_from_slice(b"        ");
    let sum: u64 = block.iter().map(|b| u64::from(*b)).sum();
    let digits = format!("{sum:06o}\0 ");
    block[148..156].copy_from_slice(digits.as_bytes());
    Ok(block)
}

/// `value` in octal, zero-padded to the field's width less its closing NUL.
fn octal(field: &mut [u8], value: u64) {
    let width = field.len() - 1;
    let digits = format!("{value:0width$o}");
    field[..width].copy_from_slice(&digits.as_bytes()[digits.len() - width..]);
    field[width] = 0;
}

#[cfg(test)]
#[path = "../../tests/unit/web_engine/tar.rs"]
mod tests;
