import { useCallback, useEffect, useState, type CSSProperties } from "react";
import CodeMirror from "@uiw/react-codemirror";
import { sql } from "@codemirror/lang-sql";

import * as commands from "./commands";
import { parsePostgresConnectionUrl } from "./connectionUrl";
import { useDbvStore } from "./store";
import type {
  ConnectionProfile,
  JsonValue,
  MutationBatch,
  ProfileSummary,
  QueryHistoryEntry,
  QueryResponse,
  SaveProfileInput,
  SchemaNode,
  TablePage,
} from "./types";

const PAGE_SIZE = 200;
const DEFAULT_CONNECTION_COLOR = "#38bdf8";
const CONNECTION_COLORS = ["#38bdf8", "#22c55e", "#a78bfa", "#f59e0b", "#ef4444", "#64748b"];

function defaultProfile(profile?: ConnectionProfile): SaveProfileInput {
  return {
    id: profile?.id,
    name: profile?.name ?? "Local PostgreSQL",
    color: profile?.color ?? DEFAULT_CONNECTION_COLOR,
    host: profile?.host ?? "localhost",
    port: profile?.port ?? 5432,
    username: profile?.username ?? "postgres",
    defaultDatabase: profile?.defaultDatabase ?? "postgres",
    tlsMode: profile?.tlsMode ?? "preferred",
    caCertPath: profile?.caCertPath ?? null,
    ssh: profile?.ssh ?? null,
    readOnly: profile?.readOnly ?? false,
    password: null,
  };
}

