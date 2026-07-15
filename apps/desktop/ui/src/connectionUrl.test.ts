import { describe, expect, it } from "vitest";

import { parsePostgresConnectionUrl } from "./connectionUrl";

describe("parsePostgresConnectionUrl", () => {
  it("imports Railway-style PostgreSQL URLs", () => {
    expect(parsePostgresConnectionUrl("postgresql://postgres:PASSWORD@caboose.proxy.rlwy.net:57394/railway")).toEqual({
      host: "caboose.proxy.rlwy.net",
      port: 57394,
      username: "postgres",
      defaultDatabase: "railway",
      tlsMode: "preferred",
      password: "PASSWORD",
      suggestedName: "railway @ caboose.proxy.rlwy.net",
    });
  });

  it("decodes credentials and maps sslmode", () => {
    expect(parsePostgresConnectionUrl("postgres://user%40example.com:p%40ss@localhost/my%20db?sslmode=require")).toMatchObject({
      username: "user@example.com",
      password: "p@ss",
      defaultDatabase: "my db",
      tlsMode: "required",
    });
  });

  it("rejects non-PostgreSQL URLs", () => {
    expect(() => parsePostgresConnectionUrl("redis://localhost:6379")).toThrow(
      "The connection URL must begin with postgres:// or postgresql://.",
    );
  });
});
