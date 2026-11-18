//! A downloaded archive unpacked by our own code, never a program's: tarballs (gzip, xz),
//! zips, one gzipped program. Only files and folders are written — links, devices and the
//! rest are skipped —, executable bits kept, and a path that would leave the folder
//! (absolute, a drive, `..`) refuses the whole archive. What [`Rules`] say is applied on
//! the way: the archive's top folder left out, only some files kept, pruned paths not
//! written, what the caller's filter refuses left out.

use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub use crate::paths::{pruned, pruned_any};

/// How a downloaded file is unpacked, by its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    TarGz,
    TarXz,
    Zip,
    /// One gzipped program (taplo's releases).
    Gz,
    /// The file itself (a PHAR, a schema).
    File,
}

impl Format {
    pub fn of(name: &str) -> Self {
        let name = name.to_ascii_lowercase();
        if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Self::TarGz
        } else if name.ends_with(".tar.xz") || name.ends_with(".txz") {
            Self::TarXz
        } else if name.ends_with(".zip") {
            Self::Zip
        } else if name.ends_with(".gz") {
            Self::Gz
        } else {
            Self::File
        }
    }
}

/// What's left out of the start of each path in the archive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strip<'a> {
    #[default]
    None,
    /// Its first folder, whatever its name (npm's `package/`).
    First,
    /// This top folder; what isn't in it is skipped.
    Folder(&'a str),
}

/// What's written of an archive, and where.
#[derive(Debug, Clone, Copy, Default)]
pub struct Rules<'a> {
    pub strip: Strip<'a>,
    /// Only these paths (after `strip`); empty: everything.
    pub keep: &'a [String],
    /// Patterns of what's left out, relative to the component's folder ([`pruned_any`]).
    pub prune: &'a [String],
    /// Where the files go in the component's folder (`node_modules/<package>`; empty: its
    /// top).
    pub prefix: &'a str,
    /// Whether a file is written, by its path in the folder (`/`-separated, after `strip`
    /// and `prefix`); `None`: every one the other rules keep.
    pub filter: Option<fn(&str) -> bool>,
}

/// What was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Written {
    pub files: u64,
    pub bytes: u64,
}

/// The most an archive may unpack to (a bomb fills no disk).
pub const MAX_UNPACKED: u64 = 2 * 1024 * 1024 * 1024;

/// The archive at `archive` (its [`Format`] by `name`) unpacked under `into` by `rules`; a
/// single gzipped program is named `program`.
pub fn unpack_path(
    archive: &Path,
    name: &str,
    into: &Path,
    rules: &Rules<'_>,
    program: Option<&str>,
    stop: &AtomicBool,
) -> Result<Written, String> {
    let open = || std::fs::File::open(archive).map_err(|e| format!("{}: {e}", archive.display()));
    match Format::of(name) {
        Format::TarGz => {
            unpack_tar(flate2::read::GzDecoder::new(BufReader::new(open()?)), into, rules, stop)
        }
        Format::TarXz => {
            unpack_tar(lzma_rust2::XzReader::new(BufReader::new(open()?), true), into, rules, stop)
        }
        Format::Zip => unpack_zip(&mut BufReader::new(open()?), into, rules, stop),
        Format::Gz => {
            let program = program.ok_or_else(|| format!("{name}: a gzipped program unnamed"))?;
            let target = place(program, &Rules { strip: Strip::None, ..*rules })?
                .ok_or_else(|| format!("{name}: its program is left out"))?;
            let mut written = Written::default();
            write_file(
                into,
                &target,
                &mut flate2::read::GzDecoder::new(BufReader::new(open()?)),
                true,
                &mut written,
                stop,
            )?;
            Ok(written)
        }
        Format::File => {
            let target = place(name, &Rules { strip: Strip::None, ..*rules })?
                .ok_or_else(|| format!("{name}: left out"))?;
            let mut written = Written::default();
            write_file(into, &target, &mut open()?, false, &mut written, stop)?;
            Ok(written)
        }
    }
}

