import { beforeEach, describe, expect, it } from "vitest";

import * as commands from "./commands";
import { useDbmStore } from "./store";

describe("DBM store", () => {
  beforeEach(() => {
    useDbmStore.setState({
      profiles: [],
      workspaces: {},
      activeProfileId: null,
      tabs: [],
      activeTabId: null,
    });
  });

  it("preserves open tabs and the active view when a profile is edited", async () => {
    const profile = await commands.saveProfile({
      name: "Keep my tabs",
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
    const tab = { id: "table-tab", title: "public.users", kind: "table" as const, profileId: profile.id, schema: "public", table: "users" };
    useDbmStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
      activeProfileId: profile.id,
      workspaces: { [profile.id]: { profile, databases: [] } },
    });

    await useDbmStore.getState().saveProfile({
      ...profile,
      color: "#a78bfa",
    });

    const state = useDbmStore.getState();
    expect(state.tabs).toEqual([tab]);
    expect(state.activeTabId).toBe(tab.id);
    expect(state.activeProfileId).toBe(profile.id);
    expect(state.workspaces[profile.id]).toBeUndefined();
  });

  it("selects a newly saved profile and clears the previous profile's active tab", async () => {
    useDbmStore.setState({
      activeProfileId: "previous-profile",
      activeTabId: "previous-tab",
      tabs: [{
        id: "previous-tab",
        title: "public.users",
        kind: "table",
        profileId: "previous-profile",
        schema: "public",
        table: "users",
      }],
    });

    const saved = await useDbmStore.getState().saveProfile({
      name: "Selected after save",
      color: "#ef4444",
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

    expect(useDbmStore.getState().activeProfileId).toBe(saved.id);
    expect(useDbmStore.getState().activeTabId).toBeNull();
  });

  it("names, renames, and collapses query tabs while activating the next open tab", () => {
    const store = useDbmStore.getState();
    store.openQuery("profile");
    store.openQuery("profile");

    const [first, second] = useDbmStore.getState().tabs;
    expect([first.title, second.title]).toEqual(["Query 1", "Query 2"]);
    expect(useDbmStore.getState().activeTabId).toBe(second.id);

    useDbmStore.getState().renameTab(second.id, "Revenue check");
    useDbmStore.getState().collapseTab(second.id);

    expect(useDbmStore.getState().tabs.map((tab) => tab.title)).toEqual(["Query 1", "Revenue check"]);
    expect(useDbmStore.getState().tabs[1].collapsed).toBe(true);
    expect(useDbmStore.getState().activeTabId).toBe(first.id);
    expect(useDbmStore.getState().tabs).toHaveLength(2);

    useDbmStore.getState().setActiveTab(second.id);
    expect(useDbmStore.getState().tabs[1].collapsed).toBe(false);
    expect(useDbmStore.getState().activeTabId).toBe(second.id);
  });

  it("opens a query tab when connecting so the workbench is ready immediately", async () => {
    const profile = await commands.saveProfile({
      name: "Ready to query",
      color: "#22c55e",
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

    await useDbmStore.getState().connect(profile.id);

    const state = useDbmStore.getState();
    expect(state.activeProfileId).toBe(profile.id);
    expect(state.workspaces[profile.id]).toBeDefined();
    expect(state.tabs).toHaveLength(1);
    expect(state.tabs[0]).toMatchObject({
      kind: "query",
      profileId: profile.id,
      title: "Query 1",
      sql: "SELECT now();",
    });
    expect(state.activeTabId).toBe(state.tabs[0].id);

    await useDbmStore.getState().connect(profile.id);
    const afterReconnect = useDbmStore.getState();
    expect(afterReconnect.tabs).toHaveLength(1);
    expect(afterReconnect.activeTabId).toBe(afterReconnect.tabs[0].id);
  });
});
