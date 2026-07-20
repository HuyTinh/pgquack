-- This is a comment
SET client_min_messages = warning;
SELECT pg_catalog.set_config('search_path', '', false);

CREATE TABLE comments_test (
    id integer,
    comment_val text
); -- Inline comment

-- Another comment
COPY comments_test (id, comment_val) FROM stdin;
1	Line with comments before
\.
