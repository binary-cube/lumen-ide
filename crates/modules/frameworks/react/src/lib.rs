//! React module: active only in projects that use React (`react` in `package.json`).
//! The hooks and the top-level API (`useState`, `useEffect`, `memo`…) get their signature
//! on hover and while typing their arguments, and the hooks are suggested as you type
//! `use…`. In JSX, the project's components are suggested after `<` and lead to their
//! declaration.

mod components;

use std::sync::{Arc, Mutex};

use code_intelligence::{
    CompletionKind, Framework, FrameworkCompletion, FrameworkContext, FrameworkSupport, HoverInfo,
    Location, Project, component_tag_at, has_dependency, suggest,
};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::SharedMemo;

use crate::components::{Component, FileComponents};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("framework.react"),
    name: "React",
    description: "Hooks and API signatures: hover, parameter help, completion",
    depends_on: &[],
};

#[derive(Debug, Default)]
pub struct ReactModule;

impl Module for ReactModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn Framework>>(Arc::new(React));
        Ok(())
    }
}

/// (name, `(params): result`), written for the editor from React's API reference (React 19).
const API: &[(&str, &str)] = &[
    (
        "useState",
        "(initialState: S | (() => S)): [S, setState: (value: S | ((prev: S) => S)) => void]",
    ),
    ("useEffect", "(setup: () => void | (() => void), dependencies?: unknown[]): void"),
    ("useLayoutEffect", "(setup: () => void | (() => void), dependencies?: unknown[]): void"),
    ("useInsertionEffect", "(setup: () => void | (() => void), dependencies?: unknown[]): void"),
    ("useContext", "(context: Context<T>): T"),
    (
        "useReducer",
        "(reducer: (state: S, action: A) => S, initialArg: S, init?: (arg: S) => S): [S, dispatch: (action: A) => void]",
    ),
    ("useRef", "(initialValue: T): { current: T }"),
    ("useMemo", "(calculateValue: () => T, dependencies: unknown[]): T"),
    ("useCallback", "(fn: T, dependencies: unknown[]): T"),
    ("useId", "(): string"),
    ("useTransition", "(): [isPending: boolean, startTransition: (action: () => void) => void]"),
    ("useDeferredValue", "(value: T, initialValue?: T): T"),
    (
        "useSyncExternalStore",
        "(subscribe: (onChange: () => void) => () => void, getSnapshot: () => T, getServerSnapshot?: () => T): T",
    ),
    ("useImperativeHandle", "(ref: Ref<T>, createHandle: () => T, dependencies?: unknown[]): void"),
    ("useDebugValue", "(value: T, format?: (value: T) => unknown): void"),
    (
        "useActionState",
        "(action: (state: S, payload: P) => S | Promise<S>, initialState: S, permalink?: string): [S, dispatch: (payload: P) => void, isPending: boolean]",
    ),
    (
        "useOptimistic",
        "(state: S, updateFn?: (current: S, action: A) => S): [S, addOptimistic: (action: A) => void]",
    ),
    ("use", "(resource: Promise<T> | Context<T>): T"),
    ("createContext", "(defaultValue: T): Context<T>"),
    ("memo", "(Component: FC<P>, arePropsEqual?: (prev: P, next: P) => boolean): FC<P>"),
    ("forwardRef", "(render: (props: P, ref: Ref<T>) => ReactNode): FC<P>"),
    ("lazy", "(load: () => Promise<{ default: FC<P> }>): FC<P>"),
    ("startTransition", "(action: () => void): void"),
    (
        "createElement",
        "(type: string | FC<P>, props?: P | null, ...children: ReactNode[]): ReactElement",
    ),
    (
        "cloneElement",
        "(element: ReactElement, props?: Partial<P>, ...children: ReactNode[]): ReactElement",
    ),
    ("createPortal", "(children: ReactNode, domNode: Element, key?: string): ReactPortal"),
    ("createRoot", "(domNode: Element, options?: RootOptions): Root"),
    ("hydrateRoot", "(domNode: Element, reactNode: ReactNode, options?: HydrationOptions): Root"),
    ("flushSync", "(callback: () => void): void"),
];

#[derive(Debug)]
pub struct React;

impl Framework for React {
    fn id(&self) -> &'static str {
        "react"
    }

    fn name(&self) -> &'static str {
        "React"
    }

    fn detect(&self, project: &Project<'_>) -> Option<String> {
        let package = project.read_relative("package.json")?;
        has_dependency(&package, "react").then(|| "package.json".into())
    }

    fn activate(&self, _project: &Project<'_>) -> Arc<dyn FrameworkSupport> {
        Arc::new(Support::default())
    }
}

