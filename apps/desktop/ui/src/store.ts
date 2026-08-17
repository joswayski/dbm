import { create } from "zustand";

import * as commands from "./commands";
import type { ConnectionProfile, ProfileSummary, Tab, WorkspaceInfo } from "./types";

type DbmStore = {
  profiles: ProfileSummary[];
  workspaces: Record<string, WorkspaceInfo>;
  activeProfileId: string | null;
  tabs: Tab[];
  activeTabId: string | null;
  loadProfiles: () => Promise<void>;
  saveProfile: (input: Parameters<typeof commands.saveProfile>[0]) => Promise<ConnectionProfile>;
  removeProfile: (profileId: string) => Promise<void>;
  connect: (profileId: string) => Promise<void>;
  selectProfile: (profileId: string) => void;
  switchDatabase: (profileId: string, database: string) => Promise<void>;
  disconnect: (profileId: string) => Promise<void>;
  openTable: (profileId: string, schema: string, table: string) => void;
  openQuery: (profileId: string) => void;
  renameTab: (tabId: string, title: string) => void;
  collapseTab: (tabId: string) => void;
  closeTab: (tabId: string) => void;
  setActiveTab: (tabId: string) => void;
};

export const useDbmStore = create<DbmStore>((set, get) => ({
  profiles: [],
  workspaces: {},
  activeProfileId: null,
  tabs: [],
  activeTabId: null,
  loadProfiles: async () => {
    set({ profiles: await commands.listProfiles() });
  },
  saveProfile: async (input) => {
    const saved = await commands.saveProfile(input);
    const profiles = await commands.listProfiles();
    set((state) => {
      const workspaces = { ...state.workspaces };
      delete workspaces[saved.id];
      return {
        profiles,
        workspaces,
        activeProfileId: saved.id,
        activeTabId: state.activeProfileId === saved.id ? state.activeTabId : null,
      };
    });
    return saved;
  },
  removeProfile: async (profileId) => {
    await commands.deleteProfile(profileId);
    const profiles = await commands.listProfiles();
    set((state) => ({
      profiles,
      ...closedProfileWorkspace(state, profileId),
    }));
  },
  connect: async (profileId) => {
    const workspace = await commands.connectProfile(profileId);
    set((state) => ({
      workspaces: { ...state.workspaces, [profileId]: workspace },
      ...activatedProfileWorkbench(state, profileId),
    }));
  },
  selectProfile: (profileId) => {
    if (!get().profiles.some((summary) => summary.profile.id === profileId)) return;
    set((state) => activatedProfileWorkbench(state, profileId));
  },
  switchDatabase: async (profileId, database) => {
    const workspace = await commands.connectDatabase(profileId, database);
    set((state) => ({ workspaces: { ...state.workspaces, [profileId]: workspace }, activeProfileId: profileId }));
  },
  disconnect: async (profileId) => {
    await commands.disconnectWorkspace(profileId);
    set((state) => closedProfileWorkspace(state, profileId));
  },
  openTable: (profileId, schema, table) => {
    const existing = get().tabs.find((tab) => tab.kind === "table" && tab.profileId === profileId && tab.schema === schema && tab.table === table);
    if (existing) {
      set((state) => ({
        tabs: revealTab(state.tabs, existing.id),
        activeTabId: existing.id,
        activeProfileId: profileId,
      }));
      return;
    }
    const tab: Tab = { id: crypto.randomUUID(), title: `${schema}.${table}`, kind: "table", profileId, schema, table };
    set((state) => ({ tabs: [...state.tabs, tab], activeTabId: tab.id, activeProfileId: profileId }));
  },
  openQuery: (profileId) => {
    set((state) => {
      const tab = createQueryTab(profileId, state.tabs);
      return { tabs: [...state.tabs, tab], activeTabId: tab.id, activeProfileId: profileId };
    });
  },
  renameTab: (tabId, title) => {
    const nextTitle = title.trim();
    if (!nextTitle) return;
    set((state) => ({
      tabs: state.tabs.map((tab) => tab.id === tabId ? { ...tab, title: nextTitle } : tab),
    }));
  },
  collapseTab: (tabId) => {
    const { tabs, activeTabId } = get();
    const index = tabs.findIndex((tab) => tab.id === tabId);
    if (index < 0) return;
    const nextActiveTab = activeTabId === tabId ? adjacentTab(tabs, index) : null;
    set((state) => ({
      tabs: state.tabs.map((tab) => {
        if (tab.id === tabId) return { ...tab, collapsed: true };
        if (tab.id === nextActiveTab?.id) return { ...tab, collapsed: false };
        return tab;
      }),
      activeTabId: activeTabId === tabId ? nextActiveTab?.id ?? null : activeTabId,
      activeProfileId: nextActiveTab?.profileId ?? state.activeProfileId,
    }));
  },
  closeTab: (tabId) => {
    const { tabs, activeTabId } = get();
    const index = tabs.findIndex((tab) => tab.id === tabId);
    const nextTabs = tabs.filter((tab) => tab.id !== tabId);
    const nextActiveTab = activeTabId === tabId ? adjacentTab(tabs, index) : null;
    set((state) => ({
      tabs: nextTabs.map((tab) => tab.id === nextActiveTab?.id ? { ...tab, collapsed: false } : tab),
      activeTabId: activeTabId === tabId ? nextActiveTab?.id ?? null : activeTabId,
      activeProfileId: nextActiveTab?.profileId ?? state.activeProfileId,
    }));
  },
  setActiveTab: (tabId) => {
    const tab = get().tabs.find((candidate) => candidate.id === tabId);
    if (tab) set((state) => ({
      tabs: state.tabs.map((candidate) => candidate.id === tabId ? { ...candidate, collapsed: false } : candidate),
      activeTabId: tabId,
      activeProfileId: tab.profileId,
    }));
  },
}));

