//! What a question's answer does, carried by the question itself to its modal: done once, by
//! the window that shows it, and gone with the modal if it's closed without an answer.

use std::sync::{Arc, Mutex, PoisonError};

/// A value taken at most once, wherever its clones are. Compared by identity.
pub struct Once<T>(Arc<Mutex<Option<T>>>);

impl<T> Once<T> {
    pub fn new(value: T) -> Self {
        Self(Arc::new(Mutex::new(Some(value))))
    }

    /// The value, the first time it's asked for.
    pub fn take(&self) -> Option<T> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}

impl<T> Default for Once<T> {
    fn default() -> Self {
        Self(Arc::default())
    }
}

impl<T> Clone for Once<T> {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

impl<T> PartialEq for Once<T> {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl<T> std::fmt::Debug for Once<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Once")
    }
}

#[cfg(test)]
#[path = "../tests/unit/answer_slot.rs"]
mod tests;
