use log::warn;
use std::collections::HashMap;
use std::io::BufRead;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct ColumnDef {
    pub name: String,
    pub db_type: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableSchema {
    pub name: String,
    pub columns: Vec<ColumnDef>,
}

#[derive(Debug, Clone)]
pub enum ParserEvent {
    TableCreated(TableSchema),
    CopyStart {
        table_name: String,
        columns: Vec<String>,
    },
    CopyRow {
        table_name: String,
        values: Vec<Option<String>>,
    },
    CopyEnd {
        table_name: String,
    },
}

#[derive(Error, Debug)]
pub enum ParserError {
    #[error("IO error at line {line}: {source}")]
    Io {
        line: usize,
        #[source]
        source: std::io::Error,
    },
    #[error("Malformed SQL at line {line}: {message}")]
    MalformedSql { line: usize, message: String },
    #[error("Malformed COPY data at line {line}: {message}")]
    MalformedCopy { line: usize, message: String },
}

pub struct Parser<R: BufRead> {
    reader: R,
    pub line_number: usize,
    state: ParserState,
    schemas: HashMap<String, TableSchema>,
    pub skipped_lines_count: usize,
}

enum ParserState {
    Idle,
    InCreateTable {
        table_name: String,
        columns: Vec<ColumnDef>,
    },
    InCopy {
        table_name: String,
        columns: Vec<String>,
    },
}

impl<R: BufRead> Parser<R> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            line_number: 0,
            state: ParserState::Idle,
            schemas: HashMap::new(),
            skipped_lines_count: 0,
        }
    }

    pub fn next_event(&mut self) -> Option<Result<ParserEvent, ParserError>> {
        loop {
            let mut line = String::new();
            match self.reader.read_line(&mut line) {
                Ok(0) => {
                    // EOF
                    match &self.state {
                        ParserState::Idle => return None,
                        ParserState::InCreateTable { table_name, .. } => {
                            return Some(Err(ParserError::MalformedSql {
                                line: self.line_number,
                                message: format!(
                                    "Unexpected EOF inside CREATE TABLE for {}",
                                    table_name
                                ),
                            }));
                        }
                        ParserState::InCopy { table_name, .. } => {
                            return Some(Err(ParserError::MalformedCopy {
                                line: self.line_number,
                                message: format!(
                                    "Unexpected EOF inside COPY data for {}",
                                    table_name
                                ),
                            }));
                        }
                    }
                }
                Ok(_) => {
                    self.line_number += 1;
                    let trimmed = trim_newline(&line);

                    match &mut self.state {
                        ParserState::Idle => {
                            let trimmed_upper = trimmed.to_uppercase();
                            if trimmed_upper.starts_with("CREATE TABLE ") {
                                if let Some(open_paren_idx) = trimmed.find('(') {
                                    let table_part = &trimmed[13..open_paren_idx];
                                    let table_name = strip_quotes(table_part);
                                    if !table_name.is_empty() {
                                        self.state = ParserState::InCreateTable {
                                            table_name,
                                            columns: Vec::new(),
                                        };
                                    }
                                }
                                // If table name couldn't be parsed, we just ignore this line or continue
                            } else if trimmed_upper.starts_with("COPY ") {
                                if let Some(event) = self.handle_copy_start(trimmed) {
                                    return Some(event);
                                }
                            }
                        }
                        ParserState::InCreateTable {
                            table_name,
                            columns,
                        } => {
                            let trimmed_cols = trimmed.trim();
                            if trimmed_cols.starts_with(')') || trimmed_cols.contains(");") {
                                // End of CREATE TABLE
                                let schema = TableSchema {
                                    name: table_name.clone(),
                                    columns: columns.clone(),
                                };
                                self.schemas.insert(table_name.clone(), schema.clone());
                                self.state = ParserState::Idle;
                                return Some(Ok(ParserEvent::TableCreated(schema)));
                            } else {
                                if let Some(col) = parse_column_line(trimmed) {
                                    columns.push(col);
                                }
                            }
                        }
                        ParserState::InCopy {
                            table_name,
                            columns,
                        } => {
                            if trimmed == "\\." {
                                let t_name = table_name.clone();
                                self.state = ParserState::Idle;
                                return Some(Ok(ParserEvent::CopyEnd { table_name: t_name }));
                            }

                            let fields = split_copy_line(trimmed);
                            if fields.len() != columns.len() {
                                warn!(
                                    "Malformed row at line {}: expected {} columns, got {} columns. Row content: {:?}",
                                    self.line_number,
                                    columns.len(),
                                    fields.len(),
                                    trimmed
                                );
                                self.skipped_lines_count += 1;
                                continue;
                            }

                            let mut parsed_values = Vec::with_capacity(fields.len());
                            let mut has_error = false;
                            for field in fields {
                                match decode_copy_field(&field) {
                                    Ok(val) => parsed_values.push(val),
                                    Err(err) => {
                                        warn!(
                                            "Malformed field encoding at line {}: {}. Field content: {:?}",
                                            self.line_number, err, field
                                        );
                                        has_error = true;
                                        break;
                                    }
                                }
                            }

                            if has_error {
                                self.skipped_lines_count += 1;
                                continue;
                            }

                            return Some(Ok(ParserEvent::CopyRow {
                                table_name: table_name.clone(),
                                values: parsed_values,
                            }));
                        }
                    }
                }
                Err(err) => {
                    return Some(Err(ParserError::Io {
                        line: self.line_number,
                        source: err,
                    }));
                }
            }
        }
    }

    fn handle_copy_start(&mut self, line: &str) -> Option<Result<ParserEvent, ParserError>> {
        // e.g. COPY users (id, name, is_active) FROM stdin;
        // or COPY users FROM stdin;
        let line_upper = line.to_uppercase();
        let from_idx = line_upper.find(" FROM ")?;
        let table_section = &line[5..from_idx].trim();

        let mut table_name = String::new();
        let mut columns = Vec::new();

        if let Some(open_paren) = table_section.find('(') {
            if let Some(close_paren) = table_section.rfind(')') {
                let name_part = &table_section[..open_paren];
                table_name = strip_quotes(name_part);
                let cols_part = &table_section[open_paren + 1..close_paren];
                columns = cols_part
                    .split(',')
                    .map(|c| strip_quotes(c.trim()))
                    .filter(|c| !c.is_empty())
                    .collect();
            }
        } else {
            table_name = strip_quotes(table_section);
        }

        if table_name.is_empty() {
            return Some(Err(ParserError::MalformedCopy {
                line: self.line_number,
                message: "Empty table name in COPY statement".into(),
            }));
        }

        if columns.is_empty() {
            if let Some(schema) = self.schemas.get(&table_name) {
                columns = schema.columns.iter().map(|c| c.name.clone()).collect();
            } else {
                return Some(Err(ParserError::MalformedCopy {
                    line: self.line_number,
                    message: format!(
                        "COPY for table {} but table schema is not defined",
                        table_name
                    ),
                }));
            }
        }

        self.state = ParserState::InCopy {
            table_name: table_name.clone(),
            columns: columns.clone(),
        };

        Some(Ok(ParserEvent::CopyStart {
            table_name,
            columns,
        }))
    }
}

