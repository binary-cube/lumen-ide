//! Shared kernel: identifiers, text positions and the event bus, common to all contexts.
//!
//! The rule: only concepts that **all** contexts understand the same way go here.
//! Anything else belongs to a bounded context.

pub mod brand;
pub mod cache;
pub mod color;
pub mod cpu;
pub mod event;
pub mod fuzzy;
pub mod geometry;
pub mod id;
pub mod memory;
pub mod paths;
pub mod queue;
pub mod request;
pub mod secret;
pub mod text;
pub mod work;

pub use cache::{GenerationalCache, Interner, Memo, Packed, Purge, SharedMemo, cache_stamp};
pub use color::Rgba;
pub use event::{DomainEvent, EventBus, Subscription};
pub use fuzzy::{FuzzyStyle, fuzzy_match, fuzzy_score};
pub use geometry::{PhysicalSize, Point, Rect, Size};
pub use id::{DocumentId, LanguageId, ModuleId, ProjectId};
pub use paths::{DependencyFinder, ProjectDirs, in_dependencies, in_dependencies_or_git};
pub use queue::{Batch, Coalescing, Latest, QueueStats, SharedQueue};
pub use request::Request;
pub use secret::{Secret, SecretStore};
pub use text::{ByteOffset, ByteRange, Column, Line, TextPosition, TextRange};
pub use work::{Cancel, Pool, Priority};
