CREATE TABLE nulls (
    id integer,
    val text,
    num integer
);

COPY nulls (id, val, num) FROM stdin;
1	hello	42
2	\N	100
3	empty	\N
4		0
\.
