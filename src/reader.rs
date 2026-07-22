//! Streaming reader for PostgreSQL dump files.
//!
//! Supports PostgreSQL SQL dumps and `pg_dump -Ft` archives, auto-detected from
//! their file extensions or compression magic bytes:
//! - Plain text (`.sql`)
//! - Gzip-compressed (`.sql.gz`, `.gz`)
//! - Zstd-compressed (`.sql.zst`, `.zst`)
//! - PostgreSQL tar archive (`.tar`, requires `pg_restore` on `PATH`)
//! - Gzip-wrapped PostgreSQL tar archive (`.tar.gz`, requires `pg_restore`)

use std::env;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read};
use std::path::Path;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::thread::{self, JoinHandle};

use flate2::read::GzDecoder;
use tempfile::{NamedTempFile, TempPath};
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
    Tar,
    TarGzip,
}

/// Reader for SQL emitted by a `pg_restore` child process.
pub struct PgRestoreReader {
    stdout: BufReader<ChildStdout>,
    child: Child,
    stderr_reader: Option<JoinHandle<io::Result<Vec<u8>>>>,
    exit_result: Option<Result<(), String>>,
    _temporary_archive: Option<TempPath>,
}

impl PgRestoreReader {
    fn finish(&mut self) -> io::Result<()> {
        if self.exit_result.is_none() {
            let result = (|| -> io::Result<()> {
                let status = self.child.wait()?;
                let stderr = self
                    .stderr_reader
                    .take()
                    .expect("pg_restore stderr reader should exist")
                    .join()
                    .map_err(|_| io::Error::other("pg_restore stderr reader panicked"))??;

                if status.success() {
                    return Ok(());
                }

                let stderr = String::from_utf8_lossy(&stderr).trim().to_string();
                let detail = if stderr.is_empty() {
                    format!("pg_restore exited with status {status}")
                } else {
                    format!("pg_restore exited with status {status}: {stderr}")
                };
                Err(io::Error::other(detail))
            })();
            self.exit_result = Some(result.map_err(|err| err.to_string()));
        }

        match self
            .exit_result
            .as_ref()
            .expect("exit result should be stored")
        {
            Ok(()) => Ok(()),
            Err(message) => Err(io::Error::other(message.clone())),
        }
    }
}

impl Read for PgRestoreReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.stdout.read(buf)?;
        if bytes_read == 0 {
            self.finish()?;
        }
        Ok(bytes_read)
    }
}

impl BufRead for PgRestoreReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        if self.stdout.fill_buf()?.is_empty() {
            self.finish()?;
        }
        self.stdout.fill_buf()
    }

    fn consume(&mut self, amt: usize) {
        self.stdout.consume(amt);
    }
}

/// A unified buffered reader over plain, compressed, and pg_dump tar archives.
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
    Zstd(Box<dyn BufRead + Send>),
    PgRestore(PgRestoreReader, CompressionFormat),
}

impl DumpReader {
    /// Open a dump file, detecting compression automatically.
    pub fn open<P: AsRef<Path>>(path: P) -> io::Result<Self> {
        let path = path.as_ref();
        let format = detect_format(path)?;
        Self::open_with_format(path, format)
    }

    /// Open with an explicit format (useful for testing).
    pub fn open_with_format<P: AsRef<Path>>(
        path: P,
        format: CompressionFormat,
    ) -> io::Result<Self> {
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
            CompressionFormat::Tar | CompressionFormat::TarGzip => {
                open_with_format_and_command(path, format, pg_restore_command())
            }
        }
    }

    /// Return which compression format this reader uses.
    pub fn format(&self) -> CompressionFormat {
        match self {
            DumpReader::Plain(_) => CompressionFormat::Plain,
            DumpReader::Gzip(_) => CompressionFormat::Gzip,
            DumpReader::Zstd(_) => CompressionFormat::Zstd,
            DumpReader::PgRestore(_, format) => *format,
        }
    }
}

impl Read for DumpReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            DumpReader::Plain(r) => r.read(buf),
            DumpReader::Gzip(r) => r.read(buf),
            DumpReader::Zstd(r) => r.read(buf),
            DumpReader::PgRestore(r, _) => r.read(buf),
        }
    }
}

