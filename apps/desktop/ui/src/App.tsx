import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import CodeMirror, { Decoration, ViewPlugin, type DecorationSet, type EditorView, type ReactCodeMirrorRef, type ViewUpdate } from "@uiw/react-codemirror";
import { MySQL, PostgreSQL, sql } from "@codemirror/lang-sql";
import {
  AlertCircle,
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Clock3,
  Database,
  FileCode2,
  Info,
  Layers3,
  LockKeyhole,
  Minimize2,
  MoreHorizontal,
  PanelLeftClose,
  PanelLeftOpen,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  ShieldCheck,
  Square,
  Table2,
  X,
} from "lucide-react";

import * as commands from "./commands";
import { parseConnectionUrl } from "./connectionUrl";
import { sqlExecutionTarget, type SqlExecutionTarget } from "./sqlSelection";
import { useDbmStore } from "./store";
import { TableView } from "./TableView";
import type {
  ConnectionProfile,
  DatabaseEngine,
  JsonValue,
  ProfileSummary,
  QueryHistoryEntry,
  QueryResponse,
  SaveProfileInput,
  SchemaNode,
  UpdateStatus,
} from "./types";

const DEFAULT_CONNECTION_COLOR = "#38bdf8";
const CONNECTION_COLORS = ["#38bdf8", "#34d399", "#a78bfa", "#f59e0b", "#fb7185", "#94a3b8"];
const DEFAULT_SIDEBAR_WIDTH = 304;
const MIN_SIDEBAR_WIDTH = 260;
const MAX_SIDEBAR_WIDTH = 440;
const COLLAPSED_SIDEBAR_WIDTH = 54;
const QUERY_HISTORY_UPDATED_EVENT = "dbm:query-history-updated";
const SQL_IDENTIFIER = String.raw`(?:"(?:[^"]|"")*"|` + "`(?:[^`]|``)*`" + String.raw`|[A-Za-z_][A-Za-z0-9_$]*)`;
const SIMPLE_FULL_TABLE_SELECT = new RegExp(
  String.raw`^\s*select\s+\*\s+from\s+(${SQL_IDENTIFIER})(?:\s*\.\s*(${SQL_IDENTIFIER}))?\s*;?\s*$`,
  "i",
);

type QueryHistoryUpdatedDetail = {
  profileId: string;
  database: string;
};

const ENGINE_PRESETS: Record<DatabaseEngine, {
  label: string;
  name: string;
  port: number;
  username: string;
  defaultDatabase: string;
  urlPlaceholder: string;
}> = {
  postgres: {
    label: "PostgreSQL",
    name: "Local PostgreSQL",
    port: 5432,
    username: "postgres",
    defaultDatabase: "postgres",
    urlPlaceholder: "postgresql://user:password@host:5432/database",
  },
  mysql: {
    label: "MySQL",
    name: "Local MySQL",
    port: 3306,
    username: "root",
    defaultDatabase: "mysql",
    urlPlaceholder: "mysql://user:password@host:3306/database",
  },
};

function defaultProfile(profile?: ConnectionProfile): SaveProfileInput {
  const engine = profile?.engine ?? "postgres";
  const preset = ENGINE_PRESETS[engine];
  return {
    id: profile?.id,
    name: profile?.name ?? preset.name,
    color: profile?.color ?? DEFAULT_CONNECTION_COLOR,
    engine,
    host: profile?.host ?? "localhost",
    port: profile?.port ?? preset.port,
    username: profile?.username ?? preset.username,
    defaultDatabase: profile?.defaultDatabase ?? preset.defaultDatabase,
    tlsMode: profile?.tlsMode ?? "preferred",
    caCertPath: profile?.caCertPath ?? null,
    ssh: profile?.ssh ?? null,
    readOnly: profile?.readOnly ?? false,
    password: null,
  };
}

