CREATE TABLE numeric_edges (
    small_id integer,
    big_id bigint
);

COPY numeric_edges (small_id, big_id) FROM stdin;
-2147483648	-9223372036854775808
2147483647	9223372036854775807
0	0
\.
