import { describe, expect, it } from "vitest";

import { listProfiles, saveProfile, toDisplayValue } from "./commands";

describe("command adapters", () => {
  it("formats nullable and structured values for grids", () => {
    expect(toDisplayValue(null)).toBe("NULL");
    expect(toDisplayValue({ ok: true })).toBe('{"ok":true}');
    expect(toDisplayValue("hello")).toBe("hello");
  });

  it("keeps browser preview profiles local", async () => {
    const profile = await saveProfile({
      name: "Test profile",
      color: "#38bdf8",
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
    expect(profiles.find((item) => item.profile.id === profile.id)?.hasPassword).toBe(true);
  });
});
