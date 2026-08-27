import { useMemo, useRef, useState, type CSSProperties, type KeyboardEvent as ReactKeyboardEvent, type MouseEvent as ReactMouseEvent, type ReactNode } from "react";

import { DEFAULT_CONNECTION_COLOR, enginePreset } from "./engines";
import {
  IconChevronRight,
  IconMore,
  IconPanelLeft,
  IconPencil,
  IconPlug,
  IconPlus,
  IconPower,
  IconRefresh,
  IconSchema,
  IconSearch,
  IconTable,
  IconView,
  IconX,
} from "./icons";
import { Field, IconButton, Menu, MenuItem } from "./ui";
import { useDismiss } from "./useDismiss";
import type { ConnectionProfile, ProfileSummary, SchemaNode, WorkspaceInfo } from "./types";

export type ActiveTableRef = { profileId: string; schema: string; table: string };

export function Sidebar({
  profiles,
  workspaces,
  activeProfileId,
  schemas,
  activeTable,
  collapsedConnectionIds,
  refreshingSchemaId,
  collapsed,
  width,
  minWidth,
  maxWidth,
  onToggleCollapsed,
  onResize,
  onResetWidth,
  onSelectProfile,
  onToggleExpanded,
  onDisconnect,
  onEditProfile,
  onNewConnection,
  onDatabaseChange,
  onRefreshSchema,
  onOpenTable,
}: {
  profiles: ProfileSummary[];
  workspaces: Record<string, WorkspaceInfo>;
  activeProfileId: string | null;
  schemas: Record<string, SchemaNode[]>;
  activeTable: ActiveTableRef | null;
  collapsedConnectionIds: Set<string>;
  refreshingSchemaId: string | null;
  collapsed: boolean;
  width: number;
  minWidth: number;
  maxWidth: number;
  onToggleCollapsed: () => void;
  onResize: (width: number) => void;
  onResetWidth: () => void;
  onSelectProfile: (profileId: string, connected: boolean) => void;
  onToggleExpanded: (profileId: string) => void;
  onDisconnect: (profileId: string) => void;
  onEditProfile: (profile: ConnectionProfile) => void;
  onNewConnection: () => void;
  onDatabaseChange: (database: string) => void;
  onRefreshSchema: (profileId: string) => void;
  onOpenTable: (profileId: string, schema: string, table: string) => void;
}) {
  const [schemaFilter, setSchemaFilter] = useState("");

  const startResize = (event: ReactMouseEvent<HTMLDivElement>) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = width;
    const move = (moveEvent: MouseEvent) => {
      onResize(Math.min(maxWidth, Math.max(minWidth, startWidth + moveEvent.clientX - startX)));
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

  const handleResizeKey = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
    event.preventDefault();
    onResize(Math.min(maxWidth, Math.max(minWidth, width + (event.key === "ArrowLeft" ? -12 : 12))));
  };

  if (collapsed) {
    return (
      <aside className="sidebar collapsed" style={{ width: 52, minWidth: 52 }}>
        <div className="sidebar-rail">
          <IconButton
            icon={<IconPanelLeft size={15} />}
            label="Expand sidebar"
            tooltipAlign="start"
            onClick={onToggleCollapsed}
          />
          <IconButton
            icon={<IconPlus size={15} />}
            label="New connection"
            tooltipAlign="start"
            onClick={onNewConnection}
          />
          {profiles.map((summary) => {
            const profileId = summary.profile.id;
            const connected = Boolean(workspaces[profileId]);
            return (
              <button
                key={profileId}
                type="button"
                className={`rail-item ${activeProfileId === profileId ? "active" : ""}`}
                style={{ "--connection-color": summary.profile.color ?? DEFAULT_CONNECTION_COLOR } as CSSProperties}
                aria-label={summary.profile.name}
                data-tooltip={summary.profile.name}
                data-tooltip-align="start"
                onClick={() => onSelectProfile(profileId, connected)}
              >
                <span className={`rail-dot ${connected ? "" : "rail-dot-offline"}`} aria-hidden="true" />
              </button>
            );
          })}
        </div>
      </aside>
    );
  }

  return (
    <aside className="sidebar" style={{ width, minWidth: width }}>
      <div className="sidebar-scroll">
        <div className="sidebar-section sidebar-first-section">
          <span>Connections</span>
          <span className="sidebar-section-actions">
            <IconButton icon={<IconPlus size={15} />} label="New connection" onClick={onNewConnection} />
            <IconButton
              icon={<IconPanelLeft size={15} />}
              label="Collapse sidebar"
              tooltipAlign="end"
              onClick={onToggleCollapsed}
            />
          </span>
        </div>

        {profiles.length === 0 ? (
          <p className="sidebar-empty">
            No saved connections yet. Use the + button above to add a PostgreSQL or MySQL server.
          </p>
        ) : (
          <div className="connection-list">
            {profiles.map((summary) => {
              const profileId = summary.profile.id;
              const workspace = workspaces[profileId];
              const active = activeProfileId === profileId;
              const expanded = active && Boolean(workspace) && !collapsedConnectionIds.has(profileId);
              return (
                <div
                  className="connection-entry"
                  key={profileId}
                  style={{ "--connection-color": summary.profile.color ?? DEFAULT_CONNECTION_COLOR } as CSSProperties}
                >
                  <ConnectionRow
                    summary={summary}
                    connected={Boolean(workspace)}
                    active={active}
                    expandable={active && Boolean(workspace)}
                    expanded={expanded}
                    onSelect={() => onSelectProfile(profileId, Boolean(workspace))}
                    onToggleExpanded={() => onToggleExpanded(profileId)}
                    onDisconnect={() => onDisconnect(profileId)}
                    onEdit={() => onEditProfile(summary.profile)}
                  />
                  {expanded && workspace ? (
                    <ConnectionTree
                      profileId={profileId}
                      workspace={workspace}
                      nodes={schemas[profileId] ?? []}
                      activeTable={activeTable?.profileId === profileId ? activeTable : null}
                      refreshing={refreshingSchemaId === profileId}
                      filter={schemaFilter}
                      onFilterChange={setSchemaFilter}
                      onDatabaseChange={onDatabaseChange}
                      onRefreshSchema={() => onRefreshSchema(profileId)}
                      onOpenTable={(schema, table) => onOpenTable(profileId, schema, table)}
                    />
                  ) : null}
                </div>
              );
            })}
          </div>
        )}
      </div>
      <div
        className="sidebar-resize-handle"
        role="separator"
        aria-label="Resize sidebar"
        aria-orientation="vertical"
        aria-valuemin={minWidth}
        aria-valuemax={maxWidth}
        aria-valuenow={width}
        tabIndex={0}
        onMouseDown={startResize}
        onDoubleClick={onResetWidth}
        onKeyDown={handleResizeKey}
      />
    </aside>
  );
}

