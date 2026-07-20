use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;
use clap::Parser as ClapParser;
use log::{error, info, warn};

use pgquack::parser::{Parser, ParserEvent};
use pgquack::engine::Engine;

#[derive(ClapParser, Debug)]
#[command(author, version, about = "Direct Query Engine for PostgreSQL Dumps")]
struct Args {
    /// Path to the SQL dump file
    dump_file: String,

    /// SQL query to execute on the database
    query: String,
}

fn main() {
    // Initialize logger
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let args = Args::parse();

    // 1. Open the dump file
    let file = match File::open(&args.dump_file) {
        Ok(f) => f,
        Err(err) => {
            error!("Failed to open dump file '{}': {}", args.dump_file, err);
            std::process::exit(1);
        }
    };

    let reader = BufReader::new(file);
    let mut parser = Parser::new(reader);

    // 2. Initialize the DuckDB Engine
    let engine = match Engine::new() {
        Ok(e) => e,
        Err(err) => {
            error!("Failed to initialize DuckDB: {}", err);
            std::process::exit(1);
        }
    };

    let mut table_schemas = HashMap::new();
    let mut current_appender = None;

    info!("Starting parsing of dump file: {}", args.dump_file);

    // 3. Process parser events stream
    while let Some(event_res) = parser.next_event() {
        let event = match event_res {
            Ok(e) => e,
            Err(err) => {
                error!("Fatal parse error at line {}: {}", parser.line_number, err);
                std::process::exit(1);
            }
        };

        match event {
            ParserEvent::TableCreated(schema) => {
                info!("Creating table: {}", schema.name);
                if let Err(err) = engine.create_table(&schema) {
                    error!("Failed to create table {} in DuckDB: {}", schema.name, err);
                    std::process::exit(1);
                }
                table_schemas.insert(schema.name.clone(), schema);
            }
            ParserEvent::CopyStart { table_name, .. } => {
                info!("Loading data into table: {}", table_name);
                if let Some(schema) = table_schemas.get(&table_name) {
                    match engine.appender(&table_name, schema.clone()) {
                        Ok(appender) => {
                            current_appender = Some(appender);
                        }
                        Err(err) => {
                            error!("Failed to start Appender for table {}: {}", table_name, err);
                            std::process::exit(1);
                        }
                    }
                } else {
                    error!("Table schema not found for COPY target: {}", table_name);
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
                info!("Finished data load for table: {}", table_name);
                current_appender = None; // Drop appender, flushing buffer
            }
        }
    }

    info!("Parsing complete.");
    if parser.skipped_lines_count > 0 {
        warn!(
            "Total of {} malformed rows were skipped during import.",
            parser.skipped_lines_count
        );
    }

    // 4. Run user query
    info!("Executing query: {}", args.query);
    if let Err(err) = engine.query(&args.query) {
        error!("Query execution failed: {}", err);
        std::process::exit(1);
    }

    // 5. Exit code based on skipped lines
    if parser.skipped_lines_count > 0 {
        warn!("Exiting with code 2 due to skipped lines.");
        std::process::exit(2);
    } else {
        std::process::exit(0);
    }
}
