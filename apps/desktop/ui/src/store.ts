import { create } from "zustand";

import * as commands from "./commands";
import type { ProfileSummary, Tab, WorkspaceInfo } from "./types";

type DbvStore = {
  profiles: ProfileSummary[];
  workspaces: Record<string, WorkspaceInfo>;
  activeProfileId: string | null;
  tabs: Tab[];
  activeTabId: string | null;
  loadProfiles: () => Promise<void>;
  saveProfile: (input: Parameters<typeof commands.saveProfile>[0]) => Promise<void>;
  removeProfile: (profileId: string) => Promise<void>;
  connect: (profileId: string) => Promise<void>;
  switchDatabase: (profileId: string, database: string) => Promise<void>;
  disconnect: (profileId: string) => Promise<void>;
  openTable: (profileId: string, schema: string, table: string) => void;
  openQuery: (profileId: string) => void;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
};

export const useDbvStore = create<DbvStore>((set, get) => ({
  profiles: [],
  workspaces: {},
  activeProfileId: null,
  tabs: [],
  activeTabId: null,
  loadProfiles: async () => {
    set({ profiles: await commands.listProfiles() });
  },
  saveProfile: async (input) => {
    await commands.saveProfile(input);
    set({ profiles: await commands.listProfiles() });
  },
  removeProfile: async (profileId) => {
    await commands.deleteProfile(profileId);
    const { workspaces, activeProfileId } = get();
    const nextWorkspaces = { ...workspaces };
    delete nextWorkspaces[profileId];
    set({
      profiles: await commands.listProfiles(),
      workspaces: nextWorkspaces,
      activeProfileId: activeProfileId === profileId ? null : activeProfileId,
    });
  },
  connect: async (profileId) => {
    const workspace = await commands.connectProfile(profileId);
    set((state) => ({
      workspaces: { ...state.workspaces, [profileId]: workspace },
      activeProfileId: profileId,
    }));
  },
  switchDatabase: async (profileId, database) => {
    const workspace = await commands.connectDatabase(profileId, database);
    set((state) => ({ workspaces: { ...state.workspaces, [profileId]: workspace }, activeProfileId: profileId }));
  },
  disconnect: async (profileId) => {
    await commands.disconnectWorkspace(profileId);
    const { workspaces } = get();
    const nextWorkspaces = { ...workspaces };
    delete nextWorkspaces[profileId];
    set((state) => ({
      workspaces: nextWorkspaces,
      activeProfileId: state.activeProfileId === profileId ? null : state.activeProfileId,
      tabs: state.tabs.filter((tab) => tab.profileId !== profileId),
      activeTabId: state.activeProfileId === profileId ? null : state.activeTabId,
    }));
  },
  openTable: (profileId, schema, table) => {
    const existing = get().tabs.find((tab) => tab.kind === "table" && tab.profileId === profileId && tab.schema === schema && tab.table === table);
    if (existing) {
      set({ activeTabId: existing.id });
      return;
    }
    const tab: Tab = { id: crypto.randomUUID(), title: `${schema}.${table}`, kind: "table", profileId, schema, table };
    set((state) => ({ tabs: [...state.tabs, tab], activeTabId: tab.id }));
  },
  openQuery: (profileId) => {
    const tab: Tab = { id: crypto.randomUUID(), title: "Query", kind: "query", profileId, sql: "SELECT now();" };
    set((state) => ({ tabs: [...state.tabs, tab], activeTabId: tab.id }));
  },
  closeTab: (tabId) => {
    const { tabs, activeTabId } = get();
    const index = tabs.findIndex((tab) => tab.id === tabId);
    const nextTabs = tabs.filter((tab) => tab.id !== tabId);
    set({ tabs: nextTabs, activeTabId: activeTabId === tabId ? nextTabs[Math.max(index - 1, 0)]?.id ?? null : activeTabId });
  },
  setActiveTab: (tabId) => set({ activeTabId: tabId }),
}));
