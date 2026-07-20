CREATE TABLE unicode_test (
    id integer,
    val text
);

COPY unicode_test (id, val) FROM stdin;
1	Xin chào Việt Nam
2	Cà phê sữa đá
3	Sparkles ✨ and Emoji 👍
4	日本語 (Japanese)
\.