function applyEngineDefaults(form: SaveProfileInput, engine: DatabaseEngine): SaveProfileInput {
  const previous = ENGINE_PRESETS[form.engine];
  const next = ENGINE_PRESETS[engine];
  return {
    ...form,
    engine,
    name: form.name === previous.name ? next.name : form.name,
    port: form.port === previous.port ? next.port : form.port,
    username: form.username === previous.username ? next.username : form.username,
    defaultDatabase: form.defaultDatabase === previous.defaultDatabase ? next.defaultDatabase : form.defaultDatabase,
  };
}

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
  const [modalProfile, setModalProfile] = useState<ConnectionProfile | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const saved = Number(window.localStorage.getItem("dbm.sidebarWidth"));
    return Number.isFinite(saved) ? Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, saved)) : DEFAULT_SIDEBAR_WIDTH;
  });
  const [sidebarCollapsed, setSidebarCollapsed] = useState(() => window.localStorage.getItem("dbm.sidebarCollapsed") === "true");
  const [collapsedConnectionIds, setCollapsedConnectionIds] = useState<Set<string>>(new Set());
  const [mountedTabIds, setMountedTabIds] = useState<Set<string>>(new Set());
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [tabTitleDraft, setTabTitleDraft] = useState("");
  const [refreshingSchemaId, setRefreshingSchemaId] = useState<string | null>(null);
  const [toast, setToast] = useState<{ kind: "info" | "success"; message: string } | null>(null);

  useEffect(() => {
    void loadProfiles().catch((reason: unknown) => setError(errorMessage(reason)));
  }, [loadProfiles]);

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

  const handleDatabaseChange = useCallback(async (database: string) => {
    if (!activeProfileId) return;
    try {
      setError(null);
      await switchDatabase(activeProfileId, database);
      const tree = await commands.loadSchemaTree(activeProfileId);
      setSchemas((current) => ({ ...current, [activeProfileId]: tree }));
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }, [activeProfileId, switchDatabase]);

  const handleRefreshSchema = useCallback(async (profileId: string) => {
    setRefreshingSchemaId(profileId);
    setError(null);
    try {
      const previous = schemas[profileId] ?? [];
      const next = await commands.loadSchemaTree(profileId);
      setSchemas((current) => ({ ...current, [profileId]: next }));
      const summary = describeSchemaRefresh(previous, next);
      setToast({
        kind: summary.changed ? "success" : "info",
        message: summary.message,
      });
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setRefreshingSchemaId(null);
    }
  }, [schemas]);

  const handleSaveProfile = useCallback(async (input: SaveProfileInput) => {
    const saved = await saveProfile(input);
    setModalProfile(undefined);
    await handleConnect(saved.id);
  }, [handleConnect, saveProfile]);

  const startSidebarResize = (event: ReactMouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = sidebarWidth;
    const move = (moveEvent: MouseEvent) => {
      setSidebarWidth(Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, startWidth + moveEvent.clientX - startX)));
    };
    const stop = () => {
      document.body.classList.remove("resizing-sidebar");
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", stop);
    };
    document.body.classList.add("resizing-sidebar");
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", stop);
  };

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

  const startRenamingTab = (tabId: string, title: string) => {
    setRenamingTabId(tabId);
    setTabTitleDraft(title);
  };

  const finishRenamingTab = (tabId: string) => {
    if (renamingTabId !== tabId) return;
    renameTab(tabId, tabTitleDraft);
    setRenamingTabId(null);
  };

  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  const activeTable = activeTab?.kind === "table" && activeTab.schema && activeTab.table
    ? {
        profileId: activeTab.profileId,
        schema: activeTab.schema,
        table: activeTab.table,
      }
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

  return (
    <div className="app-shell">
      <aside
        className={`sidebar ${sidebarCollapsed ? "collapsed" : ""}`}
        style={{ width: sidebarCollapsed ? COLLAPSED_SIDEBAR_WIDTH : sidebarWidth, minWidth: sidebarCollapsed ? COLLAPSED_SIDEBAR_WIDTH : sidebarWidth }}
      >
        <div className="brand-row">
          {!sidebarCollapsed ? <div className="brand-mark" aria-hidden="true"><Database size={17} strokeWidth={2} /></div> : null}
          {!sidebarCollapsed ? <div className="brand-copy">
            <strong>DBM</strong>
            <span>Local database studio</span>
          </div> : null}
          <button
            className="sidebar-collapse-button"
            onClick={() => setSidebarCollapsed((value) => !value)}
            title={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
            aria-label={sidebarCollapsed ? "Expand sidebar" : "Collapse sidebar"}
          >{sidebarCollapsed ? <PanelLeftOpen size={15} /> : <PanelLeftClose size={15} />}</button>
        </div>
        {!sidebarCollapsed ? <>
        <div className="sidebar-section-title">
          <span>Connections</span>
          <button className="sidebar-new-button" onClick={() => setModalProfile(null)}><Plus size={13} />New</button>
        </div>
        <div className="connection-list">
          {profiles.length === 0 ? (
            <div className="empty-sidebar">
              <div className="empty-sidebar-icon"><Database size={16} /></div>
              <strong>No connections yet</strong>
              <span>Add PostgreSQL or MySQL to begin.</span>
            </div>
          ) : profiles.map((summary) => {
            const profileId = summary.profile.id;
            const workspace = workspaces[profileId];
            const active = activeProfileId === profileId;
            const expanded = active && Boolean(workspace) && !collapsedConnectionIds.has(profileId);
            return (
              <div
                className={`connection-group ${active ? "active" : ""} ${workspace ? "connected" : ""} ${expanded ? "expanded" : ""}`}
                key={profileId}
                style={{
                  "--connection-color": summary.profile.color ?? DEFAULT_CONNECTION_COLOR,
                } as CSSProperties}
              >
                <ConnectionItem
                  summary={summary}
                  connected={Boolean(workspace)}
                  active={active}
                  onSelect={() => handleSelectProfile(profileId, Boolean(workspace))}
                  onDisconnect={() => void handleDisconnect(profileId)}
                  onEdit={() => setModalProfile(summary.profile)}
                  expandable={active && Boolean(workspace)}
                  expanded={expanded}
                  onToggleExpanded={() => toggleConnectionExpanded(profileId)}
                />
                {expanded && workspace ? (
                  <div className="workspace-panel">
                    <label className="field-label" htmlFor={`database-select-${profileId}`}>Database</label>
                    <select
                      id={`database-select-${profileId}`}
                      className="select-input"
                      value={workspace.profile.defaultDatabase}
                      onChange={(event) => void handleDatabaseChange(event.target.value)}
                    >
                      {workspace.databases.map((database) => <option key={database.name}>{database.name}</option>)}
                    </select>
                    <div className="schema-heading">
                      <span>Explorer</span>
                      <button
                        className="text-button"
                        onClick={() => void handleRefreshSchema(profileId)}
                        disabled={refreshingSchemaId === profileId}
                      ><RefreshCw size={11} className={refreshingSchemaId === profileId ? "spin" : ""} />{refreshingSchemaId === profileId ? "Refreshing…" : "Refresh"}</button>
                    </div>
                    <div className="schema-tree">
                      {(schemas[profileId] ?? []).map((node) => (
                        <SchemaBranch
                          key={`${node.kind}-${node.name}`}
                          node={node}
                          onTable={(schema, table) => handleOpenTable(profileId, schema, table)}
                          selectedTable={activeTable?.profileId === profileId ? activeTable : null}
                        />
                      ))}
                    </div>
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
        <div className="sidebar-footer">
          <LockKeyhole size={11} aria-hidden="true" />
          <span>Private workspace</span>
          <span className="privacy-chip">Local</span>
        </div>
        <div
          className="sidebar-resize-handle"
          role="separator"
          aria-label="Resize sidebar"
          aria-orientation="vertical"
          aria-valuemin={MIN_SIDEBAR_WIDTH}
          aria-valuemax={MAX_SIDEBAR_WIDTH}
          aria-valuenow={sidebarWidth}
          tabIndex={0}
          onMouseDown={startSidebarResize}
          onDoubleClick={() => setSidebarWidth(DEFAULT_SIDEBAR_WIDTH)}
          onKeyDown={(event) => {
            if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
            event.preventDefault();
            setSidebarWidth((width) => Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, width + (event.key === "ArrowLeft" ? -10 : 10))));
          }}
        />
        </> : null}
      </aside>

      <main className="main-pane" style={connectionStyle}>
        <header className="topbar">
          {activeViewProfile ? (
            <div className="connection-identity">
              <span className="connection-avatar"><Database size={15} /></span>
              <span>
                <strong>{activeViewProfile.name}</strong>
                <small>{activeViewProfile.username}@{activeViewProfile.host}:{activeViewProfile.port}</small>
              </span>
              <span className="database-chip"><span className="status-dot" />{activeViewProfile.defaultDatabase}</span>
              {activeViewProfile.readOnly ? <span className="read-only-topbar-chip"><LockKeyhole size={10} />Read only</span> : null}
            </div>
          ) : <div className="workspace-identity"><Database size={14} /><span>Workspace</span><small>No active connection</small></div>}
          <UpdateControl />
        </header>
        {error ? <div className="error-banner" role="alert"><AlertCircle size={14} /><span>{error}</span><button onClick={() => setError(null)} aria-label="Dismiss error"><X size={14} /></button></div> : null}
        <div className="tab-strip">
          {tabs.map((tab) => (
            <div
              className={`tab ${tab.id === activeTabId ? "active" : ""} ${tab.collapsed ? "collapsed" : ""}`}
              key={tab.id}
              style={{
                "--tab-color": profiles.find((summary) => summary.profile.id === tab.profileId)?.profile.color ?? DEFAULT_CONNECTION_COLOR,
              } as CSSProperties}
            >
              {tab.collapsed ? (
                <button
                  className="tab-expand"
                  onClick={() => handleSetActiveTab(tab.id)}
                  title={`Expand ${tab.title}`}
                  aria-label={`Expand ${tab.title}`}
                >
                  <span className="tab-collapse-glyph" aria-hidden="true"><ChevronRight size={12} /></span>
                  <span className="collapsed-tab-name">{tab.title}</span>
                </button>
              ) : renamingTabId === tab.id ? (
                <input
                  className="tab-title-input"
                  autoFocus
                  value={tabTitleDraft}
                  aria-label={`Rename ${tab.title}`}
                  onChange={(event) => setTabTitleDraft(event.target.value)}
                  onBlur={() => finishRenamingTab(tab.id)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter") event.currentTarget.blur();
                    if (event.key === "Escape") {
                      event.preventDefault();
                      setRenamingTabId(null);
                      setTabTitleDraft(tab.title);
                    }
                  }}
                />
              ) : (
                <button
                  className="tab-title"
                  onClick={() => handleSetActiveTab(tab.id)}
                  onDoubleClick={() => {
                    if (tab.kind === "query") startRenamingTab(tab.id, tab.title);
                  }}
                >
                  {tab.kind === "query" ? <FileCode2 size={13} /> : <Table2 size={13} />}
                  <span>{tab.title}</span>
                </button>
              )}
              {!tab.collapsed && tab.kind === "query" ? (
                <button
                  className="tab-action tab-rename"
                  onClick={() => startRenamingTab(tab.id, tab.title)}
                  title={`Rename ${tab.title}`}
                  aria-label={`Rename ${tab.title}`}
                ><Pencil size={11} /></button>
              ) : null}
              {!tab.collapsed && tab.id === activeTabId ? (
                <button
                  className="tab-action tab-collapse"
                  onClick={() => handleCollapseTab(tab.id)}
                  title={`Collapse ${tab.title}`}
                  aria-label={`Collapse ${tab.title}`}
                ><Minimize2 size={11} /></button>
              ) : null}
              <button className="tab-close" onClick={() => handleCloseTab(tab.id)} aria-label={`Close ${tab.title}`}><X size={12} /></button>
            </div>
          ))}
          {activeWorkspace && activeProfileId ? (
            <button
              className="new-query-tab"
              title="New query"
              aria-label="New query"
              onClick={() => handleOpenQuery(activeProfileId)}
            ><Plus size={15} /></button>
          ) : null}
        </div>
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
              onNewConnection={() => setModalProfile(null)}
              onConnect={selectedProfile ? () => void handleConnect(selectedProfile.id) : undefined}
              onNewQuery={selectedProfile && activeWorkspace ? () => handleOpenQuery(selectedProfile.id) : undefined}
            />
          ) : null}
        </section>
      </main>
      {modalProfile !== undefined ? (
        <ProfileModal
          key={modalProfile?.id ?? "new-profile"}
          profile={modalProfile}
          onClose={() => setModalProfile(undefined)}
          onSave={handleSaveProfile}
          onDelete={modalProfile ? async () => {
            const confirmed = window.confirm(
              `Delete connection “${modalProfile.name}”? Saved password and query history for this profile will be removed.`,
            );
            if (!confirmed) return;
            try {
              await removeProfile(modalProfile.id);
              setModalProfile(undefined);
            } catch (reason) {
              setError(errorMessage(reason));
            }
          } : undefined}
        />
      ) : null}
      {toast ? <div className={`app-toast ${toast.kind}`} role="status">
        {toast.kind === "success" ? <CheckCircle2 size={14} /> : <Info size={14} />}
        <span>{toast.message}</span>
        <button onClick={() => setToast(null)} aria-label="Dismiss notification"><X size={13} /></button>
      </div> : null}
    </div>
  );
}

function UpdateControl() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  const [actionError, setActionError] = useState("");

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void commands.getUpdateStatus()
      .then((next) => {
        if (active) setStatus(next);
      })
      .catch(() => undefined);
    void commands.listenForUpdateStatus((next) => {
      if (active) setStatus(next);
    }).then((dispose) => {
      if (active) unlisten = dispose;
      else dispose();
    }).catch(() => undefined);
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const run = async () => {
    setActionError("");
    try {
      if (status?.state === "available") {
        if (status.installable) {
          const confirmed = window.confirm(
            `Install DBM ${status.version} and restart now? Unsaved query text and pending table edits will be lost.`,
          );
          if (!confirmed) return;
        }
        await commands.installUpdate();
      } else {
        setStatus(await commands.checkForUpdates());
      }
    } catch (reason) {
      setActionError(errorMessage(reason));
    }
  };

  const downloading = status?.state === "downloading";
  const progress = downloading && status.total
    ? Math.min(100, Math.round((status.downloaded / status.total) * 100))
    : null;
  const label = status?.state === "checking"
    ? "Checking…"
    : status?.state === "available"
      ? status.installable ? `Update to ${status.version}` : `Download ${status.version}`
      : downloading
        ? progress === null ? "Installing…" : `Installing… ${progress}%`
        : status?.state === "up_to_date"
          ? "Up to date"
          : status?.state === "error" || actionError
            ? "Retry update"
            : "Check for updates";
  const detail = actionError ||
    (status?.state === "error" ? status.message : status?.state === "available" ? status.notes : "");

  return (
    <div className="update-control">
      <button
        type="button"
        className={status?.state === "available" ? "update-available" : ""}
        disabled={status?.state === "checking" || downloading}
        onClick={() => void run()}
        title={detail || `Installed version ${status?.current_version ?? "…"}`}
      >
        <RefreshCw size={11} className={status?.state === "checking" || downloading ? "spin" : ""} />
        {label}
      </button>
      {actionError ? <span className="update-control-error" role="alert" title={actionError}>!</span> : null}
    </div>
  );
}

