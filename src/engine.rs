use crate::parser::TableSchema;
use chrono::{NaiveDate, NaiveDateTime};
use duckdb::{types::ToSqlOutput, Connection, ToSql};
use log::{debug, info};

pub enum MappedValue {
    Int(i32),
    BigInt(i64),
    Bool(bool),
    Float(f64),
    Text(String),
    Timestamp(NaiveDateTime),
    Date(NaiveDate),
    Null,
}

impl ToSql for MappedValue {
    fn to_sql(&self) -> duckdb::Result<ToSqlOutput<'_>> {
        match self {
            MappedValue::Int(v) => v.to_sql(),
            MappedValue::BigInt(v) => v.to_sql(),
            MappedValue::Bool(v) => v.to_sql(),
            MappedValue::Float(v) => v.to_sql(),
            MappedValue::Text(v) => v.to_sql(),
            MappedValue::Timestamp(v) => v.to_sql(),
            MappedValue::Date(v) => v.to_sql(),
            MappedValue::Null => Option::<i32>::None.to_sql(),
        }
    }
}

pub fn map_postgres_type(pg_type: &str) -> &'static str {
    let norm = pg_type.to_lowercase();
    // Integer types
    if norm == "integer" || norm == "int" || norm == "int4" || norm == "int2" || norm == "smallint"
    {
        "INTEGER"
    } else if norm == "bigint" || norm == "int8" {
        "BIGINT"
    // Boolean
    } else if norm == "boolean" || norm == "bool" {
        "BOOLEAN"
    // Floating point
    } else if norm == "real"
        || norm == "float4"
        || norm == "float8"
        || norm == "double precision"
        || norm == "float"
        || norm.starts_with("numeric")
        || norm.starts_with("decimal")
    {
        "DOUBLE"
    // Temporal
    } else if norm.starts_with("timestamp") {
        "TIMESTAMP"
    } else if norm == "date" {
        "DATE"
    // JSON / JSONB → store as VARCHAR in DuckDB (DuckDB can still parse it)
    } else if norm == "json" || norm == "jsonb" {
        "VARCHAR"
    // UUID → store as VARCHAR
    } else if norm == "uuid" {
        "VARCHAR"
    // Arrays (e.g. "integer[]", "text[]") → store JSON-encoded as VARCHAR
    } else if norm.ends_with("[]") || norm.ends_with("array") {
        "VARCHAR"
    // Byte arrays
    } else if norm == "bytea" {
        "VARCHAR"
    } else {
        "VARCHAR"
    }
}

fn parse_timestamp(s: &str) -> Result<NaiveDateTime, String> {
    let formats = [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S",
    ];
    for fmt in &formats {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Ok(dt);
        }
    }

    // Handle timezone suffix like +00, +07:00, etc.
    if let Some(plus_idx) = s.find('+') {
        let base = &s[..plus_idx];
        for fmt in &formats {
            if let Ok(dt) = NaiveDateTime::parse_from_str(base, fmt) {
                return Ok(dt);
            }
        }
    }

    if let Some(minus_idx) = s.rfind('-') {
        // Only if it's after the date portion (e.g. YYYY-MM-DD)
        if minus_idx > 10 {
            let base = &s[..minus_idx];
            for fmt in &formats {
                if let Ok(dt) = NaiveDateTime::parse_from_str(base, fmt) {
                    return Ok(dt);
                }
            }
        }
    }

    Err(format!("Could not parse timestamp '{}'", s))
}

