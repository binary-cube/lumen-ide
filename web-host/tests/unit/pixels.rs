use std::path::PathBuf;

use super::{Painted, PixelFiles, PixelsFile, bands};

/// A folder of its own for a test (removed by the caller).
fn folder() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("web-host-pixels-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A page's pixels go only into a file of the channel's folder named as the app names them;
/// anything else (another folder, another name, a way out of the folder) is refused.
#[test]
fn pixels_go_only_into_the_channels_folder() {
    let dir = folder();
    let file = PixelsFile::open(&dir, &dir.join("frames-1.bgra"));
    assert!(file.is_some());
    assert!(PixelsFile::open(&dir, &dir.join("other.bgra")).is_none(), "another name");
    assert!(
        PixelsFile::open(&dir, &dir.join("x").join("frames-1.bgra")).is_none(),
        "another folder"
    );
    assert!(PixelsFile::open(&dir, &dir.join("../frames-1.bgra")).is_none(), "a way out");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join("frames-1.bgra")).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "its owner's only");
    }
    drop(file);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The files are removed as they drop: a page closed, or one whose browser wasn't made,
/// leaves nothing; a view refused makes no popup's file either.
#[test]
fn the_files_go_with_their_page() {
    let dir = folder();
    let view = dir.join("frames-2.bgra");
    let files = PixelFiles::open(&dir, &view).expect("made");
    let popup = web_engine_protocol::popup_pixels(&view);
    assert!(view.exists() && popup.exists());
    drop(files);
    assert!(!view.exists() && !popup.exists(), "removed");
    assert!(PixelFiles::open(&dir, &dir.join("other.bgra")).is_none());
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0, "nothing made");
    std::fs::remove_dir_all(dir).unwrap();
}

/// A first frame is written whole; then, at the same size, only its dirty rows (the rest of
/// the file keeps the previous frame); a frame of another size is written whole again.
#[test]
fn only_the_dirty_rows_are_written_once_the_file_holds_a_frame() {
    let dir = folder();
    let path = dir.join("frames-3.bgra");
    let mut file = PixelsFile::open(&dir, &path).unwrap();
    let (width, height) = (2u32, 4u32);
    let frame = |fill: u8| vec![fill; (width * height * 4) as usize];
    let first = frame(1);
    assert!(file.write(Painted { pixels: &first, width, height }, std::iter::empty()));
    assert_eq!(std::fs::read(&path).unwrap(), first, "whole, whatever it says changed");
    // Rows 1 and 2 changed: only they are written.
    let second = frame(2);
    assert!(file.write(Painted { pixels: &second, width, height }, [(1, 2)].into_iter()));
    let row = (width * 4) as usize;
    let mut expected = first.clone();
    expected[row..3 * row].copy_from_slice(&second[row..3 * row]);
    assert_eq!(std::fs::read(&path).unwrap(), expected);
    // Another size: whole.
    let (width, height) = (1u32, 2u32);
    let third = vec![3u8; (width * height * 4) as usize];
    assert!(file.write(Painted { pixels: &third, width, height }, [(0, 1)].into_iter()));
    assert_eq!(&std::fs::read(&path).unwrap()[..third.len()], third.as_slice());
    // A buffer not of its size isn't written.
    assert!(!file.write(Painted { pixels: &third[..4], width, height }, std::iter::empty()));
    drop(file);
    std::fs::remove_dir_all(dir).unwrap();
}

/// The dirty rows as bands within the frame: sorted, touching and overlapping ones merged,
/// what's outside cut off, empty ones dropped.
#[test]
fn dirty_rows_are_merged_into_bands() {
    let mut out = Vec::new();
    bands([(10, 5), (0, 2), (2, 3), (12, 10), (-3, 4), (50, 1), (5, 0)].into_iter(), 20, &mut out);
    assert_eq!(out, vec![(0, 5), (10, 10)]);
    bands([(i32::MAX, i32::MAX), (i32::MIN, 1)].into_iter(), 20, &mut out);
    assert!(out.is_empty(), "nothing within the frame");
}

/// A file already at its place is never taken over (nor removed): the page's file is made
/// new; on Unix a link at its place is refused, its target untouched.
#[test]
fn a_file_already_there_is_never_taken_over() {
    let dir = folder();
    let path = dir.join("frames-4.bgra");
    std::fs::write(&path, b"someone's").unwrap();
    assert!(PixelsFile::open(&dir, &path).is_none(), "already there");
    assert_eq!(std::fs::read(&path).unwrap(), b"someone's", "kept, unchanged");
    #[cfg(unix)]
    {
        let target = dir.join("target");
        std::fs::write(&target, b"theirs").unwrap();
        let link = dir.join("frames-5.bgra");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(PixelsFile::open(&dir, &link).is_none(), "a link");
        assert_eq!(std::fs::read(&target).unwrap(), b"theirs", "never written through");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

/// A file told to forget what it holds (a frame of it dropped) writes the next one whole,
/// whatever its frame says changed.
#[test]
fn a_file_that_forgot_writes_the_next_frame_whole() {
    let dir = folder();
    let path = dir.join("frames-6.bgra");
    let mut file = PixelsFile::open(&dir, &path).unwrap();
    let (width, height) = (2u32, 2u32);
    let frame = |fill: u8| vec![fill; (width * height * 4) as usize];
    assert!(file.write(Painted { pixels: &frame(1), width, height }, std::iter::empty()));
    file.forget();
    let second = frame(2);
    assert!(file.write(Painted { pixels: &second, width, height }, [(0, 1)].into_iter()));
    assert_eq!(std::fs::read(&path).unwrap(), second, "whole");
    drop(file);
    std::fs::remove_dir_all(dir).unwrap();
}
