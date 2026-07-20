use pgquack::engine::Engine;
use pgquack::parser::{Parser, ParserEvent};
use pgquack::reader::DumpReader;
use std::collections::HashMap;

fn load_dump_file(path: &str) -> (Engine, usize) {
    let reader = DumpReader::open(path)
        .unwrap_or_else(|e| panic!("Failed to open test file {}: {}", path, e));

    let mut parser = Parser::new(reader);
    let engine = Engine::new().expect("Failed to initialize DuckDB");

    // Use an inner scope so that AppenderSession (which borrows engine) is
    // fully dropped before we move `engine` in the return value.
    {
        let mut table_schemas = HashMap::new();
        let mut current_appender: Option<pgquack::engine::AppenderSession<'_>> = None;

        while let Some(event_res) = parser.next_event() {
            let event = event_res.expect("Parser error");
            match event {
                ParserEvent::TableCreated(schema) => {
                    engine
                        .create_table(&schema)
                        .expect("Failed to create table");
                    table_schemas.insert(schema.name.clone(), schema);
                }
                ParserEvent::CopyStart { table_name, .. } => {
                    // Drop previous appender before creating the next one.
                    let _ = current_appender.take();
                    let schema = table_schemas.get(&table_name).expect("Schema not found");
                    let appender = engine
                        .appender(&table_name, schema.clone())
                        .expect("Failed to start Appender");
                    current_appender = Some(appender);
                }
                ParserEvent::CopyRow { values, .. } => {
                    if let Some(ref mut session) = current_appender {
                        if let Err(err) = session.append_row(&values) {
                            eprintln!("Row append failed: {}", err);
                            parser.skipped_lines_count += 1;
                        }
                    }
                }
                ParserEvent::CopyEnd { .. } => {
                    current_appender = None;
                }
            }
        }
        // current_appender dropped here, releasing borrow of engine
    }

    (engine, parser.skipped_lines_count)
}

#[test]
fn test_simple_users() {
    let (engine, skipped) = load_dump_file("test_corpus/simple_users.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT count(*) FROM users").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(count, 3);

    let mut stmt = conn
        .prepare("SELECT name FROM users WHERE is_active = true ORDER BY id")
        .unwrap();
    let names: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(names, vec!["John Doe", "Bob Johnson"]);
}