pub fn convert_value(val: Option<&str>, db_type: &str) -> Result<MappedValue, String> {
    let val = match val {
        None => return Ok(MappedValue::Null),
        Some(v) => v,
    };

    let norm = db_type.to_lowercase();

    // Integer types
    if norm == "integer" || norm == "int" || norm == "int4" || norm == "int2" || norm == "smallint"
    {
        let parsed = val
            .parse::<i32>()
            .map_err(|e| format!("Failed to parse integer '{}': {}", val, e))?;
        Ok(MappedValue::Int(parsed))
    } else if norm == "bigint" || norm == "int8" {
        let parsed = val
            .parse::<i64>()
            .map_err(|e| format!("Failed to parse bigint '{}': {}", val, e))?;
        Ok(MappedValue::BigInt(parsed))
    // Boolean
    } else if norm == "boolean" || norm == "bool" {
        let parsed = match val.to_lowercase().as_str() {
            "t" | "true" | "1" | "y" | "yes" => true,
            "f" | "false" | "0" | "n" | "no" => false,
            _ => return Err(format!("Failed to parse boolean '{}'", val)),
        };
        Ok(MappedValue::Bool(parsed))
    // Floating point & numeric/decimal
    } else if norm == "real"
        || norm == "float4"
        || norm == "float8"
        || norm == "double precision"
        || norm == "float"
        || norm.starts_with("numeric")
        || norm.starts_with("decimal")
    {
        let parsed = val
            .parse::<f64>()
            .map_err(|e| format!("Failed to parse float '{}': {}", val, e))?;
        Ok(MappedValue::Float(parsed))
    // Temporal
    } else if norm.starts_with("timestamp") {
        let parsed = parse_timestamp(val)?;
        Ok(MappedValue::Timestamp(parsed))
    } else if norm == "date" {
        let parsed = NaiveDate::parse_from_str(val, "%Y-%m-%d")
            .map_err(|e| format!("Failed to parse date '{}': {}", val, e))?;
        Ok(MappedValue::Date(parsed))
    // JSON/JSONB — store as text; validate it is parseable JSON
    } else if norm == "json" || norm == "jsonb" {
        // Accept as-is (DuckDB will handle JSON natively)
        Ok(MappedValue::Text(val.to_string()))
    // UUID — store as text
    } else if norm == "uuid" {
        Ok(MappedValue::Text(val.to_string()))
    // 1D Arrays (e.g. "{1,2,3}" or "{a,b,c}") — JSON-encode
    } else if norm.ends_with("[]") || norm.ends_with("array") {
        let json = pg_array_to_json(val);
        Ok(MappedValue::Text(json))
    // Everything else (text, varchar, bytea, etc.)
    } else {
        Ok(MappedValue::Text(val.to_string()))
    }
}

/// Convert a PostgreSQL array literal (`{a,b,c}`) to a JSON array string (`["a","b","c"]`).
/// This is a best-effort conversion for simple 1D arrays without nested quoting.
pub fn pg_array_to_json(raw: &str) -> String {
    let inner = raw.trim().trim_start_matches('{').trim_end_matches('}');
    if inner.is_empty() {
        return "[]".to_string();
    }
    let items: Vec<String> = inner
        .split(',')
        .map(|item| {
            let item = item.trim();
            if item.eq_ignore_ascii_case("null") {
                "null".to_string()
            } else if item.starts_with('"') {
                // Already quoted
                item.to_string()
            } else {
                // Try to preserve numerics as-is, quote everything else
                if item.parse::<f64>().is_ok() {
                    item.to_string()
                } else {
                    format!("{}", serde_json::Value::String(item.to_string()))
                }
            }
        })
        .collect();
    format!("[{}]", items.join(","))
}

pub struct Engine {
    conn: Connection,
}

impl Engine {
    pub fn new() -> Result<Self, duckdb::Error> {
        let conn = Connection::open_in_memory()?;
        Ok(Self { conn })
    }

    pub fn create_table(&self, schema: &TableSchema) -> Result<(), duckdb::Error> {
        let cols: Vec<String> = schema
            .columns
            .iter()
            .map(|col| format!("\"{}\" {}", col.name, map_postgres_type(&col.db_type)))
            .collect();
        let create_sql = format!(
            "CREATE TABLE \"{}\" (\n  {}\n);",
            schema.name,
            cols.join(",\n  ")
        );
        debug!("Creating table in DuckDB: \n{}", create_sql);
        self.conn.execute(&create_sql, [])?;
        Ok(())
    }

