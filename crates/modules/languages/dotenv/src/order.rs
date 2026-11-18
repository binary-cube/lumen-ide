//! In which order a project's `.env` files are loaded — its framework's
//! ([`EnvFileOrder`], contributed: Symfony's, Laravel's, Vite's), else the common one
//! (`.env`, `.env.local`, `.env.{env}`, `.env.{env}.local`) — and which only document the
//! keys (`.env.example`, `.env.dist`: never loaded, listed last). Pure.

use code_intelligence::env::EnvFileOrder;

/// The order without a framework's: the loaders' most common one (Symfony's, Vite's,
/// dotenv-flow's).
pub const COMMON: EnvFileOrder = EnvFileOrder {
    name: "",
    markers: &[],
    files: &[".env", ".env.local", ".env.{env}", ".env.{env}.local"],
};

/// The names of the files that document the keys (copied into a `.env` by hand).
const DOCUMENTATION: &[&str] = &["example", "dist", "sample", "template", "defaults"];

/// A file that documents the keys rather than defining them: `.env.example`,
/// `.env.dist`, `.env.sample`, `.env.template`, `.env.defaults` (also `prod.env.example`).
pub fn documents(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    DOCUMENTATION.iter().any(|d| lower.rsplit('.').next() == Some(d))
}

/// The order of `project`'s files: the first contributed order whose marker it has
/// (`has(marker)`), else [`COMMON`].
pub fn of_project<'a>(orders: &'a [EnvFileOrder], has: &dyn Fn(&str) -> bool) -> &'a EnvFileOrder {
    orders.iter().find(|o| o.markers.iter().any(|m| has(m))).unwrap_or(&COMMON)
}

/// Where a file named `name` comes: loaded ones by `order`, the other `.env` files after
/// them, the documentation last.
pub fn rank(order: &EnvFileOrder, name: &str) -> usize {
    let after = order.files.len();
    if documents(name) {
        return after + 1;
    }
    order.rank(name).unwrap_or(after)
}

/// `items` sorted as their files (`name`) are loaded: by [`rank`], then by name.
pub fn sort<T>(items: &mut [T], order: &EnvFileOrder, name: impl Fn(&T) -> &str) {
    items.sort_by(|a, b| {
        let (a, b) = (name(a), name(b));
        (rank(order, a), a).cmp(&(rank(order, b), b))
    });
}

#[cfg(test)]
#[path = "../tests/unit/order.rs"]
mod tests;
