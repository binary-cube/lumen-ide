-- A shop's schema, in SQLite.
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    email TEXT NOT NULL UNIQUE,
    score REAL DEFAULT 0.5,
    active BOOLEAN DEFAULT TRUE
);

/* The orders, with their user. */
CREATE TABLE orders (
    id INTEGER PRIMARY KEY,
    user_id INTEGER REFERENCES users (id) ON DELETE CASCADE,
    total NUMERIC
);

SELECT u.email, count(*) AS n, sum(o.total)
FROM users u
JOIN orders o ON o.user_id = u.id
WHERE u.active = TRUE AND u.email LIKE '%@example.com' AND o.total > ?
GROUP BY u.email
ORDER BY n DESC
LIMIT 10;

INSERT INTO users (email, score) VALUES ('ana@example.com', 1.5), ('ion@example.com', NULL);

UPDATE users SET score = score + 1 WHERE id = ?1;
