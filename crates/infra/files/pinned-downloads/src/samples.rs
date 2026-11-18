//! Archives made in memory (tarballs, gzip, xz, zips), as release tools write them: what
//! tests unpack, here and in the crates that download.

#![allow(clippy::unwrap_used)]

use std::io::Write;
use std::path::PathBuf;

/// A folder of the temporary folder for test `name`.
pub fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("pinned-archive-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A tarball's entry: its name, type, mode and body.
#[derive(Debug)]
pub struct Entry<'a>(pub &'a str, pub u8, pub u32, pub &'a [u8]);

/// A tarball (ustar headers, a GNU long name where a name doesn't fit).
pub fn tar(entries: &[Entry<'_>]) -> Vec<u8> {
    fn header(out: &mut Vec<u8>, name: &str, kind: u8, mode: u32, size: usize) {
        let mut h = [0u8; 512];
        h[..name.len().min(100)].copy_from_slice(&name.as_bytes()[..name.len().min(100)]);
        h[100..107].copy_from_slice(format!("{mode:07o}").as_bytes());
        h[124..135].copy_from_slice(format!("{size:011o}").as_bytes());
        h[156] = kind;
        h[257..263].copy_from_slice(b"ustar\0");
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|b| u32::from(*b)).sum();
        h[148..155].copy_from_slice(format!("{sum:06o}\0").as_bytes());
        out.extend_from_slice(&h);
    }
    let mut out = Vec::new();
    for Entry(name, kind, mode, body) in entries {
        if name.len() > 100 {
            header(&mut out, "././@LongLink", b'L', 0o644, name.len() + 1);
            out.extend_from_slice(name.as_bytes());
            out.push(0);
            out.resize(out.len().div_ceil(512) * 512, 0);
        }
        header(&mut out, name, *kind, *mode, body.len());
        out.extend_from_slice(body);
        out.resize(out.len().div_ceil(512) * 512, 0);
    }
    out.extend_from_slice(&[0u8; 1024]);
    out
}

/// `bytes` gzipped.
pub fn gz(bytes: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(bytes).unwrap();
    e.finish().unwrap()
}

/// `bytes` in xz.
pub fn xz(bytes: &[u8]) -> Vec<u8> {
    let mut w =
        lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(1)).unwrap();
    w.write_all(bytes).unwrap();
    w.finish().unwrap()
}

/// A zip's entry: its name, its Unix mode (`None`: made on Windows), its body, deflated.
#[derive(Debug)]
pub struct ZipEntry<'a>(pub &'a str, pub Option<u32>, pub &'a [u8], pub bool);

/// A zip (no data descriptors, as release tools write them).
pub fn zip(entries: &[ZipEntry<'_>]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for ZipEntry(name, mode, body, deflate) in entries {
        let data = if *deflate {
            let mut e = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::fast());
            e.write_all(body).unwrap();
            e.finish().unwrap()
        } else {
            body.to_vec()
        };
        let offset = out.len() as u32;
        let method: u16 = if *deflate { 8 } else { 0 };
        let mut crc = flate2::Crc::new();
        crc.update(body);
        let local = [
            &0x0403_4b50u32.to_le_bytes()[..],
            &20u16.to_le_bytes(),
            &0u16.to_le_bytes(),
            &method.to_le_bytes(),
            &[0; 4],
            &crc.sum().to_le_bytes(),
            &(data.len() as u32).to_le_bytes(),
            &(body.len() as u32).to_le_bytes(),
            &(name.len() as u16).to_le_bytes(),
            &0u16.to_le_bytes(),
        ]
        .concat();
        out.extend_from_slice(&local);
        out.extend_from_slice(name.as_bytes());
        out.extend_from_slice(&data);
        let (made_by, external): (u16, u32) = match mode {
            Some(m) => ((3 << 8) | 20, m << 16),
            None => (20, 0),
        };
        let central = [
            &0x0201_4b50u32.to_le_bytes()[..],
            &made_by.to_le_bytes(),
            &20u16.to_le_bytes(),
            &0u16.to_le_bytes(),
            &method.to_le_bytes(),
            &[0; 4],
            &crc.sum().to_le_bytes(),
            &(data.len() as u32).to_le_bytes(),
            &(body.len() as u32).to_le_bytes(),
            &(name.len() as u16).to_le_bytes(),
            &0u16.to_le_bytes(),
            &0u16.to_le_bytes(),
            &0u16.to_le_bytes(),
            &0u16.to_le_bytes(),
            &external.to_le_bytes(),
            &offset.to_le_bytes(),
        ]
        .concat();
        directory.extend_from_slice(&central);
        directory.extend_from_slice(name.as_bytes());
    }
    let at = out.len() as u32;
    out.extend_from_slice(&directory);
    let end = [
        &0x0605_4b50u32.to_le_bytes()[..],
        &0u16.to_le_bytes(),
        &0u16.to_le_bytes(),
        &(entries.len() as u16).to_le_bytes(),
        &(entries.len() as u16).to_le_bytes(),
        &(directory.len() as u32).to_le_bytes(),
        &at.to_le_bytes(),
        &0u16.to_le_bytes(),
    ]
    .concat();
    out.extend_from_slice(&end);
    out
}