function ConnectionItem({
  summary,
  connected,
  active,
  onSelect,
  onDisconnect,
  onEdit,
  expandable,
  expanded,
  onToggleExpanded,
}: {
  summary: ProfileSummary;
  connected: boolean;
  active: boolean;
  onSelect: () => void;
  onDisconnect: () => void;
  onEdit: () => void;
  expandable: boolean;
  expanded: boolean;
  onToggleExpanded: () => void;
}) {
  const [actionsOpen, setActionsOpen] = useState(false);
  const actionsRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!actionsOpen) return;
    const closeOnPointerDown = (event: PointerEvent) => {
      if (!actionsRef.current?.contains(event.target as Node)) setActionsOpen(false);
    };
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") setActionsOpen(false);
    };
    window.addEventListener("pointerdown", closeOnPointerDown);
    window.addEventListener("keydown", closeOnEscape);
    return () => {
      window.removeEventListener("pointerdown", closeOnPointerDown);
      window.removeEventListener("keydown", closeOnEscape);
    };
  }, [actionsOpen]);

  return (
    <div className="connection-entry" ref={actionsRef}>
      <div className={`connection-item ${active ? "active" : ""}`}>
        <button
          className="connection-main"
          onClick={() => { setActionsOpen(false); onSelect(); }}
          aria-current={active ? "page" : undefined}
          title={connected ? undefined : "Connect"}
        >
          <span className="connection-icon" style={{ color: summary.profile.color ?? DEFAULT_CONNECTION_COLOR }}>
            <Database size={15} strokeWidth={1.8} />
          </span>
          <span className="connection-copy">
            <strong>{summary.profile.name}</strong>
            <small>{summary.profile.defaultDatabase} · {summary.profile.host}</small>
          </span>
          <span className={`connection-status ${connected ? "online" : ""}`} title={connected ? "Connected" : ENGINE_PRESETS[summary.profile.engine ?? "postgres"].label} />
        </button>
        <div className="connection-actions">
          {expandable ? <button
            className="icon-button subtle connection-toggle"
            title={expanded ? "Collapse connection" : "Expand connection"}
            aria-label={expanded ? "Collapse connection" : "Expand connection"}
            aria-expanded={expanded}
            onClick={() => { setActionsOpen(false); onToggleExpanded(); }}
          >{expanded ? <ChevronUp size={13} /> : <ChevronDown size={13} />}</button> : null}
          <button
            className="icon-button subtle"
            title="Connection actions"
            aria-label={`Connection actions for ${summary.profile.name}`}
            aria-haspopup="menu"
            aria-expanded={actionsOpen}
            onClick={() => setActionsOpen((open) => !open)}
          ><MoreHorizontal size={14} /></button>
        </div>
      </div>
      {actionsOpen ? <div className="connection-actions-menu" role="menu">
        <button role="menuitem" onClick={() => { setActionsOpen(false); onEdit(); }}><Pencil size={12} />Edit connection</button>
        {connected ? <button
          role="menuitem"
          className="danger"
          title="Close this connection and its tabs"
          onClick={() => { setActionsOpen(false); onDisconnect(); }}
        ><Square size={11} />Disconnect</button> : null}
      </div> : null}
    </div>
  );
}

