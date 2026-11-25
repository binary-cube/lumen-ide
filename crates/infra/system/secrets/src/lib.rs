//! Secrets kept safe (the kernel's [`SecretStore`], for remote projects and data sources): the system's keychain
//! ([`Keychain`]: macOS Keychain, Windows Credential Manager, the Secret Service), or a
//! project file encrypted with a key that lives in the keychain ([`ProjectVault`]: the file
//! alone reveals nothing). Secrets are never written in plain text, nor printed. Data that
//! only leaves memory for a while is sealed with a key of the run ([`RunKey`]).

mod run_key;

pub use run_key::RunKey;

use std::path::PathBuf;
use std::sync::Arc;

use kernel::{Secret, SecretStore};
use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

/// The system's keychain, under `service` (named after the app by its caller).
#[derive(Debug, Clone)]
pub struct Keychain {
    pub service: String,
}

impl SecretStore for Keychain {
    fn get(&self, connection: &str) -> Option<Secret> {
        keyring::Entry::new(&self.service, connection).ok()?.get_password().ok().map(Secret::from)
    }

    /// Kept (replacing what was there). An entry the keychain says exists but won't update
    /// (macOS: one made by another build of the app, which this one can't find) is removed
    /// and made again; if that fails too, what was there is put back (never lost to a
    /// keychain locked or a prompt refused) and the first error is told.
    fn set(&self, connection: &str, secret: &str) -> Result<(), String> {
        let entry = keyring::Entry::new(&self.service, connection).map_err(|e| e.to_string())?;
        let Err(first) = entry.set_password(secret) else { return Ok(()) };
        let before = entry.get_password().ok().map(Zeroizing::new);
        if entry.delete_credential().is_err() {
            return Err(first.to_string());
        }
        if entry.set_password(secret).is_ok() {
            return Ok(());
        }
        if let Some(before) = before {
            let _ = entry.set_password(&before);
        }
        Err(first.to_string())
    }

    fn remove(&self, connection: &str) -> Result<(), String> {
        match keyring::Entry::new(&self.service, connection).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

/// A project's secrets in one file, each encrypted (ChaCha20-Poly1305, a fresh nonce, the
/// connection's id bound to it): `id\tnonce\tciphertext` lines, in hex. The key is in
/// `keys` (the keychain) under `key_name`, made on first use.
pub struct ProjectVault {
    pub file: PathBuf,
    pub keys: Arc<dyn SecretStore>,
    pub key_name: String,
}

impl std::fmt::Debug for ProjectVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProjectVault").field("file", &self.file).finish_non_exhaustive()
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The bytes `text` spells in hex (wiped once dropped: a key, a secret's ciphertext).
fn unhex(text: &str) -> Option<Zeroizing<Vec<u8>>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect::<Option<Vec<u8>>>()
        .map(Zeroizing::new)
}

impl ProjectVault {
    /// The vault's key (made and kept in the keychain when `make`: the first time something
    /// is stored; never by a read, nor while other secrets need the old one).
    fn key(&self, make: bool) -> Result<LessSafeKey, String> {
        let bytes = match self.keys.get(&self.key_name).and_then(|k| unhex(k.expose())) {
            Some(bytes) => bytes,
            None if !make => return Err("the vault's key isn't in the keychain".to_owned()),
            None => {
                let mut bytes = Zeroizing::new(vec![0u8; 32]);
                SystemRandom::new().fill(&mut bytes).map_err(|_| "no randomness".to_owned())?;
                self.keys.set(&self.key_name, &Zeroizing::new(hex(&bytes)))?;
                bytes
            }
        };
        let key =
            UnboundKey::new(&CHACHA20_POLY1305, &bytes).map_err(|_| "a bad key".to_owned())?;
        Ok(LessSafeKey::new(key))
    }

    /// The file's lines: (id, nonce, ciphertext); none while there's no file. A file that
    /// can't be read is an error: written over, it would lose the others' credentials.
    fn lines(&self) -> Result<Vec<(String, String, String)>, String> {
        let text = match std::fs::read_to_string(&self.file) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(format!("the credentials can't be read: {e}")),
        };
        Ok(text
            .lines()
            .filter_map(|l| {
                let mut f = l.split('\t');
                Some((f.next()?.to_owned(), f.next()?.to_owned(), f.next()?.to_owned()))
            })
            .collect())
    }

    fn save(&self, lines: &[(String, String, String)]) -> Result<(), String> {
        let text: String = lines.iter().map(|(id, n, c)| format!("{id}\t{n}\t{c}\n")).collect();
        // Atomic (a crash mid-write never loses the stored credentials) and the user's
        // alone from the start; a file made earlier with wider permissions is narrowed.
        files::write_private(&self.file, text.as_bytes()).map_err(|e| e.to_string())
    }
}

impl SecretStore for ProjectVault {
    fn get(&self, connection: &str) -> Option<Secret> {
        let (_, nonce, cipher) =
            self.lines().ok()?.into_iter().find(|(id, _, _)| id == connection)?;
        let key = self.key(false).ok()?;
        let nonce = Nonce::try_assume_unique_for_key(&unhex(&nonce)?).ok()?;
        let mut data = unhex(&cipher)?;
        let plain = key.open_in_place(nonce, Aad::from(connection.as_bytes()), &mut data).ok()?;
        let text = std::str::from_utf8(plain).ok()?;
        Some(Secret::new(text))
    }

    fn set(&self, connection: &str, secret: &str) -> Result<(), String> {
        // Read first: a vault that can't be read is never written over.
        let others = self.lines()?;
        // A new key only for a vault holding nothing else: a keychain locked or briefly
        // unavailable must never replace the key the others were sealed with.
        let alone = others.iter().all(|(id, _, _)| id == connection);
        let key = self.key(alone)?;
        let mut nonce = [0u8; 12];
        SystemRandom::new().fill(&mut nonce).map_err(|_| "no randomness".to_owned())?;
        // Room for the tag up front: growing would leave a copy of the secret behind.
        let mut data =
            Zeroizing::new(Vec::with_capacity(secret.len() + CHACHA20_POLY1305.tag_len()));
        data.extend_from_slice(secret.as_bytes());
        key.seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(connection.as_bytes()),
            &mut *data,
        )
        .map_err(|_| "can't encrypt".to_owned())?;
        let mut lines: Vec<_> = others.into_iter().filter(|(id, _, _)| id != connection).collect();
        lines.push((connection.to_owned(), hex(&nonce), hex(&data)));
        self.save(&lines)
    }

    fn remove(&self, connection: &str) -> Result<(), String> {
        let lines: Vec<_> =
            self.lines()?.into_iter().filter(|(id, _, _)| id != connection).collect();
        self.save(&lines)
    }
}

#[cfg(test)]
#[path = "../tests/unit/lib.rs"]
mod tests;
