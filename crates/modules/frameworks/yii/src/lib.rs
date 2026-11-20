//! Yii module (Yii 2): active only in projects that use Yii (from `composer.json` or the
//! `yii` console script). View names in `$this->render('…')` (and `renderPartial`,
//! `renderAjax`) are suggested from the controller's view directory — `FooBarController`
//! → `views/foo-bar/` next to `controllers/` — or, inside a view, from its own directory,
//! and lead to their file.

use std::path::{Path, PathBuf};

use std::sync::Arc;

use code_intelligence::{
    CompletionKind, Framework, FrameworkCompletion, FrameworkContext, FrameworkSupport, Location,
    Project, before_string, has_dependency, quoted_at, suggest,
};
use extensions::{Module, ModuleContext, ModuleError, ModuleId, ModuleManifest};

static MANIFEST: ModuleManifest = ModuleManifest {
    id: ModuleId::new_static("framework.yii"),
    name: "Yii",
    description: "The controllers' views: completion and go to file",
    depends_on: &[],
    info: extensions::ModuleInfo { icon: "package", ..extensions::ModuleInfo::BUILT_IN },
};

#[derive(Debug, Default)]
pub struct YiiModule;

impl Module for YiiModule {
    fn manifest(&self) -> &ModuleManifest {
        &MANIFEST
    }

    fn activate(&self, ctx: &mut ModuleContext<'_>) -> Result<(), ModuleError> {
        ctx.contribute::<Arc<dyn Framework>>(Arc::new(Yii));
        Ok(())
    }
}

#[derive(Debug)]
pub struct Yii;

impl Framework for Yii {
    fn id(&self) -> &'static str {
        "yii"
    }

    fn name(&self) -> &'static str {
        "Yii"
    }

    fn family(&self) -> Option<&'static str> {
        Some("php")
    }

    fn detect(&self, project: &Project<'_>) -> Option<String> {
        let composer = project.read_relative("composer.json").unwrap_or_default();
        if ["yiisoft/yii2", "yiisoft/yii-web", "yiisoft/app"]
            .iter()
            .any(|d| has_dependency(&composer, d))
        {
            return Some("composer.json".into());
        }
        project.read_relative("yii").is_some_and(|y| y.contains("Yii")).then(|| "yii".into())
    }

    fn activate(&self, _project: &Project<'_>) -> Arc<dyn FrameworkSupport> {
        Arc::new(Support)
    }
}

/// Nothing to cache: a view directory is listed when asked (it's one directory).
#[derive(Debug)]
struct Support;

/// `FooBarController.php` → `foo-bar`.
fn controller_id(file_name: &str) -> Option<String> {
    let name = file_name.strip_suffix("Controller.php")?;
    let mut id = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            id.push('-');
        }
        id.extend(c.to_lowercase());
    }
    (!id.is_empty()).then_some(id)
}

/// Where the views rendered from `path` are: a controller's `../views/<id>/`, or a
/// view's own directory.
fn view_dir(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let name = path.file_name()?.to_str()?;
    if let Some(id) = controller_id(name) {
        return Some(dir.parent()?.join("views").join(id));
    }
    dir.components().any(|c| c.as_os_str() == "views").then(|| dir.to_path_buf())
}

/// The string at the cursor names a view (`render('…')`), with no path of its own.
fn view_at(at: &FrameworkContext<'_>) -> Option<std::ops::Range<usize>> {
    let content = quoted_at(at.text, at.offset)?;
    let before = before_string(at.text, &content);
    ["render(", "renderPartial(", "renderAjax(", "renderContent("]
        .iter()
        .any(|r| before.ends_with(r))
        .then_some(content)
}

impl FrameworkSupport for Support {
    fn complete(&self, at: &FrameworkContext<'_>) -> Option<FrameworkCompletion> {
        let content = view_at(at)?;
        let dir = view_dir(at.path)?;
        let names: Vec<String> = (at.project.list)(&dir)
            .into_iter()
            .filter_map(|(name, is_dir)| {
                (!is_dir).then(|| name.strip_suffix(".php").map(str::to_owned)).flatten()
            })
            .collect();
        let replace = content.start..at.offset;
        let items = suggest(
            &at.text[replace.clone()],
            names.iter().map(String::as_str),
            CompletionKind::Value,
            Some("view"),
        );
        (!items.is_empty()).then_some(FrameworkCompletion { replace, items, merge: false })
    }

    fn definition(&self, at: &FrameworkContext<'_>) -> Option<Location> {
        let content = view_at(at)?;
        let name = &at.text[content];
        let path = view_dir(at.path)?.join(format!("{name}.php"));
        (at.project.read)(&path).map(|_| Location { path, offset: 0, line: 0 })
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
