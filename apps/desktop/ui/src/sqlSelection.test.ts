import { describe, expect, it } from "vitest";

import { lineExecutionTarget, sqlExecutionTarget, sqlStatementAtCursor, sqlToRun } from "./sqlSelection";

describe("sqlToRun", () => {
  it("runs the selected SQL exactly instead of the entire editor", () => {
    const sql = "SELECT now();\n\nSELECT 1;";
    expect(sqlToRun(sql, 15, sql.length)).toBe("SELECT 1;");
    expect(sqlExecutionTarget(sql, 15, sql.length)).toEqual({
      from: 15,
      to: sql.length,
      sql: "SELECT 1;",
      kind: "selection",
    });
  });

  it("runs the statement at the cursor when there is no selection", () => {
    const sql = "SELECT now();\n\nSELECT 1;";
    expect(sqlStatementAtCursor(sql, sql.indexOf("1"))).toBe("SELECT 1;");
    expect(sqlStatementAtCursor(sql, sql.indexOf(";") + 1)).toBe("SELECT now();");
    expect(sqlExecutionTarget(sql, sql.indexOf("1"), sql.indexOf("1"))).toEqual({
      from: 15,
      to: sql.length,
      sql: "SELECT 1;",
      kind: "statement",
    });
  });

  it("ignores semicolons inside PostgreSQL strings, identifiers, comments, and dollar quotes", () => {
    const sql = [
      "SELECT ';' AS \"semi;colon\";",
      "-- comment ;",
      "SELECT $$body;still body$$;",
      "/* outer ; /* nested ; */ done */ SELECT 3;",
    ].join("\n");

    expect(sqlStatementAtCursor(sql, sql.indexOf("body;still"))).toContain("SELECT $$body;still body$$;");
    expect(sqlStatementAtCursor(sql, sql.lastIndexOf("3"))).toContain("SELECT 3;");
  });
});

describe("lineExecutionTarget", () => {
  it("runs the current Redis command line when there is no selection", () => {
    const text = "PING\n\nGET greeting";
    expect(lineExecutionTarget(text, 0, 0)).toEqual({
      from: 0,
      to: 4,
      sql: "PING",
      kind: "statement",
    });
    expect(lineExecutionTarget(text, text.indexOf("GET"), text.indexOf("GET"))).toEqual({
      from: 6,
      to: text.length,
      sql: "GET greeting",
      kind: "statement",
    });
  });
});
