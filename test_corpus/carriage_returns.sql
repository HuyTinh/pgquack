CREATE TABLE crlf (
    id integer,
    val text
);

COPY crlf (id, val) FROM stdin;
1	CRLF line 1
2	CRLF line 2
\.
