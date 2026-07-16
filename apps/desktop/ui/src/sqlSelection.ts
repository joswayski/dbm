type SqlRange = { from: number; to: number };

export type SqlExecutionTarget = SqlRange & {
  sql: string;
  kind: "selection" | "statement";
};

export function sqlToRun(sqlText: string, selectionFrom: number, selectionTo: number): string {
  return sqlExecutionTarget(sqlText, selectionFrom, selectionTo)?.sql ?? "";
}

export function sqlExecutionTarget(sqlText: string, selectionFrom: number, selectionTo: number): SqlExecutionTarget | null {
  const from = clamp(Math.min(selectionFrom, selectionTo), 0, sqlText.length);
  const to = clamp(Math.max(selectionFrom, selectionTo), 0, sqlText.length);
  const range = from !== to ? trimSqlRange(sqlText, { from, to }) : statementRangeAtCursor(sqlText, from);
  if (!range) return null;
  return {
    ...range,
    sql: sqlText.slice(range.from, range.to),
    kind: from !== to ? "selection" : "statement",
  };
}

export function sqlStatementAtCursor(sqlText: string, cursorPosition: number): string {
  const range = statementRangeAtCursor(sqlText, cursorPosition);
  return range ? sqlText.slice(range.from, range.to) : "";
}

function statementRangeAtCursor(sqlText: string, cursorPosition: number): SqlRange | null {
  const cursor = clamp(cursorPosition, 0, sqlText.length);
  const ranges = statementRanges(sqlText);
  let rangeIndex = ranges.findIndex((range, index) => cursor < range.to || index === ranges.length - 1);

  if (cursor > 0 && sqlText[cursor - 1] === ";" && rangeIndex > 0) rangeIndex -= 1;
  if (rangeIndex < 0) return trimSqlRange(sqlText, { from: 0, to: sqlText.length });

  const selected = trimSqlRange(sqlText, ranges[rangeIndex]);
  if (selected) return selected;

  for (let index = rangeIndex + 1; index < ranges.length; index += 1) {
    const next = trimSqlRange(sqlText, ranges[index]);
    if (next) return next;
  }
  for (let index = rangeIndex - 1; index >= 0; index -= 1) {
    const previous = trimSqlRange(sqlText, ranges[index]);
    if (previous) return previous;
  }
  return null;
}

function statementRanges(sqlText: string): SqlRange[] {
  const ranges: SqlRange[] = [];
  let statementStart = 0;
  let singleQuoted = false;
  let doubleQuoted = false;
  let lineComment = false;
  let blockCommentDepth = 0;
  let dollarQuote: string | null = null;

  for (let index = 0; index < sqlText.length; index += 1) {
    const character = sqlText[index];
    const next = sqlText[index + 1];

    if (lineComment) {
      if (character === "\n") lineComment = false;
      continue;
    }

    if (blockCommentDepth > 0) {
      if (character === "/" && next === "*") {
        blockCommentDepth += 1;
        index += 1;
      } else if (character === "*" && next === "/") {
        blockCommentDepth -= 1;
        index += 1;
      }
      continue;
    }

    if (dollarQuote) {
      if (sqlText.startsWith(dollarQuote, index)) {
        index += dollarQuote.length - 1;
        dollarQuote = null;
      }
      continue;
    }

    if (singleQuoted) {
      if (character === "\\") {
        index += 1;
      } else if (character === "'" && next === "'") {
        index += 1;
      } else if (character === "'") {
        singleQuoted = false;
      }
      continue;
    }

    if (doubleQuoted) {
      if (character === '"' && next === '"') {
        index += 1;
      } else if (character === '"') {
        doubleQuoted = false;
      }
      continue;
    }

    if (character === "-" && next === "-") {
      lineComment = true;
      index += 1;
    } else if (character === "/" && next === "*") {
      blockCommentDepth = 1;
      index += 1;
    } else if (character === "'") {
      singleQuoted = true;
    } else if (character === '"') {
      doubleQuoted = true;
    } else if (character === "$") {
      const delimiter = dollarQuoteDelimiter(sqlText, index);
      if (delimiter) {
        dollarQuote = delimiter;
        index += delimiter.length - 1;
      }
    } else if (character === ";") {
      ranges.push({ from: statementStart, to: index + 1 });
      statementStart = index + 1;
    }
  }

  ranges.push({ from: statementStart, to: sqlText.length });
  return ranges;
}

function dollarQuoteDelimiter(sqlText: string, start: number): string | null {
  const end = sqlText.indexOf("$", start + 1);
  if (end < 0) return null;
  const tag = sqlText.slice(start + 1, end);
  if (tag && !/^[A-Za-z_][A-Za-z0-9_]*$/.test(tag)) return null;
  return sqlText.slice(start, end + 1);
}

function trimSqlRange(sqlText: string, range: SqlRange): SqlRange | null {
  let from = range.from;
  let to = range.to;
  while (from < to && /\s/.test(sqlText[from])) from += 1;
  while (to > from && /\s/.test(sqlText[to - 1])) to -= 1;
  return from < to ? { from, to } : null;
}

function clamp(value: number, minimum: number, maximum: number): number {
  return Math.min(maximum, Math.max(minimum, value));
}
