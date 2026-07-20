-- Test: UUID column type
CREATE TABLE uuid_data (
    id uuid,
    label text
);

COPY uuid_data (id, label) FROM stdin;
550e8400-e29b-41d4-a716-446655440000	first
6ba7b810-9dad-11d1-80b4-00c04fd430c8	second
6ba7b811-9dad-11d1-80b4-00c04fd430c8	third
\.