function SchemaBranch({
  node,
  onTable,
  selectedTable,
  depth = 0,
}: {
  node: SchemaNode;
  onTable: (schema: string, table: string) => void;
  selectedTable: { schema: string; table: string } | null;
  depth?: number;
}) {
  const [open, setOpen] = useState(depth < 1);
  const isTable = Boolean(node.table && node.schema);
  const selected = isTable &&
    selectedTable?.schema === node.schema &&
    selectedTable.table === node.table;
  return (
    <div className="schema-node">
      <button
        className={`schema-node-button ${selected ? "active" : ""}`}
        style={{ paddingLeft: `${10 + depth * 14}px` }}
        aria-current={selected ? "page" : undefined}
        onClick={() => {
          if (isTable) onTable(node.schema!, node.table!);
          else setOpen((value) => !value);
        }}>
        <span className="tree-caret" aria-hidden="true">{isTable ? null : open ? <ChevronDown size={11} /> : <ChevronRight size={11} />}</span>
        <span className={`schema-icon ${node.kind}`} aria-hidden="true">{isTable ? <Table2 size={12} /> : <Layers3 size={12} />}</span>
        <span className="truncate">{node.name}</span>
      </button>
      {open && node.children.length > 0 ? node.children.map((child) => (
        <SchemaBranch
          key={`${child.kind}-${child.name}`}
          node={child}
          onTable={onTable}
          selectedTable={selectedTable}
          depth={depth + 1}
        />
      )) : null}
    </div>
  );
}

