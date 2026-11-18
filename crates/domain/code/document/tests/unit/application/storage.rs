use super::*;
use std::cell::RefCell;
use std::collections::HashMap;

#[derive(Default)]
struct MemoryStorage {
    files: RefCell<HashMap<PathBuf, Vec<u8>>>,
}

impl DocumentStorage for MemoryStorage {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.files.borrow().get(path).cloned().ok_or_else(|| io::ErrorKind::NotFound.into())
    }

    fn head(&self, path: &Path, max: usize) -> io::Result<(u64, Vec<u8>)> {
        let bytes = self.read(path)?;
        Ok((bytes.len() as u64, bytes[..max.min(bytes.len())].to_vec()))
    }

    fn write(&self, path: &Path, contents: &[u8]) -> io::Result<()> {
        self.files.borrow_mut().insert(path.to_path_buf(), contents.to_vec());
        Ok(())
    }
}

#[test]
fn round_trip_keeps_crlf() {
    let storage = MemoryStorage::default();
    let path = PathBuf::from("/a.php");
    storage.write(&path, b"<?php\r\necho 1;\r\n").unwrap();

    let mut doc = open_document(&path, &storage).unwrap();
    assert_eq!(doc.buffer().line_text(0), "<?php");
    crate::application::commands::insert_text(&mut doc, "// ");
    save_document(&mut doc, None, &storage).unwrap();

    assert_eq!(storage.read(&path).unwrap(), b"// <?php\r\necho 1;\r\n");
    assert!(!doc.is_dirty());
}

#[test]
fn untitled_document_needs_a_path() {
    let storage = MemoryStorage::default();
    let mut doc = Document::new("x");
    assert!(save_document(&mut doc, None, &storage).is_err());
    save_document(&mut doc, Some(PathBuf::from("/new.php")), &storage).unwrap();
    assert_eq!(doc.path(), Some(Path::new("/new.php")));
}

#[test]
fn binary_files_are_refused_before_reading() {
    let storage = MemoryStorage::default();
    let path = PathBuf::from("/clip.mp4");
    storage.write(&path, b"\0\0\0\x18ftypmp42").unwrap();
    assert_eq!(probe_text(&path, &storage).unwrap(), crate::TextCheck::Binary);
    let err = open_document(&path, &storage).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::InvalidData);
}

/// A Latin-1 file keeps its encoding while it can; a character it can't hold (`ș`) makes
/// it UTF-8 instead of being written as `?`.
#[test]
fn latin1_becomes_utf8_rather_than_losing_characters() {
    let storage = MemoryStorage::default();
    let path = PathBuf::from("/l.txt");
    storage.write(&path, b"caf\xe9\n").unwrap();
    let mut doc = open_document(&path, &storage).unwrap();
    assert_eq!(doc.encoding(), crate::domain::encoding::Encoding::Latin1);
    crate::application::commands::insert_text(&mut doc, "è");
    save_document(&mut doc, None, &storage).unwrap();
    assert_eq!(storage.read(&path).unwrap(), b"\xe8caf\xe9\n", "still Latin-1");
    crate::application::commands::insert_text(&mut doc, "ș");
    save_document(&mut doc, None, &storage).unwrap();
    assert_eq!(doc.encoding(), crate::domain::encoding::Encoding::Utf8);
    assert_eq!(String::from_utf8(storage.read(&path).unwrap()).unwrap(), "èșcafé\n");
}

/// A save made in halves: prepared here, written elsewhere, told after; typed on meanwhile,
/// the document stays unsaved against what was written.
#[test]
fn a_save_in_halves_marks_the_version_written() {
    let storage = MemoryStorage::default();
    let path = PathBuf::from("/a.txt");
    storage.write(&path, b"a").unwrap();
    let mut doc = open_document(&path, &storage).unwrap();
    doc.apply(crate::Transaction::single(1..1, "b"), None, crate::EditKind::Typing);
    let save = prepare_save(&mut doc, None).unwrap();
    assert_eq!(save.path, path);
    // Typed on before the write is told.
    doc.apply(crate::Transaction::single(0..0, "x"), None, crate::EditKind::Typing);
    storage.write(&save.path, &save.bytes).unwrap();
    doc.mark_saved_version(save.version, save.format);
    assert!(doc.is_dirty(), "typed after what was written");
    // Nothing typed since: saved.
    let save = prepare_save(&mut doc, None).unwrap();
    doc.mark_saved_version(save.version, save.format);
    assert!(!doc.is_dirty());
    assert_eq!(doc.path(), Some(path.as_path()));
}