impl BufRead for DumpReader {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        match self {
            DumpReader::Plain(r) => r.fill_buf(),
            DumpReader::Gzip(r) => r.fill_buf(),
            DumpReader::Zstd(r) => r.fill_buf(),
            DumpReader::PgRestore(r, _) => r.fill_buf(),
        }
    }

    fn consume(&mut self, amt: usize) {
        match self {
            DumpReader::Plain(r) => r.consume(amt),
            DumpReader::Gzip(r) => r.consume(amt),
            DumpReader::Zstd(r) => r.consume(amt),
            DumpReader::PgRestore(r, _) => r.consume(amt),
        }
    }
}

fn pg_restore_command() -> Command {
    Command::new(env::var_os("PGQUACK_PG_RESTORE").unwrap_or_else(|| "pg_restore".into()))
}

fn open_with_format_and_command<P: AsRef<Path>>(
    path: P,
    format: CompressionFormat,
    mut pg_restore: Command,
) -> io::Result<DumpReader> {
    let path = path.as_ref();
    match format {
        CompressionFormat::Tar => spawn_pg_restore(path, &mut pg_restore, None, format),
        CompressionFormat::TarGzip => {
            let temporary_archive = decompress_gzip_archive(path)?;
            let archive_path = temporary_archive.to_path_buf();
            spawn_pg_restore(
                &archive_path,
                &mut pg_restore,
                Some(temporary_archive),
                format,
            )
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "pg_restore is only used for PostgreSQL tar archives",
        )),
    }
}

fn decompress_gzip_archive(path: &Path) -> io::Result<TempPath> {
    let input = File::open(path)?;
    let mut decoder = GzDecoder::new(input);
    let mut temporary_archive = NamedTempFile::new()?;
    io::copy(&mut decoder, temporary_archive.as_file_mut())?;
    temporary_archive.as_file_mut().sync_all()?;
    Ok(temporary_archive.into_temp_path())
}

fn spawn_pg_restore(
    archive_path: &Path,
    pg_restore: &mut Command,
    temporary_archive: Option<TempPath>,
    format: CompressionFormat,
) -> io::Result<DumpReader> {
    let program: std::ffi::OsString = pg_restore.get_program().to_owned();
    let mut child = pg_restore
        .arg("--file=-")
        .arg(archive_path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            io::Error::new(
                err.kind(),
                format!(
                    "failed to execute '{}' for PostgreSQL tar archive '{}': {err}; install PostgreSQL client tools and ensure pg_restore is on PATH",
                    program.to_string_lossy(),
                    archive_path.display()
                ),
            )
        })?;
    let stdout = child
        .stdout
        .take()
        .expect("pg_restore stdout should be piped");
    let stderr = child
        .stderr
        .take()
        .expect("pg_restore stderr should be piped");
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut output = Vec::new();
        stderr.read_to_end(&mut output)?;
        Ok(output)
    });

    Ok(DumpReader::PgRestore(
        PgRestoreReader {
            stdout: BufReader::new(stdout),
            child,
            stderr_reader: Some(stderr_reader),
            exit_result: None,
            _temporary_archive: temporary_archive,
        },
        format,
    ))
}

