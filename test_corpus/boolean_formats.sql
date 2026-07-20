CREATE TABLE booleans (
    id integer,
    b_val boolean
);

COPY booleans (id, b_val) FROM stdin;
1	t
2	f
3	\N
\.
