use clap::Parser as ClapParser;
use log::{error, info, warn};
use std::collections::HashMap;
use std::path::Path;

use pgquack::cache::CacheManager;
use pgquack::engine::Engine;
use pgquack::parser::{Parser, ParserEvent};
use pgquack::reader::DumpReader;

#[derive(ClapParser, Debug)]
#[command(
    author,
    version,
    about = "Direct Query Engine for PostgreSQL Dumps — zero restore, DuckDB-powered"
)]
struct Args {
    /// Path to a SQL dump (.sql, .sql.gz, .sql.zst) or pg_dump tar archive (.tar, .tar.gz)
    dump_file: String,

    /// SQL query to execute on the loaded database
    query: String,

    /// Disable Parquet cache (always re-parse the dump)
    #[arg(long, default_value_t = false)]
    no_cache: bool,
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();
    let dump_path = Path::new(&args.dump_file);

    // ── 1. Initialize DuckDB engine ────────────────────────────────────────
    let engine = match Engine::new() {
        Ok(e) => e,
        Err(err) => {
            error!("Failed to initialize DuckDB: {}", err);
            std::process::exit(1);
        }
    };

    // ── 2. Check Parquet cache ─────────────────────────────────────────────
    let cache = CacheManager::new(dump_path);
    let mut skipped_lines: usize = 0;

    if !args.no_cache && cache.is_valid(dump_path) {
        info!("Cache hit — loading tables from Parquet cache (warm query)");
        match cache.load_all_into_duckdb(&engine) {
            Ok(tables) => info!("Loaded {} cached table(s)", tables.len()),
            Err(err) => {
                warn!("Cache load failed ({}), falling back to full parse", err);
                // Fall through to parse below — engine is still clean
                skipped_lines = do_parse(&args.dump_file, &engine, &cache, args.no_cache);
            }
        }
    } else {
        // ── 3. Parse dump (cold path) ──────────────────────────────────────
        if !args.no_cache {
            cache.invalidate();
        }
        skipped_lines = do_parse(&args.dump_file, &engine, &cache, args.no_cache);
    }

    // ── 4. Run user query ──────────────────────────────────────────────────
    info!("Executing query: {}", args.query);
    if let Err(err) = engine.query(&args.query) {
        error!("Query execution failed: {}", err);
        std::process::exit(1);
    }

    // ── 5. Exit code ───────────────────────────────────────────────────────
    if skipped_lines > 0 {
        warn!(
            "Exiting with code 2 — {} row(s) were skipped.",
            skipped_lines
        );
        std::process::exit(2);
    } else {
        std::process::exit(0);
    }
}

/// Parse the dump file, insert rows into the engine, and write a Parquet cache.
/// Returns the number of skipped (malformed) rows.
fn do_parse(dump_file: &str, engine: &Engine, cache: &CacheManager, no_cache: bool) -> usize {
    // Open reader (auto-detects plain / gz / zst)
    let reader = match DumpReader::open(dump_file) {
        Ok(r) => r,
        Err(err) => {
            log::error!("Failed to open dump file '{}': {}", dump_file, err);
            std::process::exit(1);
        }
    };

    info!(
        "Parsing {:?}-compressed dump: {}",
        reader.format(),
        dump_file
    );

    let mut parser = Parser::new(reader);
    let mut table_schemas = HashMap::new();
    // Tables that were successfully loaded — we'll cache these after parsing
    let mut loaded_tables: Vec<String> = Vec::new();

    // Parse events
    {
        let mut current_appender = None;

        while let Some(event_res) = parser.next_event() {
            let event = match event_res {
                Ok(e) => e,
                Err(err) => {
                    log::error!("Fatal parse error at line {}: {}", parser.line_number, err);
                    std::process::exit(1);
                }
            };

            match event {
                ParserEvent::TableCreated(schema) => {
                    info!("Creating table: {}", schema.name);
                    if let Err(err) = engine.create_table(&schema) {
                        log::error!("Failed to create table {} in DuckDB: {}", schema.name, err);
                        std::process::exit(1);
                    }
                    table_schemas.insert(schema.name.clone(), schema);
                }
                ParserEvent::CopyStart { table_name, .. } => {
                    info!("Loading data into table: {}", table_name);
                    let _ = current_appender.take(); // drop previous appender
                    if let Some(schema) = table_schemas.get(&table_name) {
                        match engine.appender(&table_name, schema.clone()) {
                            Ok(appender) => current_appender = Some(appender),
                            Err(err) => {
                                log::error!(
                                    "Failed to start appender for table {}: {}",
                                    table_name,
                                    err
                                );
                                std::process::exit(1);
                            }
                        }
                    } else {
                        log::error!("Table schema not found for COPY target: {}", table_name);
                        std::process::exit(1);
                    }
                }
                ParserEvent::CopyRow { values, .. } => {
                    if let Some(ref mut session) = current_appender {
                        if let Err(err) = session.append_row(&values) {
                            warn!("Skipped row at line {}: {}", parser.line_number, err);
                            parser.skipped_lines_count += 1;
                        }
                    }
                }
                ParserEvent::CopyEnd { table_name } => {
                    info!("Finished loading table: {}", table_name);
                    current_appender = None; // flush appender
                    loaded_tables.push(table_name);
                }
            }
        }
        // current_appender drops here — releases borrow on engine
    }

    let skipped = parser.skipped_lines_count;
    if skipped > 0 {
        warn!("{} malformed row(s) were skipped during import.", skipped);
    }
    info!("Parsing complete.");

    // ── Write Parquet cache ────────────────────────────────────────────────
    if !no_cache {
        let mut cache_ok = true;
        for table in &loaded_tables {
            if let Err(e) = cache.save_table(engine, table) {
                warn!("Could not cache table '{}': {}", table, e);
                cache_ok = false;
            }
        }
        if cache_ok && !loaded_tables.is_empty() {
            if let Err(e) = cache.write_meta(dump_file) {
                warn!("Could not write cache meta: {}", e);
            } else {
                info!("Parquet cache written for next warm query.");
            }
        }
    }

    skipped
}
