//! Editor text helpers shared by the native clients.
//!
//! These mirror `apps/desktop/ui/src/sqlSelection.ts`, the SQL-identifier
//! matching in `App.tsx`, and the CSV encoding in `TableView.tsx` so every
//! host runs the same statement, asks the same confirmation, and exports the
//! same bytes. Offsets are UTF-8 byte offsets into the editor text.

use serde::Serialize;
use serde_json::Value;

use crate::models::{DatabaseEngine, SchemaNode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionKind {
    Selection,
    Statement,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionTarget {
    pub from: usize,
    pub to: usize,
    pub sql: String,
    pub kind: ExecutionKind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    from: usize,
    to: usize,
}

/// The text Command/Ctrl+Enter runs: the trimmed selection when there is one,
/// otherwise the SQL statement (or, for Redis, the command line) at the cursor.
pub fn execution_target(
    engine: DatabaseEngine,
    text: &str,
    selection_from: usize,
    selection_to: usize,
) -> Option<ExecutionTarget> {
    let from = floor_char_boundary(text, selection_from.min(selection_to));
    let to = floor_char_boundary(text, selection_from.max(selection_to));
    let (range, kind) = if from != to {
        (
            trim_range(text, Range { from, to }),
            ExecutionKind::Selection,
        )
    } else if engine == DatabaseEngine::Redis {
        (
            range_at_cursor(text, from, &line_ranges(text), b'\n'),
            ExecutionKind::Statement,
        )
    } else {
        (
            range_at_cursor(text, from, &statement_ranges(text), b';'),
            ExecutionKind::Statement,
        )
    };
    let range = range?;
    Some(ExecutionTarget {
        from: range.from,
        to: range.to,
        sql: text[range.from..range.to].to_owned(),
        kind,
    })
}

fn floor_char_boundary(text: &str, index: usize) -> usize {
    let mut index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn range_at_cursor(text: &str, cursor: usize, ranges: &[Range], separator: u8) -> Option<Range> {
    let bytes = text.as_bytes();
    let mut index = ranges
        .iter()
        .position(|range| cursor < range.to)
        .unwrap_or(ranges.len() - 1);
    // A cursor right after a separator belongs to the statement it ends.
    if cursor > 0 && bytes[cursor - 1] == separator && index > 0 {
        index -= 1;
    }
    trim_range(text, ranges[index])
        .or_else(|| {
            ranges[index + 1..]
                .iter()
                .find_map(|r| trim_range(text, *r))
        })
        .or_else(|| {
            ranges[..index]
                .iter()
                .rev()
                .find_map(|r| trim_range(text, *r))
        })
}

fn statement_ranges(text: &str) -> Vec<Range> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    let mut index = 0;
    let mut single_quoted = false;
    let mut double_quoted = false;
    let mut line_comment = false;
    let mut block_depth = 0usize;
    let mut dollar_quote: Option<&str> = None;
    while index < bytes.len() {
        let character = bytes[index];
        let next = bytes.get(index + 1).copied();
        if line_comment {
            if character == b'\n' {
                line_comment = false;
            }
        } else if block_depth > 0 {
            if character == b'/' && next == Some(b'*') {
                block_depth += 1;
                index += 1;
            } else if character == b'*' && next == Some(b'/') {
                block_depth -= 1;
                index += 1;
            }
        } else if let Some(delimiter) = dollar_quote {
            if text[index..].starts_with(delimiter) {
                index += delimiter.len() - 1;
                dollar_quote = None;
            }
        } else if single_quoted {
            if character == b'\\' || (character == b'\'' && next == Some(b'\'')) {
                index += 1;
            } else if character == b'\'' {
                single_quoted = false;
            }
        } else if double_quoted {
            if character == b'"' && next == Some(b'"') {
                index += 1;
            } else if character == b'"' {
                double_quoted = false;
            }
        } else if character == b'-' && next == Some(b'-') {
            line_comment = true;
            index += 1;
        } else if character == b'/' && next == Some(b'*') {
            block_depth = 1;
            index += 1;
        } else if character == b'\'' {
            single_quoted = true;
        } else if character == b'"' {
            double_quoted = true;
        } else if character == b'$' {
            if let Some(delimiter) = dollar_quote_delimiter(text, index) {
                index += delimiter.len() - 1;
                dollar_quote = Some(delimiter);
            }
        } else if character == b';' {
            ranges.push(Range {
                from: start,
                to: index + 1,
            });
            start = index + 1;
        }
        index += 1;
    }
    ranges.push(Range {
        from: start,
        to: text.len(),
    });
    ranges
}

fn line_ranges(text: &str) -> Vec<Range> {
    let mut ranges = Vec::new();
    let mut start = 0;
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            ranges.push(Range {
                from: start,
                to: index,
            });
            start = index + 1;
        }
    }
    ranges.push(Range {
        from: start,
        to: text.len(),
    });
    ranges
}

