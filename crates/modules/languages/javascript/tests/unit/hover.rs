use super::*;

#[test]
fn explains_apis() {
    let src = "document.querySelector('a'); escape(x); Array.from(y); el.addEventListener(";
    let q = hover(src, src.find("querySelector").unwrap() + 1).unwrap();
    assert_eq!(q.code, "Document.querySelector(selectors: string): Element | null");
    let e = hover(src, src.find("escape").unwrap() + 1).unwrap();
    assert_eq!(e.notes, [HoverNote::Deprecated]);
    assert!(e.code.starts_with("escape(string: string)"), "{}", e.code);
    let a = hover(src, src.find("addEventListener").unwrap() + 1).unwrap();
    assert!(a.code.starts_with("EventTarget.addEventListener(type: string"), "{}", a.code);
}
