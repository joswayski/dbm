import { describe, expect, it } from "vitest";

import * as commands from "./commands";
import { useDbvStore } from "./store";

describe("DBV store", () => {
  it("preserves open tabs and the active view when a profile is edited", async () => {
    const profile = await commands.saveProfile({
      name: "Keep my tabs",
      color: "#38bdf8",
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
    useDbvStore.setState({
      tabs: [tab],
      activeTabId: tab.id,
      activeProfileId: profile.id,
      workspaces: { [profile.id]: { profile, databases: [] } },
    });

    await useDbvStore.getState().saveProfile({
      ...profile,
      color: "#a78bfa",
    });

    const state = useDbvStore.getState();
    expect(state.tabs).toEqual([tab]);
    expect(state.activeTabId).toBe(tab.id);
    expect(state.activeProfileId).toBe(profile.id);
    expect(state.workspaces[profile.id]).toBeUndefined();
  });
});
