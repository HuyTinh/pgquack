//! DuckDB extension entrypoint and `pgquack_read` table function.
//!
//! Registers a DuckDB table function:
//! ```sql
//! SELECT * FROM pgquack_read('dump.sql.gz', 'table_name');
//! ```
//!
//! The extension reads a PostgreSQL dump file (plain/gzip/zstd) and streams
//! the rows of the target table directly into DuckDB without writing any
//! temporary files.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

use duckdb::core::{DataChunkHandle, Inserter, LogicalTypeHandle, LogicalTypeId};
use duckdb::vtab::{BindInfo, InitInfo, TableFunctionInfo, VTab};

// Only needed when building the cdylib loadable extension.
#[cfg(feature = "loadable-ext")]
use duckdb::ffi;
#[cfg(feature = "loadable-ext")]
use duckdb::Connection;
#[cfg(feature = "loadable-ext")]
use duckdb_loadable_macros::duckdb_entrypoint_c_api;

use crate::engine::{convert_value, map_postgres_type, MappedValue};
use crate::parser::{ColumnDef, Parser, ParserEvent, TableSchema};
use crate::reader::DumpReader;

// ─── Bind Data ────────────────────────────────────────────────────────────────

/// Data captured during the bind phase and shared with all scan threads.
pub struct PgQuackBindData {
    /// Path to the dump file (plain/gzip/zstd).
    pub dump_path: String,
    /// The table we want to read from the dump.
    pub table_name: String,
    /// Column schema (name + postgres type string) discovered during bind.
    pub schema: TableSchema,
}

// ─── Init Data ────────────────────────────────────────────────────────────────

/// Execution state shared across scan calls.
pub struct PgQuackInitData {
    /// Mutex-protected inner scan state so DuckDB can safely access it.
    pub inner: Mutex<ScanInner>,
    /// Whether the scan is complete (done producing rows).
    pub done: AtomicBool,
    /// Rows produced so far (for debugging / cardinality hints).
    pub rows_produced: AtomicUsize,
}

pub struct ScanInner {
    /// The underlying parser (lazily advances through the dump).
    parser: Parser<DumpReader>,
    /// Number of malformed COPY rows observed before the current scan step.
    observed_skipped_lines: usize,
    /// `true` once we've entered the target table's COPY block.
    in_target_copy: bool,
    /// The column order declared in the COPY statement.
    copy_columns: Vec<String>,
}

// ─── VTab Implementation ───────────────────────────────────────────────────────

pub struct PgQuackVTab;

impl VTab for PgQuackVTab {
    type BindData = PgQuackBindData;
    type InitData = PgQuackInitData;

    /// **Bind phase**: discover the table schema by scanning through the dump,
    /// then declare the output columns for the DuckDB planner.
    fn bind(bind: &BindInfo) -> Result<PgQuackBindData, Box<dyn std::error::Error>> {
        let dump_path = bind.get_parameter(0).to_string();
        let table_name = bind.get_parameter(1).to_string();

        // Scan just to find the CREATE TABLE statement.
        let reader = DumpReader::open(&dump_path)?;
        let mut parser = Parser::new(reader);
        let mut schema: Option<TableSchema> = None;

        loop {
            match parser.next_event() {
                None => break,
                Some(Err(e)) => return Err(e.into()),
                Some(Ok(ParserEvent::TableCreated(s))) if s.name == table_name => {
                    schema = Some(s);
                    break;
                }
                Some(Ok(_)) => {}
            }
        }

        let schema = schema
            .ok_or_else(|| format!("Table '{}' not found in dump '{}'", table_name, dump_path))?;

        // Declare output columns to DuckDB.
        for col in &schema.columns {
            let logical_type = pg_type_to_logical_type(&col.db_type);
            bind.add_result_column(&col.name, logical_type);
        }

        Ok(PgQuackBindData {
            dump_path,
            table_name,
            schema,
        })
    }

