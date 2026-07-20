# PgQuack 🦆

> **Direct Query Engine for PostgreSQL SQL Dumps** (Zero-Restore, Embedded DuckDB-powered)

PgQuack is a high-performance CLI tool that allows you to execute analytical SQL queries directly on PostgreSQL SQL dump files (`.sql`) **without restoring them** into a running PostgreSQL database server. It parses the SQL dump on the fly, stream-loads it into an in-memory, embedded DuckDB instance, and executes your queries instantly.

---

## 🚀 Key Features

*   **Zero-Restore Querying**: No need to spend hours running `pg_restore`, spinning up Docker containers, or provisioning database storage.
*   **OLAP Speed**: Powered by **DuckDB**, yielding blazing-fast analytical queries (`GROUP BY`, `JOIN`, `COUNT`, `SUM`, window functions).
*   **Memory Safe & Efficient**: Processes dumps line-by-line using a streaming state-machine parser. RAM usage stays under 2GB even on massive dumps.
*   **Strict Error Reporting**: Malformed rows are logged to `stderr` and skipped without crashing the engine. If any rows are skipped, the command exits with code `2` to prevent silent data degradation.
*   **Local-First & Embedded**: Run directly in your terminal, build pipelines, or CI/CD scripts.

---

## 🛠️ How It Works (Architecture)

```text
[ PostgreSQL Dump (.sql) ]
           │
           ▼
[ Streaming State-Machine Parser ] ──► (Malformed Row) ──► Log to stderr & Skip
           │
           ▼ (Valid COPY row)
[ Type Normalizer & Mapper ]
           │ (Maps Postgres types to DuckDB types)
           ▼
[ DuckDB Appender Session ]
           │ (Bulk loads data in batches)
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
pgquack <DUMP_FILE> <SQL_QUERY>
```

#### Example

```bash
pgquack test_corpus/simple_users.sql "SELECT is_active, count(*), max(created_at) FROM users GROUP BY is_active"
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

- [x] **Phase 0 & 1 (Current)**: Plain SQL parser, `CREATE TABLE` and `COPY FROM stdin` extractor, embedded DuckDB integration, type mapper (Int, BigInt, Bool, Timestamp, Text), strict error reporter, and 20+ test corpus suite.
- [ ] **Phase 2 (Next)**: Support streaming decompression (`.sql.gz` and `.sql.zst`), add a automatic Parquet cache manager, and expand type support (UUID, JSONB, Decimal, 1D Arrays).
- [ ] **Phase 3**: Package the core engine as a native DuckDB Extension.
- [ ] **Phase 4**: Add parallel parser and Custom format (`-Fc`/`-Fd`) dump support.

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