/// Where an archive's entry `name` goes, relative to the component's folder and
/// `/`-separated (`None`: not written — the top folder itself, outside the folder kept,
/// not kept, pruned); an error for one that would leave it.
pub fn place(name: &str, rules: &Rules<'_>) -> Result<Option<String>, String> {
    let outside = || Err(format!("an archive's entry outside its folder: {name}"));
    let name = name.replace('\\', "/");
    if name.starts_with('/') {
        return outside();
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in name.split('/') {
        match part {
            "" | "." => {}
            ".." => return outside(),
            // A drive (`C:`), an alternate stream: never a plain name.
            p if p.contains(':') => return outside(),
            p => parts.push(p),
        }
    }
    let parts = match rules.strip {
        Strip::None => &parts[..],
        Strip::First => parts.get(1..).unwrap_or(&[]),
        Strip::Folder(top) => match parts.split_first() {
            Some((first, rest)) if *first == top => rest,
            _ => return Ok(None),
        },
    };
    if parts.is_empty() {
        return Ok(None);
    }
    let relative = parts.join("/");
    if !rules.keep.is_empty() && !rules.keep.contains(&relative) {
        return Ok(None);
    }
    let full = if rules.prefix.is_empty() {
        relative
    } else {
        format!("{}/{relative}", rules.prefix.trim_end_matches('/'))
    };
    if pruned_any(rules.prune, &full) || rules.filter.is_some_and(|keep| !keep(&full)) {
        return Ok(None);
    }
    Ok(Some(full))
}

/// `relative`'s path under `into`.
fn path_in(into: &Path, relative: &str) -> PathBuf {
    relative.split('/').fold(into.to_path_buf(), |path, part| path.join(part))
}

/// `body` written as file `relative` under `into` (executable: `exec`).
fn write_file(
    into: &Path,
    relative: &str,
    body: &mut dyn Read,
    exec: bool,
    written: &mut Written,
    stop: &AtomicBool,
) -> Result<(), String> {
    let path = path_in(into, relative);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut file = std::fs::File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        let n = body.read(&mut buffer).map_err(|e| format!("{relative}: {e}"))?;
        if n == 0 {
            break;
        }
        written.bytes += n as u64;
        if written.bytes > MAX_UNPACKED {
            return Err("the archive unpacks to more than it may".into());
        }
        file.write_all(&buffer[..n]).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    written.files += 1;
    drop(file);
    set_executable(&path, exec)
}

#[cfg(unix)]
fn set_executable(path: &Path, exec: bool) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    let mode = if exec { 0o755 } else { 0o644 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(not(unix))]
fn set_executable(_path: &Path, _exec: bool) -> Result<(), String> {
    Ok(())
}

/// A tarball (decompressed as it's read) unpacked under `into` by `rules`: ustar, pax and
/// GNU long names.
pub fn unpack_tar(
    reader: impl Read,
    into: &Path,
    rules: &Rules<'_>,
    stop: &AtomicBool,
) -> Result<Written, String> {
    let mut reader = reader;
    std::fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    let mut written = Written::default();
    // A pax or GNU header's name, for the entry after it.
    let mut long_name: Option<String> = None;
    let mut header = [0u8; 512];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        match read_block(&mut reader, &mut header)? {
            false => break,
            true if header.iter().all(|b| *b == 0) => break,
            true => {}
        }
        let field = |from: usize, len: usize| {
            let raw = &header[from..from + len];
            let end = raw.iter().position(|b| *b == 0).unwrap_or(len);
            String::from_utf8_lossy(&raw[..end]).into_owned()
        };
        let size = tar_number(&header[124..136])
            .ok_or_else(|| "a tarball entry without a size".to_owned())?;
        let mode = tar_number(&header[100..108]).unwrap_or(0o644);
        let kind = header[156];
        let name = match long_name.take() {
            Some(name) => name,
            None => {
                let (prefix, name) = (field(345, 155), field(0, 100));
                let ustar = &header[257..262] == b"ustar";
                if ustar && !prefix.is_empty() { format!("{prefix}/{name}") } else { name }
            }
        };
        let mut body = (&mut reader).take(size);
        match kind {
            // pax extended header: its `path` names the next entry.
            b'x' => {
                let mut text = Vec::new();
                body.read_to_end(&mut text).map_err(|e| e.to_string())?;
                long_name = pax_path(&text);
            }
            // GNU long name.
            b'L' => {
                let mut text = Vec::new();
                body.read_to_end(&mut text).map_err(|e| e.to_string())?;
                let end = text.iter().position(|b| *b == 0).unwrap_or(text.len());
                long_name = Some(String::from_utf8_lossy(&text[..end]).into_owned());
            }
            b'0' | 0 | b'7' => match place(&name, rules)? {
                Some(relative) => {
                    write_file(into, &relative, &mut body, mode & 0o111 != 0, &mut written, stop)?;
                }
                None => skip(&mut body)?,
            },
            b'5' => {
                if let Some(relative) = place(&name, rules)? {
                    let dir = path_in(into, &relative);
                    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
                }
                skip(&mut body)?;
            }
            // Global pax headers, links, devices: nothing written.
            _ => skip(&mut body)?,
        }
        if body.limit() > 0 {
            return Err("a tarball cut short".into());
        }
        // The body's padding to the next block.
        let pad = (512 - size % 512) % 512;
        skip(&mut (&mut reader).take(pad))?;
    }
    Ok(written)
}