function Welcome({
  hasProfiles,
  profile,
  connected,
  onNewConnection,
  onConnect,
  onNewQuery,
}: {
  hasProfiles: boolean;
  profile: ConnectionProfile | null;
  connected: boolean;
  onNewConnection: () => void;
  onConnect?: () => void;
  onNewQuery?: () => void;
}) {
  const title = profile
    ? connected ? `${profile.name} is ready` : `Reconnect to ${profile.name}`
    : hasProfiles ? "Choose a connection" : "Connect your first database";
  const detail = profile
    ? connected
      ? "Open a table from the explorer or start a fresh SQL query."
      : "Connect to restore this database workspace."
    : hasProfiles
      ? "Select a saved connection to open its private workspace."
      : "A focused workspace for PostgreSQL and MySQL, stored entirely on this device.";
  return (
    <div className="welcome">
      <div className="welcome-visual" aria-hidden="true">
        <div className="welcome-glow" />
        <div className="welcome-mark"><Database size={25} strokeWidth={1.6} /></div>
      </div>
      <span className="welcome-kicker">DBM WORKSPACE</span>
      <h1>{title}</h1>
      <p>{detail}</p>
      <div className="welcome-actions">
        {!profile || !hasProfiles ? <button className="primary-button welcome-primary" onClick={onNewConnection}><Plus size={14} />New connection</button> : null}
        {profile && !connected && onConnect ? <button className="primary-button welcome-primary" onClick={onConnect}><Play size={13} />Connect</button> : null}
        {profile && connected && onNewQuery ? <button className="primary-button welcome-primary" onClick={onNewQuery}><FileCode2 size={14} />New query</button> : null}
      </div>
      <div className="welcome-trust"><ShieldCheck size={13} /><span>Profiles and query history stay on this device</span></div>
    </div>
  );
}

const activeSqlStatement = ViewPlugin.fromClass(class {
  decorations: DecorationSet;

  constructor(view: EditorView) {
    this.decorations = activeSqlDecorations(view);
  }

  update(update: ViewUpdate) {
    if (update.docChanged || update.selectionSet) this.decorations = activeSqlDecorations(update.view);
  }
}, {
  decorations: (plugin) => plugin.decorations,
});

function activeSqlDecorations(view: EditorView): DecorationSet {
  const selection = view.state.selection.main;
  if (!selection.empty) return Decoration.none;
  const target = sqlExecutionTarget(view.state.doc.toString(), selection.from, selection.to);
  if (!target) return Decoration.none;

  const firstLine = view.state.doc.lineAt(target.from);
  const lastLine = view.state.doc.lineAt(Math.max(target.from, target.to - 1));
  const decorations = [];
  for (let lineNumber = firstLine.number; lineNumber <= lastLine.number; lineNumber += 1) {
    const classes = [
      "cm-active-sql-line",
      lineNumber === firstLine.number ? "cm-active-sql-start" : "",
      lineNumber === lastLine.number ? "cm-active-sql-end" : "",
    ].filter(Boolean).join(" ");
    decorations.push(Decoration.line({ attributes: { class: classes } }).range(view.state.doc.line(lineNumber).from));
  }
  return Decoration.set(decorations);
}

