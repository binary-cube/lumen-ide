use super::*;

fn uploaded(side: u32) -> ImageData {
    let image = ImageData::mask(side, side, vec![255; (side * side) as usize], kernel::Rgba::WHITE);
    image.uploaded();
    image
}

#[test]
fn the_oldest_go_over_the_budget_never_what_is_shown() {
    let old = uploaded(1024);
    let newer = uploaded(1024);
    drawn(&old);
    drawn(&newer);
    std::thread::sleep(SHOWN + Duration::from_millis(50));
    drawn(&newer);
    let shown = uploaded(1024);
    drawn(&shown);
    // 3 MB held, 2.5 MB allowed: the oldest goes.
    frame_done(2_621_440);
    assert!(old.released());
    assert!(!newer.released() && !shown.released());
    // What's on screen stays, even over.
    frame_done(0);
    assert!(!shown.released() && !newer.released());
}