    pub fn appender<'a>(
        &'a self,
        table_name: &str,
        schema: TableSchema,
    ) -> Result<AppenderSession<'a>, duckdb::Error> {
        AppenderSession::new(&self.conn, table_name, schema)
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    pub fn query(&self, sql: &str) -> Result<(), duckdb::Error> {
        let mut stmt = self.conn.prepare(sql)?;
        let col_names = stmt.column_names();
        let col_count = col_names.len();

        let mut rows_data: Vec<Vec<String>> = Vec::new();
        let mut col_widths = vec![0; col_count];

        for (i, name) in col_names.iter().enumerate() {
            col_widths[i] = name.len();
        }

        let mut rows = stmt.query([])?;
        while let Some(row) = rows.next()? {
            let mut row_strings = Vec::with_capacity(col_count);
            for i in 0..col_count {
                let s = match row.get::<_, String>(i) {
                    Ok(val) => val,
                    Err(_) => {
                        // Fallback to get_ref
                        let val = row.get_ref(i)?;
                        match val {
                            duckdb::types::ValueRef::Null => "NULL".to_string(),
                            duckdb::types::ValueRef::Boolean(b) => b.to_string(),
                            duckdb::types::ValueRef::TinyInt(v) => v.to_string(),
                            duckdb::types::ValueRef::SmallInt(v) => v.to_string(),
                            duckdb::types::ValueRef::Int(v) => v.to_string(),
                            duckdb::types::ValueRef::BigInt(v) => v.to_string(),
                            duckdb::types::ValueRef::HugeInt(v) => v.to_string(),
                            duckdb::types::ValueRef::Float(v) => v.to_string(),
                            duckdb::types::ValueRef::Double(v) => v.to_string(),
                            duckdb::types::ValueRef::Decimal(v) => v.to_string(),
                            duckdb::types::ValueRef::Text(bytes) => {
                                String::from_utf8_lossy(bytes).into_owned()
                            }
                            duckdb::types::ValueRef::Blob(bytes) => {
                                format!("BLOB ({} bytes)", bytes.len())
                            }
                            _ => format!("{:?}", val),
                        }
                    }
                };
                col_widths[i] = col_widths[i].max(s.len());
                row_strings.push(s);
            }
            rows_data.push(row_strings);
        }

        print_border(&col_widths);
        print_row(&col_names, &col_widths);
        print_border(&col_widths);
        for row in &rows_data {
            print_row(row, &col_widths);
        }
        print_border(&col_widths);
        info!("Selected {} rows", rows_data.len());

        Ok(())
    }
}

fn print_border(widths: &[usize]) {
    let mut border = String::new();
    border.push('+');
    for &w in widths {
        border.push_str(&"-".repeat(w + 2));
        border.push('+');
    }
    println!("{}", border);
}

fn print_row(cols: &[impl AsRef<str>], widths: &[usize]) {
    let mut row = String::new();
    row.push('|');
    for (i, col) in cols.iter().enumerate() {
        let text = col.as_ref();
        let pad = widths[i] - text.len();
        row.push(' ');
        row.push_str(text);
        row.push_str(&" ".repeat(pad));
        row.push_str(" |");
    }
    println!("{}", row);
}

pub struct AppenderSession<'a> {
    appender: duckdb::Appender<'a>,
    schema: TableSchema,
}

impl<'a> AppenderSession<'a> {
    pub fn new(
        conn: &'a Connection,
        table_name: &str,
        schema: TableSchema,
    ) -> Result<Self, duckdb::Error> {
        let appender = conn.appender(table_name)?;
        Ok(Self { appender, schema })
    }

    pub fn append_row(&mut self, values: &[Option<String>]) -> Result<(), String> {
        if values.len() != self.schema.columns.len() {
            return Err(format!(
                "Row length {} does not match schema column count {}",
                values.len(),
                self.schema.columns.len()
            ));
        }

        let mut mapped_values: Vec<MappedValue> = Vec::with_capacity(values.len());
        for (i, val) in values.iter().enumerate() {
            let col = &self.schema.columns[i];
            let mapped = convert_value(val.as_deref(), &col.db_type)?;
            mapped_values.push(mapped);
        }
        let params: Vec<&dyn ToSql> = mapped_values.iter().map(|v| v as &dyn ToSql).collect();
        self.appender
            .append_row(params.as_slice())
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
