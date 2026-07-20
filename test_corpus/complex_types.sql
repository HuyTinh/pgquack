CREATE TABLE complex (
    id bigint,
    description varchar(100),
    updated_at timestamp
);

COPY complex (id, description, updated_at) FROM stdin;
9223372036854775807	Short text	2026-07-20 23:59:59
-9223372036854775808	Another description	1970-01-01 00:00:00
\.