/// Detect the compression format of a file by extension first, then magic bytes.
fn detect_format(path: &Path) -> io::Result<CompressionFormat> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_lowercase();

    if file_name.ends_with(".tar.gz") || file_name.ends_with(".tgz") {
        return Ok(CompressionFormat::TarGzip);
    }
    if ext == "tar" {
        return Ok(CompressionFormat::Tar);
    }
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
    use crate::parser::{Parser, ParserEvent};
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::fs;
    use std::io::Write;
    use std::process::Command;
    use tempfile::{NamedTempFile, TempDir};

    fn fake_pg_restore(temp_dir: &TempDir, expected_archive_text: &str) -> Command {
        #[cfg(windows)]
        let script = {
            let script = temp_dir.path().join("fake_pg_restore.cmd");
            fs::write(
                &script,
                format!(
                    "@echo off\r\nfindstr /C:\"{expected_archive_text}\" \"%~2\" >nul || exit /b 17\r\necho CREATE TABLE archive_users (\r\necho     id integer\r\necho );\r\necho COPY archive_users (id) FROM stdin;\r\necho 1\r\necho \\.\r\n"
                ),
            )
            .unwrap();
            script
        };

        #[cfg(not(windows))]
        let script = {
            use std::os::unix::fs::PermissionsExt;

            let script = temp_dir.path().join("fake_pg_restore.sh");
            fs::write(
                &script,
                format!(
                    "#!/bin/sh\ngrep -F -- '{expected_archive_text}' \"$2\" >/dev/null || exit 17\nprintf '%s\\n' 'CREATE TABLE archive_users (' '    id integer' ');' 'COPY archive_users (id) FROM stdin;' '1' '\\\\.'\n"
                ),
            )
            .unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
            script
        };

        Command::new(script)
    }

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

    #[test]
    fn detect_pg_dump_tar_extensions() {
        let temp_dir = TempDir::new().unwrap();
        let tar = temp_dir.path().join("backup.tar");
        let tar_gz = temp_dir.path().join("backup.tar.gz");
        fs::write(&tar, b"tar archive").unwrap();
        fs::write(&tar_gz, b"gzip wrapped tar archive").unwrap();

        assert_eq!(detect_format(&tar).unwrap(), CompressionFormat::Tar);
        assert_eq!(detect_format(&tar_gz).unwrap(), CompressionFormat::TarGzip);
    }

    #[test]
    fn dump_reader_is_send() {
        fn assert_send<T: Send>() {}

        assert_send::<DumpReader>();
    }

    #[test]
    fn pg_dump_tar_is_converted_to_sql_with_pg_restore() {
        let temp_dir = TempDir::new().unwrap();
        let mut archive = NamedTempFile::new_in(&temp_dir).unwrap();
        archive.write_all(b"pg_dump tar payload").unwrap();

        let mut reader = open_with_format_and_command(
            archive.path(),
            CompressionFormat::Tar,
            fake_pg_restore(&temp_dir, "pg_dump tar payload"),
        )
        .unwrap();
        let mut sql = String::new();
        reader.read_to_string(&mut sql).unwrap();

        assert!(sql.contains("CREATE TABLE archive_users"));
        assert!(sql.contains("COPY archive_users"));

        let reader = open_with_format_and_command(
            archive.path(),
            CompressionFormat::Tar,
            fake_pg_restore(&temp_dir, "pg_dump tar payload"),
        )
        .unwrap();
        let mut parser = Parser::new(reader);
        assert!(matches!(
            parser.next_event(),
            Some(Ok(ParserEvent::TableCreated(schema))) if schema.name == "archive_users"
        ));
        assert!(matches!(
            parser.next_event(),
            Some(Ok(ParserEvent::CopyStart { table_name, .. })) if table_name == "archive_users"
        ));
    }

    #[test]
    fn gzipped_pg_dump_tar_is_decompressed_before_pg_restore() {
        let temp_dir = TempDir::new().unwrap();
        let archive_path = temp_dir.path().join("backup.tar.gz");
        let archive = fs::File::create(&archive_path).unwrap();
        let mut encoder = GzEncoder::new(archive, Compression::default());
        encoder.write_all(b"pg_dump tar payload").unwrap();
        encoder.finish().unwrap();

        let mut reader = open_with_format_and_command(
            &archive_path,
            CompressionFormat::TarGzip,
            fake_pg_restore(&temp_dir, "pg_dump tar payload"),
        )
        .unwrap();
        let mut sql = String::new();
        reader.read_to_string(&mut sql).unwrap();

        assert!(sql.contains("CREATE TABLE archive_users"));
    }

    #[test]
    fn missing_pg_restore_reports_actionable_error() {
        let temp_dir = TempDir::new().unwrap();
        let archive = NamedTempFile::new_in(&temp_dir).unwrap();
        let missing_command = temp_dir.path().join("missing-pg_restore");

        let error = open_with_format_and_command(
            archive.path(),
            CompressionFormat::Tar,
            Command::new(missing_command),
        )
        .err()
        .unwrap();

        assert!(error
            .to_string()
            .contains("install PostgreSQL client tools"));
    }
}
