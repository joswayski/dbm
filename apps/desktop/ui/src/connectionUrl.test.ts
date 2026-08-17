import { describe, expect, it } from "vitest";

import { parseConnectionUrl, parseMysqlConnectionUrl, parsePostgresConnectionUrl } from "./connectionUrl";

describe("parseConnectionUrl", () => {
  it("imports Railway-style PostgreSQL URLs", () => {
    expect(parsePostgresConnectionUrl("postgresql://postgres:PASSWORD@caboose.proxy.rlwy.net:57394/railway")).toEqual({
      engine: "postgres",
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
      engine: "postgres",
      username: "user@example.com",
      password: "p@ss",
      defaultDatabase: "my db",
      tlsMode: "required",
    });
  });

  it("imports MySQL and MariaDB URLs", () => {
    expect(parseMysqlConnectionUrl("mysql://root:secret@caboose.proxy.rlwy.net:3306/railway?ssl-mode=REQUIRED")).toEqual({
      engine: "mysql",
      host: "caboose.proxy.rlwy.net",
      port: 3306,
      username: "root",
      defaultDatabase: "railway",
      tlsMode: "required",
      password: "secret",
      suggestedName: "railway @ caboose.proxy.rlwy.net",
    });
    expect(parseConnectionUrl("mariadb://app@localhost/analytics")).toMatchObject({
      engine: "mysql",
      host: "localhost",
      port: 3306,
      username: "app",
      defaultDatabase: "analytics",
      tlsMode: "preferred",
    });
  });

  it("imports Redis and Valkey URLs without requiring a username", () => {
    expect(parseConnectionUrl("redis://localhost:6379/2")).toEqual({
      engine: "redis",
      host: "localhost",
      port: 6379,
      username: "",
      defaultDatabase: "2",
      tlsMode: "preferred",
      password: undefined,
      suggestedName: "2 @ localhost",
    });
    expect(parseConnectionUrl("rediss://default:secret@cache.example:6380/0")).toMatchObject({
      engine: "redis",
      host: "cache.example",
      port: 6380,
      username: "default",
      defaultDatabase: "0",
      tlsMode: "required",
      password: "secret",
    });
    expect(parseConnectionUrl("valkey://127.0.0.1/0")).toMatchObject({
      engine: "redis",
      host: "127.0.0.1",
      port: 6379,
      defaultDatabase: "0",
    });
  });

  it("rejects unsupported protocols", () => {
    expect(() => parseConnectionUrl("mongodb://localhost:27017")).toThrow(
      "The connection URL must begin with postgres://, postgresql://, mysql://, mariadb://, redis://, rediss://, valkey://, or valkeys://.",
    );
    expect(() => parsePostgresConnectionUrl("mysql://root@localhost/app")).toThrow(
      "The connection URL must begin with postgres:// or postgresql://.",
    );
  });
});
