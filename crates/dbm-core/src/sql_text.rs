//! Editor text helpers shared by the native clients.
//!
//! Every host uses them, so each one runs the same statement, asks the same
//! confirmation, and exports the same bytes. Offsets are UTF-8 byte offsets into the editor text.

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
            range_at_cursor(text, from, &statement_ranges(engine, text), b';'),
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

fn statement_ranges(engine: DatabaseEngine, text: &str) -> Vec<Range> {
    let bytes = text.as_bytes();
    let mut ranges = Vec::new();
    let mut start = 0;
    for (kind, range) in sql_segments(engine, text) {
        if kind != Segment::Code {
            continue;
        }
        let semicolons = bytes[range.from..range.to]
            .iter()
            .enumerate()
            .filter(|(_, byte)| **byte == b';');
        for (offset, _) in semicolons {
            let end = range.from + offset + 1;
            ranges.push(Range {
                from: start,
                to: end,
            });
            start = end;
        }
    }
    ranges.push(Range {
        from: start,
        to: text.len(),
    });
    ranges
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Segment {
    Code,
    Comment,
    /// A string literal, including PostgreSQL dollar-quoted bodies.
    Literal,
    QuotedIdentifier,
}

/// Splits SQL into code, comments, string literals, and quoted identifiers
/// using the engine's lexical rules:
///
/// - PostgreSQL: `'...'` strings only treat a doubled quote as an escape;
///   backslash escapes apply only in `E'...'` strings. `"..."` is an
///   identifier, `$tag$...$tag$` is a dollar-quoted string unless the `$`
///   continues an identifier (`a$tag$`), and block comments nest.
/// - MySQL: `'...'` and `"..."` strings accept backslash escapes (the default
///   `sql_mode`), backticks quote identifiers, `#` and `-- ` start line
///   comments, block comments do not nest, and the body of a `/*!...*/`
///   executable comment is code because the server runs it.
///
/// Unterminated quotes and comments run to the end of the text.
fn sql_segments(engine: DatabaseEngine, text: &str) -> Vec<(Segment, Range)> {
    sql_segments_with(engine, text, BackslashRule::ServerDefault)
}

/// How backslashes behave inside ordinary quoted strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BackslashRule {
    /// PostgreSQL `standard_conforming_strings = on`; MySQL's default `sql_mode`.
    ServerDefault,
    /// The opposite: PostgreSQL `standard_conforming_strings = off`, or MySQL
    /// `NO_BACKSLASH_ESCAPES` (which also covers `ANSI_QUOTES` identifiers).
    NonDefault,
}

fn sql_segments_with(
    engine: DatabaseEngine,
    text: &str,
    rule: BackslashRule,
) -> Vec<(Segment, Range)> {
    let mysql = engine == DatabaseEngine::Mysql;
    // Whether backslash escapes apply in plain '...' strings (and MySQL "...").
    let backslash = mysql == (rule == BackslashRule::ServerDefault);
    let bytes = text.as_bytes();
    let mut segments = Vec::new();
    let mut code_start = 0;
    let mut index = 0;
    let mut executable_comment = false;
    while index < bytes.len() {
        let character = bytes[index];
        let next = bytes.get(index + 1).copied();
        let segment = if executable_comment && character == b'*' && next == Some(b'/') {
            executable_comment = false;
            Some((Segment::Comment, index + 2))
        } else if (character == b'-'
            && next == Some(b'-')
            && (!mysql
                || bytes
                    .get(index + 2)
                    .is_none_or(|b| b.is_ascii_whitespace() || b.is_ascii_control())))
            || (mysql && character == b'#')
        {
            let end = bytes[index..]
                .iter()
                .position(|&b| b == b'\n')
                .map_or(bytes.len(), |offset| index + offset);
            Some((Segment::Comment, end))
        } else if character == b'/' && next == Some(b'*') {
            if mysql && bytes.get(index + 2) == Some(&b'!') {
                executable_comment = true;
                let digits = bytes[index + 3..]
                    .iter()
                    .take_while(|b| b.is_ascii_digit())
                    .count();
                Some((Segment::Comment, index + 3 + digits))
            } else {
                Some((Segment::Comment, block_comment_end(bytes, index, !mysql)))
            }
        } else if character == b'\'' {
            let escapes = backslash || (!mysql && postgres_escape_string(bytes, index));
            Some((Segment::Literal, quoted_end(bytes, index, escapes)))
        } else if character == b'"' {
            let kind = if mysql {
                Segment::Literal
            } else {
                Segment::QuotedIdentifier
            };
            Some((kind, quoted_end(bytes, index, mysql && backslash)))
        } else if mysql && character == b'`' {
            Some((Segment::QuotedIdentifier, quoted_end(bytes, index, false)))
        } else if !mysql && character == b'$' && (index == 0 || !identifier_byte(bytes[index - 1]))
        {
            dollar_quote_delimiter(text, index).map(|delimiter| {
                let body = index + delimiter.len();
                let end = text[body..]
                    .find(delimiter)
                    .map_or(text.len(), |offset| body + offset + delimiter.len());
                (Segment::Literal, end)
            })
        } else {
            None
        };
        match segment {
            Some((kind, end)) => {
                if code_start < index {
                    segments.push((
                        Segment::Code,
                        Range {
                            from: code_start,
                            to: index,
                        },
                    ));
                }
                segments.push((
                    kind,
                    Range {
                        from: index,
                        to: end,
                    },
                ));
                index = end;
                code_start = end;
            }
            None => index += 1,
        }
    }
    if code_start < bytes.len() {
        segments.push((
            Segment::Code,
            Range {
                from: code_start,
                to: bytes.len(),
            },
        ));
    }
    segments
}

