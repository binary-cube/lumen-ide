//! Files written on the server given their owner and group ([`WriteAs`]): a [`Connector`]
//! whose sessions, after each file they write, set who it belongs to — so every path that
//! writes (a save, an upload, a push) does it, none of them knowing.
//!
//! The project's owner and group ([`WriteAs::Project`]) are read from its folder on the
//! server once, on the first session that can; a protocol that can't change owners (FTP)
//! writes as before, and its project folder is never read. What the server refuses (the
//! account may not give files away) is told to `refused` and not asked again in that session
//! (one refusal per file would cost a round trip each); the file stays written. A change of
//! owner clears a file's setuid / setgid bits (the kernel does): the protocol's session
//! puts back those of the file it just wrote (SFTP: knowing its mode, without asking).

use std::sync::{Arc, Mutex};

use crate::application::{Connector, Moved, RemoteFs};
use crate::properties::{Accounts, Algo, Capabilities, Properties, SetProperties};
use crate::{RemoteEntry, RemoteError, WriteAs};

/// What a refusal is told to: the file's path and the server's answer.
pub type Refused = Arc<dyn Fn(&str, &RemoteError) + Send + Sync>;

/// `inner`'s sessions, writing files as `write_as` says.
pub struct OwnedWrites {
    inner: Arc<dyn Connector>,
    write_as: WriteAs,
    /// The project folder's owner and group, once read (`Some(None)`: read, none known).
    project: Mutex<Option<Option<SetProperties>>>,
    refused: Refused,
}

impl std::fmt::Debug for OwnedWrites {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnedWrites").field("write_as", &self.write_as).finish_non_exhaustive()
    }
}

impl OwnedWrites {
    /// `inner` as it is when files are written as the server makes them.
    pub fn wrap(
        inner: Arc<dyn Connector>,
        write_as: WriteAs,
        refused: Refused,
    ) -> Arc<dyn Connector> {
        if write_as == WriteAs::Server {
            return inner;
        }
        Arc::new(Self { inner, write_as, project: Mutex::new(None), refused })
    }

    /// What to set on a written file, for session `fs` (`None`: nothing).
    fn wanted(&self, fs: &mut dyn RemoteFs) -> Option<SetProperties> {
        let caps = fs.capabilities();
        if !caps.chown && !caps.chgrp {
            return None;
        }
        let set = match &self.write_as {
            WriteAs::Server => return None,
            WriteAs::Chosen { owner, group } => SetProperties {
                mode: None,
                owner: owner.as_ref().and_then(|p| p.id),
                group: group.as_ref().and_then(|p| p.id),
            },
            WriteAs::Project => {
                let known = self.project.lock().ok().and_then(|p| *p);
                let read = known.unwrap_or_else(|| {
                    let read = project_owner(fs);
                    if let Ok(mut project) = self.project.lock() {
                        *project = Some(read);
                    }
                    read
                });
                read?
            }
        };
        let set = SetProperties {
            mode: None,
            owner: set.owner.filter(|_| caps.chown),
            group: set.group.filter(|_| caps.chgrp),
        };
        (!set.is_empty()).then_some(set)
    }
}

/// The owner and group (their ids) of the project's folder on the server.
fn project_owner(fs: &mut dyn RemoteFs) -> Option<SetProperties> {
    let props = fs.properties(&[String::new()]).ok()?.into_iter().next()??;
    let set = SetProperties {
        mode: None,
        owner: props.owner.and_then(|p| p.id),
        group: props.group.and_then(|p| p.id),
    };
    (!set.is_empty()).then_some(set)
}

impl Connector for OwnedWrites {
    fn connect(&self) -> Result<Box<dyn RemoteFs>, RemoteError> {
        let mut fs = self.inner.connect()?;
        let set = self.wanted(fs.as_mut());
        Ok(match set {
            Some(set) => Box::new(Owned {
                inner: fs,
                set,
                refused: Arc::clone(&self.refused),
                given_up: false,
            }),
            None => fs,
        })
    }
}

/// A session setting `set` on each file it writes (until the server refuses it once).
struct Owned {
    inner: Box<dyn RemoteFs>,
    set: SetProperties,
    refused: Refused,
    given_up: bool,
}

impl Owned {
    fn own(&mut self, path: &str) {
        if self.given_up {
            return;
        }
        if let Err(e) = self.inner.set_properties(path, &self.set) {
            self.given_up = true;
            (self.refused)(path, &e);
        }
    }
}

impl RemoteFs for Owned {
    fn list(&mut self, dir: &str) -> Result<Vec<RemoteEntry>, RemoteError> {
        self.inner.list(dir)
    }
    fn read(&mut self, path: &str) -> Result<Vec<u8>, RemoteError> {
        self.inner.read(path)
    }
    fn write(&mut self, path: &str, data: &[u8]) -> Result<RemoteEntry, RemoteError> {
        let written = self.inner.write(path, data)?;
        self.own(path);
        Ok(written)
    }
    fn remove(&mut self, path: &str) -> Result<(), RemoteError> {
        self.inner.remove(path)
    }
    fn rename(&mut self, from: &str, to: &str) -> Result<(), RemoteError> {
        self.inner.rename(from, to)
    }
    fn stat(&mut self, path: &str) -> Result<Option<RemoteEntry>, RemoteError> {
        self.inner.stat(path)
    }
    fn read_moving(&mut self, path: &str, moved: Moved<'_>) -> Result<Vec<u8>, RemoteError> {
        self.inner.read_moving(path, moved)
    }
    fn write_moving(
        &mut self,
        path: &str,
        data: &[u8],
        moved: Moved<'_>,
    ) -> Result<RemoteEntry, RemoteError> {
        let written = self.inner.write_moving(path, data, moved)?;
        self.own(path);
        Ok(written)
    }
    fn home(&mut self) -> Result<String, RemoteError> {
        self.inner.home()
    }
    fn properties(&mut self, paths: &[String]) -> Result<Vec<Option<Properties>>, RemoteError> {
        self.inner.properties(paths)
    }
    fn list_properties(&mut self, dir: &str) -> Result<Vec<Properties>, RemoteError> {
        self.inner.list_properties(dir)
    }
    fn accounts(&mut self) -> Result<Accounts, RemoteError> {
        self.inner.accounts()
    }
    fn capabilities(&mut self) -> Capabilities {
        self.inner.capabilities()
    }
    fn checksum(&mut self, path: &str, algo: Algo) -> Result<Option<String>, RemoteError> {
        self.inner.checksum(path, algo)
    }
    fn set_properties(&mut self, path: &str, set: &SetProperties) -> Result<(), RemoteError> {
        self.inner.set_properties(path, set)
    }
}

#[cfg(test)]
#[path = "../tests/unit/ownership.rs"]
mod tests;
