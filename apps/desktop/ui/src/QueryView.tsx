import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent } from "react";
import CodeMirror, { Decoration, ViewPlugin, type DecorationSet, type EditorView, type ReactCodeMirrorRef, type ViewUpdate } from "@uiw/react-codemirror";
import { MySQL, PostgreSQL, sql } from "@codemirror/lang-sql";

import { useAppearanceStore } from "./appearance";
import * as commands from "./commands";
import { editorTheme } from "./editorTheme";
import { enginePreset } from "./engines";
import {
  IconAlertCircle,
  IconCheck,
  IconCode,
  IconHistory,
  IconPlay,
  IconRefresh,
  IconStop,
  IconTable,
  IconX,
} from "./icons";
import { runShortcutGlyph } from "./platform";
import { sqlExecutionTarget, type SqlExecutionTarget } from "./sqlSelection";
import { TableView } from "./TableView";
import { Badge, Button, Count, EmptyState, IconButton, Kbd, Notice } from "./ui";
import type { DatabaseEngine, JsonValue, QueryHistoryEntry, QueryResponse, SchemaNode } from "./types";

const QUERY_HISTORY_UPDATED_EVENT = "dbm:query-history-updated";
const EDITOR_HEIGHT_KEY = "dbm.editorHeight";
const HISTORY_OPEN_KEY = "dbm.historyOpen";
const MIN_EDITOR_HEIGHT = 120;
const MAX_EDITOR_HEIGHT = 620;
const SQL_IDENTIFIER = String.raw`(?:"(?:[^"]|"")*"|` + "`(?:[^`]|``)*`" + String.raw`|[A-Za-z_][A-Za-z0-9_$]*)`;
const SIMPLE_FULL_TABLE_SELECT = new RegExp(
  String.raw`^\s*select\s+\*\s+from\s+(${SQL_IDENTIFIER})(?:\s*\.\s*(${SQL_IDENTIFIER}))?\s*;?\s*$`,
  "i",
);