/// Bytes that can continue an unquoted identifier (PostgreSQL also allows `$`).
fn identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$' || byte >= 0x80
}

/// Whether the quote at `quote` opens a PostgreSQL `E'...'` escape string.
fn postgres_escape_string(bytes: &[u8], quote: usize) -> bool {
    quote > 0
        && matches!(bytes[quote - 1], b'e' | b'E')
        && (quote == 1 || !identifier_byte(bytes[quote - 2]))
}

/// The end (exclusive) of the quoted text opening at `start`. A doubled quote
/// continues it; with `escapes`, so does a backslash-escaped character.
fn quoted_end(bytes: &[u8], start: usize, escapes: bool) -> usize {
    let quote = bytes[start];
    let mut index = start + 1;
    while index < bytes.len() {
        if escapes && bytes[index] == b'\\' {
            index += 2;
        } else if bytes[index] == quote {
            if bytes.get(index + 1) == Some(&quote) {
                index += 2;
            } else {
                return index + 1;
            }
        } else {
            index += 1;
        }
    }
    bytes.len()
}

fn block_comment_end(bytes: &[u8], start: usize, nested: bool) -> usize {
    let mut depth = 1usize;
    let mut index = start + 2;
    while index < bytes.len() {
        let next = bytes.get(index + 1).copied();
        if nested && bytes[index] == b'/' && next == Some(b'*') {
            depth += 1;
            index += 2;
        } else if bytes[index] == b'*' && next == Some(b'/') {
            depth -= 1;
            index += 2;
            if depth == 0 {
                return index;
            }
        } else {
            index += 1;
        }
    }
    bytes.len()
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
    // PostgreSQL tags are identifier-like: letters, `_`, or any non-ASCII
    // character, then also digits.
    let valid = tag.is_empty()
        || (tag.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_' || !c.is_ascii())
            && tag
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || !c.is_ascii()));
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
/// named "update" do not trigger the prompt, using the engine's quoting and
/// comment rules. A DELETE or UPDATE counts as filtered only when its own WHERE
/// follows at the same parenthesis level, so a WHERE inside a subquery does not
/// count. The text is checked under both backslash rules because the server's
/// setting (`standard_conforming_strings`, `NO_BACKSLASH_ESCAPES`) is unknown
/// here; a backslash must not be able to hide a statement either way. Redis
/// asks before FLUSHALL/FLUSHDB.
pub fn requires_confirmation(engine: DatabaseEngine, text: &str) -> bool {
    if engine == DatabaseEngine::Redis {
        let lower = text.trim_start().to_ascii_lowercase();
        return starts_with_word(&lower, "flushall") || starts_with_word(&lower, "flushdb");
    }
    let asks = |rule| normalized_requires_confirmation(&normalize_sql(engine, text, rule));
    asks(BackslashRule::ServerDefault) || (text.contains('\\') && asks(BackslashRule::NonDefault))
}