    /// **Init phase**: open the dump and prepare the parser for streaming.
    fn init(init: &InitInfo) -> Result<PgQuackInitData, Box<dyn std::error::Error>> {
        let bind_data = unsafe { &*init.get_bind_data::<PgQuackBindData>() };

        let reader = DumpReader::open(&bind_data.dump_path)?;
        let parser = Parser::new(reader);

        Ok(PgQuackInitData {
            inner: Mutex::new(ScanInner {
                parser,
                observed_skipped_lines: 0,
                in_target_copy: false,
                copy_columns: Vec::new(),
            }),
            done: AtomicBool::new(false),
            rows_produced: AtomicUsize::new(0),
        })
    }

    /// **Scan (func) phase**: called repeatedly by DuckDB to fill the output chunk.
    fn func(
        func: &TableFunctionInfo<Self>,
        output: &mut DataChunkHandle,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let bind_data = func.get_bind_data();
        let init_data = func.get_init_data();

        if init_data.done.load(Ordering::Relaxed) {
            output.set_len(0);
            return Ok(());
        }

        let mut inner = init_data.inner.lock().unwrap();
        let col_count = bind_data.schema.columns.len();
        let mut row_count: usize = 0;
        // DuckDB's default vector size is 2048 rows.
        let max_rows = output.flat_vector(0).capacity();

        loop {
            if row_count >= max_rows {
                break;
            }

            let next_event = inner.parser.next_event();
            let skipped_lines = inner.parser.skipped_lines_count;
            if skipped_lines > inner.observed_skipped_lines {
                let newly_skipped = skipped_lines - inner.observed_skipped_lines;
                inner.observed_skipped_lines = skipped_lines;
                if inner.in_target_copy {
                    return Err(format!(
                        "Parser skipped {} malformed COPY row(s) while reading table '{}'",
                        newly_skipped, bind_data.table_name
                    )
                    .into());
                }
            }

            match next_event {
                None => {
                    init_data.done.store(true, Ordering::Relaxed);
                    break;
                }
                Some(Err(e)) => return Err(e.into()),

                Some(Ok(ParserEvent::CopyStart {
                    table_name,
                    columns,
                })) => {
                    if table_name == bind_data.table_name {
                        inner.in_target_copy = true;
                        inner.copy_columns = columns;
                    }
                }

                Some(Ok(ParserEvent::CopyEnd { table_name })) => {
                    if table_name == bind_data.table_name {
                        inner.in_target_copy = false;
                        init_data.done.store(true, Ordering::Relaxed);
                        break;
                    }
                }

                Some(Ok(ParserEvent::CopyRow { table_name, values })) => {
                    if !inner.in_target_copy || table_name != bind_data.table_name {
                        continue;
                    }

                    // Reorder values from COPY column order → schema column order.
                    let mapped = map_copy_row_to_schema(
                        &values,
                        &inner.copy_columns,
                        &bind_data.schema.columns,
                    );

                    // Write each value into the DuckDB output vector.
                    for col_idx in 0..col_count {
                        let col_def = &bind_data.schema.columns[col_idx];
                        let opt_val = mapped.get(col_idx).and_then(|v| v.as_deref());
                        let converted =
                            convert_value(opt_val, &col_def.db_type).map_err(|err| {
                                format!(
                                    "Failed to convert value for {}.{}: {}",
                                    bind_data.table_name, col_def.name, err
                                )
                            })?;

                        write_value_to_chunk(output, col_idx, row_count, &converted, col_def);
                    }

                    row_count += 1;
                }

                // Ignore additional CREATE TABLE events encountered during scan.
                Some(Ok(ParserEvent::TableCreated(_))) => {}
            }
        }

        init_data
            .rows_produced
            .fetch_add(row_count, Ordering::Relaxed);
        output.set_len(row_count);
        Ok(())
    }

