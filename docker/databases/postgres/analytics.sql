-- The analytics database: owned by editor_owner; editor reads it (connect is PUBLIC's).
ALTER SCHEMA public OWNER TO editor_owner;
SET ROLE editor_owner;
CREATE SCHEMA reports;
CREATE TABLE public.visits (
    id serial PRIMARY KEY,
    page text NOT NULL,
    seen_at timestamp NOT NULL,
    duration_ms integer CHECK (duration_ms >= 0)
);
CREATE INDEX visits_page ON public.visits (page, seen_at);
INSERT INTO public.visits (page, seen_at, duration_ms)
    SELECT '/page/' || g % 40, timestamp '2024-01-01' + g * interval '5 minutes', g % 9000
    FROM generate_series(1, 3000) g;
CREATE VIEW reports.daily AS
    SELECT date_trunc('day', seen_at) AS day, count(*) AS visits FROM public.visits GROUP BY 1;
GRANT USAGE ON SCHEMA public, reports TO editor, editor_reader;
GRANT SELECT ON public.visits, reports.daily TO editor, editor_reader;
RESET ROLE;
