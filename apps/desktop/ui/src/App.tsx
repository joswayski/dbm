import { useCallback, useEffect, useMemo, useState, type CSSProperties } from "react";

import { watchSystemAppearance } from "./appearance";
import { CommandPalette, type PaletteCommand } from "./CommandPalette";
import * as commands from "./commands";
import { ConnectionSheet } from "./ConnectionSheet";
import { DEFAULT_CONNECTION_COLOR, enginePreset } from "./engines";
import {
  IconAlertCircle,
  IconCheckCircle,
  IconCode,
  IconDatabase,
  IconInfo,
  IconPanelLeft,
  IconPencil,
  IconPlus,
  IconPower,
  IconRefresh,
  IconTable,
  IconView,
  IconX,
} from "./icons";
import { isPrimaryModifier } from "./platform";
import { QueryView } from "./QueryView";
import { Sidebar, type ActiveTableRef } from "./Sidebar";
import { StatusBar } from "./StatusBar";
import { TabStrip } from "./TabStrip";
import { TableView } from "./TableView";
import { TopBar } from "./TopBar";
import { useDbmStore } from "./store";
import { IconButton } from "./ui";
import { Welcome } from "./Welcome";
import type { ConnectionProfile, SaveProfileInput, SchemaNode } from "./types";

const DEFAULT_SIDEBAR_WIDTH = 288;
const MIN_SIDEBAR_WIDTH = 240;
const MAX_SIDEBAR_WIDTH = 460;

