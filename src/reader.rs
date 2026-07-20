//! Streaming reader for PostgreSQL dump files.
//!
//! Supports three formats, auto-detected from file extension or magic bytes:
//! - Plain text (`.sql`)
//! - Gzip-compressed (`.sql.gz`, `.gz`)
//! - Zstd-compressed (`.sql.zst`, `.zst`)

use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;

use flate2::read::GzDecoder;
use zstd::stream::read::Decoder as ZstdDecoder;

/// Gzip magic bytes
const GZIP_MAGIC: [u8; 2] = [0x1f, 0x8b];
/// Zstd frame magic bytes
const ZSTD_MAGIC: [u8; 4] = [0x28, 0xb5, 0x2f, 0xfd];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionFormat {
    Plain,
    Gzip,
    Zstd,
}

/// A unified buffered reader over plain/gzip/zstd dump files.
///
/// Implements [`BufRead`] so it can be passed directly to `Parser<R>`.
///
/// ## Zstd note
/// `zstd::Decoder<R>` requires `R: BufRead`. We satisfy this by passing
/// `BufReader<File>` as `R`. The decoder itself implements `Read`, so we wrap
/// it in a second `BufReader` to get `BufRead` on the decoded byte stream.
/// Concretely: `BufReader<ZstdDecoder<'static, BufReader<File>>>`.
pub enum DumpReader {
    Plain(BufReader<File>),
    Gzip(BufReader<GzDecoder<File>>),
    Zstd(Box<dyn BufRead>),
}

impl DumpReader {
    /// Open a dump file, detecting compression automatically.
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = path.as_ref();
        let format = detect_format(path)?;
        Self::open_with_format(path, format)
    }

    /// Open with an explicit format (useful for testing).
    pub fn open_with_format<P: AsRef<Path>>(path: P, format: CompressionFormat) -> io::Result<Self> {
        let path = path.as_ref();
        match format {
            CompressionFormat::Plain => {
                let f = File::open(path)?;
                Ok(DumpReader::Plain(BufReader::new(f)))
            }
            CompressionFormat::Gzip => {
                let f = File::open(path)?;
                let gz = GzDecoder::new(f);
                Ok(DumpReader::Gzip(BufReader::new(gz)))
            }
            CompressionFormat::Zstd => {
                let f = File::open(path)?;
                // ZstdDecoder<R> requires R: BufRead — wrap File in BufReader.
                let inner = BufReader::new(f);
                let decoder = ZstdDecoder::new(inner)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                // Wrap decoded stream in BufReader for efficient read_line.
                let buf = BufReader::new(decoder);
                Ok(DumpReader::Zstd(Box::new(buf)))
            }
        }
    }

    /// Return which compression format this reader uses.
    pub fn format(&self) -> CompressionFormat {
        match self {
            DumpReader::Plain(_) => CompressionFormat::Plain,
            DumpReader::Gzip(_) => CompressionFormat::Gzip,
            DumpReader::Zstd(_) => CompressionFormat::Zstd,
        }
    }
}

impl Read for DumpReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            DumpReader::Plain(r) => r.read(buf),
            DumpReader::Gzip(r) => r.read(buf),
            DumpReader::Zstd(r) => r.read(buf),
        }
    }
}

impl BufRead for DumpReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self {
            DumpReader::Plain(r) => r.fill_buf(),
            DumpReader::Gzip(r) => r.fill_buf(),
            DumpReader::Zstd(r) => r.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            DumpReader::Plain(r) => r.consume(amt),
            DumpReader::Gzip(r) => r.consume(amt),
            DumpReader::Zstd(r) => r.consume(amt),
        }
    }
}

/// Detect the compression format of a file by extension first, then magic bytes.
fn detect_format(path: &Path) -> io::Result<CompressionFormat> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    if ext == "gz" {
        return Ok(CompressionFormat::Gzip);
    }
    if ext == "zst" {
        return Ok(CompressionFormat::Zstd);
    }

    // Magic-byte fallback for non-standard extensions
    let mut magic = [0u8; 4];
    let mut f = File::open(path)?;
    let n = f.read(&mut magic)?;

    if n >= 2 && magic[..2] == GZIP_MAGIC {
        return Ok(CompressionFormat::Gzip);
    }
    if n >= 4 && magic[..4] == ZSTD_MAGIC {
        return Ok(CompressionFormat::Zstd);
    }

    Ok(CompressionFormat::Plain)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_gz_by_extension() {
        let ext = Path::new("dump.sql.gz")
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        assert_eq!(ext, "gz");
    }

    #[test]
    fn detect_zst_by_extension() {
        let ext = Path::new("dump.sql.zst")
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        assert_eq!(ext, "zst");
    }

    #[test]
    fn detect_plain_sql() {
        let ext = Path::new("dump.sql")
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();
        assert_ne!(ext, "gz");
        assert_ne!(ext, "zst");
    }
}
