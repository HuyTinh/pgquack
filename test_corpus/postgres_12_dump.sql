--
-- PostgreSQL database dump
--

SET statement_timeout = 0;
SET lock_timeout = 0;

CREATE TABLE pg12_table (
    id integer NOT NULL,
    val text
);

ALTER TABLE ONLY pg12_table ADD CONSTRAINT pg12_table_pkey PRIMARY KEY (id);

COPY pg12_table (id, val) FROM stdin;
1	Postgres 12 data
2	More data
\.
