-- 12+: a stored generated column.
SET ROLE editor;
ALTER TABLE sales.order_lines
    ADD COLUMN amount numeric(12, 2) GENERATED ALWAYS AS (quantity * price) STORED;
RESET ROLE;