fn normalized_requires_confirmation(normalized: &str) -> bool {
    normalized.split(';').any(|statement| {
        let lower = statement.to_ascii_lowercase();
        if has_word(&lower, "drop") || has_word(&lower, "truncate") {
            return true;
        }
        let leading_delete = starts_with_word(lower.trim_start(), "delete")
            .then(|| lower.len() - lower.trim_start().len());
        leading_delete
            .into_iter()
            .chain(delete_from_positions(&lower))
            .chain(update_set_positions(&lower))
            .any(|write| !has_own_where(&lower, write))
    })
}

/// Replaces comments with spaces and quoted text with space-padded
/// placeholders, so `DELETE FROM"users"` still reads as `delete from x`.
fn normalize_sql(engine: DatabaseEngine, text: &str, rule: BackslashRule) -> String {
    let mut out = String::with_capacity(text.len());
    for (kind, range) in sql_segments_with(engine, text, rule) {
        out.push_str(match kind {
            Segment::Code => &text[range.from..range.to],
            Segment::Comment => " ",
            Segment::Literal => " '' ",
            Segment::QuotedIdentifier => " x ",
        });
    }
    out
}

/// Whether a WHERE follows the write starting at `write` at the same
/// parenthesis depth, before the group containing the write closes.
fn has_own_where(lower: &str, write: usize) -> bool {
    let bytes = lower.as_bytes();
    let mut depth = 0usize;
    for index in write..bytes.len() {
        match bytes[index] {
            b'(' => depth += 1,
            b')' if depth == 0 => return false,
            b')' => depth -= 1,
            b'w' if depth == 0
                && bytes[index..].starts_with(b"where")
                && !identifier_byte(bytes[index - 1])
                && bytes.get(index + 5).is_none_or(|&b| !identifier_byte(b)) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
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

fn delete_from_positions(lower: &str) -> impl Iterator<Item = usize> + '_ {
    word_positions(lower, "delete").filter(|&index| {
        let rest = &lower[index + "delete".len()..];
        let trimmed = rest.trim_start();
        trimmed.len() < rest.len() && starts_with_word(trimmed, "from")
    })
}

/// Starts of `(^|[\s(])update\s+(only\s+)?T(\s+(as\s+)?A)?\s+set\b` where T
/// and A contain no whitespace or parentheses.
fn update_set_positions(lower: &str) -> impl Iterator<Item = usize> + '_ {
    lower
        .match_indices("update")
        .map(|(index, _)| index)
        .filter(|&index| {
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

/// The text to save in query history, or `None` to keep the run out of history.
///
/// Passwords must never reach the app's SQLite file. SQL that mentions a
/// password or `IDENTIFIED` (`ALTER ROLE ... PASSWORD '...'`,
/// `CREATE USER ... IDENTIFIED BY '...'`, `SET PASSWORD`, replication or
/// connection-string options) keeps its shape with every string literal
/// replaced by `'***'`; when a backslash makes the literal boundaries depend on
/// server settings, it is not saved at all. Redis commands that carry
/// credentials (`AUTH`, `HELLO ... AUTH`, `ACL SETUSER` password rules,
/// `CONFIG SET requirepass`/`masterauth`, `MIGRATE ... AUTH`, Sentinel
/// passwords) are not saved.
pub fn history_text(engine: DatabaseEngine, text: &str) -> Option<String> {
    if engine == DatabaseEngine::Redis {
        return (!redis_carries_credentials(text)).then(|| text.to_owned());
    }
    let lower = text.to_ascii_lowercase();
    if !["password", "passwd", "identified"]
        .iter()
        .any(|word| lower.contains(word))
    {
        return Some(text.to_owned());
    }
    if text.contains('\\') {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    for (kind, range) in sql_segments(engine, text) {
        if kind == Segment::Literal {
            out.push_str("'***'");
        } else {
            out.push_str(&text[range.from..range.to]);
        }
    }
    Some(out)
}

/// Whether a Redis workbench command sends a password or password hash.
/// Quotes and backslashes are dropped before matching because the workbench
/// tokenizer strips them, so `"AU"TH secret` still runs AUTH.
fn redis_carries_credentials(text: &str) -> bool {
    let plain: String = text
        .chars()
        .filter(|c| !matches!(c, '"' | '\'' | '\\'))
        .collect::<String>()
        .to_ascii_lowercase();
    let words: Vec<&str> = plain.split_whitespace().collect();
    let Some((&command, args)) = words.split_first() else {
        return false;
    };
    let sub = args.first().copied().unwrap_or("");
    let secret_setting = |word: &&str| word.contains("pass") || word.contains("auth");
    match command {
        "auth" => true,
        "hello" | "migrate" => args.iter().any(|word| word.starts_with("auth")),
        "acl" => {
            sub == "setuser"
                && args
                    .iter()
                    .any(|word| word.starts_with(['>', '<', '#', '!']))
        }
        "config" => sub == "set" && args.iter().any(secret_setting),
        "sentinel" => args.iter().any(secret_setting),
        _ => false,
    }
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

/// Redis commands the editor completes, as in the desktop app's Redis dialect.
const REDIS_COMMANDS: &[&str] = &[
    "acl",
    "append",
    "asking",
    "auth",
    "bgsave",
    "bitcount",
    "bitop",
    "bitpos",
    "blmove",
    "blmpop",
    "blpop",
    "brpop",
    "brpoplpush",
    "bzmpop",
    "bzpopmax",
    "bzpopmin",
    "client",
    "cluster",
    "command",
    "config",
    "copy",
    "dbsize",
    "decr",
    "decrby",
    "del",
    "discard",
    "dump",
    "echo",
    "eval",
    "evalsha",
    "exec",
    "exists",
    "expire",
    "expireat",
    "expiretime",
    "flushall",
    "flushdb",
    "geoadd",
    "geodist",
    "geohash",
    "geopos",
    "georadius",
    "georadiusbymember",
    "geosearch",
    "get",
    "getdel",
    "getex",
    "getrange",
    "getset",
    "hdel",
    "hello",
    "hexists",
    "hget",
    "hgetall",
    "hincrby",
    "hincrbyfloat",
    "hkeys",
    "hlen",
    "hmget",
    "hmset",
    "hrandfield",
    "hscan",
    "hset",
    "hsetnx",
    "hstrlen",
    "hvals",
    "incr",
    "incrby",
    "incrbyfloat",
    "info",
    "keys",
    "lastsave",
    "lcs",
    "lindex",
    "linsert",
    "llen",
    "lmove",
    "lmpop",
    "lpop",
    "lpos",
    "lpush",
    "lpushx",
    "lrange",
    "lrem",
    "lset",
    "ltrim",
    "memory",
    "mget",
    "migrate",
    "module",
    "move",
    "mset",
    "msetnx",
    "multi",
    "object",
    "persist",
    "pexpire",
    "pexpireat",
    "pfadd",
    "pfcount",
    "pfmerge",
    "ping",
    "psetex",
    "psubscribe",
    "pttl",
    "publish",
    "pubsub",
    "punsubscribe",
    "randomkey",
    "readonly",
    "readwrite",
    "rename",
    "renamenx",
    "replicaof",
    "restore",
    "role",
    "rpop",
    "rpoplpush",
    "rpush",
    "rpushx",
    "sadd",
    "save",
    "scan",
    "scard",
    "sdiff",
    "sdiffstore",
    "select",
    "set",
    "setex",
    "setnx",
    "setrange",
    "sinter",
    "sinterstore",
    "sismember",
    "slowlog",
    "smembers",
    "smismember",
    "smove",
    "sort",
    "spop",
    "srandmember",
    "srem",
    "sscan",
    "strlen",
    "substr",
    "sunion",
    "sunionstore",
    "swapdb",
    "time",
    "touch",
    "ttl",
    "type",
    "unlink",
    "unsubscribe",
    "unwatch",
    "wait",
    "watch",
    "xack",
    "xadd",
    "xautoclaim",
    "xclaim",
    "xdel",
    "xgroup",
    "xinfo",
    "xlen",
    "xpending",
    "xrange",
    "xread",
    "xreadgroup",
    "xrevrange",
    "xtrim",
    "zadd",
    "zcard",
    "zcount",
    "zdiff",
    "zdiffstore",
    "zincrby",
    "zinter",
    "zintercard",
    "zinterstore",
    "zlexcount",
    "zmpop",
    "zmscore",
    "zpopmax",
    "zpopmin",
    "zrandmember",
    "zrange",
    "zrangebylex",
    "zrangebyscore",
    "zrangestore",
    "zrank",
    "zrem",
    "zremrangebylex",
    "zremrangebyrank",
    "zremrangebyscore",
    "zrevrange",
    "zrevrangebylex",
    "zrevrangebyscore",
    "zrevrank",
    "zscan",
    "zscore",
    "zunion",
    "zunionstore",
];

/// Keyword or command completions for the identifier prefix before the
/// cursor, in the prefix's case (lowercase stays lowercase). The prefix
/// itself is excluded.
pub fn completions(engine: DatabaseEngine, prefix: &str) -> Vec<String> {
    if prefix.is_empty() {
        return Vec::new();
    }
    let lower = prefix.to_ascii_lowercase();
    let words = if engine == DatabaseEngine::Redis {
        REDIS_COMMANDS
    } else {
        SQL_KEYWORDS
    };
    let keep_lower = prefix.chars().all(|c| !c.is_ascii_uppercase());
    words
        .iter()
        .filter(|word| word.starts_with(&lower) && **word != lower)
        .map(|word| {
            if keep_lower {
                (*word).to_owned()
            } else {
                word.to_ascii_uppercase()
            }
        })
        .collect()
}

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

/// A before/after pair split around their common prefix and suffix, for
/// highlighting what a staged edit changed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InlineDiff {
    pub prefix: String,
    pub removed: String,
    pub added: String,
    pub suffix: String,
}

pub fn inline_diff(before: &str, after: &str) -> InlineDiff {
    let old: Vec<char> = before.chars().collect();
    let new: Vec<char> = after.chars().collect();
    let prefix = old.iter().zip(&new).take_while(|(a, b)| a == b).count();
    let suffix = old[prefix..]
        .iter()
        .rev()
        .zip(new[prefix..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    InlineDiff {
        prefix: old[..prefix].iter().collect(),
        removed: old[prefix..old.len() - suffix].iter().collect(),
        added: new[prefix..new.len() - suffix].iter().collect(),
        suffix: old[old.len() - suffix..].iter().collect(),
    }
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
    fn dollar_quoted_unicode_keeps_statement_boundaries() {
        for literal in ["$$é;🚀$$", "$body$é;🚀$body$"] {
            let first = format!("SELECT {literal};");
            let sql = format!("{first}\nSELECT 2;");
            assert_eq!(at_cursor(&sql, sql.find('🚀').unwrap()), first);
            assert_eq!(at_cursor(&sql, sql.len()), "SELECT 2;");
        }
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

    const MYSQL: DatabaseEngine = DatabaseEngine::Mysql;

    #[test]
    fn engine_specific_quoting_cannot_hide_destructive_statements() {
        for (engine, sql) in [
            (
                MYSQL,
                "INSERT INTO notes VALUES ('it\\'s'); DELETE FROM notes;",
            ),
            (MYSQL, "SELECT \"say \\\"hi\"; DELETE FROM notes"),
            (MYSQL, "DELETE FROM users # WHERE id = 5"),
            (MYSQL, "DELETE FROM users -- WHERE id = 5"),
            (MYSQL, "/*!50000 DROP TABLE users */"),
            (MYSQL, "SELECT `a``;'`; DELETE FROM notes"),
            (PG, "SELECT E'it\\'s'; TRUNCATE t;"),
            (PG, "SELECT e'\\\\'; DELETE FROM t"),
            (PG, "SELECT $$'$$; DELETE FROM users; SELECT $$'$$"),
            (PG, "SELECT $é$'$é$; DELETE FROM users; SELECT $é$'$é$"),
            (PG, "SELECT $tag$ $$ ' $tag$; DELETE FROM users"),
            (PG, "DELETE FROM\"users\""),
            // Servers with non-default backslash handling read these the
            // other way, so either reading must not hide the DELETE.
            (PG, "SELECT '\\' ; SELECT ' ; DELETE FROM users; --'"),
            (MYSQL, "SELECT 'a\\' ; DELETE FROM users; -- '"),
            (PG, "UPDATE\"users\"SET active = false"),
            (PG, "UPDATE t SET a = (SELECT max(b) FROM u WHERE u.id = 1)"),
            (PG, "DELETE FROM t USING (SELECT 1 FROM u WHERE true) s"),
            (
                PG,
                "WITH x AS (DELETE FROM a WHERE id = 1 RETURNING id) DELETE FROM b",
            ),
            (
                PG,
                "WITH gone AS (DELETE FROM users RETURNING id) SELECT * FROM gone WHERE id > 0",
            ),
        ] {
            assert!(requires_confirmation(engine, sql), "{engine:?}: {sql}");
        }
    }

    #[test]
    fn engine_specific_quoting_keeps_filtered_writes_quiet() {
        for (engine, sql) in [
            (MYSQL, "DELETE FROM users WHERE note = 'it\\'s; fine'"),
            (MYSQL, "DELETE FROM users WHERE note = 'drop it\\\\'"),
            (MYSQL, "DELETE FROM `drop` WHERE id = 1"),
            (MYSQL, "SELECT 1 # drop table users"),
            (MYSQL, "SELECT 1--1"),
            (PG, "SELECT 'C:\\'; DELETE FROM files WHERE id = 7"),
            (PG, "SELECT $$ drop table x; $$"),
            (PG, "SELECT $body$ DELETE FROM users $body$"),
            (
                PG,
                "UPDATE t SET a = (SELECT max(b) FROM u WHERE u.id = t.id) WHERE t.id = 1",
            ),
            (
                PG,
                "DELETE FROM t WHERE id IN (SELECT id FROM u WHERE u.gone)",
            ),
            (PG, "SELECT a$b FROM t WHERE x = 'drop'"),
        ] {
            assert!(!requires_confirmation(engine, sql), "{engine:?}: {sql}");
        }
    }

    #[test]
    fn history_redacts_sql_passwords() {
        for (engine, sql, saved) in [
            (
                PG,
                "ALTER ROLE app PASSWORD 'hunter2'",
                "ALTER ROLE app PASSWORD '***'",
            ),
            (
                PG,
                "CREATE ROLE app LOGIN PASSWORD $pw$hunter2$pw$; SELECT 'x'",
                "CREATE ROLE app LOGIN PASSWORD '***'; SELECT '***'",
            ),
            (
                PG,
                "SELECT dblink_connect('host=db password=hunter2')",
                "SELECT dblink_connect('***')",
            ),
            (
                MYSQL,
                "CREATE USER 'app'@'%' IDENTIFIED BY \"hunter2\"",
                "CREATE USER '***'@'***' IDENTIFIED BY '***'",
            ),
            (
                MYSQL,
                "SET PASSWORD FOR app = 'hunter2' # 'note'",
                "SET PASSWORD FOR app = '***' # 'note'",
            ),
            (
                MYSQL,
                "CHANGE REPLICATION SOURCE TO SOURCE_PASSWORD='hunter2'",
                "CHANGE REPLICATION SOURCE TO SOURCE_PASSWORD='***'",
            ),
        ] {
            assert_eq!(history_text(engine, sql).as_deref(), Some(saved), "{sql}");
        }
        // A backslash makes literal boundaries depend on server settings.
        assert_eq!(
            history_text(MYSQL, "ALTER USER app IDENTIFIED BY 'a\\'b'"),
            None
        );
        let plain = "SELECT 'hunter2' AS name FROM users WHERE id = 1";
        assert_eq!(history_text(PG, plain).as_deref(), Some(plain));
    }

    #[test]
    fn history_skips_redis_credentials() {
        let redis = DatabaseEngine::Redis;
        for command in [
            "AUTH hunter2",
            "auth default hunter2",
            "\"AU\"TH hunter2",
            "HELLO 3 AUTH default hunter2",
            "ACL SETUSER app on >hunter2 ~* +@all",
            "acl setuser app \"#0123abcd\"",
            "CONFIG SET requirepass hunter2",
            "config set maxmemory 1gb masterauth hunter2",
            "MIGRATE db 6379 key 0 5000 AUTH hunter2",
            "MIGRATE db 6379 \"\" 0 5000 AUTH2 app hunter2 KEYS a b",
            "SENTINEL SET primary auth-pass hunter2",
        ] {
            assert_eq!(history_text(redis, command), None, "{command}");
        }
        for command in [
            "GET auth",
            "SET password hunter2",
            "ACL SETUSER app on ~* +@all",
            "CONFIG GET requirepass",
            "CONFIG SET maxmemory 1gb",
            "MIGRATE db 6379 key 0 5000",
        ] {
            assert_eq!(
                history_text(redis, command).as_deref(),
                Some(command),
                "{command}"
            );
        }
    }

    fn at_cursor_in(engine: DatabaseEngine, text: &str, cursor: usize) -> String {
        execution_target(engine, text, cursor, cursor).map_or_else(String::new, |t| t.sql)
    }

    #[test]
    fn statement_under_cursor_follows_engine_quoting() {
        let sql = "SELECT * FROM files WHERE path LIKE 'C:\\';\nDELETE FROM files WHERE id = 7;";
        assert_eq!(
            at_cursor_in(PG, sql, 3),
            "SELECT * FROM files WHERE path LIKE 'C:\\';"
        );
        let sql = "SELECT E'it\\'s;';\nSELECT 2;";
        assert_eq!(at_cursor_in(PG, sql, 3), "SELECT E'it\\'s;';");
        let sql = "SELECT 'it\\'s;';\nSELECT 2;";
        assert_eq!(at_cursor_in(MYSQL, sql, 3), "SELECT 'it\\'s;';");
        let sql = "SELECT 1 # note; still a comment\n, 2;\nSELECT 3;";
        assert_eq!(
            at_cursor_in(MYSQL, sql, 3),
            "SELECT 1 # note; still a comment\n, 2;"
        );
        let sql = "SELECT `semi;colon` FROM t;\nSELECT 2;";
        assert_eq!(at_cursor_in(MYSQL, sql, 3), "SELECT `semi;colon` FROM t;");
        // `$` continues a PostgreSQL identifier, so `a$tag$` is not a dollar quote.
        let sql = "SELECT a$tag$ FROM t;\nSELECT 2;";
        assert_eq!(at_cursor_in(PG, sql, sql.len()), "SELECT 2;");
        // MySQL has no dollar quoting, and `#` is not a PostgreSQL comment.
        let sql = "SELECT $$;\nSELECT 2;";
        assert_eq!(at_cursor_in(MYSQL, sql, sql.len()), "SELECT 2;");
        let sql = "SELECT 1 # 2;\nSELECT 3;";
        assert_eq!(at_cursor_in(PG, sql, 3), "SELECT 1 # 2;");
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

    #[test]
    fn inline_diff_marks_the_changed_middle() {
        let diff = inline_diff("person2@example.com", "ada@example.com");
        assert_eq!(
            (
                diff.prefix.as_str(),
                diff.removed.as_str(),
                diff.added.as_str(),
                diff.suffix.as_str()
            ),
            ("", "person2", "ada", "@example.com")
        );
        let same = inline_diff("é", "é");
        assert_eq!(
            (
                same.removed.as_str(),
                same.added.as_str(),
                same.suffix.as_str()
            ),
            ("", "", "")
        );
        assert_eq!(same.prefix, "é");
    }

    #[test]
    fn completions_follow_the_prefix_case() {
        assert_eq!(completions(PG, "sel"), vec!["select"]);
        assert_eq!(completions(PG, "SEL"), vec!["SELECT"]);
        assert!(completions(PG, "select").is_empty());
        assert!(completions(DatabaseEngine::Redis, "hge").contains(&"hget".to_owned()));
        assert!(completions(PG, "").is_empty());
    }
}
