//! A request file's environments from the disk: the files beside it (`http-client.env.json`,
//! the private and local ones), from the nearest folder up to the project's root that has one.
//! Read on a background thread.

use std::path::{Path, PathBuf};

use http_requests::env::{Environments, LOCAL, PRIVATE, PUBLIC, Source};

/// The folder whose environment files serve `file`: the nearest from its own up to `root`
/// with one of them (none: its own).
pub fn folder_for(file: &Path, root: Option<&Path>) -> PathBuf {
    let own = file.parent().map_or_else(PathBuf::new, Path::to_path_buf);
    let mut dir = Some(own.as_path());
    while let Some(here) = dir {
        if [PUBLIC, PRIVATE, LOCAL].iter().any(|name| here.join(name).is_file()) {
            return here.to_path_buf();
        }
        if root.is_none_or(|root| here == root || !here.starts_with(root)) {
            break;
        }
        dir = here.parent();
    }
    own
}

/// `folder`'s environment files read (missing ones: none).
pub fn read(folder: &Path) -> Environments {
    let text = |name: &str| std::fs::read_to_string(folder.join(name)).ok();
    let (public, private, local) = (text(PUBLIC), text(PRIVATE), text(LOCAL));
    Environments::read(&[
        (Source::Public, public.as_deref()),
        (Source::Private, private.as_deref()),
        (Source::Local, local.as_deref()),
    ])
}

/// `file`'s environments: [`read`] of [`folder_for`].
pub fn of(file: &Path, root: Option<&Path>) -> Environments {
    read(&folder_for(file, root))
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
