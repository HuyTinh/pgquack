CREATE TABLE reordered_copy (
    id integer,
    name text,
    is_active boolean
);

COPY reordered_copy (name, is_active, id) FROM stdin;
Ada Lovelace	t	1
Grace Hopper	f	2
\.