type QueryHistoryUpdatedDetail = {
  profileId: string;
  database: string;
};

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
    decorations.push(Decoration.line({ attributes: { class: "cm-active-sql-line" } })
      .range(view.state.doc.line(lineNumber).from));
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
  const [historyOpen, setHistoryOpen] = useState(() => window.localStorage.getItem(HISTORY_OPEN_KEY) !== "false");
  const [editorHeight, setEditorHeight] = useState(() => {
    const stored = Number(window.localStorage.getItem(EDITOR_HEIGHT_KEY));
    return Number.isFinite(stored) && stored >= MIN_EDITOR_HEIGHT ? Math.min(MAX_EDITOR_HEIGHT, stored) : 240;
  });
  const editorRef = useRef<ReactCodeMirrorRef>(null);
  const theme = useAppearanceStore((state) => state.resolved);

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

  useEffect(() => {
    window.localStorage.setItem(EDITOR_HEIGHT_KEY, String(editorHeight));
  }, [editorHeight]);

  useEffect(() => {
    window.localStorage.setItem(HISTORY_OPEN_KEY, String(historyOpen));
  }, [historyOpen]);

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
    () => [sql({ dialect: engine === "mysql" ? MySQL : PostgreSQL }), activeSqlStatement, ...editorTheme(theme)],
    [engine, theme],
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

  const startEditorResize = (event: ReactMouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startY = event.clientY;
    const startHeight = editorHeight;
    const move = (moveEvent: MouseEvent) => {
      setEditorHeight(Math.min(MAX_EDITOR_HEIGHT, Math.max(MIN_EDITOR_HEIGHT, startHeight + moveEvent.clientY - startY)));
    };
    const stop = () => {
      document.body.classList.remove("resizing-editor");
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", stop);
    };
    document.body.classList.add("resizing-editor");
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", stop);
  };

  const runLabel = executionTarget?.kind === "selection" ? "Run selection" : "Run statement";
  const editableTable = executedSql ? resolveFullTableSelect(executedSql, schemaTree) : null;
  const preset = enginePreset(engine);

  return (
    <div className="query-view view">
      <div className="view-toolbar">
        <div className="view-title">
          <IconCode size={15} />
          <h2>{title}</h2>
          <span className="view-title-meta">
            <Badge>{preset.label}</Badge>
            <span className="mono">{database}</span>
          </span>
        </div>
        <div className="toolbar-actions">
          {running ? (
            <Button
              variant="secondary"
              icon={<IconStop size={12} />}
              onClick={() => void commands.cancelQuery().catch((reason: unknown) => setError(errorMessage(reason)))}
            >Cancel</Button>
          ) : null}
          <Button
            variant="secondary"
            icon={<IconRefresh size={13} />}
            onClick={() => void run(executedSql ?? undefined)}
            disabled={running || !executedSql || embeddedPendingCount > 0}
            title={executedSql
              ? "Re-run the last executed statement for fresh results"
              : "Run a statement first to enable refresh"}
          >{running && executedSql ? "Refreshing…" : "Refresh"}</Button>
          <Button
            variant="primary"
            icon={running ? undefined : <IconPlay size={11} />}
            loading={running}
            trailing={running ? undefined : <Kbd>{runShortcutGlyph()}</Kbd>}
            onClick={runFromEditor}
            disabled={running || !executionTarget}
          >{running ? "Running…" : runLabel}</Button>
          <span className="toolbar-divider" aria-hidden="true" />
          <IconButton
            icon={<IconHistory size={15} />}
            label={historyOpen ? "Hide history" : "Show history"}
            tooltipAlign="end"
            className={historyOpen ? "icon-btn-bordered" : ""}
            onClick={() => setHistoryOpen((value) => !value)}
          />
        </div>
      </div>

      <div className="query-body">
        <div className="query-main">
          <div
            className="editor-panel"
            style={{ height: editorHeight }}
            onKeyDownCapture={handleEditorKeyDownCapture}
          >
            <CodeMirror
              ref={editorRef}
              className="query-code-editor"
              value={sqlText}
              height="100%"
              theme="none"
              extensions={editorExtensions}
              onChange={setSqlText}
              onUpdate={handleEditorUpdate}
              basicSetup={{ lineNumbers: true, foldGutter: false, highlightActiveLine: false }}
            />
            {executionTarget?.kind === "selection" ? <Button
              className="editor-selection-run"
              size="sm"
              variant="secondary"
              icon={<IconPlay size={10} />}
              onMouseDown={(event) => event.preventDefault()}
              onClick={runFromEditor}
              disabled={running}
              title="Run the selected SQL (Command/Ctrl+Enter)"
            >Run selection</Button> : null}
            <div className="editor-hint">
              <Kbd>{runShortcutGlyph()}</Kbd>
              runs the outlined statement or your selection · results capped at 10,000 rows
            </div>
          </div>
          <div
            className="splitter"
            role="separator"
            aria-orientation="horizontal"
            aria-label="Resize editor"
            tabIndex={0}
            onMouseDown={startEditorResize}
            onKeyDown={(event) => {
              if (event.key !== "ArrowUp" && event.key !== "ArrowDown") return;
              event.preventDefault();
              setEditorHeight((height) => Math.min(
                MAX_EDITOR_HEIGHT,
                Math.max(MIN_EDITOR_HEIGHT, height + (event.key === "ArrowUp" ? -16 : 16)),
              ));
            }}
          />

          {error ? <div className="view-messages">
            <Notice tone="danger" icon={<IconAlertCircle size={14} />} onDismiss={() => setError(null)}>{error}</Notice>
          </div> : null}

          {response ? editableTable ? (
            <div className="editable-query-result">
              <div className="result-meta">
                <span>Table viewer · completed in {response.durationMs} ms</span>
                <span className="result-meta-actions">
                  <Badge tone="success" icon={<IconCheck size={11} />}>Editable table</Badge>
                </span>
              </div>
              <TableView
                key={`${profileId}:${editableTable.schema}:${editableTable.table}:${executionRevision}`}
                profileId={profileId}
                schema={editableTable.schema}
                table={editableTable.table}
                onPendingChange={setEmbeddedPendingCount}
              />
            </div>
          ) : (
            <div className="result-panel">
              <div className="result-meta">
                <span>
                  {response.rowCount} {response.rowCount === 1 ? "row" : "rows"}
                  {response.affectedRows !== null ? ` · ${response.affectedRows} affected` : ""}
                  {" · "}{response.durationMs} ms
                </span>
                <span className="result-meta-actions">
                  {response.truncated ? <Badge tone="warning">truncated</Badge> : null}
                  {response.columns.length > 0 ? <Badge
                    title="This query does not resolve to one complete table, so DBM cannot safely map edits back to rows."
                  >Read-only result</Badge> : null}
                </span>
              </div>
              <ResultTable columns={response.columns.map((column) => column.name)} rows={response.rows} />
            </div>
          ) : (
            <div className="query-empty">
              <EmptyState
                icon={<IconTable size={17} />}
                title="No results yet"
                description="Run the statement under your cursor with Command/Ctrl+Enter. Results appear here."
              />
            </div>
          )}
        </div>

        {historyOpen ? (
          <aside className="history-panel">
            <div className="panel-title">
              <span>History</span>
              <Count>{history.length}</Count>
              <IconButton
                size="sm"
                icon={<IconX size={13} />}
                label="Hide history"
                tooltip={false}
                onClick={() => setHistoryOpen(false)}
              />
            </div>
            {history.length === 0 ? (
              <p className="history-empty">Statements you run against <span className="mono">{database}</span> show up here, newest first.</p>
            ) : (
              <div className="history-list">
                {history.slice(0, 100).map((entry) => (
                  <button
                    type="button"
                    className={`history-item ${entry.success ? "" : "failed"}`}
                    key={entry.id}
                    title={entry.sql}
                    onClick={() => setSqlText(entry.sql)}
                  >
                    {entry.success ? <IconCheck size={12} /> : <IconAlertCircle size={12} />}
                    <span className="history-sql">{entry.sql.replace(/\s+/g, " ").slice(0, 90)}</span>
                    <small>{new Date(entry.executedAt).toLocaleTimeString()}</small>
                  </button>
                ))}
              </div>
            )}
          </aside>
        ) : null}
      </div>
    </div>
  );
}