export default function App() {
  const profiles = useDbvStore((state) => state.profiles);
  const workspaces = useDbvStore((state) => state.workspaces);
  const activeProfileId = useDbvStore((state) => state.activeProfileId);
  const tabs = useDbvStore((state) => state.tabs);
  const activeTabId = useDbvStore((state) => state.activeTabId);
  const loadProfiles = useDbvStore((state) => state.loadProfiles);
  const saveProfile = useDbvStore((state) => state.saveProfile);
  const removeProfile = useDbvStore((state) => state.removeProfile);
  const connect = useDbvStore((state) => state.connect);
  const switchDatabase = useDbvStore((state) => state.switchDatabase);
  const disconnect = useDbvStore((state) => state.disconnect);
  const openTable = useDbvStore((state) => state.openTable);
  const openQuery = useDbvStore((state) => state.openQuery);
  const closeTab = useDbvStore((state) => state.closeTab);
  const setActiveTab = useDbvStore((state) => state.setActiveTab);
  const [schemas, setSchemas] = useState<Record<string, SchemaNode[]>>({});
  const [modalProfile, setModalProfile] = useState<ConnectionProfile | null | undefined>(undefined);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void loadProfiles().catch((reason: unknown) => setError(errorMessage(reason)));
  }, [loadProfiles]);

  const handleConnect = useCallback(async (profileId: string) => {
    try {
      setError(null);
      await connect(profileId);
      const tree = await commands.loadSchemaTree(profileId);
      setSchemas((current) => ({ ...current, [profileId]: tree }));
    } catch (reason) {
      setError(errorMessage(reason));
    }
  }, [connect]);

  const handleDisconnect = useCallback(async (profileId: string) => {
    try {
      await disconnect(profileId);
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

  const handleSaveProfile = useCallback(async (input: SaveProfileInput) => {
    const saved = await saveProfile(input);
    setModalProfile(undefined);
    await handleConnect(saved.id);
  }, [handleConnect, saveProfile]);

  const activeTab = tabs.find((tab) => tab.id === activeTabId) ?? null;
  const activeWorkspace = activeProfileId ? workspaces[activeProfileId] : undefined;
  const activeViewProfile = (activeTab
    ? profiles.find((summary) => summary.profile.id === activeTab.profileId)?.profile
    : undefined) ?? activeWorkspace?.profile;
  const connectionStyle = activeViewProfile
    ? ({ "--connection-color": activeViewProfile.color ?? DEFAULT_CONNECTION_COLOR } as CSSProperties)
    : undefined;

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-row">
          <div className="brand-mark">DB</div>
          <div>
            <strong>DBV</strong>
            <span>database viewer</span>
          </div>
        </div>
        <div className="sidebar-section-title">
          <span>Connections</span>
          <button className="sidebar-new-button" onClick={() => setModalProfile(null)}>New connection</button>
        </div>
        <div className="connection-list">
          {profiles.length === 0 ? (
            <div className="empty-sidebar">No saved connections.</div>
          ) : profiles.map((summary) => (
            <ConnectionItem
              key={summary.profile.id}
              summary={summary}
              connected={Boolean(workspaces[summary.profile.id])}
              active={activeProfileId === summary.profile.id}
              onConnect={() => void handleConnect(summary.profile.id)}
              onDisconnect={() => void handleDisconnect(summary.profile.id)}
              onEdit={() => setModalProfile(summary.profile)}
            />
          ))}
        </div>
        {activeWorkspace && activeProfileId ? (
          <div className="workspace-panel">
            <div className="workspace-heading">
              <span className="status-dot" />
              <span className="truncate">{activeWorkspace.profile.name}</span>
              <button className="icon-button subtle" title="New query" onClick={() => openQuery(activeProfileId)}>＋</button>
            </div>
            <label className="field-label" htmlFor="database-select">Database</label>
            <select id="database-select" className="select-input" value={activeWorkspace.profile.defaultDatabase} onChange={(event) => void handleDatabaseChange(event.target.value)}>
              {activeWorkspace.databases.map((database) => <option key={database.name}>{database.name}</option>)}
            </select>
            <div className="schema-heading">
              <span>Schema</span>
              <button className="text-button" onClick={() => void commands.loadSchemaTree(activeProfileId).then((tree) => setSchemas((current) => ({ ...current, [activeProfileId]: tree })))}>Refresh</button>
            </div>
            <div className="schema-tree">
              {(schemas[activeProfileId] ?? []).map((node) => (
                <SchemaBranch key={`${node.kind}-${node.name}`} node={node} onTable={(schema, table) => openTable(activeProfileId, schema, table)} />
              ))}
            </div>
          </div>
        ) : null}
        <div className="sidebar-footer">
          <span className="privacy-chip">LOCAL ONLY</span>
        </div>
      </aside>

      <main className={`main-pane ${activeViewProfile ? "connection-themed" : ""}`} style={connectionStyle}>
        <header className="topbar">
          {activeViewProfile ? (
            <div className="connection-identity">
              <span className="connection-identity-dot" />
              <span>
                <strong>{activeViewProfile.name}</strong>
                <small>{activeViewProfile.username}@{activeViewProfile.host}:{activeViewProfile.port}/{activeViewProfile.defaultDatabase}</small>
              </span>
            </div>
          ) : <div className="breadcrumb">No active connection</div>}
          <div className="topbar-actions">
            {activeProfileId ? <button className="secondary-button" onClick={() => openQuery(activeProfileId)}>New query</button> : null}
          </div>
        </header>
        {error ? <div className="error-banner"><span>{error}</span><button onClick={() => setError(null)}>×</button></div> : null}
        <div className="tab-strip">
          {tabs.map((tab) => (
            <div
              className={`tab ${tab.id === activeTabId ? "active" : ""}`}
              key={tab.id}
              style={{
                "--tab-color": profiles.find((summary) => summary.profile.id === tab.profileId)?.profile.color ?? DEFAULT_CONNECTION_COLOR,
              } as CSSProperties}
            >
              <span className="tab-color" />
              <button onClick={() => setActiveTab(tab.id)}>{tab.kind === "table" ? "▦" : "⌘"} {tab.title}</button>
              <button className="tab-close" onClick={() => closeTab(tab.id)} aria-label={`Close ${tab.title}`}>×</button>
            </div>
          ))}
        </div>
        <section className="content-pane">
          {activeTab?.kind === "table" && activeTab.schema && activeTab.table ? (
            <TableView key={activeTab.id} profileId={activeTab.profileId} schema={activeTab.schema} table={activeTab.table} />
          ) : activeTab?.kind === "query" ? (
            <QueryView key={activeTab.id} profileId={activeTab.profileId} initialSql={activeTab.sql ?? "SELECT now();"} />
          ) : (
            <Welcome hasProfiles={profiles.length > 0} />
          )}
        </section>
      </main>
      {modalProfile !== undefined ? (
        <ProfileModal
          key={modalProfile?.id ?? "new-profile"}
          profile={modalProfile}
          onClose={() => setModalProfile(undefined)}
          onSave={handleSaveProfile}
          onDelete={modalProfile ? async () => {
            try {
              await removeProfile(modalProfile.id);
              setModalProfile(undefined);
            } catch (reason) {
              setError(errorMessage(reason));
            }
          } : undefined}
        />
      ) : null}
    </div>
  );
}

function ConnectionItem({
  summary,
  connected,
  active,
  onConnect,
  onDisconnect,
  onEdit,
}: {
  summary: ProfileSummary;
  connected: boolean;
  active: boolean;
  onConnect: () => void;
  onDisconnect: () => void;
  onEdit: () => void;
}) {
  return (
    <div className={`connection-item ${active ? "active" : ""}`}>
      <button className="connection-main" onClick={connected ? undefined : onConnect}>
        <span className="connection-color" style={{ background: summary.profile.color ?? DEFAULT_CONNECTION_COLOR, color: summary.profile.color ?? DEFAULT_CONNECTION_COLOR }} />
        <span className="connection-copy">
          <strong>{summary.profile.name}</strong>
          <small>{summary.profile.username}@{summary.profile.host}</small>
        </span>
        {connected ? <span className="connected-label">connected</span> : null}
      </button>
      <div className="connection-actions">
        <button className="icon-button subtle" title={connected ? "Disconnect" : "Connect"} onClick={connected ? onDisconnect : onConnect}>{connected ? "●" : "▷"}</button>
        <button className="icon-button subtle" title="Edit connection" onClick={onEdit}>⋯</button>
      </div>
    </div>
  );
}

function SchemaBranch({ node, onTable, depth = 0 }: { node: SchemaNode; onTable: (schema: string, table: string) => void; depth?: number }) {
  const [open, setOpen] = useState(depth < 1);
  const isTable = Boolean(node.table && node.schema);
  return (
    <div className="schema-node">
      <button className="schema-node-button" style={{ paddingLeft: `${10 + depth * 14}px` }} onClick={() => {
        if (isTable) onTable(node.schema!, node.table!);
        else setOpen((value) => !value);
      }}>
        <span className="tree-caret">{isTable ? "▧" : open ? "⌄" : "›"}</span>
        <span className={`schema-icon ${node.kind}`}>{isTable ? "T" : "S"}</span>
        <span className="truncate">{node.name}</span>
      </button>
      {open && node.children.length > 0 ? node.children.map((child) => (
        <SchemaBranch key={`${child.kind}-${child.name}`} node={child} onTable={onTable} depth={depth + 1} />
      )) : null}
    </div>
  );
}

function Welcome({ hasProfiles }: { hasProfiles: boolean }) {
  return (
    <div className="welcome">
      <div className="welcome-mark">DB<span>V</span></div>
      <h1>No connection selected</h1>
      <p>{hasProfiles
        ? "Select a saved connection from the sidebar to browse its data."
        : "Create a connection from the sidebar to get started."}</p>
    </div>
  );
}

function TableView({ profileId, schema, table }: { profileId: string; schema: string; table: string }) {
  const [page, setPage] = useState<TablePage | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [searchColumn, setSearchColumn] = useState("");
  const [searchTerm, setSearchTerm] = useState("");
  const [appliedSearch, setAppliedSearch] = useState("");
  const [staged, setStaged] = useState<Record<number, JsonValue[]>>({});
  const [deleted, setDeleted] = useState<Set<number>>(new Set());
  const [editing, setEditing] = useState<{ row: number; column: number } | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const nextPage = await commands.loadTablePage({
        profileId,
        schema,
        table,
        offset: pageIndex * PAGE_SIZE,
        limit: PAGE_SIZE,
        filters: appliedSearch && searchColumn ? [{ column: searchColumn, operator: "contains", value: appliedSearch }] : [],
        orderBy: null,
      });
      setPage(nextPage);
      setStaged({});
      setDeleted(new Set());
      setEditing(null);
      if (!searchColumn && nextPage.metadata.columns[0]) setSearchColumn(nextPage.metadata.columns[0].name);
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setLoading(false);
    }
  }, [appliedSearch, pageIndex, profileId, schema, searchColumn, table]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void load(); }, 0);
    return () => window.clearTimeout(timer);
  }, [load]);

  const editable = Boolean(page?.metadata.primaryKey.length);
  const displayRows = page?.rows ?? [];
  const visibleColumns = page?.metadata.columns ?? [];

  const setCell = (rowIndex: number, columnIndex: number, value: string) => {
    if (!page) return;
    const next = [...(staged[rowIndex] ?? page.rows[rowIndex].slice(0, visibleColumns.length))];
    next[columnIndex] = parseCell(value);
    setStaged((current) => ({ ...current, [rowIndex]: next }));
  };

  const save = async () => {
    if (!page || Object.keys(staged).length === 0 && deleted.size === 0) return;
    setSaving(true);
    const mutations: MutationBatch["mutations"] = [...new Set([...Object.keys(staged).map(Number), ...deleted])].map((rowIndex) => {
      const original = page.rows[rowIndex].slice(0, visibleColumns.length);
      const changes = staged[rowIndex] ?? original;
      const primaryKey = page.metadata.primaryKey.map((key) => original[visibleColumns.findIndex((column) => column.name === key)]);
      return {
        original,
        changes,
        primaryKey,
        xmin: toStringValue(page.rows[rowIndex][visibleColumns.length]),
        deleted: deleted.has(rowIndex),
      };
    });
    try {
      const result = await commands.applyTableMutations({ profileId, schema, table, mutations });
      if (result.conflicts.length > 0) setError(`${result.conflicts.length} row conflict(s); refresh before saving again.`);
      await load();
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setSaving(false);
    }
  };

  const exportRows = (format: "csv" | "tsv") => {
    if (!page) return;
    const delimiter = format === "csv" ? "," : "\t";
    const escape = (value: JsonValue) => {
      const text = commands.toDisplayValue(value);
      return format === "csv" && /[",\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
    };
    const body = [visibleColumns.map((column) => escape(column.name)).join(delimiter), ...displayRows.map((row, rowIndex) => visibleColumns.map((_, columnIndex) => escape(staged[rowIndex]?.[columnIndex] ?? row[columnIndex])).join(delimiter))].join("\n");
    if (format === "tsv") {
      void navigator.clipboard?.writeText(body);
      return;
    }
    const url = URL.createObjectURL(new Blob([body], { type: "text/csv" }));
    const anchor = document.createElement("a");
    anchor.href = url;
    anchor.download = `${table}.csv`;
    anchor.click();
    URL.revokeObjectURL(url);
  };

  return (
    <div className="table-view">
      <div className="view-toolbar">
        <div><span className="eyebrow">TABLE</span><h2>{schema}.{table}</h2></div>
        <div className="toolbar-actions">
          <button className="secondary-button" onClick={() => exportRows("tsv")} disabled={!page}>Copy TSV</button>
          <button className="secondary-button" onClick={() => exportRows("csv")} disabled={!page}>Export CSV</button>
          {editable ? <><button className="secondary-button" onClick={() => { setStaged({}); setDeleted(new Set()); }}>Revert</button><button className="primary-button" onClick={() => void save()} disabled={saving || (Object.keys(staged).length === 0 && deleted.size === 0)}>{saving ? "Saving…" : "Save changes"}</button></> : null}
        </div>
      </div>
      <div className="filter-bar">
        <select className="select-input compact" value={searchColumn} onChange={(event) => setSearchColumn(event.target.value)} disabled={!page}>
          {visibleColumns.map((column) => <option key={column.name} value={column.name}>{column.name}</option>)}
        </select>
        <input className="text-input" placeholder="Filter contains…" value={searchTerm} onChange={(event) => setSearchTerm(event.target.value)} onKeyDown={(event) => { if (event.key === "Enter") { setPageIndex(0); setAppliedSearch(searchTerm); } }} />
        <button className="secondary-button" onClick={() => { setPageIndex(0); setAppliedSearch(searchTerm); }}>Apply</button>
        {page ? <span className="row-count">{page.totalRows ?? "—"} rows · {page.metadata.columns.length} columns</span> : null}
      </div>
      {error ? <div className="inline-error">{error}</div> : null}
      <div className="grid-wrap">
        {loading ? <div className="loading-state">Loading rows…</div> : page && displayRows.length > 0 ? (
          <table className="data-grid">
            <thead><tr>{visibleColumns.map((column) => <th key={column.name}>{column.name}<small>{column.dataType}</small></th>)}{editable ? <th aria-label="row actions" /> : null}</tr></thead>
            <tbody>{displayRows.map((row, rowIndex) => {
              const values = staged[rowIndex] ?? row.slice(0, visibleColumns.length);
              return <tr key={`${rowIndex}-${String(row[0])}`} className={deleted.has(rowIndex) ? "deleted-row" : staged[rowIndex] ? "staged-row" : ""}>
                {visibleColumns.map((column, columnIndex) => {
                  const isEditing = editing?.row === rowIndex && editing.column === columnIndex;
                  const isPrimaryKey = page.metadata.primaryKey.includes(column.name);
                  return <td key={column.name} onDoubleClick={() => editable && !isPrimaryKey && !deleted.has(rowIndex) && setEditing({ row: rowIndex, column: columnIndex })}>
                    {isEditing ? <input autoFocus className="cell-input" value={commands.toDisplayValue(values[columnIndex]) === "NULL" ? "" : commands.toDisplayValue(values[columnIndex])} onChange={(event) => setCell(rowIndex, columnIndex, event.target.value)} onBlur={() => setEditing(null)} onKeyDown={(event) => { if (event.key === "Enter") setEditing(null); }} /> : <span className={values[columnIndex] === null ? "null-value" : "cell-value"}>{commands.toDisplayValue(values[columnIndex])}</span>}
                  </td>;
                })}
                {editable ? <td><button className="row-delete" title={deleted.has(rowIndex) ? "Undo delete" : "Stage delete"} onClick={() => setDeleted((current) => { const next = new Set(current); if (next.has(rowIndex)) next.delete(rowIndex); else next.add(rowIndex); return next; })}>{deleted.has(rowIndex) ? "↶" : "×"}</button></td> : null}
              </tr>;
            })}</tbody>
          </table>
        ) : <div className="empty-state">No rows match this view.</div>}
      </div>
      <div className="pagination"><button className="secondary-button" disabled={pageIndex === 0 || loading} onClick={() => setPageIndex((value) => value - 1)}>← Previous</button><span>Page {pageIndex + 1}</span><button className="secondary-button" disabled={!page?.hasMore || loading} onClick={() => setPageIndex((value) => value + 1)}>Next →</button></div>
    </div>
  );
}

