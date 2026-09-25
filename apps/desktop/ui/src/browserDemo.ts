// Demo dataset for the Vite browser preview, enabled with `?demo` in the URL.
// It exists so screenshots and layout work show realistic data; the desktop
// app never uses it, and the default preview mock (used by tests) is unchanged.
import type {
  ConnectionProfile,
  JsonValue,
  ProfileSummary,
  QueryHistoryEntry,
  QueryResponse,
  SchemaNode,
  TableColumn,
  TableMetadata,
} from "./types";

export const DEMO_MODE = typeof window !== "undefined" &&
  !(window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ &&
  new URLSearchParams(window.location.search).has("demo");

const CREATED = "2026-01-12T09:00:00.000Z";

function profile(
  id: string,
  name: string,
  color: string,
  engine: ConnectionProfile["engine"],
  host: string,
  port: number,
  username: string,
  defaultDatabase: string,
): ProfileSummary {
  return {
    profile: {
      id, name, color, engine, host, port, username, defaultDatabase,
      tlsMode: "required",
      caCertPath: null,
      ssh: null,
      readOnly: false,
      createdAt: CREATED,
      updatedAt: CREATED,
    },
  };
}

export function demoProfiles(): ProfileSummary[] {
  return [
    profile("demo-production", "Production", "#ff9f43", "postgres", "db.acme.internal", 5432, "app_admin", "acme"),
    profile("demo-staging", "Staging", "#3dd6c6", "postgres", "staging-db.acme.internal", 5432, "app_admin", "acme"),
    profile("demo-analytics", "Analytics", "#b48cff", "mysql", "warehouse.acme.internal", 3306, "analyst", "warehouse"),
    profile("demo-cache", "Session cache", "#ff6b8a", "redis", "cache.acme.internal", 6379, "", "0"),
  ];
}

type ColumnSpec = [name: string, dataType: string, nullable?: boolean, defaultValue?: string];

type DemoTable = {
  columns: ColumnSpec[];
  rows: JsonValue[][];
};

const FIRST = ["Maya", "Liam", "Priya", "Tomás", "Ada", "Jonah", "Sofia", "Kenji", "Elena", "Rahim", "Hana", "Marco", "Grace", "Owen", "Amara", "Felix", "Inês", "Noah", "Zara", "Mateo", "Leila", "Oscar", "Yuki", "Daniel", "Chloe", "Arjun", "Freya", "Samuel", "Nadia", "Lucas", "Isla", "Tariq", "Emma", "Diego", "Mei", "Theo", "Aisha", "Hugo", "Clara", "Ravi"];
const LAST = ["Okafor", "Brennan", "Raman", "Vidal", "Nguyen", "Holloway", "Marchetti", "Mori", "Vasquez", "Chowdhury", "Kim", "Bellini", "Liu", "Price", "Osei", "Wagner", "Costa", "Fischer", "Ahmed", "Rossi", "Haddad", "Lindqvist", "Tanaka", "Moreau", "Dubois", "Mehta", "Larsen", "Adeyemi", "Petrova", "Silva", "Walsh", "Rahman", "Novak", "Herrera", "Zhang", "Becker", "Bello", "Laurent", "Jensen", "Iyer"];
const DOMAINS = ["northwind.dev", "fieldstone.io", "kestrel.app", "corvid.co", "lumen.so", "brightline.com", "meridianlabs.io", "tidewater.jp", "quarry.dev", "pinecrest.org", "solstice.ai", "harbor.app", "ember.io", "granite.dev", "bluefin.co", "nordpeak.de", "lagoa.pt"];
const PLANS = ["pro", "team", "pro", "free", "pro", "team", "pro", "free", "enterprise", "pro", "free", "team", "pro", "team", "free", "pro", "team", "enterprise"];
const REGIONS = ["us-east-1", "eu-west-1", "ap-northeast-1", "us-west-2"];

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

function timestamp(dayOffset: number, minutes: number): string {
  const date = new Date(Date.UTC(2024, 2, 14) + dayOffset * 86_400_000 + minutes * 60_000);
  return `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())} ${pad(date.getUTCHours())}:${pad(date.getUTCMinutes())}:${pad(date.getUTCSeconds())}+00`;
}

function seatsFor(plan: string, index: number): number {
  if (plan === "free") return 1;
  if (plan === "team") return 3 + (index % 5);
  if (plan === "enterprise") return 48 + (index % 4) * 24;
  return 7 + (index % 9);
}

function userRows(): JsonValue[][] {
  return FIRST.map((first, index) => {
    const last = LAST[index];
    const plan = PLANS[index % PLANS.length];
    const domain = DOMAINS[index % DOMAINS.length];
    const local = index % 3 === 0 ? `${first}.${last}` : index % 3 === 1 ? `${first[0]}.${last}` : first;
    const email = `${local}@${domain}`.normalize("NFD").replace(/[̀-ͯ]/g, "").toLowerCase();
    const inactive = index === 7 || index === 10 || index === 14;
    return [
      1001 + index,
      index === 7 ? "old.account@example.com" : email,
      index === 7 ? null : `${first} ${last}`,
      plan,
      seatsFor(plan, index),
      timestamp(index * 5, 540 + index * 37),
      !inactive,
      { sso: plan === "enterprise" || plan === "team", region: REGIONS[index % REGIONS.length] },
    ];
  });
}

const STATUSES = ["paid", "paid", "open", "paid", "void", "paid", "open", "paid"];

function invoiceRows(): JsonValue[][] {
  return Array.from({ length: 36 }, (_, index) => [
    `INV-${String(24081 + index)}`,
    1001 + ((index * 7) % FIRST.length),
    STATUSES[index % STATUSES.length],
    ((index * 3779) % 90_000 + 4_900) / 100,
    "USD",
    timestamp(120 + index * 3, 600 + index * 11),
    STATUSES[index % STATUSES.length] === "paid" ? timestamp(122 + index * 3, 700 + index * 13) : null,
  ]);
}

function orderRows(): JsonValue[][] {
  return Array.from({ length: 40 }, (_, index) => [
    58_210 + index,
    1001 + ((index * 5) % FIRST.length),
    ["fulfilled", "fulfilled", "processing", "fulfilled", "refunded", "pending"][index % 6],
    1 + (index % 4),
    ((index * 2311) % 40_000 + 1_200) / 100,
    timestamp(200 + index, 400 + index * 17),
  ]);
}

const PRODUCTS: Array<[string, string, number]> = [
  ["SKU-TEAM", "Team plan (monthly)", 24], ["SKU-PRO", "Pro plan (monthly)", 49], ["SKU-ENT", "Enterprise plan (monthly)", 199],
  ["SKU-SEAT", "Additional seat", 12], ["SKU-SSO", "SSO add-on", 99], ["SKU-AUDIT", "Audit log retention", 39],
  ["SKU-SUP", "Priority support", 149], ["SKU-API", "API overage (1M calls)", 20],
];

const DEMO_TABLES: Record<string, DemoTable> = {
  users: {
    columns: [
      ["id", "bigint", false, "nextval('users_id_seq')"],
      ["email", "text", false],
      ["full_name", "text", true],
      ["plan", "plan_tier", false, "'free'"],
      ["seats", "integer", false, "1"],
      ["created_at", "timestamp with time zone", false, "now()"],
      ["is_active", "boolean", false, "true"],
      ["metadata", "jsonb", true],
    ],
    rows: userRows(),
  },
  invoices: {
    columns: [
      ["number", "text", false],
      ["user_id", "bigint", false],
      ["status", "text", false, "'open'"],
      ["amount", "numeric", false],
      ["currency", "text", false, "'USD'"],
      ["issued_at", "timestamp with time zone", false, "now()"],
      ["paid_at", "timestamp with time zone", true],
    ],
    rows: invoiceRows(),
  },
  orders: {
    columns: [
      ["id", "bigint", false],
      ["user_id", "bigint", false],
      ["status", "text", false, "'pending'"],
      ["quantity", "integer", false],
      ["total", "numeric", false],
      ["placed_at", "timestamp with time zone", false, "now()"],
    ],
    rows: orderRows(),
  },
  products: {
    columns: [["sku", "text", false], ["name", "text", false], ["price", "numeric", false], ["active", "boolean", false, "true"]],
    rows: PRODUCTS.map(([sku, name, price], index) => [sku, name, price, index !== 7]),
  },
  accounts: {
    columns: [["id", "bigint", false], ["name", "text", false], ["owner_id", "bigint", false], ["created_at", "timestamp with time zone", false]],
    rows: DOMAINS.map((domain, index) => [501 + index, domain.split(".")[0].replace(/^./, (letter) => letter.toUpperCase()), 1001 + index, timestamp(index * 9, 480)]),
  },
  api_keys: {
    columns: [["id", "uuid", false], ["account_id", "bigint", false], ["label", "text", false], ["last_used_at", "timestamp with time zone", true]],
    rows: Array.from({ length: 12 }, (_, index) => [
      `8f3c${pad(index)}a2-41d7-4c0e-9b1a-${String(700_000_000_000 + index * 7919).slice(0, 12)}`,
      501 + (index % DOMAINS.length),
      ["CI deploys", "Billing sync", "Data export", "Mobile app"][index % 4],
      index % 5 === 0 ? null : timestamp(500 + index, 300 + index * 41),
    ]),
  },
  sessions: {
    columns: [["id", "uuid", false], ["user_id", "bigint", false], ["ip", "inet", true], ["expires_at", "timestamp with time zone", false]],
    rows: Array.from({ length: 24 }, (_, index) => [
      `c0ffee${pad(index)}-5e55-4a1d-8b00-${String(100_000_000_000 + index * 104_729).slice(0, 12)}`,
      1001 + ((index * 3) % FIRST.length),
      `10.24.${index % 7}.${40 + index * 3}`,
      timestamp(560 + (index % 3), 60 * (index % 24)),
    ]),
  },
  active_subscriptions: {
    columns: [["user_id", "bigint", false], ["plan", "plan_tier", false], ["seats", "integer", false]],
    rows: userRows().filter((row) => row[3] !== "free" && row[6] === true).map((row) => [row[0], row[3], row[4]]),
  },
  monthly_revenue: {
    columns: [["month", "date", false], ["mrr", "numeric", false], ["new_accounts", "integer", false]],
    rows: Array.from({ length: 9 }, (_, index) => [`2026-${pad(index + 1)}-01`, 48_200 + index * 3_450 + (index % 3) * 910, 18 + (index * 7) % 13]),
  },
};

const PRIMARY_KEYS: Record<string, string[]> = {
  users: ["id"], invoices: ["number"], orders: ["id"], products: ["sku"], accounts: ["id"], api_keys: ["id"], sessions: ["id"],
  active_subscriptions: [], monthly_revenue: [],
};

export function demoRows(): Record<string, JsonValue[][]> {
  return Object.fromEntries(Object.entries(DEMO_TABLES).map(([name, table]) => [
    name,
    table.rows.map((row, index) => [...row, String(9_100 + index)]),
  ]));
}

export function demoTableMetadata(schema: string, table: string): TableMetadata | null {
  const definition = DEMO_TABLES[table];
  if (!definition || schema !== "public") return null;
  const columns: TableColumn[] = definition.columns.map(([name, dataType, nullable = false, defaultValue = null], index) => ({
    name, dataType, nullable, defaultValue, ordinal: index + 1,
  }));
  return { schema, table, columns, primaryKey: PRIMARY_KEYS[table] ?? [], hasXmin: (PRIMARY_KEYS[table] ?? []).length > 0 };
}

const TABLE_NAMES = ["accounts", "api_keys", "invoices", "orders", "products", "sessions", "users"];
const VIEW_NAMES = ["active_subscriptions", "monthly_revenue"];

export const DEMO_SCHEMA: SchemaNode[] = [{
  name: "public",
  kind: "schema",
  schema: "public",
  table: null,
  children: [
    ...TABLE_NAMES.map((name) => ({ name, kind: "table", schema: "public", table: name, children: [] })),
    ...VIEW_NAMES.map((name) => ({ name, kind: "view", schema: "public", table: name, children: [] })),
  ],
}];

export function demoQueryResponse(): QueryResponse {
  const users = userRows().filter((row) => row[2] !== null);
  const rows: JsonValue[][] = users.slice(0, 12).map((row, index) => [
    row[2],
    row[3],
    row[4],
    14 - index,
    Number((18_420.5 - index * 1_163.25 - (index % 3) * 87.4).toFixed(2)),
    `2026-09-${pad(24 - index)}`,
  ]).sort((left, right) => Number(right[4]) - Number(left[4]));
  return {
    columns: [
      { name: "full_name", dataType: "text" },
      { name: "plan", dataType: "plan_tier" },
      { name: "seats", dataType: "int4" },
      { name: "invoices", dataType: "int8" },
      { name: "paid_total", dataType: "numeric" },
      { name: "last_paid", dataType: "date" },
    ],
    rows,
    rowCount: rows.length,
    affectedRows: null,
    durationMs: 41,
    truncated: false,
    notices: [],
  };
}

export function demoHistory(): QueryHistoryEntry[] {
  const statements: Array<[string, boolean]> = [
    ["SELECT count(*) FROM sessions WHERE expires_at < now();", true],
    ["UPDATE users SET plan = 'team' WHERE id = 1012;", true],
    ["SELECT * FROM invoices WHERE status = 'open' ORDER BY issued_at;", true],
    ["SELECT plan, count(*) FROM users GROUP BY plan;", true],
    ["SELECT * FROM user_events LIMIT 50;", false],
    ["EXPLAIN ANALYZE SELECT * FROM orders WHERE user_id = 1003;", true],
  ];
  return statements.map(([sql, success], index) => ({
    id: `demo-history-${index}`,
    profileId: "demo-production",
    database: "acme",
    sql,
    executedAt: new Date(Date.UTC(2026, 8, 25, 9, 40 - index * 7)).toISOString(),
    durationMs: 12 + index * 9,
    success,
  }));
}
