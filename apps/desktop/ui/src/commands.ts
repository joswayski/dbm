import { invoke } from "@tauri-apps/api/core";

import type {
  ConnectionProfile,
  DatabaseRef,
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
  WorkspaceInfo,
} from "./types";

let browserProfiles: ProfileSummary[] = [];
const browserHistory: QueryHistoryEntry[] = [];

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
    const summary = { profile, hasPassword: Boolean(input.password) };
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
    const metadata = request.table === "orders"
      ? {
          schema: request.schema,
          table: request.table,
          columns: [
            { name: "id", dataType: "integer", nullable: false, defaultValue: null, ordinal: 1 },
            { name: "user_id", dataType: "integer", nullable: false, defaultValue: null, ordinal: 2 },
            { name: "status", dataType: "text", nullable: false, defaultValue: "'pending'", ordinal: 3 },
          ],
          primaryKey: ["id"],
          hasXmin: true,
        }
      : {
          schema: request.schema,
          table: request.table,
          columns: [
            { name: "id", dataType: "integer", nullable: false, defaultValue: null, ordinal: 1 },
            { name: "email", dataType: "text", nullable: false, defaultValue: null, ordinal: 2 },
            { name: "active", dataType: "boolean", nullable: false, defaultValue: "true", ordinal: 3 },
          ],
          primaryKey: ["id"],
          hasXmin: true,
        };
    const rows = Array.from({ length: 12 }, (_, index) => request.table === "orders"
      ? [index + 1, index + 10, index % 2 ? "paid" : "pending", String(index + 100)]
      : [index + 1, `person${index + 1}@example.com`, index % 3 !== 0, String(index + 100)]);
    return {
      metadata,
      columns: [...metadata.columns.map((column) => column.name), "__dbv_xmin"],
      rows,
      totalRows: rows.length,
      offset: request.offset,
      limit: request.limit,
      hasMore: false,
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
  return call("apply_table_mutations", { batch }, () => ({ applied: batch.mutations.length, conflicts: [] }));
}

export function toDisplayValue(value: JsonValue): string {
  if (value === null) return "NULL";
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}
