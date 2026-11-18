//! Requests between modules: a module contributes an [`Answers`] for a request's type; another
//! asks through its host (`host.ask(request)`), and gets `None` when nobody answers — the
//! module may simply not be there (`presence::request` says so beforehand). The answer runs at once on the UI thread with the asking
//! window's host, so it must stay short: what takes time goes to `host.spawn` (a slow answer is
//! reported to the app).

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::Arc;
use std::time::{Duration, Instant};

use kernel::Request;

use crate::WorkbenchHost;

/// An answer slower than this is reported: it held the UI thread for a frame.
const SLOW: Duration = Duration::from_millis(16);

type Handler = Arc<dyn Fn(Box<dyn Any>, &mut dyn WorkbenchHost) -> Box<dyn Any> + Send + Sync>;

/// What a module answers: one request's type.
#[derive(Clone)]
pub struct Answers {
    id: TypeId,
    name: &'static str,
    handler: Handler,
}

impl std::fmt::Debug for Answers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Answers").field("request", &self.name).finish_non_exhaustive()
    }
}

impl Answers {
    /// `answer` answers requests of type `R`.
    pub fn of<R: Request>(
        answer: impl Fn(R, &mut dyn WorkbenchHost) -> R::Answer + Send + Sync + 'static,
    ) -> Self {
        let handler: Handler = Arc::new(move |request, host| {
            match request.downcast::<R>() {
                Ok(request) => Box::new(answer(*request, host)),
                // Never: the book finds an answer by its request's type.
                Err(other) => other,
            }
        });
        Self { id: TypeId::of::<R>(), name: std::any::type_name::<R>(), handler }
    }
}

/// Every request answered in the app.
#[derive(Debug)]
pub struct AnswerBook {
    answers: HashMap<TypeId, Answers>,
    /// The requests answered twice (the first answer stayed): an error of the modules'.
    twice: Vec<&'static str>,
    /// The clock an answer is timed by (the domain has none), and who hears of a slow one.
    now: fn() -> Instant,
    slow: fn(&'static str, Duration),
}

impl AnswerBook {
    /// The book of `answers` (a request answered twice keeps its first: [`Self::twice`]);
    /// `slow` hears of an answer that held the UI thread, timed by `now`.
    pub fn new(
        answers: impl IntoIterator<Item = Answers>,
        now: fn() -> Instant,
        slow: fn(&'static str, Duration),
    ) -> Self {
        let mut book = Self { answers: HashMap::new(), twice: Vec::new(), now, slow };
        for answer in answers {
            let name = answer.name;
            match book.answers.entry(answer.id) {
                Entry::Occupied(_) => book.twice.push(name),
                Entry::Vacant(free) => {
                    free.insert(answer);
                }
            }
        }
        book
    }

    /// The requests with more than one answer.
    pub fn twice(&self) -> &[&'static str] {
        &self.twice
    }

    fn get<R: Request>(&self) -> Option<&Answers> {
        self.answers.get(&TypeId::of::<R>())
    }

    pub fn has<R: Request>(&self) -> bool {
        self.get::<R>().is_some()
    }
}

impl dyn WorkbenchHost + '_ {
    /// `request` answered by whoever answers it; `None`: nobody does.
    pub fn ask<R: Request>(&mut self, request: R) -> Option<R::Answer> {
        let book = self.answers()?;
        let answer = book.get::<R>()?;
        let started = (book.now)();
        let reply = (answer.handler)(Box::new(request), self);
        let took = (book.now)().saturating_duration_since(started);
        if took > SLOW {
            (book.slow)(answer.name, took);
        }
        reply.downcast::<R::Answer>().ok().map(|reply| *reply)
    }
}

#[cfg(test)]
#[path = "../tests/unit/answers.rs"]
mod tests;
