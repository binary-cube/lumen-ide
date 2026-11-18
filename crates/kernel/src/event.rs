//! Typed event bus for domain events.
//!
//! Delivery is synchronous, on the publishing thread. Handlers must be short:
//! heavy work is passed on (channel, async task), not done in the handler.

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, Weak};

/// Marker for domain events (e.g. `DocumentChanged`, `ThemeChanged`).
pub trait DomainEvent: Any + Send + Sync {}

type Handler = Arc<dyn Fn(&dyn Any) + Send + Sync>;

#[derive(Default)]
struct Registry {
    handlers: HashMap<TypeId, Vec<(u64, Handler)>>,
}

/// Event bus. Cloning is cheap: all clones share the same subscribers.
#[derive(Clone, Default)]
pub struct EventBus {
    registry: Arc<RwLock<Registry>>,
    next_id: Arc<AtomicU64>,
}

impl fmt::Debug for EventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventBus").finish_non_exhaustive()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    /// Subscribes a handler to events of type `E`.
    /// The subscription stays active as long as the `Subscription` is alive.
    #[must_use = "the subscription ends when the Subscription is dropped"]
    pub fn subscribe<E, F>(&self, handler: F) -> Subscription
    where
        E: DomainEvent,
        F: Fn(&E) + Send + Sync + 'static,
    {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let handler: Handler = Arc::new(move |event: &dyn Any| {
            if let Some(event) = event.downcast_ref::<E>() {
                handler(event);
            }
        });
        write_lock(&self.registry)
            .handlers
            .entry(TypeId::of::<E>())
            .or_default()
            .push((id, handler));

        Subscription {
            registry: Arc::downgrade(&self.registry),
            type_id: TypeId::of::<E>(),
            id,
            ended: None,
        }
    }

    /// Publishes an event to all subscribers of type `E`.
    pub fn publish<E: DomainEvent>(&self, event: &E) {
        // We copy the handlers and release the lock before calling, so a handler
        // can (un)subscribe or publish other events without deadlock.
        let handlers: Vec<Handler> = read_lock(&self.registry)
            .handlers
            .get(&TypeId::of::<E>())
            .map(|hs| hs.iter().map(|(_, h)| Arc::clone(h)).collect())
            .unwrap_or_default();

        for handler in handlers {
            handler(event);
        }
    }
}

/// Subscription guard: on drop, the handler is removed from the bus.
pub struct Subscription {
    registry: Weak<RwLock<Registry>>,
    type_id: TypeId,
    id: u64,
    /// What [`Subscription::on_end`] runs when it's dropped.
    ended: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl std::fmt::Debug for Subscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Subscription").field("id", &self.id).finish_non_exhaustive()
    }
}

impl Subscription {
    /// No handler, only `f`, run when it's dropped: what a project's service registered
    /// beside its subscriptions (a static table's entry) goes with them.
    pub fn on_end(f: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self { registry: Weak::new(), type_id: TypeId::of::<()>(), id: 0, ended: Some(Box::new(f)) }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(ended) = self.ended.take() {
            ended();
            return;
        }
        if let Some(registry) = self.registry.upgrade()
            && let Some(handlers) = write_lock(&registry).handlers.get_mut(&self.type_id)
        {
            handlers.retain(|(id, _)| *id != self.id);
        }
    }
}

// A handler that panicked can't leave the registry inconsistent
// (we only modify whole vectors), so we carry on even after a "poisoned" lock.
fn read_lock(lock: &RwLock<Registry>) -> std::sync::RwLockReadGuard<'_, Registry> {
    lock.read().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn write_lock(lock: &RwLock<Registry>) -> std::sync::RwLockWriteGuard<'_, Registry> {
    lock.write().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
#[path = "../tests/unit/event.rs"]
mod tests;
