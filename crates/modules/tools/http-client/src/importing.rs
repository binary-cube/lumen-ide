//! A collection of another client imported into the project (on a worker): a Postman or
//! Insomnia JSON file, or a Bruno collection (any of its `.bru` files picked: the whole
//! collection, from the folder with its `bruno.json`). It becomes a folder of its own under
//! the project's `http/` — its requests' file, its environments beside them as
//! `http-client.env.json` (the format's environment file) —, named after it, never over another.

use std::path::{Path, PathBuf};

use http_requests::import::{self, Imported};

/// What an import made: the file to open, how many requests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    pub open: PathBuf,
    pub requests: usize,
}

/// `picked` imported into the project at `root`.
pub fn import_file(root: &Path, picked: &Path) -> Result<Made, String> {
    let imported = if picked.extension().is_some_and(|e| e == "bru") {
        bruno(picked)?
    } else {
        let text = std::fs::read_to_string(picked).map_err(|e| e.to_string())?;
        import::from_text(&text).map_err(|e| e.0)?
    };
    let stem = picked.file_stem().and_then(|s| s.to_str()).unwrap_or("imported");
    let name = slug(if imported.name.trim().is_empty() { stem } else { &imported.name });
    let folder = free_folder(&root.join("http"), &name);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut open = None;
    if imported.count > 0 {
        let path = folder.join("requests.http");
        files::write_atomic(&path, imported.requests.as_bytes()).map_err(|e| e.to_string())?;
        open = Some(path);
    }
    for (relative, body) in &imported.bodies {
        let path = folder.join(relative);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        files::write_atomic(&path, body.as_bytes()).map_err(|e| e.to_string())?;
    }
    if let Some(environments) = &imported.environments {
        let path = folder.join(http_requests::env::PUBLIC);
        files::write_atomic(&path, environments.as_bytes()).map_err(|e| e.to_string())?;
        open.get_or_insert(path);
    }
    let open = open.ok_or_else(|| "nothing to import in it".to_owned())?;
    Ok(Made { open, requests: imported.count })
}

/// The most `.bru` files read from a collection, and how deep its folders are followed.
const BRUNO_FILES: usize = 2_000;
const BRUNO_DEPTH: usize = 16;

/// The Bruno collection `picked` is a file of (the folder with its `bruno.json`; none above
/// it: the file alone): its `.bru` requests, its environments. Links aren't followed.
fn bruno(picked: &Path) -> Result<Imported, String> {
    let start = picked.parent().unwrap_or(Path::new("."));
    let Some(root) = start.ancestors().find(|dir| dir.join("bruno.json").is_file()) else {
        let text = std::fs::read_to_string(picked).map_err(|e| e.to_string())?;
        let name =
            picked.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let stem =
            picked.file_stem().map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        return Ok(import::from_bruno(&stem, &[(name, text)], &[]));
    };
    let root = root.to_path_buf();
    let name = std::fs::read_to_string(root.join("bruno.json"))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_owned))
        .unwrap_or_else(|| {
            root.file_name().map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        });
    let (mut requests, mut environments) = (Vec::new(), Vec::new());
    let mut stack = vec![(root.clone(), 0)];
    let mut read = 0;
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            // Its own type, not a link's target's (a link back up never ends).
            let Ok(kind) = entry.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                if depth < BRUNO_DEPTH {
                    stack.push((path, depth + 1));
                }
                continue;
            }
            if read >= BRUNO_FILES {
                break;
            }
            if path.extension().is_none_or(|e| e != "bru") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else { continue };
            read += 1;
            let relative = path.strip_prefix(&root).unwrap_or(&path);
            let relative = relative.to_string_lossy().replace('\\', "/");
            let file = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if relative.starts_with("environments/") {
                let env = file.trim_end_matches(".bru").to_owned();
                environments.push((env, text));
            } else if file != "folder.bru" && file != "collection.bru" {
                requests.push((relative, text));
            }
        }
    }
    requests.sort();
    environments.sort();
    Ok(import::from_bruno(&name, &requests, &environments))
}

/// A folder's name from a collection's: letters, digits, `-`.
fn slug(name: &str) -> String {
    let words: Vec<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    if words.is_empty() { "imported".to_owned() } else { words.join("-") }
}

/// `parent/name`, else `name-2`… where nothing is.
fn free_folder(parent: &Path, name: &str) -> PathBuf {
    let first = parent.join(name);
    if !first.exists() {
        return first;
    }
    (2..).map(|n| parent.join(format!("{name}-{n}"))).find(|p| !p.exists()).unwrap_or(first)
}

#[cfg(test)]
#[path = "../tests/unit/importing.rs"]
mod tests;
