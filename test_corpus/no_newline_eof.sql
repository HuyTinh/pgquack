CREATE TABLE no_eof_newline (
    id integer,
    val text
);

COPY no_eof_newline (id, val) FROM stdin;
1	Data row
\.