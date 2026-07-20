import os

corpus_dir = os.path.dirname(os.path.abspath(__file__))

# 1. simple_users.sql
simple_users = """-- PostgreSQL database dump
CREATE TABLE users (
    id integer,
    name text,
    is_active boolean,
    created_at timestamp
);

COPY users (id, name, is_active, created_at) FROM stdin;
1	John Doe	t	2026-07-20 12:00:00
2	Jane Smith	f	2026-07-20 12:30:00
3	Bob Johnson	t	2026-07-20 13:00:00
\\.
"""

# 2. escaped_strings.sql
escaped_strings = """CREATE TABLE escaped (
    id integer,
    content text
);

COPY escaped (id, content) FROM stdin;
1	Hello\\tWorld
2	Line1\\nLine2
3	Carriage\\rReturn
4	Backslash\\\\Character
\\.
"""

# 3. unicode_data.sql
unicode_data = """CREATE TABLE unicode_test (
    id integer,
    val text
);

COPY unicode_test (id, val) FROM stdin;
1	Xin chào Việt Nam
2	Cà phê sữa đá
3	Sparkles ✨ and Emoji 👍
4	日本語 (Japanese)
\\.
"""

# 4. null_representations.sql
null_representations = """CREATE TABLE nulls (
    id integer,
    val text,
    num integer
);

COPY nulls (id, val, num) FROM stdin;
1	hello	42
2	\\N	100
3	empty	\\N
4		0
\\.
"""

# 5. malformed_lines.sql
malformed_lines = """CREATE TABLE malformed (
    id integer,
    name text,
    age integer
);

COPY malformed (id, name, age) FROM stdin;
1	Alice	30
2	Bob
3	Charlie	forty_two
4	David	40
\\.
"""

# 6. empty_table.sql
empty_table = """CREATE TABLE empty (
    id integer,
    name varchar(255)
);

COPY empty (id, name) FROM stdin;
\\.
"""

# 7. complex_types.sql
complex_types = """CREATE TABLE complex (
    id bigint,
    description varchar(100),
    updated_at timestamp
);

COPY complex (id, description, updated_at) FROM stdin;
9223372036854775807	Short text	2026-07-20 23:59:59
-9223372036854775808	Another description	1970-01-01 00:00:00
\\.
"""

# 8. multiple_tables.sql
multiple_tables = """CREATE TABLE orders (
    order_id integer,
    customer_id integer,
    amount bigint
);

COPY orders (order_id, customer_id, amount) FROM stdin;
101	1	50000
102	2	120000
\\.

CREATE TABLE items (
    item_id integer,
    name text
);

COPY items (item_id, name) FROM stdin;
1	Laptop
2	Mouse
\\.
"""

# 9. postgres_12_dump.sql
postgres_12_dump = """--
-- PostgreSQL database dump
--

SET statement_timeout = 0;
SET lock_timeout = 0;

CREATE TABLE pg12_table (
    id integer NOT NULL,
    val text
);

ALTER TABLE ONLY pg12_table ADD CONSTRAINT pg12_table_pkey PRIMARY KEY (id);

COPY pg12_table (id, val) FROM stdin;
1	Postgres 12 data
2	More data
\\.
"""

# 10. postgres_17_dump.sql
postgres_17_dump = """--
-- PostgreSQL database dump
--

-- Dumped from database version 17.0
-- Dumped by pg_dump version 17.0

SET client_encoding = 'UTF8';

CREATE TABLE pg17_table (
    id bigint,
    info text
);

COPY pg17_table (id, info) FROM stdin;
1000	Postgres 17 syntax
2000	Works perfectly
\\.
"""

# 11. no_newline_eof.sql (No trailing newline after \.)
no_newline_eof = """CREATE TABLE no_eof_newline (
    id integer,
    val text
);

COPY no_eof_newline (id, val) FROM stdin;
1	Data row
\\."""