    /// Declare the two VARCHAR input parameters: (`dump_path`, `table_name`).
    fn parameters() -> Option<Vec<LogicalTypeHandle>> {
        Some(vec![
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
            LogicalTypeHandle::from(LogicalTypeId::Varchar),
        ])
    }
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Map a COPY row (ordered by COPY column list) to the schema column order.
fn map_copy_row_to_schema(
    values: &[Option<String>],
    copy_cols: &[String],
    schema_cols: &[ColumnDef],
) -> Vec<Option<String>> {
    schema_cols
        .iter()
        .map(|sc| {
            if let Some(idx) = copy_cols.iter().position(|c| c == &sc.name) {
                values.get(idx).cloned().flatten()
            } else {
                None
            }
        })
        .collect()
}

/// Convert a Postgres type string to a DuckDB `LogicalTypeHandle`.
pub fn pg_type_to_logical_type(pg_type: &str) -> LogicalTypeHandle {
    match map_postgres_type(pg_type) {
        "INTEGER" => LogicalTypeHandle::from(LogicalTypeId::Integer),
        "BIGINT" => LogicalTypeHandle::from(LogicalTypeId::Bigint),
        "BOOLEAN" => LogicalTypeHandle::from(LogicalTypeId::Boolean),
        "DOUBLE" => LogicalTypeHandle::from(LogicalTypeId::Double),
        "TIMESTAMP" => LogicalTypeHandle::from(LogicalTypeId::Timestamp),
        "DATE" => LogicalTypeHandle::from(LogicalTypeId::Date),
        _ => LogicalTypeHandle::from(LogicalTypeId::Varchar),
    }
}

/// Write a single `MappedValue` into column `col_idx`, row `row_idx` of `chunk`.
fn write_value_to_chunk(
    chunk: &mut DataChunkHandle,
    col_idx: usize,
    row_idx: usize,
    value: &MappedValue,
    col_def: &ColumnDef,
) {
    match value {
        MappedValue::Null => {
            let mut vec = chunk.flat_vector(col_idx);
            vec.set_null(row_idx);
        }
        MappedValue::Int(v) => {
            let mut vec = chunk.flat_vector(col_idx);
            let data = vec.as_mut_slice::<i32>();
            data[row_idx] = *v;
        }
        MappedValue::BigInt(v) => {
            let mut vec = chunk.flat_vector(col_idx);
            let data = vec.as_mut_slice::<i64>();
            data[row_idx] = *v;
        }
        MappedValue::Bool(v) => {
            let mut vec = chunk.flat_vector(col_idx);
            let data = vec.as_mut_slice::<bool>();
            data[row_idx] = *v;
        }
        MappedValue::Float(v) => {
            let mut vec = chunk.flat_vector(col_idx);
            let data = vec.as_mut_slice::<f64>();
            data[row_idx] = *v;
        }
        MappedValue::Timestamp(v) => {
            // DuckDB stores TIMESTAMP as microseconds since epoch (i64).
            use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
            let epoch = NaiveDateTime::new(
                NaiveDate::from_ymd_opt(1970, 1, 1).unwrap(),
                NaiveTime::from_hms_opt(0, 0, 0).unwrap(),
            );
            let micros = (*v - epoch).num_microseconds().unwrap_or(0);
            let mut vec = chunk.flat_vector(col_idx);
            let data = vec.as_mut_slice::<i64>();
            data[row_idx] = micros;
        }
        MappedValue::Date(v) => {
            // DuckDB stores DATE as days since epoch (i32).
            use chrono::NaiveDate;
            let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
            let days = v.signed_duration_since(epoch).num_days() as i32;
            let mut vec = chunk.flat_vector(col_idx);
            let data = vec.as_mut_slice::<i32>();
            data[row_idx] = days;
        }
        MappedValue::Text(s) => {
            // Use the Inserter<&str> impl for FlatVector (VARCHAR columns).
            let vec = chunk.flat_vector(col_idx);
            vec.insert(row_idx, s.as_str());
        }
    }
    let _ = col_def; // col_def used implicitly via caller for type dispatch
}

// ─── Extension Entrypoint ─────────────────────────────────────────────────────

/// Register `pgquack_read` with any DuckDB connection.
///
/// This can be used directly in integration tests or in the CLI.
pub fn register(conn: &duckdb::Connection) -> duckdb::Result<()> {
    conn.register_table_function::<PgQuackVTab>("pgquack_read")
}

/// Extension entry point – only compiled when building the cdylib loadable extension.
///
/// Enable with: `cargo build --release --features loadable-ext`
#[cfg(feature = "loadable-ext")]
#[duckdb_entrypoint_c_api(ext_name = "pgquack", min_duckdb_version = "v0.0.1")]
pub fn pgquack_ext_init(conn: Connection) -> Result<(), Box<dyn std::error::Error>> {
    conn.register_table_function::<PgQuackVTab>("pgquack_read")?;
    Ok(())
}
