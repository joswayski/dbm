import { invoke } from "@tauri-apps/api/core";

import type {
  ConnectionProfile,
  DatabaseRef,
  FilterCondition,
  JsonValue,
  MutationBatch,
  MutationResult,
  ProfileSummary,
  QueryHistoryEntry,
  QueryRequest,
  QueryResponse,
  SaveProfileInput,
  SchemaNode,
  TablePage,
  TablePageRequest,
  TableMetadata,
  WorkspaceInfo,
} from "./types";

let browserProfiles: ProfileSummary[] = [];
const browserHistory: QueryHistoryEntry[] = [];
const browserRows: Record<string, JsonValue[][]> = {
  users: Array.from({ length: 12 }, (_, index) => [index + 1, `person${index + 1}@example.com`, index % 3 !== 0, String(index + 100)]),
  orders: Array.from({ length: 12 }, (_, index) => [index + 1, index + 10, index % 2 ? "paid" : "pending", String(index + 100)]),
};

function inTauri(): boolean {
  return typeof window !== "undefined" &&
    Boolean((window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__);
}

async function call<T>(command: string, args: Record<string, unknown>, fallback: () => T | Promise<T>): Promise<T> {
  if (inTauri()) {
    return invoke<T>(command, args);
  }
  return fallback();
}

export function listProfiles(): Promise<ProfileSummary[]> {
  return call("list_profiles", {}, () => browserProfiles);
}

export function saveProfile(input: SaveProfileInput): Promise<ConnectionProfile> {
  return call("save_profile", { input }, () => {
    const now = new Date().toISOString();
    const existing = input.id ? browserProfiles.find((item) => item.profile.id === input.id) : undefined;
    const profile: ConnectionProfile = {
      id: input.id ?? crypto.randomUUID(),
      name: input.name,
      color: input.color,
      host: input.host,
      port: input.port,
      username: input.username,
      defaultDatabase: input.defaultDatabase,
      tlsMode: input.tlsMode,
      caCertPath: input.caCertPath,
      ssh: input.ssh,
      readOnly: input.readOnly,
      createdAt: existing?.profile.createdAt ?? now,
      updatedAt: now,
    };
    const summary = { profile };
    browserProfiles = existing
      ? browserProfiles.map((item) => (item.profile.id === profile.id ? summary : item))
      : [...browserProfiles, summary];
    return profile;
  });
}

export function deleteProfile(profileId: string): Promise<void> {
  return call("delete_profile", { profileId }, () => {
    browserProfiles = browserProfiles.filter((item) => item.profile.id !== profileId);
  });
}

export function testProfile(input: SaveProfileInput): Promise<void> {
  return call("test_profile", { input }, async () => {
    if (!input.host || !input.username) {
      throw new Error("Host and username are required");
    }
  });
}

export function connectProfile(profileId: string): Promise<WorkspaceInfo> {
  return call("connect_profile", { profileId }, () => {
    const summary = browserProfiles.find((item) => item.profile.id === profileId);
    if (!summary) throw new Error("Profile not found");
    const databases: DatabaseRef[] = [
      { name: summary.profile.defaultDatabase, isTemplate: false, isConnectable: true },
      { name: "postgres", isTemplate: false, isConnectable: true },
    ];
    return { profile: summary.profile, databases };
  });
}

export function connectDatabase(profileId: string, database: string): Promise<WorkspaceInfo> {
  return call("connect_database", { profileId, database }, async () => {
    const workspace = await connectProfile(profileId);
    return { ...workspace, profile: { ...workspace.profile, defaultDatabase: database } };
  });
}

export function disconnectWorkspace(profileId: string): Promise<void> {
  return call("disconnect_workspace", { profileId }, () => undefined);
}

export function listDatabases(profileId: string): Promise<DatabaseRef[]> {
  return call("list_databases", { profileId }, () => {
    const summary = browserProfiles.find((item) => item.profile.id === profileId);
    return summary ? [{ name: summary.profile.defaultDatabase, isTemplate: false, isConnectable: true }] : [];
  });
}

const browserSchema: SchemaNode[] = [
  {
    name: "public",
    kind: "schema",
    schema: "public",
    table: null,
    children: [
      { name: "users", kind: "table", schema: "public", table: "users", children: [] },
      { name: "orders", kind: "table", schema: "public", table: "orders", children: [] },
    ],
  },
];

export function loadSchemaTree(profileId: string): Promise<SchemaNode[]> {
  return call("load_schema_tree", { profileId }, () => browserSchema);
}

export function loadTablePage(request: TablePageRequest): Promise<TablePage> {
  return call("load_table_page", { request }, () => {
    const metadata = browserTableMetadata(request.schema, request.table);
    const rows = (browserRows[request.table] ?? [])
      .filter((row) => request.filters.every((filter) => browserFilterMatches(row, metadata, filter)))
      .slice();
    const orderColumn = request.orderBy?.column ?? metadata.primaryKey[0];
    if (orderColumn) {
      const orderIndex = metadata.columns.findIndex((column) => column.name === orderColumn);
      const descending = request.orderBy?.descending ?? false;
      rows.sort((left, right) => compareBrowserValues(left[orderIndex], right[orderIndex]) * (descending ? -1 : 1));
    }
    const pageRows = rows.slice(request.offset, request.offset + request.limit).map((row) => [...row]);
    return {
      metadata,
      columns: [...metadata.columns.map((column) => column.name), "__dbv_xmin"],
      rows: pageRows,
      totalRows: request.includeTotal === false ? null : rows.length,
      offset: request.offset,
      limit: request.limit,
      hasMore: request.offset + pageRows.length < rows.length,
    };
  });
}

export function runQuery(request: QueryRequest): Promise<QueryResponse> {
  return call("run_query", { request }, () => {
    const now = new Date().toISOString();
    const entry: QueryHistoryEntry = {
      id: crypto.randomUUID(),
      profileId: request.profileId,
      database: "postgres",
      sql: request.sql,
      executedAt: now,
      durationMs: 2,
      success: true,
    };
    browserHistory.unshift(entry);
    if (/^\s*(select|show|with|values)/i.test(request.sql)) {
      return {
        columns: [{ name: "result", dataType: "text" }],
        rows: [["DBV browser preview"]],
        rowCount: 1,
        affectedRows: null,
        durationMs: 2,
        truncated: false,
        notices: [],
      };
    }
    return {
      columns: [],
      rows: [],
      rowCount: 0,
      affectedRows: 0,
      durationMs: 2,
      truncated: false,
      notices: [],
    };
  });
}

export function cancelQuery(): Promise<void> {
  return call("cancel_query", {}, () => undefined);
}

export function listQueryHistory(profileId: string, limit = 100): Promise<QueryHistoryEntry[]> {
  return call("list_query_history", { profileId, limit }, () => browserHistory.slice(0, limit));
}

export function applyTableMutations(batch: MutationBatch): Promise<MutationResult> {
  return call("apply_table_mutations", { batch }, () => {
    const profile = browserProfiles.find((item) => item.profile.id === batch.profileId)?.profile;
    if (profile?.readOnly) throw new Error("Profile is read-only");
    const metadata = browserTableMetadata(batch.schema, batch.table);
    const rows = browserRows[batch.table] ?? [];
    const primaryKeyIndexes = metadata.primaryKey.map((key) => metadata.columns.findIndex((column) => column.name === key));
    let applied = 0;
    const conflicts: JsonValue[][] = [];
    for (const mutation of batch.mutations) {
      const rowIndex = rows.findIndex((row) => primaryKeyIndexes.every((columnIndex, keyIndex) => valuesEqual(row[columnIndex], mutation.primaryKey[keyIndex])));
      if (rowIndex < 0) {
        conflicts.push(mutation.primaryKey);
        continue;
      }
      if (mutation.xmin !== null && String(rows[rowIndex][metadata.columns.length]) !== mutation.xmin) {
        conflicts.push(mutation.primaryKey);
        continue;
      }
      if (mutation.deleted) {
        rows.splice(rowIndex, 1);
      } else {
        const nextXmin = String(Number(rows[rowIndex][metadata.columns.length] ?? 0) + 1);
        rows[rowIndex] = [...mutation.changes, nextXmin];
      }
      applied += 1;
    }
    return { applied, conflicts };
  });
}

export function toDisplayValue(value: JsonValue): string {
  if (value === null) return "NULL";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function browserTableMetadata(schema: string, table: string): TableMetadata {
  return table === "orders"
    ? {
        schema,
        table,
        columns: [
          { name: "id", dataType: "integer", nullable: false, defaultValue: null, ordinal: 1 },
          { name: "user_id", dataType: "integer", nullable: false, defaultValue: null, ordinal: 2 },
          { name: "status", dataType: "text", nullable: false, defaultValue: "'pending'", ordinal: 3 },
        ],
        primaryKey: ["id"],
        hasXmin: true,
      }
    : {
        schema,
        table,
        columns: [
          { name: "id", dataType: "integer", nullable: false, defaultValue: null, ordinal: 1 },
          { name: "email", dataType: "text", nullable: false, defaultValue: null, ordinal: 2 },
          { name: "active", dataType: "boolean", nullable: false, defaultValue: "true", ordinal: 3 },
        ],
        primaryKey: ["id"],
        hasXmin: true,
      };
}

function browserFilterMatches(row: JsonValue[], metadata: TableMetadata, filter: FilterCondition): boolean {
  const index = metadata.columns.findIndex((column) => column.name === filter.column);
  if (index < 0) return false;
  const cell = row[index];
  const value = filter.value ?? "";
  const cellText = cell === null ? "" : String(cell);
  const normalizedCell = cellText.toLocaleLowerCase();
  const normalizedValue = value.toLocaleLowerCase();
  const list = value.split(",").map((item) => item.trim()).filter(Boolean);
  const comparison = compareBrowserValues(cell, browserFilterValue(cell, value));
  if (cell === null) return filter.operator === "notEquals" || filter.operator === "isNull";
  switch (filter.operator) {
    case "equals": return cell !== null && cellText === value;
    case "notEquals": return cell === null || cellText !== value;
    case "contains": return normalizedCell.includes(normalizedValue);
    case "startsWith": return normalizedCell.startsWith(normalizedValue);
    case "endsWith": return normalizedCell.endsWith(normalizedValue);
    case "greaterThan": return comparison > 0;
    case "greaterThanOrEqual": return comparison >= 0;
    case "lessThan": return comparison < 0;
    case "lessThanOrEqual": return comparison <= 0;
    case "in": return list.includes(cellText);
    case "notIn": return !list.includes(cellText);
    case "isNull": return cell === null;
    case "isNotNull": return cell !== null;
  }
}

function browserFilterValue(cell: JsonValue, value: string): JsonValue {
  if (typeof cell === "number") return Number(value);
  if (typeof cell === "boolean") return value.toLocaleLowerCase() === "true";
  return value;
}

function compareBrowserValues(left: JsonValue, right: JsonValue): number {
  if (left === right) return 0;
  if (left === null) return 1;
  if (right === null) return -1;
  if (typeof left === "number" && typeof right === "number") return left - right;
  if (typeof left === "boolean" && typeof right === "boolean") return Number(left) - Number(right);
  return String(left).localeCompare(String(right), undefined, { numeric: true });
}

function valuesEqual(left: JsonValue, right: JsonValue): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}
