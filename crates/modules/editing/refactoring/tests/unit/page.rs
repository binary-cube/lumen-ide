use super::*;

/// A page too short for its form and the list: down, the form goes up first, then the list
/// would scroll; up, the form comes back. A page that fits doesn't scroll.
#[test]
fn a_short_page_scrolls_its_form_away_first() {
    let mut page = PageScroll::default();
    let mut list = PlanList::default();
    // 200 high above 40 of keys; the form ends at 180, the list wants 100.
    let (area, y) = page.begin(Rect::new(0.0, 0.0, 400.0, 240.0), 40.0, 20.0);
    assert_eq!((area.size.height, y), (200.0, 20.0));
    let form_end = y + 160.0;
    page.list_area(0.0, 400.0, form_end, 200.0, 100.0);
    assert!(page.scroll(&mut list, 50.0));
    assert_eq!(page.offset, 50.0);
    // At most what the list needs: 180 + 100 - 200.
    page.scroll(&mut list, 500.0);
    assert_eq!(page.offset, 80.0);
    assert!(page.scroll(&mut list, -30.0));
    assert_eq!(page.offset, 50.0);
    page.scroll(&mut list, -500.0);
    assert_eq!(page.offset, 0.0);

    let mut fits = PageScroll::default();
    let (_, y) = fits.begin(Rect::new(0.0, 0.0, 400.0, 800.0), 40.0, 20.0);
    fits.list_area(0.0, 400.0, y + 160.0, 760.0, 100.0);
    assert!(!fits.scroll(&mut list, 50.0));
}
