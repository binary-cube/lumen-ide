//! A key that lives only as long as the app's process: data sealed with it (ChaCha20-Poly1305,
//! a fresh nonce each time, what it belongs to bound to it) can be read back during the run
//! and by nothing afterwards — what's written to disk only to leave memory for a while (a
//! terminal's history) needs no keychain and asks for no password. Never written anywhere.

use std::sync::OnceLock;

use ring::aead::{Aad, CHACHA20_POLY1305, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use zeroize::Zeroizing;

/// The run's key (see the module).
pub struct RunKey {
    key: LessSafeKey,
}

impl std::fmt::Debug for RunKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RunKey")
    }
}

impl RunKey {
    /// A new random key (`None`: the system gave no randomness).
    pub fn new() -> Option<Self> {
        let mut bytes = Zeroizing::new([0u8; 32]);
        SystemRandom::new().fill(&mut *bytes).ok()?;
        let key = UnboundKey::new(&CHACHA20_POLY1305, &*bytes).ok()?;
        Some(Self { key: LessSafeKey::new(key) })
    }

    /// The process's own, made the first time it's asked.
    pub fn shared() -> Option<&'static Self> {
        static SHARED: OnceLock<Option<RunKey>> = OnceLock::new();
        SHARED.get_or_init(Self::new).as_ref()
    }

    /// `plain` sealed for `aad` (what it belongs to: read back only for the same): the
    /// nonce, then the ciphertext and its tag.
    pub fn seal(&self, aad: &[u8], plain: &[u8]) -> Result<Vec<u8>, String> {
        let mut nonce = [0u8; NONCE_LEN];
        SystemRandom::new().fill(&mut nonce).map_err(|_| "no randomness".to_owned())?;
        let mut sealed = Vec::with_capacity(NONCE_LEN + plain.len() + CHACHA20_POLY1305.tag_len());
        sealed.extend_from_slice(&nonce);
        sealed.extend_from_slice(plain);
        let mut body = sealed.split_off(NONCE_LEN);
        self.key
            .seal_in_place_append_tag(
                Nonce::assume_unique_for_key(nonce),
                Aad::from(aad),
                &mut body,
            )
            .map_err(|_| "can't encrypt".to_owned())?;
        sealed.append(&mut body);
        Ok(sealed)
    }

    /// What [`Self::seal`] sealed for `aad`; `None` if it's another's, changed, or not sealed
    /// with this key (an earlier run's).
    pub fn open(&self, aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
        let (nonce, body) = sealed.split_at_checked(NONCE_LEN)?;
        let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
        let mut body = body.to_vec();
        let plain = self.key.open_in_place(nonce, Aad::from(aad), &mut body).ok()?.len();
        body.truncate(plain);
        Some(body)
    }
}

#[cfg(test)]
#[path = "../tests/unit/run_key.rs"]
mod tests;
