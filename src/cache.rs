//! Parquet Cache Manager for pgquack.
//!
//! # Strategy
//! - Cache directory: `<dump_path>.pgquack_cache/`  (sibling of the dump file)
//! - One Parquet file per table: `<cache_dir>/<table_name>.parquet`
//! - Meta file: `<cache_dir>/.meta.json` — stores `{ "dump_size": u64, "dump_mtime_secs": i64 }`
//! - Validity check: cache is valid iff size AND mtime match current dump file
//!   (no full hash — avoids re-reading multi-GB files on every query)
//!
//! # Usage
//! ```ignore
//! let cache = CacheManager::new("backup.sql.gz");
//! if cache.is_valid("backup.sql.gz") {
//!     cache.load_all_into_duckdb(&engine).unwrap();
//! } else {
//!     // ... parse & insert rows ...
//!     cache.save_table(&engine, "users").unwrap();
//!     cache.write_meta("backup.sql.gz").unwrap();
//! }
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use log::{debug, info, warn};
use serde::{Deserialize, Serialize};

use crate::engine::Engine;

/// Metadata stored alongside the Parquet cache to detect staleness.
#[derive(Debug, Serialize, Deserialize)]
struct CacheMeta {
    /// File size in bytes of the original dump at cache-write time.
    dump_size: u64,
    /// Modification time (seconds since UNIX epoch) of the dump.
    dump_mtime_secs: i64,
}

pub struct CacheManager {
    /// Directory where Parquet files and `.meta.json` live.
    cache_dir: PathBuf,
}

impl CacheManager {
    /// Create a `CacheManager` for the given dump file path.
    /// The cache directory is `<dump_path>.pgquack_cache/`.
    pub fn new<P: AsRef<Path>>(dump_path: P) -> Self {
        let dir_name = format!(
            "{}.pgquack_cache",
            dump_path.as_ref().file_name().unwrap_or_default().to_string_lossy()
        );
        let cache_dir = dump_path
            .as_ref()
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join(dir_name);
        Self { cache_dir }
    }

    /// Returns `true` if a valid cache exists for the given dump file.
    pub fn is_valid<P: AsRef<Path>>(&self, dump_path: P) -> bool {
        let meta_path = self.meta_path();
        if !meta_path.exists() {
            debug!("Cache meta not found at {:?}", meta_path);
            return false;
        }

        let current = match dump_metadata(dump_path.as_ref()) {
            Ok(m) => m,
            Err(e) => {
                warn!("Failed to read dump metadata for cache validation: {}", e);
                return false;
            }
        };

        let cached: CacheMeta = match fs::read_to_string(&meta_path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
        {
            Some(m) => m,
            None => {
                debug!("Cache meta corrupt or missing");
                return false;
            }
        };

        let valid = cached.dump_size == current.dump_size
            && cached.dump_mtime_secs == current.dump_mtime_secs;
        debug!(
            "Cache validity: {} (size {}/{}, mtime {}/{})",
            valid, cached.dump_size, current.dump_size,
            cached.dump_mtime_secs, current.dump_mtime_secs
        );
        valid
    }

    /// Write the cache metadata file after a successful cache build.
    pub fn write_meta<P: AsRef<Path>>(&self, dump_path: P) -> Result<(), String> {
        let meta = dump_metadata(dump_path.as_ref()).map_err(|e| e.to_string())?;
        fs::create_dir_all(&self.cache_dir).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(&meta).map_err(|e| e.to_string())?;
        fs::write(self.meta_path(), json).map_err(|e| e.to_string())?;
        info!("Cache meta written to {:?}", self.meta_path());
        Ok(())
    }

    /// Export a single DuckDB table to a Parquet file inside the cache directory.
    ///
    /// Uses DuckDB's built-in `COPY ... TO '...' (FORMAT PARQUET)`.
    pub fn save_table(&self, engine: &Engine, table_name: &str) -> Result<(), String> {
        fs::create_dir_all(&self.cache_dir).map_err(|e| e.to_string())?;
        let parquet_path = self.table_parquet_path(table_name);
        // DuckDB COPY TO handles the write natively.
        let sql = format!(
            "COPY \"{}\" TO '{}' (FORMAT PARQUET)",
            table_name,
            parquet_path.to_string_lossy().replace('\\', "/")
        );
        debug!("Saving table '{}' to Parquet: {}", table_name, sql);
        engine
            .connection()
            .execute(&sql, [])
            .map_err(|e| format!("Failed to save table '{}' to Parquet: {}", table_name, e))?;
        info!("Saved table '{}' → {:?}", table_name, parquet_path);
        Ok(())
    }

    /// Load all Parquet files from the cache directory into DuckDB.
    ///
    /// For each `<table>.parquet` file found, executes:
    /// ```sql
    /// CREATE TABLE "<table>" AS SELECT * FROM read_parquet('<path>');
    /// ```
    pub fn load_all_into_duckdb(&self, engine: &Engine) -> Result<Vec<String>, String> {
        if !self.cache_dir.exists() {
            return Ok(vec![]);
        }

        let mut loaded = Vec::new();
        let entries = fs::read_dir(&self.cache_dir)
            .map_err(|e| format!("Failed to read cache dir: {}", e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("parquet") {
                continue;
            }
            let table_name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_string();
            if table_name.is_empty() {
                continue;
            }

            let parquet_str = path.to_string_lossy().replace('\\', "/");
            let sql = format!(
                "CREATE TABLE \"{}\" AS SELECT * FROM read_parquet('{}')",
                table_name, parquet_str
            );
            debug!("Loading cached table '{}' from Parquet", table_name);
            engine
                .connection()
                .execute(&sql, [])
                .map_err(|e| format!("Failed to load cached table '{}': {}", table_name, e))?;
            info!("Loaded cached table '{}' from Parquet", table_name);
            loaded.push(table_name);
        }

        Ok(loaded)
    }

    /// Delete the entire cache directory (e.g., on invalidation).
    pub fn invalidate(&self) {
        if self.cache_dir.exists() {
            let _ = fs::remove_dir_all(&self.cache_dir);
            info!("Cache invalidated: {:?}", self.cache_dir);
        }
    }

    /// Return the path to the `.meta.json` file.
    fn meta_path(&self) -> PathBuf {
        self.cache_dir.join(".meta.json")
    }

    /// Return the path to the Parquet file for a given table.
    fn table_parquet_path(&self, table_name: &str) -> PathBuf {
        self.cache_dir.join(format!("{}.parquet", table_name))
    }
}

/// Read size and mtime from a file's metadata.
fn dump_metadata(path: &Path) -> Result<CacheMeta, std::io::Error> {
    let meta = fs::metadata(path)?;
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    Ok(CacheMeta {
        dump_size: meta.len(),
        dump_mtime_secs: mtime,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_dir_naming() {
        let cm = CacheManager::new("/tmp/backup.sql.gz");
        assert!(cm.cache_dir.to_string_lossy().contains("backup.sql.gz.pgquack_cache"));
    }
}
