import type { DatabaseEngine } from "./types";

export const DEFAULT_CONNECTION_COLOR = "#4ad4e8";

/** Restrained, distinguishable identity colors that hold up on dark and light. */
export const CONNECTION_COLORS = [
  "#4ad4e8",
  "#5ac48a",
  "#a78bfa",
  "#f5b544",
  "#f87171",
  "#8b93a7",
];

export const ENGINE_PRESETS: Record<DatabaseEngine, {
  label: string;
  short: string;
  name: string;
  port: number;
  username: string;
  defaultDatabase: string;
  urlPlaceholder: string;
}> = {
  postgres: {
    label: "PostgreSQL",
    short: "postgres",
    name: "Local PostgreSQL",
    port: 5432,
    username: "postgres",
    defaultDatabase: "postgres",
    urlPlaceholder: "postgresql://user:password@host:5432/database",
  },
  mysql: {
    label: "MySQL",
    short: "mysql",
    name: "Local MySQL",
    port: 3306,
    username: "root",
    defaultDatabase: "mysql",
    urlPlaceholder: "mysql://user:password@host:3306/database",
  },
};

export function enginePreset(engine: DatabaseEngine | undefined) {
  return ENGINE_PRESETS[engine ?? "postgres"];
}