/// What React gathers from one project: its components (recomputed when files change).
#[derive(Debug, Default)]
struct Support {
    components: SharedMemo<u64, Arc<Vec<Component>>>,
    files: Mutex<Option<FileComponents>>,
}

impl Support {
    fn components(&self, at: &FrameworkContext<'_>) -> Arc<Vec<Component>> {
        self.components.get_or_make(at.own, || {
            let mut files = self.files.lock().unwrap_or_else(|e| e.into_inner());
            let files = files.get_or_insert_with(|| FileComponents::new(10_000));
            Arc::new(components::gather(at.project, files))
        })
    }
}

fn script(language: &str) -> bool {
    matches!(language, "javascript" | "jsx" | "typescript" | "tsx")
}

/// The identifier being typed at `offset` (letters, digits, `_`, `$`) and whether it
/// follows a `.` other than `React.` (a member of something else).
fn identifier_before(text: &str, offset: usize) -> Option<std::ops::Range<usize>> {
    let before = &text[..offset];
    let start = before
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_alphanumeric() || matches!(c, '_' | '$'))
        .last()
        .map_or(offset, |(i, _)| i);
    let lead = &before[..start];
    let member = lead.ends_with('.') && !lead.ends_with("React.") && !lead.ends_with("ReactDOM.");
    (start < offset && !member).then_some(start..offset)
}

/// Only the hooks, once `us…` is typed: they would crowd every other suggestion. By
/// characters: what's typed can be any letter (`ă`), never cut inside one.
fn typing_a_hook(typed: &str) -> bool {
    let start: String = typed.chars().take(3).collect();
    typed.chars().count() >= 2 && "use".starts_with(&start)
}

/// The files its components are read from: its model made again only when one changes
/// (not at each save of any file, PHP too).
const WATCHES: &[&str] = &["**/*.jsx", "**/*.tsx", "**/*.js", "**/*.ts"];

impl FrameworkSupport for Support {
    fn watches(&self) -> &'static [&'static str] {
        WATCHES
    }

    /// Its components, gathered ahead (the UI thread finds them made).
    fn prepare(&self, at: &FrameworkContext<'_>) {
        self.components(at);
    }

    fn complete(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        if !script(at.language) {
            return None;
        }
        // `<Us|` in JSX: the project's components.
        if let Some(tag) = component_tag_at(at.text, at.offset) {
            let replace = tag.start..at.offset;
            let components = self.components(at);
            let names = components.iter().map(|c| c.name.as_str());
            let items =
                suggest(&at.text[replace.clone()], names, CompletionKind::Class, Some("component"));
            return (!items.is_empty()).then_some(FrameworkCompletion {
                replace,
                items,
                merge: false,
            });
        }
        let replace = identifier_before(at.text, at.offset)?;
        let typed = &at.text[replace.clone()];
        if !typing_a_hook(typed) {
            return None;
        }
        let items: Vec<_> = API
            .iter()
            .filter(|(name, _)| name.starts_with("use") && name.starts_with(typed))
            .map(|(name, sig)| {
                let mut item = code_intelligence::CompletionItem::word(
                    name,
                    CompletionKind::Function,
                    Some("React"),
                );
                item.insert = format!("{name}()");
                item.cursor_back = usize::from(!sig.starts_with("()"));
                item.score = code_intelligence::fuzzy_score(typed, name).unwrap_or(0) + 4;
                item
            })
            .collect();
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: true })
    }

    fn definition(&self, at: &FrameworkContext<'_>) -> Option<Location> {
        if !script(at.language) {
            return None;
        }
        let tag = component_tag_at(at.text, at.offset)?;
        let components = self.components(at);
        let found = components.iter().find(|c| c.name == at.text[tag.clone()])?;
        let line = (at.project.read)(&found.file)
            .map_or(0, |t| code_intelligence::line_of(&t, found.offset));
        Some(Location { path: found.file.clone(), offset: found.offset, line })
    }

    fn hover(&self, at: &FrameworkContext<'_>) -> Option<HoverInfo> {
        if !script(at.language) {
            return None;
        }
        let range = code_intelligence::word_at(at.text, at.offset, "$")?;
        let word = &at.text[range.clone()];
        let lead = at.text[..range.start].trim_end();
        // A declaration (`function useState`) or another object's member isn't React's.
        if lead.ends_with("function")
            || (lead.ends_with('.') && !lead.ends_with("React.") && !lead.ends_with("ReactDOM."))
        {
            return None;
        }
        let (name, sig) = API.iter().find(|(name, _)| *name == word)?;
        Some(HoverInfo {
            range,
            code: format!("{name}{sig}"),
            notes: Vec::new(),
            doc: None,
            ..Default::default()
        })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