function ConnectionRow({
  summary,
  connected,
  active,
  expandable,
  expanded,
  onSelect,
  onToggleExpanded,
  onDisconnect,
  onEdit,
}: {
  summary: ProfileSummary;
  connected: boolean;
  active: boolean;
  expandable: boolean;
  expanded: boolean;
  onSelect: () => void;
  onToggleExpanded: () => void;
  onDisconnect: () => void;
  onEdit: () => void;
}) {
  const [menuOpen, setMenuOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  useDismiss(anchor, menuOpen, () => setMenuOpen(false));
  const preset = enginePreset(summary.profile.engine);

  return (
    <div className="connection-row-wrap" ref={anchor}>
      <div className={`connection-row ${active ? "active" : ""}`}>
        {expandable ? (
          <button
            type="button"
            className="connection-caret"
            aria-label={expanded ? "Collapse connection" : "Expand connection"}
            aria-expanded={expanded}
            onClick={() => { setMenuOpen(false); onToggleExpanded(); }}
          >
            <IconChevronRight size={13} className={expanded ? "tree-caret-open" : undefined} />
          </button>
        ) : <span className="connection-caret" aria-hidden="true" />}
        <button
          type="button"
          className="connection-main"
          onClick={() => { setMenuOpen(false); onSelect(); }}
          aria-current={active ? "page" : undefined}
          title={connected ? undefined : "Connect"}
        >
          <span className={`connection-status ${connected ? "" : "connection-status-offline"}`} aria-hidden="true" />
          <span className="connection-copy">
            <strong>{summary.profile.name}</strong>
            <span className="connection-engine">{preset.short}</span>
          </span>
        </button>
        <span className="connection-actions">
          <IconButton
            size="sm"
            icon={<IconMore size={15} />}
            label={`Connection actions for ${summary.profile.name}`}
            tooltip={false}
            aria-haspopup="menu"
            aria-expanded={menuOpen}
            onClick={() => setMenuOpen((open) => !open)}
          />
        </span>
      </div>
      {menuOpen ? <Menu label={`Actions for ${summary.profile.name}`}>
        {connected ? null : <MenuItem
          icon={<IconPlug size={14} />}
          label="Connect"
          onClick={() => { setMenuOpen(false); onSelect(); }}
        />}
        <MenuItem
          icon={<IconPencil size={14} />}
          label="Edit connection"
          onClick={() => { setMenuOpen(false); onEdit(); }}
        />
        {connected ? <MenuItem
          icon={<IconPower size={14} />}
          label="Disconnect"
          danger
          title="Close this connection and its tabs"
          onClick={() => { setMenuOpen(false); onDisconnect(); }}
        /> : null}
      </Menu> : null}
    </div>
  );
}

function ConnectionTree({
  profileId,
  workspace,
  nodes,
  activeTable,
  refreshing,
  filter,
  onFilterChange,
  onDatabaseChange,
  onRefreshSchema,
  onOpenTable,
}: {
  profileId: string;
  workspace: WorkspaceInfo;
  nodes: SchemaNode[];
  activeTable: ActiveTableRef | null;
  refreshing: boolean;
  filter: string;
  onFilterChange: (value: string) => void;
  onDatabaseChange: (database: string) => void;
  onRefreshSchema: () => void;
  onOpenTable: (schema: string, table: string) => void;
}) {
  const matches = useMemo(() => (filter.trim() ? flattenObjects(nodes, filter.trim().toLowerCase()) : null), [filter, nodes]);

  return (
    <div className="connection-tree">
      <div className="database-picker">
        <Field label="Database" htmlFor={`database-select-${profileId}`}>
          <select
            id={`database-select-${profileId}`}
            className="select"
            value={workspace.profile.defaultDatabase}
            onChange={(event) => onDatabaseChange(event.target.value)}
          >
            {workspace.databases.map((database) => <option key={database.name}>{database.name}</option>)}
          </select>
        </Field>
      </div>
      <div className="sidebar-section">
        <span>Schema</span>
        <span className="sidebar-section-actions">
          <IconButton
            size="sm"
            icon={<IconRefresh size={14} />}
            label="Refresh"
            tooltip={refreshing ? false : "Refresh schema"}
            tooltipAlign="end"
            disabled={refreshing}
            onClick={onRefreshSchema}
          />
        </span>
      </div>
      <div className="schema-tools">
        <span className="search-field">
          <IconSearch size={13} />
          <input
            className="input input-search"
            value={filter}
            aria-label="Filter tables"
            placeholder="Filter tables…"
            spellCheck={false}
            onChange={(event) => onFilterChange(event.target.value)}
          />
          {filter ? <IconButton
            className="input-clear"
            size="sm"
            icon={<IconX size={13} />}
            label="Clear filter"
            tooltip={false}
            onClick={() => onFilterChange("")}
          /> : null}
        </span>
      </div>
      <div className="tree">
        {matches ? (
          matches.length === 0
            ? <p className="tree-empty">No tables match “{filter.trim()}”.</p>
            : matches.map((node) => (
              <TreeRow
                key={`${node.kind}-${node.schema}-${node.name}`}
                icon={node.kind === "view" ? <IconView size={13} /> : <IconTable size={13} />}
                label={node.name}
                meta={node.schema ?? undefined}
                depth={0}
                active={activeTable?.schema === node.schema && activeTable?.table === node.table}
                onClick={() => { if (node.schema && node.table) onOpenTable(node.schema, node.table); }}
              />
            ))
        ) : nodes.length === 0 ? (
          <p className="tree-empty">No tables in this database yet.</p>
        ) : nodes.map((node) => (
          <SchemaBranch
            key={`${node.kind}-${node.name}`}
            node={node}
            activeTable={activeTable}
            onOpenTable={onOpenTable}
          />
        ))}
      </div>
    </div>
  );
}

function SchemaBranch({
  node,
  activeTable,
  onOpenTable,
  depth = 0,
}: {
  node: SchemaNode;
  activeTable: ActiveTableRef | null;
  onOpenTable: (schema: string, table: string) => void;
  depth?: number;
}) {
  const [open, setOpen] = useState(depth < 1);
  const isObject = Boolean(node.table && node.schema);
  const active = isObject && activeTable?.schema === node.schema && activeTable.table === node.table;

  return (
    <>
      <TreeRow
        icon={isObject ? (node.kind === "view" ? <IconView size={13} /> : <IconTable size={13} />) : <IconSchema size={13} />}
        caret={isObject ? undefined : open}
        label={node.name}
        depth={depth}
        active={active}
        onClick={() => {
          if (isObject) onOpenTable(node.schema!, node.table!);
          else setOpen((value) => !value);
        }}
      />
      {open && node.children.length > 0 ? node.children.map((child) => (
        <SchemaBranch
          key={`${child.kind}-${child.name}`}
          node={child}
          activeTable={activeTable}
          onOpenTable={onOpenTable}
          depth={depth + 1}
        />
      )) : null}
    </>
  );
}

function TreeRow({
  icon,
  caret,
  label,
  meta,
  depth,
  active,
  onClick,
}: {
  icon: ReactNode;
  caret?: boolean;
  label: string;
  meta?: string;
  depth: number;
  active?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      className={`tree-row ${active ? "active" : ""}`}
      style={{ paddingLeft: `${6 + depth * 13}px` }}
      aria-current={active ? "page" : undefined}
      aria-expanded={caret === undefined ? undefined : caret}
      onClick={onClick}
    >
      <span className="tree-caret" aria-hidden="true">
        {caret === undefined ? null : <IconChevronRight size={12} className={caret ? "tree-caret-open" : undefined} />}
      </span>
      <span className="tree-icon" aria-hidden="true">{icon}</span>
      <span className="truncate">{label}</span>
      {meta ? <span className="tree-meta">{meta}</span> : null}
    </button>
  );
}

function flattenObjects(nodes: SchemaNode[], query: string): SchemaNode[] {
  const matches: SchemaNode[] = [];
  const visit = (node: SchemaNode) => {
    if (node.table && node.schema) {
      const qualified = `${node.schema}.${node.name}`.toLowerCase();
      if (qualified.includes(query) || node.name.toLowerCase().includes(query)) matches.push(node);
    }
    node.children.forEach(visit);
  };
  nodes.forEach(visit);
  return matches.slice(0, 200);
}