function adjacentTab(tabs: Tab[], index: number): Tab | null {
  return tabs.slice(index + 1)[0] ?? tabs.slice(0, index).at(-1) ?? null;
}

function latestProfileTab(tabs: Tab[], profileId: string): Tab | undefined {
  return [...tabs].reverse().find((tab) => tab.profileId === profileId);
}

function revealTab(tabs: Tab[], tabId: string): Tab[] {
  return tabs.map((tab) => tab.id === tabId ? { ...tab, collapsed: false } : tab);
}

function createQueryTab(profileId: string, tabs: Tab[]): Tab {
  const existingTitles = new Set(
    tabs
      .filter((tab) => tab.kind === "query" && tab.profileId === profileId)
      .map((tab) => tab.title),
  );
  let queryNumber = 1;
  while (existingTitles.has(`Query ${queryNumber}`)) queryNumber += 1;
  return {
    id: crypto.randomUUID(),
    title: `Query ${queryNumber}`,
    kind: "query",
    profileId,
    sql: "SELECT now();",
  };
}

function activatedProfileWorkbench(
  state: Pick<DbmStore, "tabs" | "activeTabId" | "activeProfileId">,
  profileId: string,
): Pick<DbmStore, "tabs" | "activeTabId" | "activeProfileId"> {
  const currentTab = state.tabs.find((tab) => tab.id === state.activeTabId);
  if (currentTab?.profileId === profileId) {
    return {
      tabs: state.tabs,
      activeTabId: currentTab.id,
      activeProfileId: profileId,
    };
  }

  const nextTab = latestProfileTab(state.tabs, profileId);
  if (nextTab) {
    return {
      tabs: revealTab(state.tabs, nextTab.id),
      activeTabId: nextTab.id,
      activeProfileId: profileId,
    };
  }

  const tab = createQueryTab(profileId, state.tabs);
  return {
    tabs: [...state.tabs, tab],
    activeTabId: tab.id,
    activeProfileId: profileId,
  };
}

function closedProfileWorkspace(
  state: Pick<DbmStore, "workspaces" | "tabs" | "activeTabId" | "activeProfileId">,
  profileId: string,
): Pick<DbmStore, "workspaces" | "tabs" | "activeTabId" | "activeProfileId"> {
  const workspaces = { ...state.workspaces };
  delete workspaces[profileId];
  const leavingActiveProfile = state.activeProfileId === profileId;
  return {
    workspaces,
    tabs: state.tabs.filter((tab) => tab.profileId !== profileId),
    activeProfileId: leavingActiveProfile ? null : state.activeProfileId,
    activeTabId: leavingActiveProfile ? null : state.activeTabId,
  };
}
