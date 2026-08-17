import { describe, expect, it } from "vitest";

import { applyTableMutations, connectProfile, listProfiles, loadSchemaTree, loadTablePage, saveProfile, toDisplayValue } from "./commands";

describe("command adapters", () => {
  it("formats nullable and structured values for grids", () => {
    expect(toDisplayValue(null)).toBe("NULL");
    expect(toDisplayValue({ ok: true })).toBe('{"ok":true}');
    expect(toDisplayValue("hello")).toBe("hello");
  });

  it("keeps MySQL preview profiles on the MySQL schema tree", async () => {
    const profile = await saveProfile({
      name: "MySQL preview",
      color: "#22c55e",
      engine: "mysql",
      host: "localhost",
      port: 3306,
      username: "root",
      defaultDatabase: "app",
      tlsMode: "disabled",
      caCertPath: null,
      ssh: null,
      readOnly: false,
    });
    const workspace = await connectProfile(profile.id);
    expect(workspace.databases.map((database) => database.name)).toEqual(["app", "mysql"]);
    const tree = await loadSchemaTree(profile.id);
    expect(tree).toEqual([{
      name: "app",
      kind: "schema",
      schema: "app",
      table: null,
      children: [
        { name: "users", kind: "table", schema: "app", table: "users", children: [] },
        { name: "orders", kind: "table", schema: "app", table: "orders", children: [] },
      ],
    }]);
  });

  it("exposes Redis key folders in the browser preview", async () => {
    const profile = await saveProfile({
      name: "Cache",
      color: "#22c55e",
      engine: "redis",
      host: "localhost",
      port: 6379,
      username: "",
      defaultDatabase: "0",
      tlsMode: "disabled",
      caCertPath: null,
      ssh: null,
      readOnly: false,
    });
    const workspace = await connectProfile(profile.id);
    expect(workspace.databases.map((database) => database.name)).toEqual(["0"]);
    const tree = await loadSchemaTree(profile.id);
    expect(tree[0]?.schema).toBe("keys");
    const page = await loadTablePage({
      profileId: profile.id,
      schema: "keys",
      table: "string",
      offset: 0,
      limit: 50,
      filters: [],
      orderBy: null,
    });
    expect(page.rows.map((row) => row[0])).toEqual(["session:1"]);
  });

  it("keeps browser preview profiles local", async () => {
    const profile = await saveProfile({
      name: "Test profile",
      color: "#38bdf8",
      engine: "postgres",
      host: "localhost",
      port: 5432,
      username: "postgres",
      defaultDatabase: "postgres",
      tlsMode: "disabled",
      caCertPath: null,
      ssh: null,
      readOnly: false,
      password: "secret",
    });
    const profiles = await listProfiles();
    expect(profiles.some((item) => item.profile.id === profile.id)).toBe(true);
  });

  it("applies multiple filters, sorting, and pagination in the browser preview", async () => {
    const page = await loadTablePage({
      profileId: "preview",
      schema: "public",
      table: "users",
      offset: 0,
      limit: 3,
      filters: [
        { column: "active", operator: "equals", value: "true" },
        { column: "id", operator: "greaterThan", value: "3" },
      ],
      orderBy: { column: "id", descending: true },
    });

    expect(page.rows.map((row) => row[0])).toEqual([12, 11, 9]);
    expect(page.totalRows).toBe(6);
    expect(page.hasMore).toBe(true);
  });

  it("persists saved row deletions in the browser preview", async () => {
    const profile = await saveProfile({
      name: "Mutation preview",
      color: "#38bdf8",
      engine: "postgres",
      host: "localhost",
      port: 5432,
      username: "postgres",
      defaultDatabase: "postgres",
      tlsMode: "disabled",
      caCertPath: null,
      ssh: null,
      readOnly: false,
    });
    await applyTableMutations({
      profileId: profile.id,
      schema: "public",
      table: "orders",
      mutations: [{
        original: [1, 10, "pending"],
        changes: [1, 10, "pending"],
        primaryKey: [1],
        xmin: "100",
        deleted: true,
      }],
    });
    const page = await loadTablePage({
      profileId: profile.id,
      schema: "public",
      table: "orders",
      offset: 0,
      limit: 200,
      filters: [{ column: "id", operator: "equals", value: "1" }],
      orderBy: null,
    });

    expect(page.rows).toHaveLength(0);
  });
});
