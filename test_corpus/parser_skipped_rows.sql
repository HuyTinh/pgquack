CREATE TABLE parser_skipped_rows (
    id integer,
    name text
);

COPY parser_skipped_rows (id, name) FROM stdin;
1	Alice
2
3	Charlie
\.