export function QueryView({
  profileId,
  database = "postgres",
  engine = "postgres",
  schemaTree = [],
  initialSql,
  title = "Query",
}: {
  profileId: string;
  database?: string;
  engine?: DatabaseEngine;
  schemaTree?: SchemaNode[];
  initialSql: string;
  title?: string;
}) {
  const [sqlText, setSqlText] = useState(initialSql);
  const [response, setResponse] = useState<QueryResponse | null>(null);
  const [executedSql, setExecutedSql] = useState<string | null>(null);
  const [executionRevision, setExecutionRevision] = useState(0);
  const [history, setHistory] = useState<QueryHistoryEntry[]>([]);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [embeddedPendingCount, setEmbeddedPendingCount] = useState(0);
  const [executionTarget, setExecutionTarget] = useState<SqlExecutionTarget | null>(() => sqlExecutionTarget(initialSql, 0, 0));
  const editorRef = useRef<ReactCodeMirrorRef>(null);

  const refreshHistory = useCallback(async () => {
    try {
      setHistory(await commands.listQueryHistory(profileId, database));
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }, [database, profileId]);

  useEffect(() => {
    const initialLoadTimer = window.setTimeout(() => void refreshHistory(), 0);
    const handleHistoryUpdated = (event: Event) => {
      const detail = (event as CustomEvent<QueryHistoryUpdatedDetail>).detail;
      if (detail.profileId === profileId && detail.database === database) void refreshHistory();
    };
    window.addEventListener(QUERY_HISTORY_UPDATED_EVENT, handleHistoryUpdated);
    return () => {
      window.clearTimeout(initialLoadTimer);
      window.removeEventListener(QUERY_HISTORY_UPDATED_EVENT, handleHistoryUpdated);
    };
  }, [database, profileId, refreshHistory]);

  useEffect(() => {
    if (!error) return;
    const timer = window.setTimeout(() => setError(null), 10_000);
    return () => window.clearTimeout(timer);
  }, [error]);

  const run = useCallback(async (statement?: string) => {
    if (running) return;
    if (embeddedPendingCount > 0) {
      setError("Save or discard the pending table changes before running another query.");
      return;
    }
    const executableSql = (statement ?? sqlText).trim();
    if (!executableSql) return;
    if (requiresConfirmation(executableSql) && !window.confirm("This query may change or remove many rows. Run it anyway?")) return;
    setRunning(true);
    setError(null);
    try {
      const next = await commands.runQuery({ profileId, sql: executableSql, maxRows: 10_000 });
      setResponse(next);
      setExecutedSql(executableSql);
      setExecutionRevision((current) => current + 1);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      window.dispatchEvent(new CustomEvent<QueryHistoryUpdatedDetail>(
        QUERY_HISTORY_UPDATED_EVENT,
        { detail: { profileId, database } },
      ));
      setRunning(false);
    }
  }, [database, embeddedPendingCount, profileId, running, sqlText]);

  const selectedOrCurrentStatement = useCallback((view?: EditorView) => {
    if (!view) return sqlText.trim();
    const selection = view.state.selection.main;
    return sqlExecutionTarget(view.state.doc.toString(), selection.from, selection.to)?.sql ?? "";
  }, [sqlText]);

  const editorExtensions = useMemo(
    () => [sql({ dialect: engine === "mysql" ? MySQL : PostgreSQL }), activeSqlStatement],
    [engine],
  );

  const runFromEditor = () => {
    void run(selectedOrCurrentStatement(editorRef.current?.view));
  };

  const handleEditorKeyDownCapture = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "Enter" || (!event.metaKey && !event.ctrlKey)) return;
    event.preventDefault();
    event.stopPropagation();
    runFromEditor();
  };

  const handleEditorUpdate = (update: ViewUpdate) => {
    if (!update.docChanged && !update.selectionSet) return;
    const selection = update.state.selection.main;
    setExecutionTarget(sqlExecutionTarget(update.state.doc.toString(), selection.from, selection.to));
  };

  const runLabel = executionTarget?.kind === "selection" ? "Run selection" : "Run statement";
  const editableTable = executedSql ? resolveFullTableSelect(executedSql, schemaTree) : null;

  return (
    <div className="query-view">
      <div className="view-toolbar">
        <div className="view-heading">
          <span className="view-icon"><FileCode2 size={15} /></span>
          <div><span className="eyebrow">SQL WORKBENCH</span><h2>{title}</h2></div>
        </div>
        <div className="toolbar-actions">
          {running ? (
            <button
              className="secondary-button"
              onClick={() => void commands.cancelQuery().catch((reason: unknown) => setError(errorMessage(reason)))}
            ><Square size={11} />Cancel</button>
          ) : null}
          <button
            className="secondary-button"
            onClick={() => void run(executedSql ?? undefined)}
            disabled={running || !executedSql || embeddedPendingCount > 0}
            title={executedSql
              ? "Re-run the last executed statement for fresh results"
              : "Run a statement first to enable refresh"}
          ><RefreshCw size={12} className={running && executedSql ? "spin" : ""} />{running && executedSql ? "Refreshing…" : "Refresh"}</button>
          <button className="primary-button" onClick={runFromEditor} disabled={running || !executionTarget}>
            <Play size={12} fill="currentColor" />{running ? "Running…" : runLabel}<kbd>{runShortcutGlyph()}</kbd>
          </button>
        </div>
      </div>
      <div className="query-layout">
        <div className="editor-panel" onKeyDownCapture={handleEditorKeyDownCapture}>
          <div className="panel-chrome">
            <span><span className="status-dot" />{database}</span>
            <small>{engine === "mysql" ? "MySQL" : "PostgreSQL"}</small>
          </div>
          <CodeMirror ref={editorRef} className="query-code-editor" value={sqlText} height="100%" theme="dark" extensions={editorExtensions} onChange={setSqlText} onUpdate={handleEditorUpdate} basicSetup={{ lineNumbers: true, foldGutter: false }} />
          {executionTarget?.kind === "selection" ? <button
            className="editor-selection-run"
            onMouseDown={(event) => event.preventDefault()}
            onClick={runFromEditor}
            disabled={running}
            title="Run the selected SQL (Command/Ctrl+Enter)"
          ><Play size={10} fill="currentColor" />Run selection</button> : null}
          <div className="editor-hint"><span>The highlighted statement or selection will run</span><span><kbd>{runShortcutGlyph()}</kbd> Results capped at 10,000 rows</span></div>
        </div>
        <aside className="history-panel"><div className="panel-title"><span className="panel-title-label"><Clock3 size={12} />History</span><span>{history.length}</span></div>{history.length === 0 ? <div className="history-empty"><Clock3 size={17} /><p>Executed queries will appear here.</p></div> : <div className="history-list">{history.slice(0, 100).map((entry) => <button className="history-item" key={entry.id} onClick={() => setSqlText(entry.sql)}><span className={`history-status ${entry.success ? "success" : "error"}`}>{entry.success ? <CheckCircle2 size={11} /> : <AlertCircle size={11} />}</span><span className="history-sql">{entry.sql.replace(/\s+/g, " ").slice(0, 70)}</span><small>{new Date(entry.executedAt).toLocaleTimeString()}</small></button>)}</div>}</aside>
      </div>
      {error ? <div className="inline-error dismissible-message"><AlertCircle size={14} /><span>{error}</span><button onClick={() => setError(null)} aria-label="Dismiss message"><X size={13} /></button></div> : null}
      {response ? editableTable ? <div className="result-panel editable-query-result">
        <div className="result-meta">
          <span className="result-label"><Table2 size={12} />Table result <small>Completed in {response.durationMs} ms</small></span>
          <span className="editable-result-chip">Editable table</span>
        </div>
        <TableView
          key={`${profileId}:${editableTable.schema}:${editableTable.table}:${executionRevision}`}
          profileId={profileId}
          schema={editableTable.schema}
          table={editableTable.table}
          onPendingChange={setEmbeddedPendingCount}
        />
      </div> : <div className="result-panel">
        <div className="result-meta">
          <span className="result-label"><Table2 size={12} />Results <small>{response.rowCount} rows{response.affectedRows !== null ? ` · ${response.affectedRows} affected` : ""} · {response.durationMs} ms</small></span>
          <span className="result-meta-actions">
            {response.truncated ? <span className="warning-chip">truncated</span> : null}
            {response.columns.length > 0 ? <span
              className="read-only-result-chip"
              title="This query does not resolve to one complete table, so DBM cannot safely map edits back to rows."
            >Read-only result</span> : null}
          </span>
        </div>
        <ResultTable columns={response.columns.map((column) => column.name)} rows={response.rows} />
      </div> : <div className="query-empty"><div className="query-empty-icon"><Table2 size={17} /></div><strong>Ready for results</strong><span>Run the current statement to see rows here.</span></div>}
    </div>
  );
}

