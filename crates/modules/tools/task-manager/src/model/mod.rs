//! The Task Manager's view model: how the parts are grouped ([`groups`]), what a view lists
//! for a look and its state ([`view_model`]) and the texts written once for it
//! ([`format`]).

pub mod format;
pub mod groups;
pub mod view_model;

pub use format::{PartTexts, ProcessTexts, duration, percent, rate};
pub use groups::{GroupKey, Grouping};
pub use view_model::{Column, Item, OperationRow, Selected, ViewModel, ViewState, selection};
