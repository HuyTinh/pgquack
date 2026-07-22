# PgQuack 🦆

> **Direct Query Engine for PostgreSQL SQL Dumps** (Zero-Restore, Embedded DuckDB-powered)

PgQuack is a high-performance CLI tool that allows you to execute analytical SQL queries directly on PostgreSQL SQL dump files (`.sql`) **without restoring them** into a running PostgreSQL database server. It parses the SQL dump on the fly, stream-loads it into an in-memory, embedded DuckDB instance, and executes your queries instantly.

---

## 🚀 Key Features

*   **Zero-Restore Querying**: No need to restore into a PostgreSQL server, spin up Docker containers, or provision database storage.
*   **Compressed Dump Support**: Reads `.sql`, `.sql.gz`, and `.sql.zst` transparently — auto-detected by extension or magic bytes.
*   **pg_dump TAR Archive Support**: Reads `pg_dump -Ft` archives (`.tar` and `.tar.gz`) through the PostgreSQL `pg_restore` client without restoring to a server.
*   **Parquet Cache**: First query (cold) parses the dump and writes a `.pgquack_cache/` Parquet cache. Subsequent queries (warm) skip parsing entirely and read from Parquet for near-instant startup.
*   **OLAP Speed**: Powered by **DuckDB**, yielding blazing-fast analytical queries (`GROUP BY`, `JOIN`, `COUNT`, `SUM`, window functions).
*   **Extended Type Support**: `INT`, `BIGINT`, `BOOLEAN`, `FLOAT`/`DOUBLE`, `NUMERIC`/`DECIMAL`, `TIMESTAMP`, `DATE`, `TEXT`/`VARCHAR`, `JSON`/`JSONB`, `UUID`, `TEXT[]` arrays.
*   **DuckDB Loadable Extension**: Build a native extension that exposes `pgquack_read(dump_path, table_name)` directly to DuckDB.
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

# PostgreSQL archive created with pg_dump -Ft (requires pg_restore on PATH).
# pg_restore emits schema-qualified names, so quote the imported DuckDB table name.
pgquack backup.tar 'SELECT count(*) FROM "public.users"'

# Gzip-wrapped PostgreSQL tar archive (also requires pg_restore on PATH)
pgquack backup.tar.gz 'SELECT count(*) FROM "public.users"'

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

#### PostgreSQL TAR Archive Prerequisite

`pg_dump -Ft` produces a PostgreSQL archive rather than a plain SQL file.
PgQuack invokes the matching `pg_restore` client locally to emit SQL into its
streaming parser; it does **not** connect to or restore into a PostgreSQL
server. Install PostgreSQL client tools and ensure `pg_restore` is available on
`PATH` before querying `.tar` or `.tar.gz` archives:

```bash
pg_restore --version
pgquack backup.tar 'SELECT count(*) FROM "public.users"'
```

`pg_restore` emits a schema-qualified identifier such as `public.users`.
PgQuack preserves that identifier as one DuckDB table name, so queries must
quote it as `"public.users"`.

Set `PGQUACK_PG_RESTORE` to an absolute executable path when `pg_restore` is
installed outside `PATH`.

If `pg_restore` is missing or cannot read the archive, PgQuack returns its
stderr and exit status. For a gzip-wrapped archive, PgQuack decompresses to an
auto-removing local temporary TAR file before invoking `pg_restore`.

### Logging & Error Handling

To see diagnostic logs or parser warning messages, set the `RUST_LOG` environment variable:

```bash
# Unix
RUST_LOG=info pgquack backup.sql "SELECT count(*) FROM tables"

# Windows PowerShell
$env:RUST_LOG="info"
cargo run -- backup.sql "SELECT count(*) FROM tables"
```

### Use from the DuckDB CLI

PgQuack can also be built as a local DuckDB C API extension. It registers the
table function below, which streams one table from a PostgreSQL dump directly
into the DuckDB query:

```sql
SELECT * FROM pgquack_read('backup.sql.gz', 'users');
```

Build the loadable shared library without the CLI binary:

```bash
cargo build --release --features loadable-ext --lib
```

Package the shared library with DuckDB metadata. The following Linux example
targets DuckDB CLI v1.2.2; use the platform string appropriate for the artifact
you built (for example, `windows_amd64` for a Windows `.dll`).

```bash
python3 scripts/package_loadable_extension.py \
  --source target/release/libpgquack.so \
  --output target/release/pgquack.duckdb_extension \
  --platform linux_amd64 \
  --duckdb-version v1.2.0 \
  --extension-version v0.3.0
```

The generated artifact is unsigned because it is a local build, not a DuckDB
extension repository release. Load only an artifact you built or otherwise
trust, and pass `-unsigned` only for that local DuckDB CLI session:

```bash
duckdb -unsigned
```

```sql
LOAD 'target/release/pgquack.duckdb_extension';
SELECT count(*) FROM pgquack_read('test_corpus/simple_users.sql', 'users');
-- 3
```

`INSTALL pgquack` is not available yet: that requires publishing a signed,
repository-distributed extension artifact.

---

## 🗺️ Roadmap

- [x] **Phase 0 & 1**: Plain SQL parser, `CREATE TABLE` and `COPY FROM stdin` extractor, embedded DuckDB integration, type mapper (Int, BigInt, Bool, Timestamp, Text), strict error reporter, 20+ test corpus suite.
- [x] **Phase 2 (Current)**: Streaming decompression (`.sql.gz` / `.sql.zst`), automatic Parquet cache manager (cold/warm queries), extended type support (`UUID`, `JSONB`, `DECIMAL`, `FLOAT`, `DATE`, `1D Arrays`), `--no-cache` flag.
- [x] **Phase 3**: Build and package the core engine as a local native DuckDB Extension, exposing `pgquack_read(dump_path, table_name)`.
- [ ] **Phase 3 follow-up**: Publish a signed DuckDB extension artifact to support `INSTALL pgquack; LOAD pgquack;`.
- [ ] **Phase 4**: Parallel parser, Custom format (`-Fc`/`-Fd`) dump support, interactive CLI with syntax highlight.

---

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