function ResultTable({ columns, rows }: { columns: string[]; rows: JsonValue[][] }) {
  if (columns.length === 0) return <div className="empty-state"><CheckCircle2 size={17} /><strong>Statement completed</strong><span>No result set was returned.</span></div>;
  return <div className="result-grid-wrap"><table className="data-grid result-grid"><thead><tr>{columns.map((column) => <th key={column}>{column}</th>)}</tr></thead><tbody>{rows.map((row, rowIndex) => <tr key={rowIndex}>{columns.map((column, columnIndex) => <td key={`${column}-${columnIndex}`}><span className={row[columnIndex] === null ? "null-value" : "cell-value"}>{commands.toDisplayValue(row[columnIndex] ?? null)}</span></td>)}</tr>)}</tbody></table></div>;
}

function ProfileModal({
  profile,
  onClose,
  onSave,
  onDelete,
}: {
  profile: ConnectionProfile | null;
  onClose: () => void;
  onSave: (input: SaveProfileInput) => Promise<void>;
  onDelete?: () => Promise<void>;
}) {
  const [form, setForm] = useState<SaveProfileInput>(() => defaultProfile(profile ?? undefined));
  const [connectionUrl, setConnectionUrl] = useState("");
  const [testing, setTesting] = useState(false);
  const [saveStage, setSaveStage] = useState<"testing" | "connecting" | null>(null);
  const [feedback, setFeedback] = useState<{ kind: "success" | "error" | "info"; message: string } | null>(null);
  const saving = saveStage !== null;
  const update = <K extends keyof SaveProfileInput>(key: K, value: SaveProfileInput[K]) => setForm((current) => ({ ...current, [key]: value }));

  useEffect(() => {
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [onClose]);

  const importConnectionUrl = (value: string) => {
    try {
      const imported = parseConnectionUrl(value);
      setForm((current) => ({
        ...current,
        engine: imported.engine,
        name: !profile && (!current.name.trim() || current.name === ENGINE_PRESETS[current.engine].name)
          ? imported.suggestedName
          : current.name,
        host: imported.host,
        port: imported.port,
        username: imported.username,
        defaultDatabase: imported.defaultDatabase,
        tlsMode: imported.tlsMode,
        password: imported.password ?? current.password,
      }));
      setFeedback({ kind: "info", message: "Connection URL imported. Review the details, then save and connect." });
    } catch (reason) {
      setFeedback({ kind: "error", message: errorMessage(reason) });
    }
  };

  const test = async () => {
    setTesting(true);
    setFeedback(null);
    try {
      await commands.testProfile(form);
      setFeedback({ kind: "success", message: "Connection successful." });
    } catch (reason) {
      setFeedback({ kind: "error", message: errorMessage(reason) });
    } finally {
      setTesting(false);
    }
  };

  const save = async () => {
    setSaveStage("testing");
    setFeedback({ kind: "info", message: "Testing connection before saving…" });
    try {
      await commands.testProfile(form);
      setSaveStage("connecting");
      setFeedback({ kind: "success", message: "Connection successful. Saving and connecting…" });
      await onSave(form);
    } catch (reason) {
      setFeedback({ kind: "error", message: errorMessage(reason) });
      setSaveStage(null);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div className="modal-card" role="dialog" aria-modal="true" aria-labelledby="connection-modal-title">
        <div className="modal-header">
          <div className="modal-title-group">
            <span className="modal-title-icon"><Database size={16} /></span>
            <div>
              <h2 id="connection-modal-title">{profile ? "Edit connection" : "New connection"}</h2>
              <p>{profile ? "Update how DBM reaches this database." : "Add a private database workspace."}</p>
            </div>
          </div>
          <button className="icon-button modal-close" onClick={onClose} aria-label="Close"><X size={15} /></button>
        </div>
        <div className="modal-body">
          <section className="modal-section quick-connect-section">
            <div className="modal-section-heading">
              <span>Quick setup</span>
              <small>Paste a URL or choose an engine</small>
            </div>
            <div className="engine-picker" role="group" aria-label="Database engine">
              {(["postgres", "mysql"] as const).map((engine) => (
                <button
                  key={engine}
                  type="button"
                  className={`engine-option ${form.engine === engine ? "selected" : ""}`}
                  aria-pressed={form.engine === engine}
                  aria-label={ENGINE_PRESETS[engine].label}
                  onClick={() => setForm((current) => applyEngineDefaults(current, engine))}
                >
                  <span className="engine-icon"><Database size={14} /></span>
                  <span><strong>{ENGINE_PRESETS[engine].label}</strong><small>{engine === "postgres" ? "Port 5432" : "Port 3306"}</small></span>
                  {form.engine === engine ? <CheckCircle2 size={14} className="engine-check" /> : null}
                </button>
              ))}
            </div>
            <label className="form-field url-form-field">
              <span>Connection URL <small>Optional</small></span>
              <div className="url-field">
                <input
                  className="text-input"
                  type="password"
                  value={connectionUrl}
                  onChange={(event) => setConnectionUrl(event.target.value)}
                  onPaste={(event) => {
                    const pasted = event.clipboardData.getData("text");
                    event.preventDefault();
                    setConnectionUrl(pasted);
                    importConnectionUrl(pasted);
                  }}
                  autoComplete="off"
                  autoCapitalize="none"
                  spellCheck={false}
                  placeholder={ENGINE_PRESETS[form.engine].urlPlaceholder}
                />
                <button className="secondary-button" onClick={() => importConnectionUrl(connectionUrl)} disabled={!connectionUrl.trim()}>Import</button>
              </div>
            </label>
          </section>

          <section className="modal-section">
            <div className="modal-section-heading">
              <span>Connection details</span>
              <small>{ENGINE_PRESETS[form.engine].label.toUpperCase()}</small>
            </div>
            <div className="form-grid">
              <label className="form-field full">
                <span>Name</span>
                <input className="text-input" value={form.name} onChange={(event) => update("name", event.target.value)} placeholder={ENGINE_PRESETS[form.engine].name} />
              </label>
              <label className="form-field">
                <span>Host</span>
                <input className="text-input" value={form.host} onChange={(event) => update("host", event.target.value)} />
              </label>
              <label className="form-field">
                <span>Port</span>
                <input className="text-input" type="number" value={form.port} onChange={(event) => update("port", Number(event.target.value))} />
              </label>
              <label className="form-field">
                <span>Username</span>
                <input className="text-input" value={form.username} onChange={(event) => update("username", event.target.value)} />
              </label>
              <label className="form-field">
                <span>Database</span>
                <input className="text-input" value={form.defaultDatabase} onChange={(event) => update("defaultDatabase", event.target.value)} />
              </label>
              <label className="form-field">
                <span>Password</span>
                <input className="text-input" type="password" value={form.password ?? ""} onChange={(event) => update("password", event.target.value || undefined)} placeholder={profile ? "Keep saved password" : "Stored securely"} />
              </label>
              <label className="form-field">
                <span>TLS mode</span>
                <select className="select-input" value={form.tlsMode} onChange={(event) => update("tlsMode", event.target.value as SaveProfileInput["tlsMode"])}>
                  <option value="preferred">Preferred</option>
                  <option value="required">Required</option>
                  <option value="disabled">Disabled</option>
                </select>
              </label>
              <label className="form-field full">
                <span>CA certificate path <small>Optional</small></span>
                <input className="text-input" value={form.caCertPath ?? ""} onChange={(event) => update("caCertPath", event.target.value || null)} placeholder="/path/to/root-ca.pem" />
              </label>
            </div>
          </section>

          <section className="modal-section profile-options-section">
            <div className="modal-section-heading"><span>Workspace identity</span><small>Used throughout DBM</small></div>
            <div className="color-picker">
              {CONNECTION_COLORS.map((color) => (
                <button
                  key={color}
                  className={`color-swatch ${form.color === color ? "selected" : ""}`}
                  style={{ background: color }}
                  onClick={() => update("color", color)}
                  aria-label={`Use connection color ${color}`}
                >
                  {form.color === color ? <CheckCircle2 size={13} /> : null}
                </button>
              ))}
              <label className="custom-color" title="Choose a custom color">
                <input type="color" value={form.color ?? DEFAULT_CONNECTION_COLOR} onChange={(event) => update("color", event.target.value)} />
                <span>Custom</span>
              </label>
            </div>
            <label className="checkbox-field">
              <input type="checkbox" checked={form.readOnly} onChange={(event) => update("readOnly", event.target.checked)} />
              <span><strong>Open as read-only</strong><small>Block edits and mutation queries from the interface.</small></span>
            </label>
          </section>

          {feedback ? <div className={`modal-feedback ${feedback.kind}`} role="status">
            {feedback.kind === "success" ? <CheckCircle2 size={14} /> : feedback.kind === "error" ? <AlertCircle size={14} /> : <Info size={14} />}
            <span>{feedback.message}</span>
          </div> : null}
          <div className="modal-note"><ShieldCheck size={15} /><span><strong>Credentials stay local</strong>Passwords are stored in your operating system credential manager, never in DBM's profile database.</span></div>
        </div>
        <div className="modal-actions">
          {onDelete ? <button className="danger-button" onClick={() => void onDelete()} disabled={testing || saving}>Delete</button> : <span />}
          <div className="modal-actions-right">
            <button className="secondary-button" onClick={() => void test()} disabled={testing || saving}>
              <RefreshCw size={12} className={testing ? "spin" : ""} />{testing ? "Testing…" : "Test connection"}
            </button>
            <button className="primary-button" onClick={() => void save()} disabled={testing || saving}>
              <Play size={12} fill="currentColor" />
              {saveStage === "testing" ? "Testing…" : saveStage === "connecting" ? "Connecting…" : "Save & connect"}
            </button>
          </div>
        </div>
      </div>
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

function resolveFullTableSelect(sqlText: string, schemaTree: SchemaNode[]): { schema: string; table: string } | null {
  const match = SIMPLE_FULL_TABLE_SELECT.exec(sqlText);
  if (!match) return null;
  const firstIdentifier = decodeSqlIdentifier(match[1]);
  const secondIdentifier = match[2] ? decodeSqlIdentifier(match[2]) : null;
  const requestedSchema = secondIdentifier ? firstIdentifier : null;
  const requestedTable = secondIdentifier ?? firstIdentifier;
  const matches: Array<{ schema: string; table: string }> = [];
  const visit = (node: SchemaNode) => {
    if (
      node.kind === "table" &&
      node.schema &&
      node.table === requestedTable &&
      (!requestedSchema || node.schema === requestedSchema)
    ) {
      matches.push({ schema: node.schema, table: node.table });
    }
    node.children.forEach(visit);
  };
  schemaTree.forEach(visit);
  return matches.length === 1 ? matches[0] : null;
}

function decodeSqlIdentifier(identifier: string): string {
  if (identifier.startsWith("\"")) return identifier.slice(1, -1).replaceAll("\"\"", "\"");
  if (identifier.startsWith("`")) return identifier.slice(1, -1).replaceAll("``", "`");
  return identifier.toLowerCase();
}

function runShortcutGlyph(): string {
  return isApplePlatform() ? "⌘↵" : "Ctrl+↵";
}

function isApplePlatform(): boolean {
  if (typeof navigator === "undefined") return false;
  return /Mac|iPhone|iPad|iPod/.test(navigator.platform) || /Mac OS X/.test(navigator.userAgent);
}

function errorMessage(reason: unknown): string {
  const message = reason instanceof Error ? reason.message : String(reason);
  if (/credential error:/i.test(message) && /(cancel|denied|interaction)/i.test(message)) {
    return "DBM could not read this connection's saved password because access to the operating system credential manager was not approved. Approve the system prompt, then select the connection again.";
  }
  return message;
}

function requiresConfirmation(sqlText: string): boolean {
  const normalized = sqlText.replace(/--[^\n]*/g, " ").replace(/\/\*[\s\S]*?\*\//g, " ");
  return /\b(drop|truncate)\b/i.test(normalized) ||
    /\b(delete|update)\b/i.test(normalized) && !/\bwhere\b/i.test(normalized);
}
