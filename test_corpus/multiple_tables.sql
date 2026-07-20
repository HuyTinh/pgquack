CREATE TABLE orders (
    order_id integer,
    customer_id integer,
    amount bigint
);

COPY orders (order_id, customer_id, amount) FROM stdin;
101	1	50000
102	2	120000
\.

CREATE TABLE items (
    item_id integer,
    name text
);

COPY items (item_id, name) FROM stdin;
1	Laptop
2	Mouse
\.
