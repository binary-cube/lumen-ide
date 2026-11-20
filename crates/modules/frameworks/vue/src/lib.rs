//! Vue module: active only in projects that use Vue (`vue` in `package.json`). In a
//! template's tags, the directives (`v-if`, `v-for`, `v-model`…) join HTML's suggestions,
//! with their syntax on hover; in scripts, the Composition API (`ref`, `computed`,
//! `watch`, `defineProps`…) gets its signature on hover and while typing the arguments.
//! In any template, the directives' values and, in a component file, the `{{ }}`
//! interpolations are JavaScript (TypeScript when its script is `lang="ts"`).

mod expressions;

use std::sync::Arc;

use code_intelligence::{
    CompletionKind, Framework, FrameworkCompletion, FrameworkContext, FrameworkSupport, HoverInfo,
    Location, Project, component_tag_at, has_dependency, pascal_case, suggest,
};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};
use kernel::SharedMemo;

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("framework.vue"),
    name: "Vue",
    description: "Template directives and the Composition API: completion, hover, parameters, their expressions colored",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "package", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct VueModule;

impl Module for VueModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn Framework>>(Arc::new(Vue));
        ctx.contribute::<Arc<dyn syntax_ts::InjectionProvider>>(Arc::new(
            expressions::TemplateExpressions::default(),
        ));
        Ok(())
    }
}

/// (directive, value syntax; `""`: no value), written for the editor from Vue 3's directives.
const DIRECTIVES: &[(&str, &str)] = &[
    ("v-if", "<condition>"),
    ("v-else-if", "<condition>"),
    ("v-else", ""),
    ("v-for", "item in items | (item, index) in items | (value, key, index) in object | n in 10"),
    ("v-show", "<condition>"),
    ("v-model", "<ref>   (v-model.lazy / .number / .trim, v-model:<prop>)"),
    ("v-bind", "<object>   (v-bind:<attr>=\"<expr>\", shorthand :<attr>)"),
    ("v-on", "<handler>   (v-on:<event>[.stop|.prevent|.once|…]=\"…\", shorthand @<event>)"),
    ("v-slot", "<props>   (v-slot:<name>, shorthand #<name>)"),
    ("v-html", "<html string>"),
    ("v-text", "<string>"),
    ("v-once", ""),
    ("v-memo", "[<dependency>, …]"),
    ("v-cloak", ""),
    ("v-pre", ""),
];

/// (name, `(params): result`), from Vue 3's Composition API.
const API: &[(&str, &str)] = &[
    ("ref", "(value: T): Ref<T>"),
    ("shallowRef", "(value: T): ShallowRef<T>"),
    ("reactive", "(target: T extends object): T"),
    ("shallowReactive", "(target: T extends object): T"),
    ("readonly", "(target: T extends object): Readonly<T>"),
    ("computed", "(getter: () => T | { get: () => T, set: (value: T) => void }): ComputedRef<T>"),
    (
        "watch",
        "(source: Ref | (() => T) | array, callback: (value: T, oldValue: T, onCleanup: (fn: () => void) => void) => void, options?: { immediate?: boolean, deep?: boolean | number, once?: boolean, flush?: 'pre' | 'post' | 'sync' }): WatchHandle",
    ),
    (
        "watchEffect",
        "(effect: (onCleanup: (fn: () => void) => void) => void, options?: { flush?: 'pre' | 'post' | 'sync' }): WatchHandle",
    ),
    ("onMounted", "(callback: () => void): void"),
    ("onUpdated", "(callback: () => void): void"),
    ("onUnmounted", "(callback: () => void): void"),
    ("onBeforeMount", "(callback: () => void): void"),
    ("onBeforeUpdate", "(callback: () => void): void"),
    ("onBeforeUnmount", "(callback: () => void): void"),
    (
        "onErrorCaptured",
        "(callback: (err: unknown, instance: ComponentPublicInstance | null, info: string) => boolean | void): void",
    ),
    ("nextTick", "(callback?: () => void): Promise<void>"),
    ("defineProps", "(props: string[] | { [name]: Type | { type, required?, default? } }): Props"),
    (
        "defineEmits",
        "(emits: string[] | { [event]: (...args) => boolean }): (event: string, ...args: unknown[]) => void",
    ),
    ("defineExpose", "(exposed: object): void"),
    ("defineModel", "(name?: string, options?: { required?: boolean, default?: T }): ModelRef<T>"),
    ("defineSlots", "(): Slots"),
    ("withDefaults", "(props: Props, defaults: Partial<Props>): Props"),
    ("toRef", "(source: object | Ref | (() => T), key?: string, defaultValue?: T): Ref<T>"),
    ("toRefs", "(object: T extends object): { [K in keyof T]: Ref<T[K]> }"),
    ("toValue", "(source: T | Ref<T> | (() => T)): T"),
    ("unref", "(ref: T | Ref<T>): T"),
    ("isRef", "(value: unknown): boolean"),
    ("provide", "(key: InjectionKey<T> | string, value: T): void"),
    ("inject", "(key: InjectionKey<T> | string, defaultValue?: T): T"),
    ("useSlots", "(): Slots"),
    ("useAttrs", "(): Attrs"),
    ("useTemplateRef", "(key: string): Ref<Element | null>"),
    ("useId", "(): string"),
    ("createApp", "(rootComponent: Component, rootProps?: object): App"),
    (
        "defineComponent",
        "(options: ComponentOptions | (props, context) => RenderFunction): Component",
    ),
    ("defineAsyncComponent", "(loader: () => Promise<Component>): Component"),
];