fn dollar_quote_delimiter(text: &str, start: usize) -> Option<&str> {
    let end = start + 1 + text[start + 1..].find('$')?;
    let tag = &text[start + 1..end];
    let valid = tag.is_empty()
        || (tag.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
    valid.then_some(&text[start..=end])
}

fn trim_range(text: &str, range: Range) -> Option<Range> {
    let slice = &text[range.from..range.to];
    let from = range.from + (slice.len() - slice.trim_start().len());
    let to = range.to - (slice.len() - slice.trim_end().len());
    (from < to).then_some(Range { from, to })
}

/// Whether running `text` should ask first: it drops or truncates something,
/// or deletes or updates rows without a WHERE clause. Comments, string
/// literals, and quoted identifiers are ignored so `SELECT 'drop'` or a column
/// named "update" do not trigger the prompt. Redis asks before FLUSHALL/FLUSHDB.
pub fn requires_confirmation(engine: DatabaseEngine, text: &str) -> bool {
    if engine == DatabaseEngine::Redis {
        let lower = text.trim_start().to_ascii_lowercase();
        return starts_with_word(&lower, "flushall") || starts_with_word(&lower, "flushdb");
    }
    normalize_sql(text).split(';').any(|statement| {
        let lower = statement.to_ascii_lowercase();
        if has_word(&lower, "drop") || has_word(&lower, "truncate") {
            return true;
        }
        let deletes = starts_with_word(lower.trim_start(), "delete") || delete_from(&lower);
        (deletes || unfiltered_update(&lower)) && !has_word(&lower, "where")
    })
}

/// Replaces comments with spaces and quoted text with placeholders.
fn normalize_sql(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '-' if chars.peek() == Some(&'-') => {
                for next in chars.by_ref() {
                    if next == '\n' {
                        out.push('\n');
                        break;
                    }
                }
                out.push(' ');
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut previous = '\0';
                for next in chars.by_ref() {
                    if previous == '*' && next == '/' {
                        break;
                    }
                    previous = next;
                }
                out.push(' ');
            }
            '\'' | '"' | '`' => {
                // A doubled quote continues the literal, as in 'it''s'.
                loop {
                    match chars.next() {
                        Some(next) if next == character => {
                            if chars.peek() == Some(&character) {
                                chars.next();
                            } else {
                                break;
                            }
                        }
                        Some(_) => {}
                        None => break,
                    }
                }
                out.push_str(if character == '\'' { "''" } else { "x" });
            }
            _ => out.push(character),
        }
    }
    out
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

fn word_positions<'a>(text: &'a str, word: &'a str) -> impl Iterator<Item = usize> + 'a {
    text.match_indices(word).filter_map(move |(index, _)| {
        let before = text[..index].chars().next_back();
        let after = text[index + word.len()..].chars().next();
        (before.is_none_or(|c| !is_word(c)) && after.is_none_or(|c| !is_word(c))).then_some(index)
    })
}