/// The next 512-byte block (`false`: the archive ended there).
fn read_block(reader: &mut impl Read, block: &mut [u8; 512]) -> Result<bool, String> {
    let mut at = 0;
    while at < block.len() {
        match reader.read(&mut block[at..]) {
            Ok(0) if at == 0 => return Ok(false),
            Ok(0) => return Err("a tarball cut short".into()),
            Ok(n) => at += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(format!("not a readable tarball: {e}")),
        }
    }
    Ok(true)
}

fn skip(reader: &mut impl Read) -> Result<(), String> {
    std::io::copy(reader, &mut std::io::sink()).map(drop).map_err(|e| e.to_string())
}

/// A tar header's number: octal text, or base-256 (the high bit set) for large ones.
fn tar_number(field: &[u8]) -> Option<u64> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        return Some(field[1..].iter().fold(0u64, |n, b| (n << 8) | u64::from(*b)));
    }
    let text = String::from_utf8_lossy(field);
    let digits = text.trim_matches(|c: char| c == '\0' || c == ' ');
    if digits.is_empty() {
        return Some(0);
    }
    u64::from_str_radix(digits, 8).ok()
}

/// A pax header's `path` record.
fn pax_path(body: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(body);
    text.lines().find_map(|line| {
        let (_, record) = line.split_once(' ')?;
        record.strip_prefix("path=").map(str::to_owned)
    })
}

