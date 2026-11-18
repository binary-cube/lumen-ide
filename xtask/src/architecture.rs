//! The project's architecture as written in `architecture.toml` (the layers, what each may
//! depend on), read for `check-deps`.

use std::collections::BTreeMap;
use std::path::Path;

/// A layer of crates.
#[derive(Debug, Clone)]
pub struct LayerRules {
    pub name: String,
    paths: Vec<String>,
    /// The layers it may use (`None`: any).
    pub may_use: Option<Vec<String>>,
    /// The external crates it may use (`None`: any).
    pub external: Option<Vec<String>>,
}

/// The whole file.
#[derive(Debug, Clone)]
pub struct Architecture {
    pub layers: Vec<LayerRules>,
    /// The app's screens' crate, and the layers that may use it.
    pub app_screens: String,
    pub app_screens_users: Vec<String>,
    /// The infrastructure any module may use, and what's one feature's.
    pub module_infra: Vec<String>,
    pub module_owned_infra: BTreeMap<String, Vec<String>>,
}

fn strings(value: Option<&toml::Value>) -> Option<Vec<String>> {
    let items = value?.as_array()?.iter();
    Some(items.filter_map(|v| v.as_str().map(str::to_owned)).collect())
}

/// `dir` (with `/`) matches `pattern`, `*` being one folder.
fn matches(pattern: &str, dir: &str) -> bool {
    let (mut pattern, mut dir) = (pattern.split('/'), dir.split('/'));
    loop {
        match (pattern.next(), dir.next()) {
            (None, None) => return true,
            (Some(p), Some(d)) if p == "*" || p == d => {}
            _ => return false,
        }
    }
}

impl Architecture {
    /// Read from `architecture.toml` at the workspace's `root`.
    pub fn read(root: &Path) -> Result<Self, String> {
        let path = root.join("architecture.toml");
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let file: toml::Table = text.parse().map_err(|e| format!("architecture.toml: {e}"))?;
        let mut layers = Vec::new();
        for layer in file.get("layer").and_then(|l| l.as_array()).into_iter().flatten() {
            let name =
                layer.get("name").and_then(|n| n.as_str()).ok_or("a layer without a name")?;
            layers.push(LayerRules {
                name: name.to_owned(),
                paths: strings(layer.get("paths")).ok_or(format!("{name}: no paths"))?,
                may_use: strings(layer.get("may_use")),
                external: strings(layer.get("external")),
            });
        }
        for used in layers.iter().flat_map(|l| l.may_use.iter().flatten()) {
            if !layers.iter().any(|l| l.name == *used) {
                return Err(format!("architecture.toml: no layer `{used}`"));
            }
        }
        let screens = file.get("app_screens");
        let infra = file.get("module_infra");
        let owned = infra.and_then(|i| i.get("owned")).and_then(|o| o.as_table());
        Ok(Self {
            layers,
            app_screens: screens
                .and_then(|s| s.get("crate"))
                .and_then(|c| c.as_str())
                .unwrap_or_default()
                .to_owned(),
            app_screens_users: strings(screens.and_then(|s| s.get("used_by"))).unwrap_or_default(),
            module_infra: strings(infra.and_then(|i| i.get("any"))).unwrap_or_default(),
            module_owned_infra: owned
                .into_iter()
                .flatten()
                .filter_map(|(k, v)| Some((k.clone(), strings(Some(v))?)))
                .collect(),
        })
    }

    /// The layer of the crate at `dir` (relative to the root, with `/`).
    pub fn layer_of(&self, dir: &str) -> Option<&LayerRules> {
        self.layers.iter().find(|l| l.paths.iter().any(|p| matches(p, dir)))
    }

    pub fn layer(&self, name: &str) -> Option<&LayerRules> {
        self.layers.iter().find(|l| l.name == name)
    }

    /// Whether module `module` may use infrastructure crate `dep`.
    pub fn module_may_use(&self, module: &str, dep: &str) -> bool {
        self.module_infra.iter().any(|d| d == dep)
            || self.module_owned_infra.get(dep).is_some_and(|m| m.iter().any(|n| n == module))
    }
}

#[cfg(test)]
#[path = "../tests/unit/architecture.rs"]
mod tests;
