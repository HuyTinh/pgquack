CREATE TABLE escaped (
    id integer,
    content text
);

COPY escaped (id, content) FROM stdin;
1	Hello\tWorld
2	Line1\nLine2
3	Carriage\rReturn
4	Backslash\\Character
\.
