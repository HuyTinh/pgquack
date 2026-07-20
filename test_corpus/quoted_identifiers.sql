CREATE TABLE "Special Table" (
    "id" integer,
    "My Column" text,
    "is_active" boolean
);

COPY "Special Table" ("id", "My Column", "is_active") FROM stdin;
1	Quoted Identifier test	t
\.
