-- 9.4+: jsonb, with a GIN index.
SET ROLE editor;
CREATE TABLE sales.carts (
    id serial PRIMARY KEY,
    customer_id integer REFERENCES public.customers (id) ON DELETE SET NULL,
    items jsonb NOT NULL DEFAULT '[]'
);
CREATE INDEX carts_items ON sales.carts USING gin (items);
INSERT INTO sales.carts (customer_id, items)
    SELECT 1 + g % 2000, ('[{"product": ' || 1 + g % 200 || ', "quantity": ' || 1 + g % 3 || '}]')::jsonb
    FROM generate_series(1, 300) g;
RESET ROLE;
GRANT SELECT ON sales.carts TO editor_reader;
