CREATE TABLE octals (
    id integer,
    bytes_val text
);

COPY octals (id, bytes_val) FROM stdin;
1	Hello\x41World
2	Hex\x0d\\x0aTest
\.