fn trim_newline(s: &str) -> &str {
    s.strip_suffix('\n')
        .map(|s| s.strip_suffix('\r').unwrap_or(s))
        .unwrap_or(s)
}

fn strip_quotes(s: &str) -> String {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

fn parse_column_line(line: &str) -> Option<ColumnDef> {
    let line = line.trim();
    if line.is_empty() || line.starts_with("--") {
        return None;
    }

    let upper = line.to_uppercase();
    if upper.starts_with("CONSTRAINT")
        || upper.starts_with("PRIMARY KEY")
        || upper.starts_with("UNIQUE")
        || upper.starts_with("FOREIGN KEY")
        || upper.starts_with("KEY")
    {
        return None;
    }

    let (column_name, remaining) = if let Some(stripped) = line.strip_prefix('"') {
        if let Some(close_idx) = stripped.find('"') {
            (
                stripped[..close_idx].to_string(),
                stripped[close_idx + 1..].trim(),
            )
        } else {
            return None;
        }
    } else {
        if let Some(space_idx) = line.find(char::is_whitespace) {
            (line[..space_idx].to_string(), line[space_idx..].trim())
        } else {
            return None;
        }
    };

    if column_name.is_empty() {
        return None;
    }

    let mut db_type = String::new();
    let mut paren_count = 0;

    for c in remaining.chars() {
        if c == '(' {
            paren_count += 1;
            db_type.push(c);
        } else if c == ')' {
            paren_count -= 1;
            db_type.push(c);
            if paren_count == 0 {
                break;
            }
        } else if paren_count > 0 {
            db_type.push(c);
        } else if c == ',' || c.is_whitespace() {
            break;
        } else {
            db_type.push(c);
        }
    }

    if db_type.is_empty() {
        return None;
    }

    Some(ColumnDef {
        name: column_name,
        db_type,
    })
}

pub fn split_copy_line(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            current.push(c);
            if let Some(&next_c) = chars.peek() {
                current.push(next_c);
                chars.next();
            }
        } else if c == '\t' {
            fields.push(current);
            current = String::new();
        } else {
            current.push(c);
        }
    }
    fields.push(current);
    fields
}

pub fn decode_copy_field(raw: &str) -> Result<Option<String>, String> {
    if raw == "\\N" {
        return Ok(None);
    }

    let mut decoded = String::new();
    let mut chars = raw.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next_c) = chars.next() {
                match next_c {
                    't' => decoded.push('\t'),
                    'n' => decoded.push('\n'),
                    'r' => decoded.push('\r'),
                    'b' => decoded.push('\x08'),
                    'f' => decoded.push('\x0c'),
                    '\\' => decoded.push('\\'),
                    'x' => {
                        let mut hex_str = String::new();
                        for _ in 0..2 {
                            if let Some(&h) = chars.peek() {
                                if h.is_ascii_hexdigit() {
                                    hex_str.push(h);
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                        }
                        if hex_str.is_empty() {
                            return Err("Empty hex escape \\x".to_string());
                        }
                        let byte = u8::from_str_radix(&hex_str, 16)
                            .map_err(|e| format!("Invalid hex escape \\x{}: {}", hex_str, e))?;
                        decoded.push(byte as char);
                    }
                    '0'..='7' => {
                        let mut octal_str = String::new();
                        octal_str.push(next_c);
                        for _ in 0..2 {
                            if let Some(&o) = chars.peek() {
                                if ('0'..='7').contains(&o) {
                                    octal_str.push(o);
                                    chars.next();
                                } else {
                                    break;
                                }
                            }
                        }
                        let val = u32::from_str_radix(&octal_str, 8)
                            .map_err(|e| format!("Invalid octal escape \\{}: {}", octal_str, e))?;
                        if let Some(ch) = std::char::from_u32(val) {
                            decoded.push(ch);
                        } else {
                            return Err(format!(
                                "Invalid Unicode codepoint from octal escape \\{}",
                                octal_str
                            ));
                        }
                    }
                    other => {
                        decoded.push(other);
                    }
                }
            } else {
                return Err("Trailing backslash at end of field".to_string());
            }
        } else {
            decoded.push(c);
        }
    }

    Ok(Some(decoded))
}
