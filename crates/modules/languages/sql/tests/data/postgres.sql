-- Reports, in PostgreSQL.
WITH recent AS (
    SELECT user_id, sum(total) AS spent
    FROM orders
    WHERE created_at > now() - interval '30 days'
    GROUP BY user_id
)
SELECT u.id, r.spent::numeric(10, 2),
       CASE WHEN r.spent > 1000 THEN 'gold' ELSE 'regular' END AS tier
FROM users u
LEFT JOIN recent r ON r.user_id = u.id
WHERE u.email = $1;

INSERT INTO users (email) VALUES ('ana@example.com')
ON CONFLICT (email) DO UPDATE SET email = excluded.email
RETURNING id;

CREATE OR REPLACE FUNCTION bump(p_id integer) RETURNS void
LANGUAGE plpgsql AS $$
BEGIN
    UPDATE users SET score = score + 1 WHERE id = p_id;
END;
$$;
