CREATE TABLE invalid_typed_value (
    id integer,
    age integer
);

COPY invalid_typed_value (id, age) FROM stdin;
1	invalid_integer
\.
