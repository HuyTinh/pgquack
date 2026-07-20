CREATE TABLE malformed (
    id integer,
    name text,
    age integer
);

COPY malformed (id, name, age) FROM stdin;
1	Alice	30
2	Bob
3	Charlie	forty_two
4	David	40
\.
