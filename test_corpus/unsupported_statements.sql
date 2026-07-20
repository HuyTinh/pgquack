CREATE TABLE unsupported (
    id integer,
    note text
);

INSERT INTO unsupported (id, note) VALUES (99, 'Insert should be ignored');

COPY unsupported (id, note) FROM stdin;
1	Only copy block parsed
\.

CREATE INDEX idx_unsupported_id ON unsupported(id);
