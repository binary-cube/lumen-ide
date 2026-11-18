//! A request between parts that don't know each other: its type is the message, [`Request::Answer`]
//! what comes back. Who answers and how it's reached is the workbench's (`workbench_model::Answers`).

/// A question or an order with one answer, at most one party answering it.
pub trait Request: Send + 'static {
    type Answer: Send + 'static;
}
