//! What Laravel adds to Blade: the variables a controller passes to the view, those Laravel
//! gives every view, the `<x-…>` components and their attributes. Gathered ahead, in the
//! background; the suggestions only read what was gathered.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use code_intelligence::template::{
    BladePlace, Passed, PassedFiles, blade_place, declared_props, name_start,
};
use code_intelligence::{
    CompletionItem, CompletionKind, FrameworkCompletion, FrameworkContext, attribute_name_at,
    suggest,
};

use crate::{Views, view_name};

/// How a controller names a view.
const VIEWS: &[&str] = &["view(", "View::make("];
/// Where the views are, under the project's root.
const ROOT: &str = "resources/views/";
/// Where the anonymous components are, under the views.
const COMPONENTS: &str = "components/";
/// What every view has.
const GIVEN: &[&str] = &["$errors", "$__env", "$app"];
/// What a component has besides.
const IN_COMPONENT: &[&str] = &["$slot", "$attributes", "$component"];

/// A component: its tag (`x-forms.input`) and the attributes `@props` declares.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Component {
    pub tag: String,
    pub props: Vec<String>,
}

/// The tag of the component in `file` (relative to the views), if it's one.
fn component_tag(file: &str) -> Option<String> {
    let name = view_name(file.strip_prefix(COMPONENTS)?);
    Some(format!("x-{}", name.strip_suffix(".index").unwrap_or(&name)))
}

#[derive(Debug, Default)]
pub(crate) struct Blade {
    files: Mutex<PassedFiles>,
    passed: Mutex<Arc<Passed>>,
    components: Mutex<Arc<Vec<Component>>>,
}

impl Blade {
    /// Brings up to date what the PHP code passes (`sources`, from the project's root) and
    /// the components (of `views`).
    pub(crate) fn prepare(&self, at: &FrameworkContext<'_>, sources: &[String], views: &Views) {
        let mut known = self.files.lock().unwrap_or_else(PoisonError::into_inner);
        let passed = known.update(at.project, sources, VIEWS);
        *self.passed.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(passed);
        let Some(root) = at.project.root else { return };
        let mut components: Vec<Component> = views
            .iter()
            .filter_map(|(_, file)| {
                let tag = component_tag(file)?;
                let text = (at.project.read)(&root.join(ROOT).join(file)).unwrap_or_default();
                Some(Component { tag, props: declared_props(&text) })
            })
            .collect();
        components.sort_by(|a, b| a.tag.cmp(&b.tag));
        *self.components.lock().unwrap_or_else(PoisonError::into_inner) = Arc::new(components);
    }

    /// The view this file is (`None`: not under the views), and whether it's a component.
    fn this_view(at: &FrameworkContext<'_>) -> Option<(String, bool)> {
        let relative = at.path.strip_prefix(at.project.root?).ok().map(Path::to_string_lossy)?;
        let relative = relative.replace('\\', "/");
        let file = relative.strip_prefix(ROOT)?;
        Some((view_name(file), file.starts_with(COMPONENTS)))
    }

    /// The component tag being typed (`<x-al`): where it starts.
    fn tag_at(text: &str, offset: usize) -> Option<usize> {
        let start = name_start(text, offset, "-.:");
        (text[..start].ends_with('<') && text[start..offset].starts_with("x-")).then_some(start)
    }

    /// The component whose attribute is being typed at `start`.
    fn tag_of_attribute(text: &str, start: usize) -> Option<&str> {
        let open = text[..start].rfind('<')? + 1;
        let rest = &text[open..start];
        let name = &rest[..rest.find(char::is_whitespace)?];
        name.starts_with("x-").then_some(name)
    }

    fn only(
        replace: std::ops::Range<usize>,
        items: Vec<CompletionItem>,
    ) -> Option<FrameworkCompletion> {
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }

    /// The suggestions at `at`, if it's a place of a Blade template Laravel knows.
    pub(crate) fn complete(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        if !at.path.to_string_lossy().ends_with(".blade.php") {
            return None;
        }
        let components =
            || Arc::clone(&self.components.lock().unwrap_or_else(PoisonError::into_inner));
        if let Some(start) = Self::tag_at(at.text, at.offset) {
            let all = components();
            let tags = all.iter().map(|c| c.tag.as_str());
            let items = suggest(&at.text[start..at.offset], tags, CompletionKind::Tag, None);
            return Self::only(start..at.offset, items);
        }
        if let Some(replace) = attribute_name_at(at.text, at.offset, &['-', ':'])
            && let Some(tag) = Self::tag_of_attribute(at.text, replace.start)
        {
            let all = components();
            let props = all.iter().find(|c| c.tag == tag)?.props.iter().map(String::as_str);
            let items = suggest(&at.text[replace.clone()], props, CompletionKind::Attribute, None);
            return Self::only(replace, items);
        }
        let (BladePlace::Variable, replace) = blade_place(at.text, at.offset)? else { return None };
        let (view, component) = Self::this_view(at).unwrap_or_default();
        let passed = Arc::clone(&self.passed.lock().unwrap_or_else(PoisonError::into_inner));
        let given: Vec<String> =
            passed.get(&view).into_iter().flatten().map(|name| format!("${name}")).collect();
        let extra = if component { IN_COMPONENT } else { &[] };
        let names = given
            .iter()
            .map(String::as_str)
            .chain(GIVEN.iter().copied())
            .chain(extra.iter().copied());
        let items = suggest(&at.text[replace.clone()], names, CompletionKind::Variable, None);
        Self::only(replace, items)
    }
}

#[cfg(test)]
#[path = "../tests/unit/blade.rs"]
mod tests;