# 12. comments_and_set.sql
comments_and_set = """-- This is a comment
SET client_min_messages = warning;
SELECT pg_catalog.set_config('search_path', '', false);

CREATE TABLE comments_test (
    id integer,
    comment_val text
); -- Inline comment

-- Another comment
COPY comments_test (id, comment_val) FROM stdin;
1	Line with comments before
\\.
"""

# 13. quoted_identifiers.sql
quoted_identifiers = """CREATE TABLE "Special Table" (
    "id" integer,
    "My Column" text,
    "is_active" boolean
);

COPY "Special Table" ("id", "My Column", "is_active") FROM stdin;
1	Quoted Identifier test	t
\\.
"""

# 14. unsupported_statements.sql
unsupported_statements = """CREATE TABLE unsupported (
    id integer,
    note text
);

INSERT INTO unsupported (id, note) VALUES (99, 'Insert should be ignored');

COPY unsupported (id, note) FROM stdin;
1	Only copy block parsed
\\.

CREATE INDEX idx_unsupported_id ON unsupported(id);
"""

# 15. numeric_edge_cases.sql
numeric_edge_cases = """CREATE TABLE numeric_edges (
    small_id integer,
    big_id bigint
);

COPY numeric_edges (small_id, big_id) FROM stdin;
-2147483648	-9223372036854775808
2147483647	9223372036854775807
0	0
\\.
"""

# 16. boolean_formats.sql
boolean_formats = """CREATE TABLE booleans (
    id integer,
    b_val boolean
);

COPY booleans (id, b_val) FROM stdin;
1	t
2	f
3	\\N
\\.
"""

# 17. escaped_octals.sql
escaped_octals = """CREATE TABLE octals (
    id integer,
    bytes_val text
);

COPY octals (id, bytes_val) FROM stdin;
1	Hello\\x41World
2	Hex\\x0d\\\\x0aTest
\\.
"""

# 18. carriage_returns.sql (CRLF lines)
carriage_returns = "CREATE TABLE crlf (\r\n    id integer,\r\n    val text\r\n);\r\n\r\nCOPY crlf (id, val) FROM stdin;\r\n1\tCRLF line 1\r\n2\tCRLF line 2\r\n\\.\r\n"

# 19. special_table_names.sql
special_table_names = """CREATE TABLE "select" (
    id integer,
    "order" text
);

COPY "select" (id, "order") FROM stdin;
1	Keyword as identifier
\\.
"""

# 20. mixed_case_types.sql
mixed_case_types = """CREATE TABLE mixed_case (
    id InTeGeR,
    val TeXt,
    flag BoOlEaN
);

COPY mixed_case (id, val, flag) FROM stdin;
1	Upper/lower case types	t
\\.
"""

files = {
    "simple_users.sql": simple_users,
    "escaped_strings.sql": escaped_strings,
    "unicode_data.sql": unicode_data,
    "null_representations.sql": null_representations,
    "malformed_lines.sql": malformed_lines,
    "empty_table.sql": empty_table,
    "complex_types.sql": complex_types,
    "multiple_tables.sql": multiple_tables,
    "postgres_12_dump.sql": postgres_12_dump,
    "postgres_17_dump.sql": postgres_17_dump,
    "no_newline_eof.sql": no_newline_eof,
    "comments_and_set.sql": comments_and_set,
    "quoted_identifiers.sql": quoted_identifiers,
    "unsupported_statements.sql": unsupported_statements,
    "numeric_edge_cases.sql": numeric_edge_cases,
    "boolean_formats.sql": boolean_formats,
    "escaped_octals.sql": escaped_octals,
    "carriage_returns.sql": carriage_returns,
    "special_table_names.sql": special_table_names,
    "mixed_case_types.sql": mixed_case_types,
}

for filename, content in files.items():
    filepath = os.path.join(corpus_dir, filename)
    with open(filepath, "w", encoding="utf-8", newline="") as f:
        f.write(content)

print(f"Generated {len(files)} corpus dump files successfully in {corpus_dir}")