function QueryView({ profileId, initialSql }: { profileId: string; initialSql: string }) {
  const [sqlText, setSqlText] = useState(initialSql);
  const [response, setResponse] = useState<QueryResponse | null>(null);
  const [history, setHistory] = useState<QueryHistoryEntry[]>([]);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    void commands.listQueryHistory(profileId).then(setHistory).catch((reason: unknown) => setError(errorMessage(reason)));
  }, [profileId]);

  const run = async () => {
    if (requiresConfirmation(sqlText) && !window.confirm("This query may change or remove many rows. Run it anyway?")) return;
    setRunning(true);
    setError(null);
    try {
      const next = await commands.runQuery({ profileId, sql: sqlText, maxRows: 10_000 });
      setResponse(next);
      setHistory(await commands.listQueryHistory(profileId));
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setRunning(false);
    }
  };

  return (
    <div className="query-view">
      <div className="view-toolbar"><div><span className="eyebrow">SQL WORKBENCH</span><h2>Query</h2></div><div className="toolbar-actions"><button className="secondary-button" onClick={() => void commands.cancelQuery().catch((reason: unknown) => setError(errorMessage(reason)))} disabled={!running}>Cancel</button><button className="primary-button" onClick={() => void run()} disabled={running}>{running ? "Running…" : "Run query"}<kbd>⌘↵</kbd></button></div></div>
      <div className="query-layout">
        <div className="editor-panel"><CodeMirror value={sqlText} height="260px" theme="dark" extensions={[sql()]} onChange={setSqlText} basicSetup={{ lineNumbers: true, foldGutter: false }} /><div className="editor-hint">Dedicated session · results capped at 10,000 rows · writes are enabled</div></div>
        <aside className="history-panel"><div className="panel-title">History</div>{history.length === 0 ? <p className="muted">Run a query to start history.</p> : history.slice(0, 12).map((entry) => <button className="history-item" key={entry.id} onClick={() => setSqlText(entry.sql)}><span>{entry.success ? "✓" : "!"}</span><span className="history-sql">{entry.sql.replace(/\s+/g, " ").slice(0, 70)}</span><small>{new Date(entry.executedAt).toLocaleTimeString()}</small></button>)}</aside>
      </div>
      {error ? <div className="inline-error">{error}</div> : null}
      {response ? <div className="result-panel"><div className="result-meta"><span>{response.rowCount} rows{response.affectedRows !== null ? ` · ${response.affectedRows} affected` : ""} · {response.durationMs} ms</span>{response.truncated ? <span className="warning-chip">truncated</span> : null}</div><ResultTable columns={response.columns.map((column) => column.name)} rows={response.rows} /></div> : <div className="query-empty">Results will appear here.</div>}
    </div>
  );
}

