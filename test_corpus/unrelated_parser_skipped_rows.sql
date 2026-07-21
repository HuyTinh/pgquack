CREATE TABLE unrelated_bad_rows (
    id integer,
    name text
);

COPY unrelated_bad_rows (id, name) FROM stdin;
1	ignored
2
\.

CREATE TABLE target_rows (
    id integer,
    name text
);

COPY target_rows (id, name) FROM stdin;
1	kept
\.