/// A zip (stored or deflated entries; zip64's sizes and offsets read) unpacked under `into`
/// by `rules`.
pub fn unpack_zip<R: Read + Seek>(
    file: &mut R,
    into: &Path,
    rules: &Rules<'_>,
    stop: &AtomicBool,
) -> Result<Written, String> {
    std::fs::create_dir_all(into).map_err(|e| format!("{}: {e}", into.display()))?;
    let bad = |what: &str| format!("not a readable zip: {what}");
    let io = |e: std::io::Error| bad(&e.to_string());
    let len = file.seek(SeekFrom::End(0)).map_err(io)?;
    // The end of the central directory: in the last 64 KiB (its comment) + 22 bytes.
    let tail_len = len.min(65_557);
    file.seek(SeekFrom::Start(len - tail_len)).map_err(io)?;
    let mut tail = vec![0u8; tail_len as usize];
    file.read_exact(&mut tail).map_err(io)?;
    let at = (0..tail.len().saturating_sub(21))
        .rev()
        .find(|&i| tail[i..i + 4] == [0x50, 0x4b, 0x05, 0x06])
        .ok_or_else(|| bad("no end of its directory"))?;
    let u16_at = |b: &[u8], i: usize| u16::from_le_bytes([b[i], b[i + 1]]);
    let u32_at = |b: &[u8], i: usize| u32::from_le_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]);
    let u64_at = |b: &[u8], i: usize| u64::from_le_bytes(b[i..i + 8].try_into().unwrap_or([0; 8]));
    let eocd = &tail[at..];
    let mut entries = u64::from(u16_at(eocd, 10));
    let mut dir_size = u64::from(u32_at(eocd, 12));
    let mut dir_offset = u64::from(u32_at(eocd, 16));
    if entries == 0xFFFF || dir_offset == 0xFFFF_FFFF || dir_size == 0xFFFF_FFFF {
        // zip64: its locator just before, pointing at its own end record.
        let locator = at.checked_sub(20).ok_or_else(|| bad("no zip64 locator"))?;
        if tail[locator..locator + 4] != [0x50, 0x4b, 0x06, 0x07] {
            return Err(bad("no zip64 locator"));
        }
        let record = u64_at(&tail, locator + 8);
        file.seek(SeekFrom::Start(record)).map_err(io)?;
        let mut end = [0u8; 56];
        file.read_exact(&mut end).map_err(io)?;
        if end[..4] != [0x50, 0x4b, 0x06, 0x06] {
            return Err(bad("no zip64 end record"));
        }
        entries = u64_at(&end, 32);
        dir_size = u64_at(&end, 40);
        dir_offset = u64_at(&end, 48);
    }
    if dir_size > 256 * 1024 * 1024 || dir_offset.saturating_add(dir_size) > len {
        return Err(bad("its directory is outside it"));
    }
    file.seek(SeekFrom::Start(dir_offset)).map_err(io)?;
    let mut dir = vec![0u8; dir_size as usize];
    file.read_exact(&mut dir).map_err(io)?;
    let mut written = Written::default();
    let mut i = 0usize;
    for _ in 0..entries {
        if stop.load(Ordering::Relaxed) {
            return Err("stopped".into());
        }
        if i + 46 > dir.len() || dir[i..i + 4] != [0x50, 0x4b, 0x01, 0x02] {
            return Err(bad("a broken directory entry"));
        }
        let made_by = u16_at(&dir, i + 4);
        let flags = u16_at(&dir, i + 8);
        let method = u16_at(&dir, i + 10);
        let mut compressed = u64::from(u32_at(&dir, i + 20));
        let mut size = u64::from(u32_at(&dir, i + 24));
        let name_len = usize::from(u16_at(&dir, i + 28));
        let extra_len = usize::from(u16_at(&dir, i + 30));
        let comment_len = usize::from(u16_at(&dir, i + 32));
        let external = u32_at(&dir, i + 38);
        let mut offset = u64::from(u32_at(&dir, i + 42));
        let name_end = i + 46 + name_len;
        let extra_end = name_end + extra_len;
        if extra_end + comment_len > dir.len() {
            return Err(bad("a broken directory entry"));
        }
        let name = String::from_utf8_lossy(&dir[i + 46..name_end]).into_owned();
        // zip64's extra field: the sizes and the offset that didn't fit, in that order.
        let mut extra = &dir[name_end..extra_end];
        while extra.len() >= 4 {
            let (id, len) = (u16_at(extra, 0), usize::from(u16_at(extra, 2)));
            let data = extra.get(4..4 + len).unwrap_or(&[]);
            if id == 1 {
                let mut values = data.as_chunks::<8>().0.iter().map(|c| u64::from_le_bytes(*c));
                if size == 0xFFFF_FFFF {
                    size = values.next().unwrap_or(size);
                }
                if compressed == 0xFFFF_FFFF {
                    compressed = values.next().unwrap_or(compressed);
                }
                if offset == 0xFFFF_FFFF {
                    offset = values.next().unwrap_or(offset);
                }
            }
            extra = extra.get(4 + len..).unwrap_or(&[]);
        }
        i = extra_end + comment_len;
        if flags & 1 != 0 {
            return Err(bad("an encrypted entry"));
        }
        // The Unix mode, when it was made on Unix.
        let unix = (made_by >> 8 == 3).then_some(external >> 16);
        if unix.is_some_and(|m| m & 0o170_000 == 0o120_000) {
            // A link: skipped.
            continue;
        }
        let Some(relative) = place(&name, rules)? else { continue };
        if name.ends_with('/') {
            let path = path_in(into, &relative);
            std::fs::create_dir_all(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            continue;
        }
        // The entry's own header, then its data.
        file.seek(SeekFrom::Start(offset)).map_err(io)?;
        let mut local = [0u8; 30];
        file.read_exact(&mut local).map_err(io)?;
        if local[..4] != [0x50, 0x4b, 0x03, 0x04] {
            return Err(bad("a broken entry"));
        }
        let skip_len = u64::from(u16_at(&local, 26)) + u64::from(u16_at(&local, 28));
        file.seek(SeekFrom::Current(skip_len as i64)).map_err(io)?;
        let exec = unix.is_some_and(|m| m & 0o111 != 0);
        let mut data = (&mut *file).take(compressed);
        match method {
            0 => write_file(into, &relative, &mut data, exec, &mut written, stop)?,
            8 => write_file(
                into,
                &relative,
                &mut flate2::read::DeflateDecoder::new(data).take(size.max(1) * 2 + 1024),
                exec,
                &mut written,
                stop,
            )?,
            m => return Err(bad(&format!("{name}: compression method {m}"))),
        }
    }
    Ok(written)
}

#[cfg(test)]
#[path = "../tests/unit/archive.rs"]
mod tests;
