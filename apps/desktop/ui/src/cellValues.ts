import type { JsonValue, TableColumn } from "./types";

const BOOLEAN_TYPE = /^bool/i;
const NUMERIC_TYPE = /int|serial|numeric|decimal|real|double|float|number/i;
const TRUE_TEXT = new Set(["true", "t", "1", "yes"]);
const FALSE_TEXT = new Set(["false", "f", "0", "no"]);

/** The text shown in the inline editor for a cell. NULL edits as an empty field. */
export function editableText(value: JsonValue): string {
  if (value === null) return "";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

/**
 * Turns inline-editor text back into a cell value for the column's type.
 *
 * Text that still matches the original value keeps the original, so opening and
 * closing an editor never stages a change. Only numeric and boolean columns get
 * numbers and booleans; other columns keep the exact text (`007` stays `007`).
 * Integers beyond JavaScript's exact range stay strings so they are not rounded.
 */
export function parseCellInput(text: string, column: TableColumn, original: JsonValue): JsonValue {
  if (text === editableText(original)) return original;
  if (text === "") return column.nullable ? null : "";

  const trimmed = text.trim();
  const normalized = trimmed.toLowerCase();
  if (BOOLEAN_TYPE.test(column.dataType)) {
    if (TRUE_TEXT.has(normalized)) return true;
    if (FALSE_TEXT.has(normalized)) return false;
    return text;
  }
  if (NUMERIC_TYPE.test(column.dataType)) {
    if (normalized === "true") return true;
    if (normalized === "false") return false;
    if (/^-?\d+$/.test(trimmed)) {
      const integer = Number(trimmed);
      return Number.isSafeInteger(integer) ? integer : trimmed;
    }
    // Decimals stay text so numeric/decimal columns keep their exact digits.
    return trimmed;
  }
  return text;
}
