-- Test: JSON and JSONB column types
CREATE TABLE json_data (
    id integer,
    metadata json,
    settings jsonb
);

COPY json_data (id, metadata, settings) FROM stdin;
1	{"name":"Alice","age":30}	{"theme":"dark","lang":"vi"}
2	{"name":"Bob","score":99.5}	{"theme":"light","lang":"en"}
3	{"items":[1,2,3]}	{"enabled":true}
\.
