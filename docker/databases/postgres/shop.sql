-- The shop database, as every server from 9.2 takes it (newer servers' own parts in
-- shop-93.sql, shop-94.sql, shop-10.sql, shop-12.sql). Run by the superuser; each object made
-- by the role that should own it.

-- ─── Schemas: public (editor's), sales (editor's), audit (editor_owner's) ───
ALTER SCHEMA public OWNER TO editor;
CREATE SCHEMA sales AUTHORIZATION editor;
CREATE SCHEMA audit AUTHORIZATION editor_owner;
GRANT USAGE ON SCHEMA audit TO editor;
-- editor_owner makes a table in editor's public too (15+ no longer lets everyone).
GRANT CREATE ON SCHEMA public TO editor_owner;
COMMENT ON SCHEMA sales IS 'Orders and what they hold';
COMMENT ON SCHEMA audit IS 'Owned by another role: editor reads part of it';

SET ROLE editor;

CREATE TYPE sales.order_status AS ENUM ('new', 'paid', 'shipped', 'cancelled');

CREATE TABLE public.countries (
    code char(2) PRIMARY KEY,
    name text NOT NULL UNIQUE
);
COMMENT ON TABLE public.countries IS 'ISO 3166 countries';

CREATE TABLE public.customers (
    id serial PRIMARY KEY,
    email varchar(200) NOT NULL,
    name text NOT NULL,
    country char(2) REFERENCES public.countries (code) ON UPDATE CASCADE,
    created_at timestamptz NOT NULL DEFAULT now(),
    profile json,
    vip boolean NOT NULL DEFAULT false,
    CONSTRAINT customers_email_check CHECK (email LIKE '%@%'),
    CONSTRAINT customers_email_key UNIQUE (email)
);
COMMENT ON TABLE public.customers IS 'Who buys';
COMMENT ON COLUMN public.customers.email IS 'Where we write to them';
-- An expression index and a partial one.
CREATE INDEX customers_lower_name ON public.customers (lower(name));
CREATE INDEX customers_vip_created ON public.customers (created_at DESC) WHERE vip;

CREATE TABLE sales.products (
    id serial PRIMARY KEY,
    sku varchar(32) NOT NULL UNIQUE,
    name text NOT NULL,
    price numeric(10, 2) NOT NULL CHECK (price > 0),
    tags text[] NOT NULL DEFAULT '{}'
);

CREATE TABLE sales.orders (
    id bigserial PRIMARY KEY,
    customer_id integer NOT NULL REFERENCES public.customers (id) ON DELETE CASCADE,
    status sales.order_status NOT NULL DEFAULT 'new',
    ordered_at timestamp NOT NULL,
    note text
);
CREATE INDEX orders_customer ON sales.orders (customer_id, ordered_at);
COMMENT ON COLUMN sales.orders.status IS 'Where the order is';

CREATE TABLE sales.order_lines (
    order_id bigint NOT NULL REFERENCES sales.orders (id) ON DELETE CASCADE,
    line_no smallint NOT NULL,
    product_id integer NOT NULL REFERENCES sales.products (id),
    quantity integer NOT NULL CHECK (quantity > 0),
    price numeric(10, 2) NOT NULL,
    PRIMARY KEY (order_id, line_no)
);

-- A composite foreign key.
CREATE TABLE sales.shipments (
    id serial PRIMARY KEY,
    order_id bigint NOT NULL,
    line_no smallint NOT NULL,
    shipped_on date NOT NULL,
    carrier text,
    CONSTRAINT shipments_line FOREIGN KEY (order_id, line_no)
        REFERENCES sales.order_lines (order_id, line_no) ON DELETE CASCADE
);

CREATE VIEW sales.order_totals AS
    SELECT o.id, o.customer_id, o.status, sum(l.quantity * l.price) AS total
    FROM sales.orders o JOIN sales.order_lines l ON l.order_id = o.id
    GROUP BY o.id, o.customer_id, o.status;
COMMENT ON VIEW sales.order_totals IS 'Each order''s total';

CREATE FUNCTION sales.order_total(o bigint) RETURNS numeric AS
    'SELECT sum(quantity * price) FROM sales.order_lines WHERE order_id = $1'
    LANGUAGE sql STABLE;

-- ─── The data: ~10k rows ───
INSERT INTO public.countries VALUES
    ('RO', 'Romania'), ('DE', 'Germany'), ('FR', 'France'), ('US', 'United States'),
    ('JP', 'Japan'), ('BR', 'Brazil');
INSERT INTO public.customers (email, name, country, created_at, profile, vip)
    SELECT 'customer' || g || '@example.com', 'Customer ' || g,
           (ARRAY['RO', 'DE', 'FR', 'US', 'JP', NULL])[1 + g % 6],
           timestamp '2024-01-01' + g * interval '37 minutes',
           ('{"newsletter": ' || (g % 2 = 0)::text || ', "visits": ' || g % 17 || '}')::json,
           g % 10 = 0
    FROM generate_series(1, 2000) g;
INSERT INTO sales.products (sku, name, price, tags)
    SELECT 'SKU-' || lpad(g::text, 4, '0'), 'Product ' || g, (g % 50) * 3 + 0.99,
           ARRAY['tag' || g % 5]
    FROM generate_series(1, 200) g;
INSERT INTO sales.orders (customer_id, status, ordered_at, note)
    SELECT 1 + g % 2000,
           (ARRAY['new', 'paid', 'shipped', 'cancelled'])[1 + g % 4]::sales.order_status,
           timestamp '2024-02-01' + g * interval '13 minutes',
           CASE WHEN g % 7 = 0 THEN 'gift' END
    FROM generate_series(1, 4000) g;
INSERT INTO sales.order_lines
    SELECT o.id, n, 1 + (o.id * n) % 200, 1 + (o.id + n) % 5, 9.99 + n
    FROM sales.orders o, generate_series(1, 2) n;
INSERT INTO sales.shipments (order_id, line_no, shipped_on, carrier)
    SELECT order_id, line_no, date '2024-03-01' + (order_id % 90)::integer, 'Carrier ' || order_id % 3
    FROM sales.order_lines WHERE order_id % 4 = 2;

RESET ROLE;

-- ─── audit: editor_owner's tables (one editor may read, one it may not) ───
SET ROLE editor_owner;
CREATE TABLE audit.events (
    id serial PRIMARY KEY,
    at timestamptz NOT NULL DEFAULT now(),
    actor text NOT NULL,
    action text NOT NULL
);
COMMENT ON TABLE audit.events IS 'Owned by editor_owner, readable by editor';
INSERT INTO audit.events (at, actor, action)
    SELECT timestamp '2024-01-01' + g * interval '1 hour', 'user' || g % 9, 'action ' || g % 4
    FROM generate_series(1, 500) g;
GRANT SELECT ON audit.events TO editor;
CREATE TABLE audit.secrets (id integer PRIMARY KEY, token text NOT NULL);
COMMENT ON TABLE audit.secrets IS 'Owned by editor_owner, not readable by editor';
INSERT INTO audit.secrets VALUES (1, 'nope');
RESET ROLE;

-- A table owned by another role in editor's own schema (readable by editor).
SET ROLE editor_owner;
CREATE TABLE public.legacy_notes (id integer PRIMARY KEY, body text);
INSERT INTO public.legacy_notes SELECT g, 'note ' || g FROM generate_series(1, 50) g;
GRANT SELECT, INSERT, UPDATE, DELETE ON public.legacy_notes TO editor;
RESET ROLE;

-- editor_reader: reads everything editor has, writes nothing.
GRANT CONNECT ON DATABASE shop TO editor_reader;
GRANT USAGE ON SCHEMA public, sales, audit TO editor_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA public TO editor_reader;
GRANT SELECT ON ALL TABLES IN SCHEMA sales TO editor_reader;
GRANT SELECT ON audit.events TO editor_reader;
