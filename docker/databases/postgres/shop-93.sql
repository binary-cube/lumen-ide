-- 9.3+: a materialized view.
SET ROLE editor;
CREATE MATERIALIZED VIEW sales.monthly_sales AS
    SELECT date_trunc('month', o.ordered_at) AS month, count(*) AS orders,
           sum(l.quantity * l.price) AS total
    FROM sales.orders o JOIN sales.order_lines l ON l.order_id = o.id
    GROUP BY 1;
CREATE UNIQUE INDEX monthly_sales_month ON sales.monthly_sales (month);
COMMENT ON MATERIALIZED VIEW sales.monthly_sales IS 'Refreshed by hand';
RESET ROLE;
GRANT SELECT ON sales.monthly_sales TO editor_reader;