export default function App() {
  const profiles = useDbmStore((state) => state.profiles);
  const workspaces = useDbmStore((state) => state.workspaces);
  const activeProfileId = useDbmStore((state) => state.activeProfileId);
  const tabs = useDbmStore((state) => state.tabs);
  const activeTabId = useDbmStore((state) => state.activeTabId);
  const loadProfiles = useDbmStore((state) => state.loadProfiles);
  const saveProfile = useDbmStore((state) => state.saveProfile);
  const removeProfile = useDbmStore((state) => state.removeProfile);
  const connect = useDbmStore((state) => state.connect);
  const selectProfile = useDbmStore((state) => state.selectProfile);
  const switchDatabase = useDbmStore((state) => state.switchDatabase);
  const disconnect = useDbmStore((state) => state.disconnect);
  const openTable = useDbmStore((state) => state.openTable);
  const openQuery = useDbmStore((state) => state.openQuery);
  const renameTab = useDbmStore((state) => state.renameTab);
  const collapseTab = useDbmStore((state) => state.collapseTab);
  const closeTab = useDbmStore((state) => state.closeTab);
  const setActiveTab = useDbmStore((state) => state.setActiveTab);
  const [schemas, setSchemas] = useState<Record<string, SchemaNode[]>>({});
  const [sheetProfile, setSheetProfile] = useState<ConnectionProfile | null | undefined>(undefined);
  const [paletteOpen, setPaletteOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const saved = Number(window.localStorage.getItem("dbm.sidebarWidth"));
    return Number.isFinite(saved) && saved > 0
      ? Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, saved))
      : DEFAULT_SIDEBAR_WIDTH;
  });
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => window.localStorage.getItem("dbm.sidebarCollapsed") === "true");
  const [collapsedConnectionIds, setCollapsedConnectionIds] = useState<Set<string>>(new Set());
  const [mountedTabIds, setMountedTabIds] = useState<Set<string>>(new Set());
  const [refreshingSchemaId, setRefreshingSchemaId] = useState<string | null>(null);
  const [toast, setToast] = useState<{ kind: "info" | "success"; message: string } | null>(null);

  useEffect(() => {
    void loadProfiles().catch((reason: unknown) => setError(errorMessage(reason)));
  }, [loadProfiles]);

  useEffect(() => watchSystemAppearance(), []);

  useEffect(() => {
    window.localStorage.setItem("dbm.sidebarWidth", String(sidebarWidth));
  }, [sidebarWidth]);

  useEffect(() => {
    window.localStorage.setItem("dbm.sidebarCollapsed", String(sidebarCollapsed));
  }, [sidebarCollapsed]);

  useEffect(() => {
    if (!error) return;
    const timer = window.setTimeout(() => setError(null), 10_000);
    return () => window.clearTimeout(timer);
  }, [error]);

  useEffect(() => {
    if (!toast) return;
    const timer = window.setTimeout(() => setToast(null), 6_000);
    return () => window.clearTimeout(timer);
  }, [toast]);

  const handleConnect = useCallback(async (profileId: string) => {
    try {
      setError(null);
      await connect(profileId);
      const tree = await commands.loadSchemaTree(profileId);
      setSchemas((current) => ({ ...current, [profileId]: tree }));
      setCollapsedConnectionIds((current) => {
        if (!current.has(profileId)) return current;
        const next = new Set(current);
        next.delete(profileId);
        return next;
      });
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }, [connect]);

  const handleDisconnect = useCallback(async (profileId: string) => {
    try {
      await disconnect(profileId);
      setCollapsedConnectionIds((current) => {
        if (!current.has(profileId)) return current;
        const next = new Set(current);
        next.delete(profileId);
        return next;
      });
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }, [disconnect]);

  const handleDatabaseChange = useCallback(async (profileId: string, database: string) => {
    try {
      setError(null);
      await switchDatabase(profileId, database);
      const tree = await commands.loadSchemaTree(profileId);
      setSchemas((current) => ({ ...current, [profileId]: tree }));
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }, [switchDatabase]);

  const handleRefreshSchema = useCallback(async (profileId: string) => {
    setRefreshingSchemaId(profileId);
    setError(null);
    try {
      const previous = schemas[profileId] ?? [];
      const next = await commands.loadSchemaTree(profileId);
      setSchemas((current) => ({ ...current, [profileId]: next }));
      const summary = describeSchemaRefresh(previous, next);
      setToast({ kind: summary.changed ? "success" : "info", message: summary.message });
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setRefreshingSchemaId(null);
    }
  }, [schemas]);

  const handleSaveProfile = useCallback(async (input: SaveProfileInput) => {
    const saved = await saveProfile(input);
    setSheetProfile(undefined);
    await handleConnect(saved.id);
  }, [handleConnect, saveProfile]);

  const rememberTab = (tabId: string | null) => {
    if (!tabId) return;
    setMountedTabIds((current) => {
      if (current.has(tabId)) return current;
      return new Set(current).add(tabId);
    });
  };

  const rememberActiveTab = () => rememberTab(activeTabId);

  const handleSelectProfile = (profileId: string, connected: boolean) => {
    rememberActiveTab();
    if (connected) selectProfile(profileId);
    else void handleConnect(profileId);
  };

  const handleOpenTable = (profileId: string, schema: string, table: string) => {
    rememberActiveTab();
    openTable(profileId, schema, table);
  };

  const handleOpenQuery = (profileId: string) => {
    rememberActiveTab();
    openQuery(profileId);
  };

  const handleSetActiveTab = (tabId: string) => {
    rememberActiveTab();
    setActiveTab(tabId);
  };

  const handleCloseTab = (tabId: string) => {
    setMountedTabIds((current) => {
      if (!current.has(tabId)) return current;
      const next = new Set(current);
      next.delete(tabId);
      return next;
    });
    closeTab(tabId);
  };

  const handleCollapseTab = (tabId: string) => {
    rememberTab(tabId);
    collapseTab(tabId);
  };

  const toggleConnectionExpanded = (profileId: string) => {
    setCollapsedConnectionIds((current) => {
      const next = new Set(current);
      if (next.has(profileId)) next.delete(profileId);
      else next.add(profileId);
      return next;
    });
  };

  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  const activeTable: ActiveTableRef | null = activeTab?.kind === "table" && activeTab.schema && activeTab.table
    ? { profileId: activeTab.profileId, schema: activeTab.schema, table: activeTab.table }
    : null;
  const activeWorkspace = activeProfileId ? workspaces[activeProfileId] : undefined;
  const selectedProfile = activeProfileId
    ? profiles.find((summary) => summary.profile.id === activeProfileId)?.profile
    : undefined;
  const activeViewProfile = (activeTab
    ? profiles.find((summary) => summary.profile.id === activeTab.profileId)?.profile
    : undefined) ?? activeWorkspace?.profile ?? selectedProfile;
  const connectionStyle = activeViewProfile
    ? ({ "--connection-color": activeViewProfile.color ?? DEFAULT_CONNECTION_COLOR } as CSSProperties)
    : undefined;
  const topProfile = activeWorkspace?.profile ?? activeViewProfile ?? null;
  const topConnected = Boolean(topProfile && workspaces[topProfile.id]);

  const paletteCommands = useMemo<PaletteCommand[]>(() => {
    const items: PaletteCommand[] = [];
    for (const summary of profiles) {
      const profile = summary.profile;
      const connected = Boolean(workspaces[profile.id]);
      items.push({
        id: `connection:${profile.id}`,
        group: "Connections",
        label: profile.name,
        meta: `${enginePreset(profile.engine).short} · ${profile.host}`,
        keywords: `${profile.host} ${profile.username} ${profile.defaultDatabase} connect`,
        color: profile.color ?? DEFAULT_CONNECTION_COLOR,
        run: () => handleSelectProfile(profile.id, connected),
      });
    }

    if (activeProfileId && activeWorkspace) {
      for (const database of activeWorkspace.databases) {
        if (database.name === activeWorkspace.profile.defaultDatabase) continue;
        items.push({
          id: `database:${database.name}`,
          group: "Databases",
          label: database.name,
          meta: "switch database",
          icon: <IconDatabase size={14} />,
          run: () => void handleDatabaseChange(activeProfileId, database.name),
        });
      }

      const objects: Array<{ node: SchemaNode }> = [];
      const visit = (node: SchemaNode) => {
        if (node.schema && node.table) objects.push({ node });
        node.children.forEach(visit);
      };
      (schemas[activeProfileId] ?? []).forEach(visit);
      for (const { node } of objects.slice(0, 300)) {
        items.push({
          id: `table:${node.schema}.${node.table}`,
          group: "Tables and views",
          label: node.name,
          meta: node.schema ?? undefined,
          keywords: `${node.schema}.${node.name} ${node.kind}`,
          icon: node.kind === "view" ? <IconView size={14} /> : <IconTable size={14} />,
          run: () => handleOpenTable(activeProfileId, node.schema!, node.table!),
        });
      }
    }

    for (const tab of tabs) {
      if (tab.id === activeTabId) continue;
      items.push({
        id: `tab:${tab.id}`,
        group: "Open tabs",
        label: tab.title,
        meta: tab.kind === "query" ? "query" : "table",
        icon: tab.kind === "query" ? <IconCode size={14} /> : <IconTable size={14} />,
        run: () => handleSetActiveTab(tab.id),
      });
    }

    items.push({
      id: "action:new-connection",
      group: "Actions",
      label: "New connection…",
      icon: <IconPlus size={14} />,
      keywords: "add create profile server",
      run: () => setSheetProfile(null),
    });

    if (activeProfileId && activeWorkspace) {
      items.push({
        id: "action:new-query",
        group: "Actions",
        label: "New query tab",
        icon: <IconCode size={14} />,
        keywords: "sql editor run",
        run: () => handleOpenQuery(activeProfileId),
      }, {
        id: "action:refresh-schema",
        group: "Actions",
        label: "Refresh schema",
        icon: <IconRefresh size={14} />,
        keywords: "reload tables",
        run: () => void handleRefreshSchema(activeProfileId),
      }, {
        id: "action:disconnect",
        group: "Actions",
        label: `Disconnect ${activeWorkspace.profile.name}`,
        icon: <IconPower size={14} />,
        keywords: "close session",
        run: () => void handleDisconnect(activeProfileId),
      });
    }

    if (selectedProfile) {
      items.push({
        id: "action:edit-connection",
        group: "Actions",
        label: `Edit ${selectedProfile.name}…`,
        icon: <IconPencil size={14} />,
        keywords: "settings host port password",
        run: () => setSheetProfile(selectedProfile),
      });
    }

    items.push({
      id: "action:toggle-sidebar",
      group: "Actions",
      label: sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar",
      icon: <IconPanelLeft size={14} />,
      keywords: "layout panel",
      run: () => setSidebarCollapsed((value) => !value),
    });

    return items;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    activeProfileId,
    activeTabId,
    activeWorkspace,
    handleDatabaseChange,
    handleDisconnect,
    handleRefreshSchema,
    profiles,
    schemas,
    selectedProfile,
    sidebarCollapsed,
    tabs,
    workspaces,
  ]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!isPrimaryModifier(event)) return;
      const key = event.key.toLowerCase();
      if (key === "k") {
        event.preventDefault();
        setPaletteOpen((open) => !open);
        return;
      }
      if (key === "t" && activeProfileId && activeWorkspace) {
        event.preventDefault();
        handleOpenQuery(activeProfileId);
        return;
      }
      if (/^[1-9]$/.test(key)) {
        const tab = tabs[Number(key) - 1];
        if (!tab) return;
        event.preventDefault();
        handleSetActiveTab(tab.id);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeProfileId, activeWorkspace, tabs]);

  return (
    <div className="app-shell" style={connectionStyle}>
      <TopBar
        profile={topProfile}
        connected={topConnected}
        engineLabel={topProfile ? enginePreset(topProfile.engine).short : null}
        onOpenPalette={() => setPaletteOpen(true)}
        onNewConnection={() => setSheetProfile(null)}
      />

      <Sidebar
        profiles={profiles}
        workspaces={workspaces}
        activeProfileId={activeProfileId}
        schemas={schemas}
        activeTable={activeTable}
        collapsedConnectionIds={collapsedConnectionIds}
        refreshingSchemaId={refreshingSchemaId}
        collapsed={sidebarCollapsed}
        width={sidebarWidth}
        minWidth={MIN_SIDEBAR_WIDTH}
        maxWidth={MAX_SIDEBAR_WIDTH}
        onToggleCollapsed={() => setSidebarCollapsed((value) => !value)}
        onResize={setSidebarWidth}
        onResetWidth={() => setSidebarWidth(DEFAULT_SIDEBAR_WIDTH)}
        onSelectProfile={handleSelectProfile}
        onToggleExpanded={toggleConnectionExpanded}
        onDisconnect={(profileId) => void handleDisconnect(profileId)}
        onEditProfile={(profile) => setSheetProfile(profile)}
        onNewConnection={() => setSheetProfile(null)}
        onDatabaseChange={(database) => { if (activeProfileId) void handleDatabaseChange(activeProfileId, database); }}
        onRefreshSchema={(profileId) => void handleRefreshSchema(profileId)}
        onOpenTable={handleOpenTable}
      />

      <main className="main-pane" style={connectionStyle}>
        {error ? <div className="error-banner" role="alert">
          <IconAlertCircle size={14} />
          <span>{error}</span>
          <IconButton size="sm" icon={<IconX size={13} />} label="Dismiss error" tooltip={false} onClick={() => setError(null)} />
        </div> : null}
        <TabStrip
          tabs={tabs}
          activeTabId={activeTabId}
          profiles={profiles}
          canOpenQuery={Boolean(activeWorkspace && activeProfileId)}
          onActivate={handleSetActiveTab}
          onRename={renameTab}
          onCollapse={handleCollapseTab}
          onClose={handleCloseTab}
          onNewQuery={() => { if (activeProfileId) handleOpenQuery(activeProfileId); }}
        />
        <section className="content-pane">
          {tabs.map((tab) => {
            const shouldMount = tab.id === activeTabId || mountedTabIds.has(tab.id);
            const tabProfile = workspaces[tab.profileId]?.profile
              ?? profiles.find((summary) => summary.profile.id === tab.profileId)?.profile;
            const database = tabProfile?.defaultDatabase
              ?? (tabProfile?.engine === "mysql" ? "mysql" : "postgres");
            return <div className={`tab-pane ${tab.id === activeTabId ? "active" : ""}`} key={tab.id} aria-hidden={tab.id !== activeTabId}>
              {shouldMount && tab.kind === "table" && tab.schema && tab.table ? (
                <TableView profileId={tab.profileId} schema={tab.schema} table={tab.table} />
              ) : shouldMount && tab.kind === "query" ? (
                <QueryView
                  profileId={tab.profileId}
                  database={database}
                  engine={tabProfile?.engine ?? "postgres"}
                  schemaTree={schemas[tab.profileId] ?? []}
                  initialSql={tab.sql ?? "SELECT now();"}
                  title={tab.title}
                />
              ) : null}
            </div>;
          })}
          {!activeTab ? (
            <Welcome
              hasProfiles={profiles.length > 0}
              profile={selectedProfile ?? null}
              connected={Boolean(activeWorkspace)}
              onNewConnection={() => setSheetProfile(null)}
              onConnect={() => { if (activeProfileId) void handleConnect(activeProfileId); }}
              onOpenPalette={() => setPaletteOpen(true)}
            />
          ) : null}
        </section>
      </main>

      <StatusBar
        profile={topProfile}
        connected={topConnected}
        engineLabel={topProfile ? enginePreset(topProfile.engine).label : null}
        tabCount={tabs.length}
      />

      {sheetProfile !== undefined ? (
        <ConnectionSheet
          key={sheetProfile?.id ?? "new-profile"}
          profile={sheetProfile}
          onClose={() => setSheetProfile(undefined)}
          onSave={handleSaveProfile}
          onDelete={sheetProfile ? async () => {
            const confirmed = window.confirm(
              `Delete connection “${sheetProfile.name}”? Saved password and query history for this profile will be removed.`,
            );
            if (!confirmed) return;
            try {
              await removeProfile(sheetProfile.id);
              setSheetProfile(undefined);
            } catch (reason) {
              setError(errorMessage(reason));
            }
          } : undefined}
        />
      ) : null}

      {paletteOpen ? (
        <CommandPalette commands={paletteCommands} onClose={() => setPaletteOpen(false)} />
      ) : null}

      {toast ? <div className="toast-stack">
        <div className={`toast toast-${toast.kind}`} role="status">
          {toast.kind === "success" ? <IconCheckCircle size={14} /> : <IconInfo size={14} />}
          <span>{toast.message}</span>
          <IconButton size="sm" icon={<IconX size={13} />} label="Dismiss notification" tooltip={false} onClick={() => setToast(null)} />
        </div>
      </div> : null}
    </div>
  );
}

function describeSchemaRefresh(previous: SchemaNode[], next: SchemaNode[]): { changed: boolean; message: string } {
  const previousObjects = schemaObjects(previous);
  const nextObjects = schemaObjects(next);
  const previousKeys = new Set(previousObjects.map((object) => object.key));
  const nextKeys = new Set(nextObjects.map((object) => object.key));
  const added = nextObjects.filter((object) => !previousKeys.has(object.key)).map((object) => object.label);
  const removed = previousObjects.filter((object) => !nextKeys.has(object.key)).map((object) => object.label);
  if (added.length === 0 && removed.length === 0) {
    return { changed: false, message: "Schema is already up to date." };
  }

  const changes = [
    added.length > 0 ? `Added ${summarizeSchemaObjects(added)}` : null,
    removed.length > 0 ? `Removed ${summarizeSchemaObjects(removed)}` : null,
  ].filter((change): change is string => Boolean(change));
  return { changed: true, message: `Schema refreshed · ${changes.join(" · ")}.` };
}

function schemaObjects(nodes: SchemaNode[]): Array<{ key: string; label: string }> {
  const objects: Array<{ key: string; label: string }> = [];
  const visit = (node: SchemaNode) => {
    const qualifiedName = node.schema && node.table ? `${node.schema}.${node.table}` : node.name;
    objects.push({
      key: `${node.kind}:${qualifiedName}`,
      label: `${node.kind} ${qualifiedName}`,
    });
    node.children.forEach(visit);
  };
  nodes.forEach(visit);
  return objects.sort((left, right) => left.key.localeCompare(right.key));
}

function summarizeSchemaObjects(labels: string[]): string {
  if (labels.length === 1) return labels[0];
  const visible = labels.slice(0, 3);
  const remainder = labels.length - visible.length;
  return `${labels.length} objects: ${visible.join(", ")}${remainder > 0 ? `, and ${remainder} more` : ""}`;
}

function errorMessage(reason: unknown): string {
  const message = reason instanceof Error ? reason.message : String(reason);
  if (/credential error:/i.test(message) && /(cancel|denied|interaction)/i.test(message)) {
    return "DBM could not read this connection's saved password because access to the operating system credential manager was not approved. Approve the system prompt, then select the connection again.";
  }
  return message;
}
