-- Test: DATE column type and 1D integer array
CREATE TABLE date_and_array (
    id integer,
    event_date date,
    tags text[]
);

COPY date_and_array (id, event_date, tags) FROM stdin;
1	2024-01-15	{rust,duckdb,olap}
2	2026-07-20	{postgres,backup}
3	2000-02-29	{leapday}
\.
