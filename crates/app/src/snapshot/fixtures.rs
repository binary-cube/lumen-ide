//! What the screens are shown with: fixed times and the consoles' texts.

/// The snapshots' time (2026-10-31 10:00:23.698 UTC): a history's entries counted from it.
pub(super) const SNAPSHOT_WALL_MS: i64 = 1_793_440_823_698;

/// The statements of the history shown (`CONSOLE=history`), oldest first: their text, time,
/// rows and how they ended.
pub(super) const HISTORY_SAMPLE: &[(&str, u64, Option<u64>, database::history::Outcome)] = &[
    ("SELECT * FROM customers", 9, Some(12), database::history::Outcome::Ok),
    (
        "SELECT id, customer_id, total, status\nFROM orders\nWHERE status = 'paid'",
        14,
        Some(4),
        database::history::Outcome::Ok,
    ),
    (
        "SELECT * FROM order_items WHERE order_id = :order",
        6,
        Some(3),
        database::history::Outcome::Ok,
    ),
    (
        "UPDATE orders SET status = 'shipped' WHERE id = 3",
        4,
        Some(1),
        database::history::Outcome::Ok,
    ),
    ("SELECT count(*) FROM orderz", 2, None, database::history::Outcome::Failed),
    (
        "SELECT c.name, count(o.id) AS orders\nFROM customers c\nJOIN orders o ON o.customer_id = c.id\nGROUP BY c.id\nORDER BY orders DESC",
        28,
        Some(9),
        database::history::Outcome::Ok,
    ),
    (
        "SELECT * FROM products ORDER BY price DESC LIMIT 5",
        5,
        Some(5),
        database::history::Outcome::Ok,
    ),
];

/// The project's SQL file of `CONSOLE=file|file-attached` (the script removes it after).
pub(super) const SNAPSHOT_SQL_FILE: &str = "report.sql";

/// The sample console's text (`CONSOLE` snapshots).
pub(super) const CONSOLE_SQL: &str = "-- The best customers: what they ordered, what they spent.
SELECT c.name, c.country, count(o.id) AS orders, round(sum(o.total), 2) AS spent
FROM customers c
JOIN orders o ON o.customer_id = c.id
WHERE o.status <> 'cancelled'
GROUP BY c.id
ORDER BY spent DESC
LIMIT 10;

-- Orders paid and waiting to be shipped.
SELECT id, customer_id, total, status
FROM orders
WHERE status = 'paid';

UPDATE orders SET status = 'shipped' WHERE id = 3;
";

/// A console's script (`CONSOLE=output|results-many`): a query of one table, one joining two,
/// a write, another table's rows.
pub(super) const CONSOLE_SCRIPT: &str = "-- What the shop sold, and to whom.
SELECT id, name, country, email FROM customers;

SELECT c.name, count(o.id) AS orders, round(sum(o.total), 2) AS spent
FROM customers c
JOIN orders o ON o.customer_id = c.id
GROUP BY c.id
ORDER BY spent DESC;

UPDATE orders SET status = 'shipped' WHERE id = 3;

SELECT * FROM orders WHERE status = 'paid';
";

/// A console committing by hand (`CONSOLE=tx-manual`): two writes waiting for Commit, then
/// what they did read back.
pub(super) const CONSOLE_TX: &str =
    "-- Ship the paid orders, by hand: nothing is kept before Commit.
UPDATE orders SET status = 'shipped' WHERE status = 'paid';
DELETE FROM orders WHERE status = 'cancelled';

SELECT id, customer_id, total, status FROM orders;
";

/// A console about to lose every row (`CONSOLE=danger`): confirmed first.
pub(super) const CONSOLE_DANGER: &str = "-- Start over: the orders table emptied.
DELETE FROM orders;
DROP TABLE products;
";

/// A write on a production source in Tx: Auto (`CONSOLE=prod-confirm`): confirmed, with
/// "I understand" first.
pub(super) const CONSOLE_PROD: &str = "-- Mark order 3 as shipped.
UPDATE orders SET status = 'shipped' WHERE id = 3;
";

/// A console whose statement has parameters (`CONSOLE=params`): their values asked first.
pub(super) const CONSOLE_PARAMS: &str = "-- A customer's orders in a status, the biggest first.
SELECT o.id, o.total, o.status, o.note
FROM orders o
WHERE o.customer_id = :customer
  AND o.status = :status
  AND o.total >= :min_total
ORDER BY o.total DESC
LIMIT :limit;
";

/// A console explaining a query (`CONSOLE=plan`): its plan as a tree.
pub(super) const CONSOLE_PLAN: &str = "-- What each customer spent, and their biggest order.
WITH spent AS (
  SELECT customer_id, sum(total) AS total, count(*) AS orders
  FROM orders
  WHERE status <> 'cancelled'
  GROUP BY customer_id
)
SELECT c.name, c.country, s.total, s.orders,
       (SELECT max(o.total) FROM orders o WHERE o.customer_id = c.id) AS biggest
FROM customers c
JOIN spent s ON s.customer_id = c.id
WHERE c.country IN ('Romania', 'Germany', 'France')
ORDER BY s.total DESC;
";

/// A console reading one table (`CONSOLE=result`).
pub(super) const CONSOLE_CUSTOMERS: &str = "SELECT * FROM customers;
";

/// A console completing a table after `FROM` (`CONSOLE=completion`).
/// A console calling a function (`CONSOLE=signature`): its parameters shown as it's typed.
pub(super) const CONSOLE_SIGNATURE: &str = "-- The customers' initials.
SELECT substr";

pub(super) const CONSOLE_COMPLETION: &str = "-- Who ordered what.
SELECT *
FROM ";

/// A console completing a `JOIN` (`CONSOLE=join`): the related table with its condition.
pub(super) const CONSOLE_JOIN: &str = "-- Each order with its customer.
SELECT o.id, o.total
FROM orders o
JOIN ";

/// A console hovering a table (`CONSOLE=hover`): its card lists the columns.
pub(super) const CONSOLE_HOVER: &str = "-- Big orders, newest first.
SELECT o.id, o.total, c.name
FROM orders o
JOIN customers c ON c.id = o.customer_id
WHERE o.total > 100
ORDER BY o.created_at DESC;
";

/// A console naming a table the schema doesn't have (`CONSOLE=unknown-table`): underlined,
/// its message shown; a CTE and a table it creates aren't.
pub(super) const CONSOLE_UNKNOWN: &str = "-- What each order holds.
WITH paid AS (SELECT * FROM orders WHERE status = 'paid')
SELECT p.id, l.qty
FROM paid p
JOIN order_lines l ON l.order_id = p.id;
";

/// A console whose statement the engine refuses (`CONSOLE=error`): its place underlined.
pub(super) const CONSOLE_ERROR: &str = "-- Orders paid and waiting to be shipped.
SELECT id, customer_id, total
FROM orders WHER status = 'paid';
";
