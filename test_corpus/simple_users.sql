-- PostgreSQL database dump
CREATE TABLE users (
    id integer,
    name text,
    is_active boolean,
    created_at timestamp
);

COPY users (id, name, is_active, created_at) FROM stdin;
1	John Doe	t	2026-07-20 12:00:00
2	Jane Smith	f	2026-07-20 12:30:00
3	Bob Johnson	t	2026-07-20 13:00:00
\.
