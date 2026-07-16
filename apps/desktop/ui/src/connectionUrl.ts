import type { SaveProfileInput } from "./types";

export type ImportedPostgresConnection = Pick<
  SaveProfileInput,
  "host" | "port" | "username" | "defaultDatabase" | "tlsMode"
> & {
  password?: string;
  suggestedName: string;
};

export function parsePostgresConnectionUrl(value: string): ImportedPostgresConnection {
  const trimmed = value.trim();
  let url: URL;

  try {
    url = new URL(trimmed);
  } catch {
    throw new Error("Enter a valid PostgreSQL connection URL.");
  }

  if (url.protocol !== "postgres:" && url.protocol !== "postgresql:") {
    throw new Error("The connection URL must begin with postgres:// or postgresql://.");
  }

  const host = stripIpv6Brackets(url.hostname);
  const username = decode(url.username, "username");
  if (!host || !username) {
    throw new Error("The connection URL must include a host and username.");
  }

  const port = url.port ? Number(url.port) : 5432;
  if (!Number.isInteger(port) || port < 1 || port > 65_535) {
    throw new Error("The connection URL contains an invalid port.");
  }

  const defaultDatabase = decode(url.pathname.replace(/^\/+/, ""), "database") || "postgres";
  const password = url.password ? decode(url.password, "password") : undefined;
  const sslMode = url.searchParams.get("sslmode")?.toLowerCase();
  const tlsMode: SaveProfileInput["tlsMode"] = sslMode === "disable"
    ? "disabled"
    : sslMode === "require" || sslMode === "verify-ca" || sslMode === "verify-full" || url.searchParams.get("ssl") === "true"
      ? "required"
      : "preferred";

  return {
    host,
    port,
    username,
    defaultDatabase,
    tlsMode,
    password,
    suggestedName: `${defaultDatabase} @ ${host}`,
  };
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
