export type JsonValue =
  | null
  | boolean
  | number
  | string
  | JsonValue[]
  | { [key: string]: JsonValue };

export type TlsMode = "disabled" | "preferred" | "required";

export type SshConfig = {
  host: string;
  port: number;
  username: string;
  privateKeyPath: string | null;
  useAgent: boolean;
  passwordAuth: boolean;
};

export type ConnectionProfile = {
  id: string;
  name: string;
  color: string | null;
  host: string;
  port: number;
  username: string;
  defaultDatabase: string;
  tlsMode: TlsMode;
  caCertPath: string | null;
  ssh: SshConfig | null;
  readOnly: boolean;
  createdAt: string;
  updatedAt: string;
};

export type SaveProfileInput = {
  id?: string;
  name: string;
  color: string | null;
  host: string;
  port: number;
  username: string;
  defaultDatabase: string;
  tlsMode: TlsMode;
  caCertPath: string | null;
  ssh: SshConfig | null;
  readOnly: boolean;
  password?: string | null;
};

export type ProfileSummary = {
  profile: ConnectionProfile;
};

export type DatabaseRef = {
  name: string;
  isTemplate: boolean;
  isConnectable: boolean;
};

export type WorkspaceInfo = {
  profile: ConnectionProfile;
  databases: DatabaseRef[];
};

export type SchemaNode = {
  name: string;
  kind: "schema" | "table" | "view" | string;
  schema: string | null;
  table: string | null;
  children: SchemaNode[];
};

export type TableColumn = {
  name: string;
  dataType: string;
  nullable: boolean;
  defaultValue: string | null;
  ordinal: number;
};

export type TableMetadata = {
  schema: string;
  table: string;
  columns: TableColumn[];
  primaryKey: string[];
  hasXmin: boolean;
};

export type FilterOperator =
  | "equals"
  | "notEquals"
  | "contains"
  | "startsWith"
  | "endsWith"
  | "greaterThan"
  | "greaterThanOrEqual"
  | "lessThan"
  | "lessThanOrEqual"
  | "in"
  | "notIn"
  | "isNull"
  | "isNotNull";

export type FilterCondition = {
  column: string;
  operator: FilterOperator;
  value: string | null;
};

export type OrderSpec = {
  column: string;
  descending: boolean;
};

export type TablePageRequest = {
  profileId: string;
  schema: string;
  table: string;
  offset: number;
  limit: number;
  filters: FilterCondition[];
  orderBy: OrderSpec | null;
  includeTotal?: boolean;
};

export type TablePage = {
  metadata: TableMetadata;
  columns: string[];
  rows: JsonValue[][];
  totalRows: number | null;
  offset: number;
  limit: number;
  hasMore: boolean;
};

export type RowMutation = {
  original: JsonValue[];
  changes: JsonValue[];
  primaryKey: JsonValue[];
  xmin: string | null;
  deleted: boolean;
};

export type MutationBatch = {
  profileId: string;
  schema: string;
  table: string;
  mutations: RowMutation[];
};

export type MutationResult = {
  applied: number;
  conflicts: JsonValue[][];
};

export type QueryRequest = {
  profileId: string;
  sql: string;
  maxRows?: number;
};

export type QueryColumn = {
  name: string;
  dataType: string;
};

export type QueryResponse = {
  columns: QueryColumn[];
  rows: JsonValue[][];
  rowCount: number;
  affectedRows: number | null;
  durationMs: number;
  truncated: boolean;
  notices: string[];
};

export type QueryHistoryEntry = {
  id: string;
  profileId: string;
  database: string;
  sql: string;
  executedAt: string;
  durationMs: number;
  success: boolean;
};

export type Tab = {
  id: string;
  title: string;
  kind: "table" | "query";
  profileId: string;
  schema?: string;
  table?: string;
  sql?: string;
  collapsed?: boolean;
};
