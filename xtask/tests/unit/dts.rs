use super::*;

const SRC: &str = "
/** Doc. */
interface ParentNode extends Node {
    querySelector<K extends keyof HTMLElementTagNameMap>(selectors: K): HTMLElementTagNameMap[K] | null;
    querySelector<E extends Element = Element>(selectors: string): E | null;
}
interface Document extends Node, ParentNode {
    readonly title: string;
    addEventListener<K extends keyof DocumentEventMap>(type: K, listener: (this: Document, ev: DocumentEventMap[K]) => any): void;
    addEventListener(type: string, listener: EventListener, options?: boolean): void;
    get body(): HTMLElement;
}
declare var Document: {
    prototype: Document;
    new(): Document;
    parseHTMLUnsafe(html: string): Document;
};
interface ArrayConstructor {
    from<T>(arrayLike: ArrayLike<T>): T[];
    from<T, U>(arrayLike: ArrayLike<T>, mapfn: (v: T, k: number) => U, thisArg?: any): U[];
}
declare var Array: ArrayConstructor;
declare function parseInt(string: string, radix?: number): number;
interface Ns { create(ns: \"http://www.w3.org/1999/xhtml\"): Node; } // trailing
";

#[test]
fn reads_signatures() {
    let mut decls = Decls::default();
    decls.add(SRC);
    assert_eq!(
        decls.member("Document", "querySelector").unwrap(),
        "(selectors: string): Element | null"
    );
    assert_eq!(
        decls.member("Document", "addEventListener").unwrap(),
        "(type: string, listener: EventListener, options?: boolean): void"
    );
    assert_eq!(decls.member("Document", "title").unwrap(), ": string");
    assert_eq!(decls.member("Document", "body").unwrap(), ": HTMLElement");
    assert_eq!(decls.member("Document", "parseHTMLUnsafe").unwrap(), "(html: string): Document");
    assert_eq!(
        decls.member("Array", "from").unwrap(),
        "(arrayLike: ArrayLike<T>, mapfn: (v: T, k: number) => U, thisArg?: any): U[]"
    );
    assert_eq!(decls.global("parseInt").unwrap(), "(string: string, radix?: number): number");
    assert_eq!(decls.global("Document").unwrap(), "(): Document");
    assert_eq!(
        decls.member("Ns", "create").unwrap(),
        "(ns: \"http://www.w3.org/1999/xhtml\"): Node"
    );
}
