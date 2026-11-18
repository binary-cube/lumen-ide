-- 10+: an identity column, a partitioned table and its partitions.
SET ROLE editor;
CREATE TABLE sales.coupons (
    id integer GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    code text NOT NULL UNIQUE,
    percent smallint NOT NULL CHECK (percent BETWEEN 1 AND 90)
);
INSERT INTO sales.coupons (code, percent) SELECT 'C' || g, 5 + g % 30 FROM generate_series(1, 40) g;
RESET ROLE;

SET ROLE editor_owner;
CREATE TABLE audit.log (
    at date NOT NULL,
    message text NOT NULL
) PARTITION BY RANGE (at);
CREATE TABLE audit.log_2024 PARTITION OF audit.log FOR VALUES FROM ('2024-01-01') TO ('2025-01-01');
CREATE TABLE audit.log_2025 PARTITION OF audit.log FOR VALUES FROM ('2025-01-01') TO ('2026-01-01');
INSERT INTO audit.log SELECT date '2024-06-01' + g, 'message ' || g FROM generate_series(1, 400) g;
GRANT SELECT ON audit.log, audit.log_2024, audit.log_2025 TO editor;
RESET ROLE;
GRANT SELECT ON sales.coupons, audit.log TO editor_reader;
