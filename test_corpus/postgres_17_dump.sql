--
-- PostgreSQL database dump
--

-- Dumped from database version 17.0
-- Dumped by pg_dump version 17.0

SET client_encoding = 'UTF8';

CREATE TABLE pg17_table (
    id bigint,
    info text
);

COPY pg17_table (id, info) FROM stdin;
1000	Postgres 17 syntax
2000	Works perfectly
\.
