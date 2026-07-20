-- Test: NUMERIC / DECIMAL / FLOAT columns
CREATE TABLE numeric_data (
    id integer,
    price numeric(10,2),
    weight float8,
    ratio real
);

COPY numeric_data (id, price, weight, ratio) FROM stdin;
1	19.99	1.5	0.75
2	1099.00	72.3	0.1
3	0.01	0.001	1.0
\.
