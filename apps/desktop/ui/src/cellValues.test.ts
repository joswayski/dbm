import { describe, expect, it } from "vitest";

import { editableText, parseCellInput } from "./cellValues";
import type { TableColumn } from "./types";

function column(dataType: string, nullable = true): TableColumn {
  return { name: "value", dataType, nullable, defaultValue: null, ordinal: 1 };
}

describe("inline cell values", () => {
  it("keeps the original value when the text is unchanged", () => {
    expect(parseCellInput("", column("text"), "")).toBe("");
    expect(parseCellInput("", column("text"), null)).toBeNull();
    expect(parseCellInput("7", column("integer"), 7)).toBe(7);
    expect(parseCellInput('{"a":1}', column("jsonb"), { a: 1 })).toEqual({ a: 1 });
  });

  it("keeps text columns as typed", () => {
    expect(parseCellInput("007", column("character varying"), "008")).toBe("007");
    expect(parseCellInput("true", column("varchar(10)"), "no")).toBe("true");
    expect(parseCellInput(" padded ", column("text"), "x")).toBe(" padded ");
  });

  it("clears nullable cells to NULL and required text cells to an empty string", () => {
    expect(parseCellInput("", column("text"), "x")).toBeNull();
    expect(parseCellInput("", column("text", false), "x")).toBe("");
  });

  it("parses numbers and booleans only for matching column types", () => {
    expect(parseCellInput("42", column("integer"), 1)).toBe(42);
    expect(parseCellInput("9007199254740993", column("bigint"), 1)).toBe("9007199254740993");
    expect(parseCellInput("12.50", column("numeric(10,2)"), "1.00")).toBe("12.50");
    expect(parseCellInput("f", column("boolean"), true)).toBe(false);
    expect(parseCellInput("true", column("tinyint(1)"), 0)).toBe(true);
  });

  it("renders NULL as an empty editor and objects as JSON", () => {
    expect(editableText(null)).toBe("");
    expect(editableText({ a: [1] })).toBe('{"a":[1]}');
    expect(editableText(false)).toBe("false");
  });
});