function ResultTable({ columns, rows }: { columns: string[]; rows: JsonValue[][] }) {
  if (columns.length === 0) return <div className="empty-state">Statement completed without a result set.</div>;
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
  const [saving, setSaving] = useState(false);
  const [feedback, setFeedback] = useState<{ kind: "success" | "error" | "info"; message: string } | null>(null);
  const update = <K extends keyof SaveProfileInput>(key: K, value: SaveProfileInput[K]) => setForm((current) => ({ ...current, [key]: value }));

  const importConnectionUrl = (value: string) => {
    try {
      const imported = parsePostgresConnectionUrl(value);
      setForm((current) => ({
        ...current,
        name: !profile && (!current.name.trim() || current.name === "Local PostgreSQL")
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
    setSaving(true);
    setFeedback(null);
    try {
      await onSave(form);
    } catch (reason) {
      setFeedback({ kind: "error", message: errorMessage(reason) });
      setSaving(false);
    }
  };

  return (
    <div className="modal-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}>
      <div className="modal-card">
        <div className="modal-header">
          <div><span className="eyebrow">POSTGRESQL</span><h2>{profile ? "Edit connection" : "New connection"}</h2></div>
          <button className="icon-button" onClick={onClose} aria-label="Close">×</button>
        </div>
        <div className="form-grid">
          <label className="form-field full">
            <span>Connection URL</span>
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
                placeholder="postgresql://user:password@host:5432/database"
              />
              <button className="secondary-button" onClick={() => importConnectionUrl(connectionUrl)} disabled={!connectionUrl.trim()}>Import URL</button>
            </div>
          </label>
          <label className="form-field full">
            <span>Name</span>
            <input className="text-input" value={form.name} onChange={(event) => update("name", event.target.value)} placeholder="Local PostgreSQL" />
          </label>
          <div className="form-field full">
            <span>Connection color</span>
            <div className="color-picker">
              {CONNECTION_COLORS.map((color) => (
                <button
                  key={color}
                  className={`color-swatch ${form.color === color ? "selected" : ""}`}
                  style={{ background: color }}
                  onClick={() => update("color", color)}
                  aria-label={`Use connection color ${color}`}
                />
              ))}
              <label className="custom-color" title="Choose a custom color">
                <input type="color" value={form.color ?? DEFAULT_CONNECTION_COLOR} onChange={(event) => update("color", event.target.value)} />
                <span>Custom</span>
              </label>
            </div>
          </div>
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
            <input className="text-input" type="password" value={form.password ?? ""} onChange={(event) => update("password", event.target.value || undefined)} placeholder={profile ? "Leave blank to keep saved password" : "Stored in OS keychain"} />
          </label>
          <label className="form-field">
            <span>TLS</span>
            <select className="select-input" value={form.tlsMode} onChange={(event) => update("tlsMode", event.target.value as SaveProfileInput["tlsMode"])}>
              <option value="preferred">Preferred</option>
              <option value="required">Required</option>
              <option value="disabled">Disabled</option>
            </select>
          </label>
          <label className="form-field full">
            <span>CA certificate path (optional)</span>
            <input className="text-input" value={form.caCertPath ?? ""} onChange={(event) => update("caCertPath", event.target.value || null)} placeholder="/path/to/root-ca.pem" />
          </label>
          <label className="checkbox-field full">
            <input type="checkbox" checked={form.readOnly} onChange={(event) => update("readOnly", event.target.checked)} />
            <span>Read-only profile (blocks GUI edits and mutations)</span>
          </label>
        </div>
        {feedback ? <div className={`modal-feedback ${feedback.kind}`} role="status">{feedback.message}</div> : null}
        <div className="modal-note">Passwords are stored in your operating system credential manager and are never written to DBV's profile database.</div>
        <div className="modal-actions">
          {onDelete ? <button className="danger-button" onClick={() => void onDelete()} disabled={testing || saving}>Delete</button> : <span />}
          <div className="modal-actions-right">
            <button className="secondary-button" onClick={() => void test()} disabled={testing || saving}>{testing ? "Testing…" : "Test connection"}</button>
            <button className="primary-button" onClick={() => void save()} disabled={testing || saving}>{saving ? "Connecting…" : "Save & connect"}</button>
          </div>
        </div>
      </div>
    </div>
  );
}

function parseCell(value: string): JsonValue {
  if (value.trim() === "") return null;
  if (value === "true") return true;
  if (value === "false") return false;
  if (/^-?\d+$/.test(value)) return Number(value);
  if (/^-?\d+\.\d+$/.test(value)) return Number(value);
  return value;
}

function toStringValue(value: JsonValue): string | null {
  return value === null ? null : String(value);
}

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

function requiresConfirmation(sqlText: string): boolean {
  const normalized = sqlText.replace(/--[^\n]*/g, " ").replace(/\/\*[\s\S]*?\*\//g, " ");
  return /\b(drop|truncate)\b/i.test(normalized) ||
    /\b(delete|update)\b/i.test(normalized) && !/\bwhere\b/i.test(normalized);
}
