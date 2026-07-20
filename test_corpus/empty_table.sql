CREATE TABLE empty (
    id integer,
    name varchar(255)
);

COPY empty (id, name) FROM stdin;
\.