fn starts_with_word(text: &str, word: &str) -> bool {
    text.starts_with(word)
        && text[word.len()..]
            .chars()
            .next()
            .is_none_or(|c| !is_word(c))
}

fn has_word(text: &str, word: &str) -> bool {
    word_positions(text, word).next().is_some()
}

fn delete_from(lower: &str) -> bool {
    word_positions(lower, "delete").any(|index| {
        let rest = &lower[index + "delete".len()..];
        let trimmed = rest.trim_start();
        trimmed.len() < rest.len() && starts_with_word(trimmed, "from")
    })
}

/// `(^|[\s(])update\s+(only\s+)?T(\s+(as\s+)?A)?\s+set\b` where T and A
/// contain no whitespace or parentheses.
fn unfiltered_update(lower: &str) -> bool {
    lower.match_indices("update").any(|(index, _)| {
        let before = lower[..index].chars().next_back();
        if before.is_some_and(|c| !c.is_whitespace() && c != '(') {
            return false;
        }
        let rest = &lower[index + "update".len()..];
        if !rest.starts_with(char::is_whitespace) {
            return false;
        }
        let words: Vec<&str> = rest.split_whitespace().take(5).collect();
        let name = |i: usize| words.get(i).is_some_and(|w| !w.contains('('));
        let set = |i: usize| words.get(i).is_some_and(|w| starts_with_word(w, "set"));
        let skips: &[usize] = if words.first() == Some(&"only") {
            &[0, 1]
        } else {
            &[0]
        };
        skips.iter().any(|&t| {
            name(t)
                && (set(t + 1)
                    || (name(t + 1) && set(t + 2))
                    || (words.get(t + 1) == Some(&"as") && name(t + 2) && set(t + 3)))
        })
    })
}

/// Resolves `SELECT * FROM [schema.]table` to exactly one table or Redis key in
/// the loaded schema tree, so the result can open in the editable table viewer.
pub fn resolve_full_table_select(text: &str, tree: &[SchemaNode]) -> Option<(String, String)> {
    let mut rest = text.trim();
    rest = rest.strip_suffix(';').unwrap_or(rest).trim_end();
    rest = strip_keyword(rest, "select")?;
    rest = rest.strip_prefix('*')?.trim_start();
    rest = strip_keyword(rest, "from")?;
    let (first, after) = take_identifier(rest)?;
    let after = after.trim_start();
    let (schema, table) = if let Some(after_dot) = after.strip_prefix('.') {
        let (second, after) = take_identifier(after_dot.trim_start())?;
        if !after.trim().is_empty() {
            return None;
        }
        (Some(first), second)
    } else if after.is_empty() {
        (None, first)
    } else {
        return None;
    };
    let mut matches = Vec::new();
    collect_tables(tree, schema.as_deref(), &table, &mut matches);
    (matches.len() == 1).then(|| matches.remove(0))
}

fn strip_keyword<'a>(text: &'a str, keyword: &str) -> Option<&'a str> {
    let head = text.get(..keyword.len())?;
    let rest = &text[keyword.len()..];
    (head.eq_ignore_ascii_case(keyword) && rest.starts_with(char::is_whitespace))
        .then(|| rest.trim_start())
}

/// Returns the decoded identifier and the remaining text.
fn take_identifier(text: &str) -> Option<(String, &str)> {
    for quote in ['"', '`'] {
        if let Some(body) = text.strip_prefix(quote) {
            let mut decoded = String::new();
            let mut chars = body.char_indices().peekable();
            while let Some((index, c)) = chars.next() {
                if c == quote {
                    if chars.peek().is_some_and(|(_, next)| *next == quote) {
                        chars.next();
                    } else {
                        return Some((decoded, &body[index + 1..]));
                    }
                }
                decoded.push(c);
            }
            return None;
        }
    }
    if !text.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_') {
        return None;
    }
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '$'))
        .unwrap_or(text.len());
    Some((text[..end].to_lowercase(), &text[end..]))
}