#[derive(Debug)]
pub struct Vue;

impl Framework for Vue {
    fn id(&self) -> &'static str {
        "vue"
    }

    fn name(&self) -> &'static str {
        "Vue"
    }

    fn detect(&self, project: &Project<'_>) -> Option<String> {
        let package = project.read_relative("package.json")?;
        (has_dependency(&package, "vue") || has_dependency(&package, "nuxt"))
            .then(|| "package.json".into())
    }

    fn activate(&self, _project: &Project<'_>) -> Arc<dyn FrameworkSupport> {
        Arc::new(Support::default())
    }
}

/// The project's single-file components: (name in PascalCase, the `.vue` file).
type Components = Arc<Vec<(String, std::path::PathBuf)>>;

/// What Vue gathers from one project: its components (recomputed when files change).
#[derive(Debug, Default)]
struct Support {
    components: SharedMemo<u64, Components>,
}

impl Support {
    fn components(&self, at: &FrameworkContext<'_>) -> Components {
        self.components.get_or_make(at.own, || {
            let Some(root) = at.project.root else { return Components::default() };
            let mut found: Vec<(String, std::path::PathBuf)> = at
                .project
                .files_under("", &[".vue"], 4000)
                .into_iter()
                .map(|relative| {
                    let stem = relative.rsplit('/').next().unwrap_or(&relative);
                    (pascal_case(stem.trim_end_matches(".vue")), root.join(&relative))
                })
                .collect();
            found.sort();
            found.dedup_by(|a, b| a.0 == b.0);
            Arc::new(found)
        })
    }
}

/// The files its components are: its model made again only when one is made, changed or
/// removed.
const WATCHES: &[&str] = &["**/*.vue"];

impl FrameworkSupport for Support {
    fn watches(&self) -> &'static [&'static str] {
        WATCHES
    }

    /// Its components, gathered ahead (the UI thread finds them made).
    fn prepare(&self, at: &FrameworkContext<'_>) {
        self.components(at);
    }

    fn complete(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        if at.language != "html" {
            return None;
        }
        // `<Us|` in a template: the project's components.
        if let Some(tag) = component_tag_at(at.text, at.offset) {
            let replace = tag.start..at.offset;
            let components = self.components(at);
            let names = components.iter().map(|(n, _)| n.as_str());
            let items =
                suggest(&at.text[replace.clone()], names, CompletionKind::Class, Some("component"));
            return (!items.is_empty()).then_some(FrameworkCompletion {
                replace,
                items,
                merge: false,
            });
        }
        let replace =
            code_intelligence::attribute_name_at(at.text, at.offset, &['-', ':', '.', '@', '#'])?;
        let mut items = suggest(
            &at.text[replace.clone()],
            DIRECTIVES.iter().map(|(d, _)| *d),
            CompletionKind::Attribute,
            Some("Vue"),
        );
        for item in &mut items {
            if DIRECTIVES.iter().any(|(d, syntax)| *d == item.label && !syntax.is_empty()) {
                item.insert = format!("{}=\"\"", item.label);
                item.cursor_back = 1;
            }
        }
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: true })
    }

    fn definition(&self, at: &FrameworkContext<'_>) -> Option<Location> {
        let tag = component_tag_at(at.text, at.offset)?;
        let name = pascal_case(&at.text[tag]);
        let components = self.components(at);
        let (_, path) = components.iter().find(|(n, _)| *n == name)?;
        Some(Location { path: path.clone(), offset: 0, line: 0 })
    }

    fn hover(&self, at: &FrameworkContext<'_>) -> Option<HoverInfo> {
        let range = code_intelligence::word_at(at.text, at.offset, "-")?;
        let word = &at.text[range.clone()];
        if at.language == "html" {
            let (name, syntax) = DIRECTIVES.iter().find(|(d, _)| *d == word)?;
            let code =
                if syntax.is_empty() { (*name).to_owned() } else { format!("{name}=\"{syntax}\"") };
            return Some(HoverInfo {
                range,
                code,
                notes: Vec::new(),
                doc: None,
                ..Default::default()
            });
        }
        if !matches!(at.language, "javascript" | "typescript") {
            return None;
        }
        let lead = at.text[..range.start].trim_end();
        if lead.ends_with("function") || lead.ends_with('.') {
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
