import { describe, expect, it } from "vitest";

import { filterSchemaNodes } from "./schemaFilter";
import type { SchemaNode } from "./types";

function table(schema: string, name: string): SchemaNode {
  return { name, kind: "table", schema, table: name, children: [] };
}

const tree: SchemaNode[] = [
  { name: "public", kind: "schema", schema: "public", table: null, children: [table("public", "users"), table("public", "orders")] },
  { name: "billing", kind: "schema", schema: "billing", table: null, children: [table("billing", "invoices")] },
  { name: "empty", kind: "schema", schema: "empty", table: null, children: [] },
];

describe("filterSchemaNodes", () => {
  it("keeps matching tables under their schemas", () => {
    expect(filterSchemaNodes(tree, "ord")).toEqual([
      { ...tree[0], children: [table("public", "orders")] },
    ]);
  });

  it("keeps every table in a schema whose name matches", () => {
    expect(filterSchemaNodes(tree, "bill")).toEqual([tree[1]]);
  });

  it("matches schemas by name even when empty, and returns nothing without matches", () => {
    expect(filterSchemaNodes(tree, "empty")).toEqual([tree[2]]);
    expect(filterSchemaNodes(tree, "missing")).toEqual([]);
  });
});
