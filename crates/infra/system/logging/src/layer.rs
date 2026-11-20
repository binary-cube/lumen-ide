//! The `tracing` layer: each event made into its line on the thread that logs it (its
//! fields can only be read there), then handed to the writer on a bounded channel. Full:
//! the line is counted, not waited for, and the next line sent says how many were dropped.
//! A span's fields are kept with it (in its extensions) and written with each event in it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError};

use serde_json::Value;
use tracing::span::{Attributes, Id, Record};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;

use crate::line::{self, Fields, Head};
use crate::writer::Msg;

/// The layer (see the module's doc).
pub struct JsonLayer {
    tx: SyncSender<Msg>,
    run: String,
    dropped: AtomicU64,
}

impl std::fmt::Debug for JsonLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonLayer").field("run", &self.run).finish_non_exhaustive()
    }
}

impl JsonLayer {
    pub(crate) fn new(tx: SyncSender<Msg>, run: String) -> Self {
        Self { tx, run, dropped: AtomicU64::new(0) }
    }

    /// Lines dropped so far (the channel was full), not yet said in a line.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn send(&self, text: String, at_ms: i64, urgent: bool) {
        match self.tx.try_send(Msg::Line { text, at_ms, urgent }) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

/// A span's fields, kept in its extensions.
struct SpanFields(serde_json::Map<String, Value>);

/// Milliseconds since the epoch, now.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// A level as the lines write it.
pub fn level_name(level: Level) -> &'static str {
    match level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARN",
        Level::INFO => "INFO",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "TRACE",
    }
}

impl<S> tracing_subscriber::Layer<S> for JsonLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &Attributes<'_>, id: &Id, ctx: Context<'_, S>) {
        let mut fields = Fields::default();
        attrs.record(&mut fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanFields(fields.fields));
        }
    }

    fn on_record(&self, id: &Id, values: &Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut fields = Fields::default();
        values.record(&mut fields);
        let mut extensions = span.extensions_mut();
        match extensions.get_mut::<SpanFields>() {
            Some(kept) => kept.0.extend(fields.fields),
            None => extensions.insert(SpanFields(fields.fields)),
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let at_ms = now_ms();
        let meta = event.metadata();
        let mut fields = Fields::default();
        event.record(&mut fields);
        let spans: Vec<Value> = ctx
            .event_scope(event)
            .map(|scope| {
                scope
                    .from_root()
                    .map(|span| {
                        let mut object = serde_json::Map::new();
                        object.insert("name".into(), Value::String(span.name().to_owned()));
                        if let Some(kept) = span.extensions().get::<SpanFields>()
                            && !kept.0.is_empty()
                        {
                            object.insert("fields".into(), Value::Object(kept.0.clone()));
                        }
                        Value::Object(object)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let thread = std::thread::current();
        let head = Head {
            at_ms,
            level: level_name(*meta.level()),
            target: meta.target(),
            thread: thread.name().unwrap_or("unnamed"),
            run: &self.run,
        };
        // Dropped before (the channel was full): said first, once there's room.
        let dropped = self.dropped.swap(0, Ordering::Relaxed);
        if dropped > 0 {
            let mut note = Fields {
                message: Some("log lines dropped: the writer was behind".into()),
                ..Fields::default()
            };
            note.fields.insert("dropped".into(), Value::from(dropped));
            let warn = Head { level: "WARN", target: "logging", ..head };
            self.send(line::line(warn, note, Vec::new()), at_ms, true);
        }
        let urgent = *meta.level() <= Level::WARN;
        self.send(line::line(head, fields, spans), at_ms, urgent);
    }
}

#[cfg(test)]
#[path = "../tests/unit/layer.rs"]
mod tests;
