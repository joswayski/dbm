import type { SchemaNode } from "./types";

/**
 * Keeps nodes whose name contains `query` (already lowercased) plus the schema
 * branches that lead to them. A matching schema keeps all of its children.
 */
export function filterSchemaNodes(nodes: SchemaNode[], query: string): SchemaNode[] {
  return nodes.flatMap((node) => {
    const matches = node.name.toLowerCase().includes(query);
    if (matches) return [node];
    const children = filterSchemaNodes(node.children, query);
    return children.length > 0 ? [{ ...node, children }] : [];
  });
}