fn collect_tables(
    nodes: &[SchemaNode],
    schema: Option<&str>,
    table: &str,
    out: &mut Vec<(String, String)>,
) {
    for node in nodes {
        if (node.kind == "table" || node.kind == "key")
            && node.table.as_deref() == Some(table)
            && let Some(node_schema) = &node.schema
            && schema.is_none_or(|s| s == node_schema)
        {
            out.push((node_schema.clone(), table.to_owned()));
        }
        collect_tables(&node.children, schema, table, out);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TokenKind {
    Keyword,
    String,
    Number,
    Comment,
    QuotedIdentifier,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Token {
    pub from: usize,
    pub to: usize,
    pub kind: TokenKind,
}

const SQL_KEYWORDS: &[&str] = &[
    "add",
    "all",
    "alter",
    "and",
    "any",
    "as",
    "asc",
    "begin",
    "between",
    "by",
    "case",
    "cast",
    "check",
    "column",
    "commit",
    "constraint",
    "create",
    "cross",
    "database",
    "default",
    "delete",
    "desc",
    "distinct",
    "do",
    "drop",
    "else",
    "end",
    "exists",
    "explain",
    "false",
    "fetch",
    "for",
    "foreign",
    "from",
    "full",
    "function",
    "grant",
    "group",
    "having",
    "if",
    "ilike",
    "in",
    "index",
    "inner",
    "insert",
    "interval",
    "into",
    "is",
    "join",
    "key",
    "left",
    "like",
    "limit",
    "not",
    "null",
    "offset",
    "on",
    "or",
    "order",
    "outer",
    "over",
    "partition",
    "primary",
    "references",
    "returning",
    "revoke",
    "right",
    "rollback",
    "schema",
    "select",
    "set",
    "show",
    "table",
    "then",
    "to",
    "true",
    "truncate",
    "union",
    "unique",
    "update",
    "use",
    "using",
    "values",
    "view",
    "when",
    "where",
    "window",
    "with",
];

/// Syntax tokens for editor highlighting. SQL gets keywords, literals,
/// comments, and quoted identifiers; Redis highlights the command word of each
/// line and quoted arguments. Unlisted text keeps the default color.
pub fn highlight(engine: DatabaseEngine, text: &str) -> Vec<Token> {
    let bytes = text.as_bytes();
    let mut tokens = Vec::new();
    let mut index = 0;
    let mut line_start = true;
    while index < bytes.len() {
        let character = bytes[index];
        let next = bytes.get(index + 1).copied();
        let start = index;
        let push = |tokens: &mut Vec<Token>, to: usize, kind| {
            tokens.push(Token {
                from: start,
                to,
                kind,
            });
        };
        if character == b'\n' {
            line_start = true;
            index += 1;
            continue;
        }
        if character.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        let redis = engine == DatabaseEngine::Redis;
        if !redis && character == b'-' && next == Some(b'-') {
            index = text[index..]
                .find('\n')
                .map_or(text.len(), |end| index + end);
            push(&mut tokens, index, TokenKind::Comment);
        } else if !redis && character == b'/' && next == Some(b'*') {
            index = text[index + 2..]
                .find("*/")
                .map_or(text.len(), |end| index + end + 4);
            push(&mut tokens, index, TokenKind::Comment);
        } else if !redis && character == b'$' && dollar_quote_delimiter(text, index).is_some() {
            let delimiter = dollar_quote_delimiter(text, index).unwrap_or("$$");
            let body = index + delimiter.len();
            index = text[body..]
                .find(delimiter)
                .map_or(text.len(), |end| body + end + delimiter.len());
            push(&mut tokens, index, TokenKind::String);
        } else if matches!(character, b'\'' | b'"' | b'`') {
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' && redis {
                    index += 2;
                    continue;
                }
                if bytes[index] == character {
                    if bytes.get(index + 1) == Some(&character) && !redis {
                        index += 2;
                        continue;
                    }
                    index += 1;
                    break;
                }
                index += 1;
            }
            index = index.min(bytes.len());
            let kind = if character == b'\'' || redis {
                TokenKind::String
            } else {
                TokenKind::QuotedIdentifier
            };
            push(&mut tokens, index, kind);
        } else if character.is_ascii_alphabetic() || character == b'_' {
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || matches!(bytes[index], b'_' | b'$'))
            {
                index += 1;
            }
            let word = text[start..index].to_ascii_lowercase();
            let keyword = if redis {
                line_start
            } else {
                SQL_KEYWORDS.binary_search(&word.as_str()).is_ok()
            };
            if keyword {
                push(&mut tokens, index, TokenKind::Keyword);
            }
        } else if character.is_ascii_digit() && !redis {
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
                index += 1;
            }
            push(&mut tokens, index, TokenKind::Number);
        } else {
            index += text[index..].chars().next().map_or(1, char::len_utf8);
        }
        line_start = false;
    }
    tokens
}

