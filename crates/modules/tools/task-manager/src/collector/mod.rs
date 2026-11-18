//! The Task Manager's collector: what a look finds ([`frame`]), what it works out at each
//! look ([`sample`]), the values over time ([`history`]) and the window's hub running the
//! looks for its views ([`hub`]).

pub mod frame;
pub mod history;
pub mod hub;
mod sample;

pub use frame::{Frame, Line, ProcessRow, Tick, key};
pub use history::{History, Metric, Ring, Trace};
pub use hub::{Hub, HubInfo, Subscription, configure};
pub use sample::*;
