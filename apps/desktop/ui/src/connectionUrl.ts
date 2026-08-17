import type { DatabaseEngine, SaveProfileInput } from "./types";

export type ImportedConnection = Pick<
  SaveProfileInput,
  "engine" | "host" | "port" | "username" | "defaultDatabase" | "tlsMode"
> & {
  password?: string;
  suggestedName: string;
};

export type ImportedPostgresConnection = ImportedConnection;

const ENGINE_DEFAULTS: Record<DatabaseEngine, { port: number; database: string; label: string }> = {
  postgres: { port: 5432, database: "postgres", label: "PostgreSQL" },
  mysql: { port: 3306, database: "mysql", label: "MySQL" },
};

export function parseConnectionUrl(value: string): ImportedConnection {
  const trimmed = value.trim();
  let url: URL;

  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("Enter a valid connection URL.");
  }

  const engine = engineFromProtocol(url.protocol);
  if (!engine) {
    throw new Error("The connection URL must begin with postgres://, postgresql://, mysql://, or mariadb://.");
  }

  const defaults = ENGINE_DEFAULTS[engine];
  const host = stripIpv6Brackets(url.hostname);
  const username = decode(url.username, "username");
  if (!host || !username) {
    throw new Error("The connection URL must include a host and username.");
  }

  const port = url.port ? Number(url.port) : defaults.port;
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error("The connection URL contains an invalid port.");
  }

  const defaultDatabase = decode(url.pathname.replace(/^\/+/, ""), "database") || defaults.database;
  const password = url.password ? decode(url.password, "password") : undefined;

  return {
    engine,
    host,
    port,
    username,
    defaultDatabase,
    tlsMode: tlsModeFromUrl(url),
    password,
    suggestedName: `${defaultDatabase} @ ${host}`,
  };
}

export function parsePostgresConnectionUrl(value: string): ImportedPostgresConnection {
  const imported = parseConnectionUrl(value);
  if (imported.engine !== "postgres") {
    throw new Error("The connection URL must begin with postgres:// or postgresql://.");
  }
  return imported;
}

export function parseMysqlConnectionUrl(value: string): ImportedConnection {
  const imported = parseConnectionUrl(value);
  if (imported.engine !== "mysql") {
    throw new Error("The connection URL must begin with mysql:// or mariadb://.");
  }
  return imported;
}

function engineFromProtocol(protocol: string): DatabaseEngine | null {
  if (protocol === "postgres:" || protocol === "postgresql:") return "postgres";
  if (protocol === "mysql:" || protocol === "mariadb:") return "mysql";
  return null;
}

function tlsModeFromUrl(url: URL): SaveProfileInput["tlsMode"] {
  const sslMode = (
    url.searchParams.get("sslmode")
    ?? url.searchParams.get("ssl-mode")
    ?? url.searchParams.get("sslMode")
  )?.toLowerCase();
  if (sslMode === "disable" || sslMode === "disabled") return "disabled";
  if (
    sslMode === "require"
    || sslMode === "required"
    || sslMode === "verify_ca"
    || sslMode === "verify-ca"
    || sslMode === "verify_identity"
    || sslMode === "verify-identity"
    || sslMode === "verify-full"
    || url.searchParams.get("ssl") === "true"
  ) {
    return "required";
  }
  return "preferred";
}

function decode(value: string, field: string): string {
  try {
    return decodeURIComponent(value);
  } catch {
    throw new Error(`The connection URL contains an invalid ${field}.`);
  }
}

function stripIpv6Brackets(host: string): string {
  return host.startsWith("[") && host.endsWith("]") ? host.slice(1, -1) : host;
}