/// Keeps nodes whose name contains `query` (case-insensitive) plus the
/// branches that lead to them. A matching branch keeps all of its children.
pub fn filter_schema_nodes(nodes: &[SchemaNode], query: &str) -> Vec<SchemaNode> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return nodes.to_vec();
    }
    fn visit(nodes: &[SchemaNode], query: &str) -> Vec<SchemaNode> {
        nodes
            .iter()
            .filter_map(|node| {
                if node.name.to_lowercase().contains(query) {
                    return Some(node.clone());
                }
                let children = visit(&node.children, query);
                (!children.is_empty()).then(|| SchemaNode {
                    children,
                    ..node.clone()
                })
            })
            .collect()
    }
    visit(nodes, &query)
}

/// Converts a UTF-16 code unit offset (as `NSString`/`NSRange` use) to a
/// byte offset, snapping to the enclosing character.
pub fn utf16_to_byte(text: &str, offset: usize) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units >= offset {
            return byte;
        }
        units += character.len_utf16();
        if units > offset {
            return byte;
        }
    }
    text.len()
}

/// Converts a byte offset to UTF-16 code units.
pub fn byte_to_utf16(text: &str, offset: usize) -> usize {
    text[..floor_char_boundary(text, offset)]
        .encode_utf16()
        .count()
}

/// The notice after refreshing a schema or keyspace tree, as the desktop app
/// words it. Returns whether anything changed and the message.
pub fn describe_schema_refresh(
    previous: &[SchemaNode],
    next: &[SchemaNode],
    kind: &str,
) -> (bool, String) {
    fn objects(nodes: &[SchemaNode], out: &mut Vec<(String, String)>) {
        for node in nodes {
            let name = match (&node.schema, &node.table) {
                (Some(schema), Some(table)) => format!("{schema}.{table}"),
                _ => node.name.clone(),
            };
            out.push((
                format!("{}:{name}", node.kind),
                format!("{} {name}", node.kind),
            ));
            objects(&node.children, out);
        }
    }
    let (mut before, mut after) = (Vec::new(), Vec::new());
    objects(previous, &mut before);
    objects(next, &mut after);
    before.sort();
    after.sort();
    let keys = |list: &[(String, String)]| {
        list.iter()
            .map(|(key, _)| key.clone())
            .collect::<std::collections::HashSet<_>>()
    };
    let (before_keys, after_keys) = (keys(&before), keys(&after));
    let added: Vec<&str> = after
        .iter()
        .filter(|(k, _)| !before_keys.contains(k))
        .map(|(_, l)| l.as_str())
        .collect();
    let removed: Vec<&str> = before
        .iter()
        .filter(|(k, _)| !after_keys.contains(k))
        .map(|(_, l)| l.as_str())
        .collect();
    if added.is_empty() && removed.is_empty() {
        return (false, format!("{kind} is already up to date."));
    }
    let summarize = |labels: &[&str]| {
        if labels.len() == 1 {
            return labels[0].to_owned();
        }
        let visible = labels[..labels.len().min(3)].join(", ");
        let rest = labels.len().saturating_sub(3);
        let more = if rest > 0 {
            format!(", and {rest} more")
        } else {
            String::new()
        };
        format!("{} objects: {visible}{more}", labels.len())
    };
    let mut changes = Vec::new();
    if !added.is_empty() {
        changes.push(format!("Added {}", summarize(&added)));
    }
    if !removed.is_empty() {
        changes.push(format!("Removed {}", summarize(&removed)));
    }
    (true, format!("{kind} refreshed · {}.", changes.join(" · ")))
}

