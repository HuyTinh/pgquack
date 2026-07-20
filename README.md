# PgQuack 🦆

> **Direct Query Engine for PostgreSQL SQL Dumps** (Zero-Restore, Embedded DuckDB-powered)

PgQuack is a high-performance CLI tool that allows you to execute analytical SQL queries directly on PostgreSQL SQL dump files (`.sql`) **without restoring them** into a running PostgreSQL database server. It parses the SQL dump on the fly, stream-loads it into an in-memory, embedded DuckDB instance, and executes your queries instantly.

---

## 🚀 Key Features

*   **Zero-Restore Querying**: No need to spend hours running `pg_restore`, spinning up Docker containers, or provisioning database storage.
*   **Compressed Dump Support**: Reads `.sql`, `.sql.gz`, and `.sql.zst` transparently — auto-detected by extension or magic bytes.
*   **Parquet Cache**: First query (cold) parses the dump and writes a `.pgquack_cache/` Parquet cache. Subsequent queries (warm) skip parsing entirely and read from Parquet for near-instant startup.
*   **OLAP Speed**: Powered by **DuckDB**, yielding blazing-fast analytical queries (`GROUP BY`, `JOIN`, `COUNT`, `SUM`, window functions).
*   **Extended Type Support**: `INT`, `BIGINT`, `BOOLEAN`, `FLOAT`/`DOUBLE`, `NUMERIC`/`DECIMAL`, `TIMESTAMP`, `DATE`, `TEXT`/`VARCHAR`, `JSON`/`JSONB`, `UUID`, `TEXT[]` arrays.
*   **Memory Safe & Efficient**: Streaming state-machine parser — RAM stays under 2GB on massive dumps.
*   **Strict Error Reporting**: Malformed rows are logged to `stderr` and skipped without crashing. Exit code `2` if any rows were skipped.

---

## 🛠️ How It Works (Architecture)

```text
[ PostgreSQL Dump (.sql / .sql.gz / .sql.zst) ]
           │
           ▼
[ DumpReader — auto-detect compression ]
           │
           ├──► Cache Hit? ──► Load from Parquet Cache ──► DuckDB ──► Query
           │
           ▼ (Cache Miss)
[ Streaming State-Machine Parser ] ──► (Malformed Row) ──► Log to stderr & Skip
           │
           ▼ (Valid COPY row)
[ Type Normalizer & Mapper ]
           │ (Postgres → DuckDB types, incl. JSON/UUID/NUMERIC/Date/Array)
           ▼
[ DuckDB Appender Session ]
           │
           ├──► Write Parquet Cache (for next warm query)
           ▼
[ Embedded DuckDB Connection ]
           │
           ▼
[ Run Analytical SQL Queries & Print Output ]
```

---

## 📖 Usage

### Query a Dump File

```bash
pgquack <DUMP_FILE> <SQL_QUERY> [--no-cache]
```

#### Examples

```bash
# Plain SQL dump
pgquack backup.sql "SELECT count(*) FROM users"

# Gzip-compressed dump
pgquack backup.sql.gz "SELECT is_active, count(*) FROM users GROUP BY is_active"

# Zstd-compressed dump
pgquack backup.sql.zst "SELECT sum(amount) FROM orders"

# Bypass cache (always re-parse)
pgquack backup.sql.gz "SELECT * FROM events LIMIT 10" --no-cache
```

#### CLI Output Example

```text
+-----------+----------+---------------------+
| is_active | count(*) | max(created_at)     |
+-----------+----------+---------------------+
| true      | 2        | 2026-07-20 13:00:00 |
| false     | 1        | 2026-07-20 12:30:00 |
+-----------+----------+---------------------+
[INFO] Selected 2 rows
```

### Logging & Error Handling

To see diagnostic logs or parser warning messages, set the `RUST_LOG` environment variable:

```bash
# Unix
RUST_LOG=info pgquack backup.sql "SELECT count(*) FROM tables"

# Windows PowerShell
$env:RUST_LOG="info"
cargo run -- backup.sql "SELECT count(*) FROM tables"
```

---

## 🗺️ Roadmap

- [x] **Phase 0 & 1**: Plain SQL parser, `CREATE TABLE` and `COPY FROM stdin` extractor, embedded DuckDB integration, type mapper (Int, BigInt, Bool, Timestamp, Text), strict error reporter, 20+ test corpus suite.
- [x] **Phase 2 (Current)**: Streaming decompression (`.sql.gz` / `.sql.zst`), automatic Parquet cache manager (cold/warm queries), extended type support (`UUID`, `JSONB`, `DECIMAL`, `FLOAT`, `DATE`, `1D Arrays`), `--no-cache` flag.
- [ ] **Phase 3**: Package the core engine as a native DuckDB Extension (`INSTALL pgquack; LOAD pgquack;`).
- [ ] **Phase 4**: Parallel parser, Custom format (`-Fc`/`-Fd`) dump support, interactive CLI with syntax highlight.

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
