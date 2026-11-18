-- The shop database, as MySQL 5.7+ and MariaDB 10.11+ take it (MySQL 8's own parts in
-- shop-80.sql). No recursive CTE (5.7 has none): rows from a table of digits.

CREATE TABLE digits (d tinyint PRIMARY KEY);
INSERT INTO digits VALUES (0), (1), (2), (3), (4), (5), (6), (7), (8), (9);
CREATE VIEW numbers AS
    SELECT a.d + 10 * b.d + 100 * c.d + 1000 * e.d + 1 AS n
    FROM digits a, digits b, digits c, digits e;

CREATE TABLE countries (
    code char(2) PRIMARY KEY,
    name varchar(100) NOT NULL UNIQUE
) COMMENT 'ISO 3166 countries';

CREATE TABLE customers (
    id int AUTO_INCREMENT PRIMARY KEY,
    email varchar(200) NOT NULL COMMENT 'Where we write to them',
    name varchar(200) NOT NULL,
    country char(2),
    created_at datetime NOT NULL DEFAULT CURRENT_TIMESTAMP,
    profile json,
    vip boolean NOT NULL DEFAULT false,
    UNIQUE KEY customers_email_key (email),
    KEY customers_created (created_at),
    CONSTRAINT customers_country FOREIGN KEY (country) REFERENCES countries (code)
        ON UPDATE CASCADE,
    CONSTRAINT customers_email_check CHECK (email LIKE '%@%')
) COMMENT 'Who buys';

CREATE TABLE products (
    id int AUTO_INCREMENT PRIMARY KEY,
    sku varchar(32) NOT NULL UNIQUE,
    name varchar(200) NOT NULL,
    price decimal(10, 2) NOT NULL,
    size enum('S', 'M', 'L', 'XL') NOT NULL DEFAULT 'M',
    flags set('new', 'sale', 'gift') NOT NULL DEFAULT '',
    CONSTRAINT products_price CHECK (price > 0)
);

CREATE TABLE orders (
    id bigint AUTO_INCREMENT PRIMARY KEY,
    customer_id int NOT NULL,
    status enum('new', 'paid', 'shipped', 'cancelled') NOT NULL DEFAULT 'new'
        COMMENT 'Where the order is',
    ordered_at datetime NOT NULL,
    note text,
    KEY orders_customer (customer_id, ordered_at),
    CONSTRAINT orders_customer_fk FOREIGN KEY (customer_id) REFERENCES customers (id)
        ON DELETE CASCADE
);

CREATE TABLE order_lines (
    order_id bigint NOT NULL,
    line_no smallint NOT NULL,
    product_id int NOT NULL,
    quantity int NOT NULL,
    price decimal(10, 2) NOT NULL,
    -- A generated column (5.7+, MariaDB 10.2+).
    amount decimal(12, 2) GENERATED ALWAYS AS (quantity * price) STORED,
    PRIMARY KEY (order_id, line_no),
    CONSTRAINT order_lines_order FOREIGN KEY (order_id) REFERENCES orders (id) ON DELETE CASCADE,
    CONSTRAINT order_lines_product FOREIGN KEY (product_id) REFERENCES products (id),
    CONSTRAINT order_lines_quantity CHECK (quantity > 0)
);

-- A composite foreign key.
CREATE TABLE shipments (
    id int AUTO_INCREMENT PRIMARY KEY,
    order_id bigint NOT NULL,
    line_no smallint NOT NULL,
    shipped_on date NOT NULL,
    carrier varchar(50),
    CONSTRAINT shipments_line FOREIGN KEY (order_id, line_no)
        REFERENCES order_lines (order_id, line_no) ON DELETE CASCADE
);

CREATE VIEW order_totals AS
    SELECT o.id, o.customer_id, o.status, SUM(l.quantity * l.price) AS total
    FROM orders o JOIN order_lines l ON l.order_id = o.id
    GROUP BY o.id, o.customer_id, o.status;

-- "Owned by another role": editor_owner's view of a table editor may not read
-- (editor_private's).
CREATE DEFINER = 'editor_owner'@'%' SQL SECURITY DEFINER VIEW private_summary AS
    SELECT COUNT(*) AS hidden FROM editor_private.hidden;

CREATE FUNCTION order_total(o bigint) RETURNS decimal(12, 2) READS SQL DATA
    RETURN (SELECT SUM(quantity * price) FROM order_lines WHERE order_id = o);

-- ─── The data: ~10k rows ───
INSERT INTO countries VALUES
    ('RO', 'Romania'), ('DE', 'Germany'), ('FR', 'France'), ('US', 'United States'),
    ('JP', 'Japan'), ('BR', 'Brazil');
INSERT INTO customers (email, name, country, created_at, profile, vip)
    SELECT CONCAT('customer', n, '@example.com'), CONCAT('Customer ', n),
           ELT(1 + n % 6, 'RO', 'DE', 'FR', 'US', 'JP', NULL),
           '2024-01-01' + INTERVAL n * 37 MINUTE,
           CONCAT('{"newsletter": ', IF(n % 2 = 0, 'true', 'false'), ', "visits": ', n % 17, '}'),
           n % 10 = 0
    FROM numbers WHERE n <= 2000;
INSERT INTO products (sku, name, price, size, flags)
    SELECT CONCAT('SKU-', LPAD(n, 4, '0')), CONCAT('Product ', n), (n % 50) * 3 + 0.99,
           ELT(1 + n % 4, 'S', 'M', 'L', 'XL'), IF(n % 3 = 0, 'new,sale', '')
    FROM numbers WHERE n <= 200;
INSERT INTO orders (customer_id, status, ordered_at, note)
    SELECT 1 + n % 2000, ELT(1 + n % 4, 'new', 'paid', 'shipped', 'cancelled'),
           '2024-02-01' + INTERVAL n * 13 MINUTE, IF(n % 7 = 0, 'gift', NULL)
    FROM numbers WHERE n <= 4000;
INSERT INTO order_lines (order_id, line_no, product_id, quantity, price)
    SELECT o.id, l.n, 1 + (o.id * l.n) % 200, 1 + (o.id + l.n) % 5, 9.99 + l.n
    FROM orders o, (SELECT 1 AS n UNION ALL SELECT 2) l;
INSERT INTO shipments (order_id, line_no, shipped_on, carrier)
    SELECT order_id, line_no, '2024-03-01' + INTERVAL order_id % 90 DAY, CONCAT('Carrier ', order_id % 3)
    FROM order_lines WHERE order_id % 4 = 2;
