use super::*;

/// `.drawio` files are its, by their name alone.
#[test]
fn drawio_files_open_in_it() {
    let p = Provider;
    assert_eq!(p.kind(Path::new("/p/arch.drawio")), Some(ViewerKind::Replace));
    assert_eq!(p.kind(Path::new("/p/ARCH.DRAWIO")), Some(ViewerKind::Replace));
    assert_eq!(p.kind(Path::new("/p/arch.xml")), None);
    assert_eq!(p.kind(Path::new("/p/drawio")), None);
}
