//! Where the request's top row puts its parts at a width: the method, the URL field, Send.
//! The URL takes what's left; under its least, Send shows its icon alone, then the method
//! narrows: nothing is drawn over another.

/// What the row's parts need (physical pixels).
#[derive(Debug, Clone, Copy)]
pub(super) struct TopWidths {
    /// The method's field, and the least it narrows to.
    pub method: (f32, f32),
    /// The URL field's least.
    pub url: f32,
    /// Send with its label, and by its icon alone.
    pub send: (f32, f32),
    /// Between two parts.
    pub gap: f32,
}

/// Each part's left edge and width, and whether Send shows its label.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct TopRow {
    /// The method's field (left edge, width).
    pub method: (f32, f32),
    /// The URL field: what's left between the method and Send.
    pub url: (f32, f32),
    /// Send (or Cancel), at the row's end.
    pub send: (f32, f32),
    /// Send shows its label (else its icon alone, round).
    pub send_labeled: bool,
}

/// The top row between `min_x` and `max_x`.
pub(super) fn top_row((min_x, max_x): (f32, f32), w: TopWidths) -> TopRow {
    let room = (max_x - min_x).max(0.0);
    let gaps = 2.0 * w.gap;
    let send_labeled = w.method.0 + w.url + w.send.0 + gaps <= room;
    let send = if send_labeled { w.send.0 } else { w.send.1 }.min(room);
    let method = (room - gaps - w.url - send)
        .max(w.method.1)
        .min(w.method.0)
        .min((room - w.gap - send).max(0.0));
    let url = (room - method - send - gaps).max(0.0);
    TopRow {
        method: (min_x, method),
        url: (min_x + method + w.gap, url),
        send: (max_x - send, send),
        send_labeled,
    }
}

#[cfg(test)]
#[path = "../../tests/unit/editor/layout.rs"]
mod tests;