/// Grid and CSV text for a value: `NULL`, raw strings, JSON for everything else.
pub fn display_value(value: &Value) -> String {
    match value {
        Value::Null => "NULL".into(),
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

/// One CSV record without a line terminator, quoted only where needed.
pub fn csv_line<'a>(values: impl IntoIterator<Item = &'a Value>) -> String {
    values
        .into_iter()
        .map(|value| {
            let text = display_value(value);
            if text.contains(['"', ',', '\r', '\n']) {
                format!("\"{}\"", text.replace('"', "\"\""))
            } else {
                text
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// A header plus rows, newline-separated, as the desktop app copies them.
pub fn csv_document(columns: &[String], rows: &[Vec<Value>]) -> String {
    let header: Vec<Value> = columns.iter().cloned().map(Value::String).collect();
    std::iter::once(csv_line(&header))
        .chain(
            rows.iter()
                .map(|row| csv_line(row.iter().take(columns.len()))),
        )
        .collect::<Vec<_>>()
        .join("\n")
}

/// A file name with characters that Windows, macOS, or Linux reject replaced.
pub fn safe_file_name(value: &str) -> String {
    value
        .chars()
        .map(|c| if "\\/:*?\"<>|".contains(c) { '_' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    const PG: DatabaseEngine = DatabaseEngine::Postgres;

    fn at_cursor(text: &str, cursor: usize) -> String {
        execution_target(PG, text, cursor, cursor).map_or_else(String::new, |t| t.sql)
    }

    #[test]
    fn runs_selection_exactly() {
        let sql = "SELECT now();\n\nSELECT 1;";
        assert_eq!(
            execution_target(PG, sql, sql.len(), 15),
            Some(ExecutionTarget {
                from: 15,
                to: sql.len(),
                sql: "SELECT 1;".into(),
                kind: ExecutionKind::Selection,
            })
        );
    }

    #[test]
    fn runs_statement_at_cursor() {
        let sql = "SELECT now();\n\nSELECT 1;";
        assert_eq!(at_cursor(sql, sql.find('1').unwrap()), "SELECT 1;");
        assert_eq!(at_cursor(sql, sql.find(';').unwrap() + 1), "SELECT now();");
        let target = execution_target(PG, sql, sql.find('1').unwrap(), sql.find('1').unwrap());
        assert_eq!(target.unwrap().kind, ExecutionKind::Statement);
        assert_eq!(at_cursor("  \n ", 1), "");
        assert_eq!(at_cursor("SELECT 1;\n\n", 11), "SELECT 1;");
    }

    #[test]
    fn ignores_semicolons_in_strings_identifiers_comments_and_dollar_quotes() {
        let sql = [
            "SELECT ';' AS \"semi;colon\";",
            "-- comment ;",
            "SELECT $$body;still body$$;",
            "/* outer ; /* nested ; */ done */ SELECT 3;",
        ]
        .join("\n");
        assert!(
            at_cursor(&sql, sql.find("body;still").unwrap())
                .contains("SELECT $$body;still body$$;")
        );
        assert!(at_cursor(&sql, sql.rfind('3').unwrap()).contains("SELECT 3;"));
        assert_eq!(at_cursor(&sql, 3), "SELECT ';' AS \"semi;colon\";");
    }

    #[test]
    fn non_ascii_text_keeps_char_boundaries() {
        let sql = "SELECT 'é';\nSELECT 'ü';";
        assert_eq!(at_cursor(sql, sql.len()), "SELECT 'ü';");
        // An offset inside a multibyte character snaps to its start.
        assert_eq!(at_cursor(sql, 9), "SELECT 'é';");
    }

    #[test]
    fn redis_runs_the_current_line() {
        let text = "PING\n\nGET greeting";
        let redis = DatabaseEngine::Redis;
        assert_eq!(execution_target(redis, text, 0, 0).unwrap().sql, "PING");
        let get = text.find("GET").unwrap();
        let target = execution_target(redis, text, get, get).unwrap();
        assert_eq!((target.from, target.to), (6, text.len()));
        assert_eq!(target.sql, "GET greeting");
    }

    #[test]
    fn asks_before_drops_truncates_and_unfiltered_writes() {
        for sql in [
            "DROP TABLE users",
            "truncate orders",
            "DELETE FROM users",
            "UPDATE users SET active = false",
            "UPDATE users u SET active = false",
            "UPDATE ONLY users AS u SET active = false",
            "WITH gone AS (DELETE FROM users RETURNING id) SELECT * FROM gone",
            "SELECT 1; DELETE FROM users",
        ] {
            assert!(requires_confirmation(PG, sql), "{sql}");
        }
        assert!(requires_confirmation(DatabaseEngine::Redis, "FLUSHALL"));
        assert!(requires_confirmation(
            DatabaseEngine::Redis,
            "  flushdb async"
        ));
    }

    #[test]
    fn does_not_ask_for_filtered_writes_or_keywords_in_text() {
        for sql in [
            "DELETE FROM users WHERE id = 1",
            "UPDATE users SET active = false WHERE id = 1",
            "SELECT * FROM jobs FOR UPDATE",
            "SELECT 'drop table users' AS note",
            "SELECT 'it''s; drop' AS note",
            "SELECT \"update\", \"delete\" FROM audit",
            "-- drop later\nSELECT 1",
            "CREATE TABLE a (b int REFERENCES c ON DELETE CASCADE ON UPDATE SET NULL)",
            "INSERT INTO t VALUES (1) ON CONFLICT (id) DO UPDATE SET v = 2",
            "SELECT dropped FROM t",
        ] {
            assert!(!requires_confirmation(PG, sql), "{sql}");
        }
        assert!(!requires_confirmation(DatabaseEngine::Redis, "GET drop"));
        assert!(!requires_confirmation(DatabaseEngine::Redis, "FLUSHALLX"));
    }

    fn tree() -> Vec<SchemaNode> {
        let table = |schema: &str, name: &str| SchemaNode {
            name: name.into(),
            kind: "table".into(),
            schema: Some(schema.into()),
            table: Some(name.into()),
            children: vec![],
        };
        let schema = |name: &str, children| SchemaNode {
            name: name.into(),
            kind: "schema".into(),
            schema: Some(name.into()),
            table: None,
            children,
        };
        vec![
            schema(
                "public",
                vec![table("public", "users"), table("public", "Orders")],
            ),
            schema("audit", vec![table("audit", "users")]),
        ]
    }

    #[test]
    fn resolves_simple_full_table_selects() {
        let tree = tree();
        let resolve = |sql| resolve_full_table_select(sql, &tree);
        let users = Some(("public".to_owned(), "users".to_owned()));
        assert_eq!(resolve("select * from public.users;"), users);
        assert_eq!(resolve("SELECT *\nFROM \"public\" . \"users\""), users);
        assert_eq!(resolve("SELECT * FROM users"), None, "ambiguous");
        assert_eq!(
            resolve("SELECT * FROM \"Orders\""),
            Some(("public".into(), "Orders".into()))
        );
        assert_eq!(
            resolve("SELECT * FROM Orders"),
            None,
            "unquoted names fold to lowercase"
        );
        assert_eq!(resolve("SELECT * FROM public.users WHERE id = 1"), None);
        assert_eq!(resolve("SELECT id FROM public.users"), None);
    }

    #[test]
    fn csv_quotes_only_when_needed() {
        let columns = vec!["id".to_owned(), "note".to_owned()];
        let rows = vec![
            vec![json!(1), json!("plain"), json!("xmin")],
            vec![json!(2), json!("a, \"b\"\nc")],
            vec![Value::Null, json!({"k": [1]})],
        ];
        assert_eq!(
            csv_document(&columns, &rows),
            "id,note\n1,plain\n2,\"a, \"\"b\"\"\nc\"\nNULL,\"{\"\"k\"\":[1]}\""
        );
        assert_eq!(safe_file_name("public/a:b"), "public_a_b");
    }

    #[test]
    fn highlights_sql_and_redis_tokens() {
        assert!(SQL_KEYWORDS.windows(2).all(|pair| pair[0] < pair[1]));
        let sql = "SELECT 'a''b', \"Id\" -- note\nFROM t WHERE n > 1.5 /* c */ AND $$x$$";
        let kinds: Vec<_> = highlight(PG, sql)
            .into_iter()
            .map(|t| (&sql[t.from..t.to], t.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("SELECT", TokenKind::Keyword),
                ("'a''b'", TokenKind::String),
                ("\"Id\"", TokenKind::QuotedIdentifier),
                ("-- note", TokenKind::Comment),
                ("FROM", TokenKind::Keyword),
                ("WHERE", TokenKind::Keyword),
                ("1.5", TokenKind::Number),
                ("/* c */", TokenKind::Comment),
                ("AND", TokenKind::Keyword),
                ("$$x$$", TokenKind::String),
            ]
        );
        let redis = "SET greeting \"hi\"\nget greeting";
        let kinds: Vec<_> = highlight(DatabaseEngine::Redis, redis)
            .into_iter()
            .map(|t| (&redis[t.from..t.to], t.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("SET", TokenKind::Keyword),
                ("\"hi\"", TokenKind::String),
                ("get", TokenKind::Keyword),
            ]
        );
        // Unterminated literals end at the text without panicking.
        assert_eq!(
            highlight(PG, "SELECT 'é").last().unwrap().to,
            "SELECT 'é".len()
        );
    }

    #[test]
    fn schema_filter_keeps_matching_branches() {
        let filtered = filter_schema_nodes(&tree(), "USER");
        assert_eq!(filtered.len(), 2);
        assert_eq!(filtered[0].children.len(), 1);
        assert_eq!(filtered[0].children[0].name, "users");
        assert_eq!(filter_schema_nodes(&tree(), "audit")[0].children.len(), 1);
        assert!(filter_schema_nodes(&tree(), "nothing").is_empty());
        assert_eq!(filter_schema_nodes(&tree(), "  ").len(), 2);
    }

    #[test]
    fn utf16_offsets_round_trip_through_bytes() {
        let text = "a🚀é;b";
        assert_eq!(utf16_to_byte(text, 0), 0);
        assert_eq!(utf16_to_byte(text, 1), 1);
        assert_eq!(
            utf16_to_byte(text, 2),
            1,
            "inside a surrogate pair snaps back"
        );
        assert_eq!(utf16_to_byte(text, 3), 5);
        assert_eq!(utf16_to_byte(text, 4), 7);
        assert_eq!(utf16_to_byte(text, 99), text.len());
        assert_eq!(byte_to_utf16(text, 5), 3);
        assert_eq!(byte_to_utf16(text, text.len()), 6);
    }

    #[test]
    fn schema_refresh_messages_match_the_desktop_app() {
        let before = tree();
        assert_eq!(
            describe_schema_refresh(&before, &before, "Schema"),
            (false, "Schema is already up to date.".to_owned())
        );
        let mut after = tree();
        after[0].children.pop();
        after.remove(1);
        let (changed, message) = describe_schema_refresh(&before, &after, "Keyspace");
        assert!(changed);
        assert_eq!(
            message,
            "Keyspace refreshed · Removed 3 objects: schema audit, table audit.users, table public.Orders."
        );
    }
}
