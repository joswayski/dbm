import type { SqlExecutionTarget } from "./sqlSelection";

export function commandExecutionTarget(text: string, from: number, to: number): SqlExecutionTarget | null {
  if (from !== to) {
    const sql = text.slice(Math.min(from, to), Math.max(from, to)).trim();
    return sql ? { kind: "selection", sql, from: Math.min(from, to), to: Math.max(from, to) } : null;
  }

  const lineStart = text.lastIndexOf("\n", Math.max(0, from - 1)) + 1;
  const lineEnd = text.indexOf("\n", from);
  const end = lineEnd === -1 ? text.length : lineEnd;
  const line = text.slice(lineStart, end);
  const sql = line.trim();
  if (!sql || sql.startsWith("#")) return null;
  const leading = line.length - line.trimStart().length;
  return {
    kind: "statement",
    sql,
    from: lineStart + leading,
    to: lineStart + leading + sql.length,
  };
}