#[test]
fn test_escaped_strings() {
    let (engine, skipped) = load_dump_file("test_corpus/escaped_strings.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn
        .prepare("SELECT content FROM escaped ORDER BY id")
        .unwrap();
    let contents: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(contents[0], "Hello\tWorld");
    assert_eq!(contents[1], "Line1\nLine2");
    assert_eq!(contents[2], "Carriage\rReturn");
    assert_eq!(contents[3], "Backslash\\Character");
}

#[test]
fn test_unicode_data() {
    let (engine, skipped) = load_dump_file("test_corpus/unicode_data.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn
        .prepare("SELECT val FROM unicode_test ORDER BY id")
        .unwrap();
    let vals: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(vals[0], "Xin chào Việt Nam");
    assert_eq!(vals[1], "Cà phê sữa đá");
    assert_eq!(vals[2], "Sparkles ✨ and Emoji 👍");
    assert_eq!(vals[3], "日本語 (Japanese)");
}

#[test]
fn test_null_representations() {
    let (engine, skipped) = load_dump_file("test_corpus/null_representations.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn
        .prepare("SELECT val, num FROM nulls ORDER BY id")
        .unwrap();
    let rows: Vec<(Option<String>, Option<i32>)> = stmt
        .query_map([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    assert_eq!(rows[0], (Some("hello".to_string()), Some(42)));
    assert_eq!(rows[1], (None, Some(100)));
    assert_eq!(rows[2], (Some("empty".to_string()), None));
    assert_eq!(rows[3], (Some("".to_string()), Some(0)));
}

#[test]
fn test_malformed_lines() {
    let (engine, skipped) = load_dump_file("test_corpus/malformed_lines.sql");
    // Bob has missing age field (only 2 cols), Charlie has age 'forty_two' (type parse error)
    assert_eq!(skipped, 2);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT count(*) FROM malformed").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    // Only Alice and David should be inserted successfully
    assert_eq!(count, 2);
}

#[test]
fn test_empty_table() {
    let (engine, skipped) = load_dump_file("test_corpus/empty_table.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT count(*) FROM empty").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(count, 0);
}

#[test]
fn test_complex_types() {
    let (engine, skipped) = load_dump_file("test_corpus/complex_types.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn
        .prepare("SELECT id, description FROM complex ORDER BY id DESC")
        .unwrap();
    let (id, desc): (i64, String) = stmt
        .query_row([], |r| Ok((r.get(0).unwrap(), r.get(1).unwrap())))
        .unwrap();
    assert_eq!(id, 9223372036854775807i64);
    assert_eq!(desc, "Short text");
}

#[test]
fn test_multiple_tables() {
    let (engine, skipped) = load_dump_file("test_corpus/multiple_tables.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT sum(amount) FROM orders").unwrap();
    let sum: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(sum, 170000);

    let mut stmt = conn
        .prepare("SELECT name FROM items ORDER BY item_id")
        .unwrap();
    let items: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(items, vec!["Laptop", "Mouse"]);
}

#[test]
fn test_postgres_12_and_17_dumps() {
    let (engine12, skipped12) = load_dump_file("test_corpus/postgres_12_dump.sql");
    assert_eq!(skipped12, 0);
    let mut stmt = engine12
        .connection()
        .prepare("SELECT val FROM pg12_table ORDER BY id")
        .unwrap();
    let vals12: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(vals12, vec!["Postgres 12 data", "More data"]);

    let (engine17, skipped17) = load_dump_file("test_corpus/postgres_17_dump.sql");
    assert_eq!(skipped17, 0);
    let mut stmt = engine17
        .connection()
        .prepare("SELECT info FROM pg17_table ORDER BY id")
        .unwrap();
    let vals17: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(vals17, vec!["Postgres 17 syntax", "Works perfectly"]);
}

#[test]
fn test_no_newline_eof() {
    let (engine, skipped) = load_dump_file("test_corpus/no_newline_eof.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT val FROM no_eof_newline").unwrap();
    let val: String = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(val, "Data row");
}

#[test]
fn test_quoted_identifiers() {
    let (engine, skipped) = load_dump_file("test_corpus/quoted_identifiers.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn
        .prepare("SELECT \"My Column\" FROM \"Special Table\"")
        .unwrap();
    let val: String = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(val, "Quoted Identifier test");
}

#[test]
fn test_carriage_returns() {
    let (engine, skipped) = load_dump_file("test_corpus/carriage_returns.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT val FROM crlf ORDER BY id").unwrap();
    let vals: Vec<String> = stmt
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(vals, vec!["CRLF line 1", "CRLF line 2"]);
}

#[test]
fn test_special_table_names() {
    let (engine, skipped) = load_dump_file("test_corpus/special_table_names.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT \"order\" FROM \"select\"").unwrap();
    let val: String = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(val, "Keyword as identifier");
}

// ─── Phase 2: Compression tests ──────────────────────────────────────────────

#[test]
fn test_gzip_simple_users() {
    // Reads the gzip-compressed version of simple_users.sql
    let (engine, skipped) = load_dump_file("test_corpus/simple_users.sql.gz");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT count(*) FROM users").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(count, 3);
}

#[test]
fn test_gzip_unicode() {
    let (engine, skipped) = load_dump_file("test_corpus/unicode_data.sql.gz");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT count(*) FROM unicode_test").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(count, 4);
}

#[test]
fn test_zstd_multiple_tables() {
    let (engine, skipped) = load_dump_file("test_corpus/multiple_tables.sql.zst");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT sum(amount) FROM orders").unwrap();
    let sum: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(sum, 170000);
}

#[test]
fn test_zstd_null_representations() {
    let (engine, skipped) = load_dump_file("test_corpus/null_representations.sql.zst");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT count(*) FROM nulls").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(count, 4);
}

// ─── Phase 2: Extended type mapper tests ─────────────────────────────────────

#[test]
fn test_json_types() {
    let (engine, skipped) = load_dump_file("test_corpus/json_types.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT count(*) FROM json_data").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(count, 3);

    // metadata is stored as VARCHAR — check one value is non-empty
    let mut stmt = conn
        .prepare("SELECT metadata FROM json_data WHERE id = 1")
        .unwrap();
    let meta: String = stmt.query_row([], |r| r.get(0)).unwrap();
    assert!(meta.contains("Alice"));
}

#[test]
fn test_uuid_types() {
    let (engine, skipped) = load_dump_file("test_corpus/uuid_types.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn
        .prepare("SELECT id FROM uuid_data WHERE label = 'first'")
        .unwrap();
    let uuid: String = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(uuid, "550e8400-e29b-41d4-a716-446655440000");

    let mut stmt = conn.prepare("SELECT count(*) FROM uuid_data").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(count, 3);
}

#[test]
fn test_float_and_numeric_types() {
    let (engine, skipped) = load_dump_file("test_corpus/float_types.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    // price is stored as DOUBLE
    let mut stmt = conn
        .prepare("SELECT price FROM numeric_data WHERE id = 1")
        .unwrap();
    let price: f64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert!((price - 19.99).abs() < 0.001);

    let mut stmt = conn
        .prepare("SELECT sum(weight) FROM numeric_data")
        .unwrap();
    let total: f64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert!((total - 73.801).abs() < 0.01);
}

#[test]
fn test_date_and_array_types() {
    let (engine, skipped) = load_dump_file("test_corpus/date_array_types.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn
        .prepare("SELECT count(*) FROM date_and_array WHERE event_date < '2025-01-01'")
        .unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    // 2024-01-15 and 2000-02-29 are before 2025
    assert_eq!(count, 2);

    // tags is stored as JSON-encoded VARCHAR
    let mut stmt = conn
        .prepare("SELECT tags FROM date_and_array WHERE id = 1")
        .unwrap();
    let tags: String = stmt.query_row([], |r| r.get(0)).unwrap();
    assert!(tags.contains("rust"));
}

// ─── Phase 2: Cache roundtrip test ───────────────────────────────────────────

#[test]
fn test_parquet_cache_roundtrip() {
    use pgquack::cache::CacheManager;
    use std::path::Path;

    let dump_path = "test_corpus/simple_users.sql";

    // Cold parse — populate engine from dump
    let (engine_cold, skipped) = load_dump_file(dump_path);
    assert_eq!(skipped, 0);

    // Save to Parquet cache
    let cache = CacheManager::new(dump_path);
    cache
        .save_table(&engine_cold, "users")
        .expect("save_table failed");
    cache.write_meta(dump_path).expect("write_meta failed");

    // Validate cache is now marked valid
    assert!(
        cache.is_valid(dump_path),
        "Cache should be valid after write"
    );

    // Warm load — fresh engine, load from Parquet
    let engine_warm = Engine::new().expect("DuckDB init failed");
    let loaded = cache
        .load_all_into_duckdb(&engine_warm)
        .expect("load_all failed");
    assert!(loaded.contains(&"users".to_string()));

    // Verify data matches
    let conn = engine_warm.connection();
    let mut stmt = conn.prepare("SELECT count(*) FROM users").unwrap();
    let count: i64 = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(count, 3);

    // Cleanup cache directory so it doesn't interfere with other test runs
    cache.invalidate();
    assert!(
        !cache.is_valid(dump_path),
        "Cache should be invalid after invalidation"
    );

    // Suppress unused warning for Path
    let _ = Path::new(dump_path);
}

#[test]
fn test_mixed_case_types() {
    let (engine, skipped) = load_dump_file("test_corpus/mixed_case_types.sql");
    assert_eq!(skipped, 0);

    let conn = engine.connection();
    let mut stmt = conn.prepare("SELECT flag FROM mixed_case").unwrap();
    let flag: bool = stmt.query_row([], |r| r.get(0)).unwrap();
    assert_eq!(flag, true);
}
