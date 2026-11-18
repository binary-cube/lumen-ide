-- MySQL 8.0+: a functional index (8.0.13), a descending one, a CTE-made table.
CREATE INDEX customers_lower_name ON customers ((LOWER(name)));
CREATE INDEX orders_recent ON orders (ordered_at DESC);
CREATE TABLE coupons AS
    WITH RECURSIVE c (n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM c WHERE n < 40)
    SELECT n AS id, CONCAT('C', n) AS code, 5 + n % 30 AS percent FROM c;
ALTER TABLE coupons ADD PRIMARY KEY (id), ADD UNIQUE KEY coupons_code (code);