function ResultTable({ columns, rows }: { columns: string[]; rows: JsonValue[][] }) {
  if (columns.length === 0) {
    return <div className="grid-region"><EmptyState
      icon={<IconCheck size={17} />}
      title="Statement completed"
      description="This statement finished without returning a result set."
    /></div>;
  }
  return (
    <div className="grid-region">
      <div className="grid-scroll">
        <table className="data-grid result-grid">
          <thead>
            <tr>{columns.map((column) => <th key={column}>{column}</th>)}</tr>
          </thead>
          <tbody>
            {rows.map((row, rowIndex) => (
              <tr key={rowIndex}>
                {columns.map((column, columnIndex) => (
                  <td key={`${column}-${columnIndex}`}>
                    <span className={row[columnIndex] === null ? "null-value" : "cell-value"}>
                      {commands.toDisplayValue(row[columnIndex] ?? null)}
                    </span>
                  </td>
                ))}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
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

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}

function requiresConfirmation(sqlText: string): boolean {
  const normalized = sqlText.replace(/--[^\n]*/g, " ").replace(/\/\*[\s\S]*?\*\//g, " ");
  return /\b(drop|truncate)\b/i.test(normalized) ||
    /\b(delete|update)\b/i.test(normalized) && !/\bwhere\b/i.test(normalized);
}
