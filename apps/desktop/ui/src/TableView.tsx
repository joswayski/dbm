import { useCallback, useEffect, useRef, useState, type CSSProperties, type MouseEvent as ReactMouseEvent } from "react";
import { createPortal } from "react-dom";

import * as commands from "./commands";
import {
  IconAlertCircle,
  IconAlertTriangle,
  IconArrowDown,
  IconArrowRight,
  IconArrowUp,
  IconCheck,
  IconCheckCircle,
  IconChevronDown,
  IconChevronLeft,
  IconChevronRight,
  IconChevronsLeft,
  IconChevronsRight,
  IconChevronsUpDown,
  IconChevronUp,
  IconCopy,
  IconDownload,
  IconFolderOpen,
  IconPlus,
  IconRefresh,
  IconTable,
  IconTrash,
  IconX,
} from "./icons";
import {
  Badge,
  Button,
  EmptyState,
  IconButton,
  Menu,
  MenuItem,
  Notice,
} from "./ui";
import { useDismiss } from "./useDismiss";
import type {
  FilterCondition,
  FilterOperator,
  JsonValue,
  MutationBatch,
  OrderSpec,
  TableColumn,
  TableMetadata,
  TablePage,
} from "./types";

const MAX_PREVIEW_ROWS = 200;
const EXPORT_PAGE_SIZE = 1_000;
const LARGE_EXPORT_WARNING_ROWS = 100_000;
const COLLAPSED_COLUMN_WIDTH = 76;
const PENDING_EXPORT_ERROR = "Save or discard pending row changes before exporting.";
const PENDING_REFRESH_ERROR = "Save or discard pending row changes before refreshing.";

const FILTER_OPERATORS: Array<{ value: FilterOperator; label: string }> = [
  { value: "equals", label: "Equals" },
  { value: "notEquals", label: "Does not equal" },
  { value: "contains", label: "Contains" },
  { value: "startsWith", label: "Starts with" },
  { value: "endsWith", label: "Ends with" },
  { value: "greaterThan", label: "Greater than" },
  { value: "greaterThanOrEqual", label: "Greater than or equal" },
  { value: "lessThan", label: "Less than" },
  { value: "lessThanOrEqual", label: "Less than or equal" },
  { value: "in", label: "In list" },
  { value: "notIn", label: "Not in list" },
  { value: "isNull", label: "Is null" },
  { value: "isNotNull", label: "Is not null" },
];

type FilterDraft = {
  id: string;
  column: string;
  operator: FilterOperator;
  value: string;
};

type PendingRow = {
  original: JsonValue[];
  changes: JsonValue[];
  primaryKey: JsonValue[];
  xmin: string | null;
  deleted: boolean;
};

type RowEntry = {
  key: string;
  row: JsonValue[];
  rowIndex: number;
  pending?: PendingRow;
  values: JsonValue[];
};

type ChangePreviewState = {
  rowKey: string;
  left: number;
  top?: number;
  bottom?: number;
  maxHeight: number;
};

type EditingCell = {
  rowKey: string;
  column: number;
  value: string;
};

type ExportResult = {
  path: string;
  rows: number;
};

type InlineDiffPart = {
  value: string;
  changed: boolean;
};

export function TableView({
  profileId,
  schema,
  table,
  onPendingChange,
}: {
  profileId: string;
  schema: string;
  table: string;
  onPendingChange?: (count: number) => void;
}) {
  const [page, setPage] = useState<TablePage | null>(null);
  const [pageIndex, setPageIndex] = useState(0);
  const [previewLimit, setPreviewLimit] = useState(MAX_PREVIEW_ROWS);
  const [limitInput, setLimitInput] = useState(String(MAX_PREVIEW_ROWS));
  const [filterDrafts, setFilterDrafts] = useState<FilterDraft[]>([]);
  const [appliedFilters, setAppliedFilters] = useState<FilterCondition[]>([]);
  const [orderBy, setOrderBy] = useState<OrderSpec | null>(null);
  const [pendingRows, setPendingRows] = useState<Record<string, PendingRow>>({});
  const [editing, setEditing] = useState<EditingCell | null>(null);
  const [selectedRows, setSelectedRows] = useState<Set<string>>(new Set());
  const [selectionAnchor, setSelectionAnchor] = useState<number | null>(null);
  const [changePreview, setChangePreview] = useState<ChangePreviewState | null>(null);
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [selectionMenuOpen, setSelectionMenuOpen] = useState(false);
  const [columnWidths, setColumnWidths] = useState<Record<string, number>>({});
  const [collapsedColumns, setCollapsedColumns] = useState<Set<string>>(new Set());
  const [hoveredColumnAction, setHoveredColumnAction] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [showLoadingOverlay, setShowLoadingOverlay] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [exporting, setExporting] = useState(false);
  const [exportProgress, setExportProgress] = useState(0);
  const [exportResult, setExportResult] = useState<ExportResult | null>(null);
  const filtersInitialized = useRef(false);
  const gridRef = useRef<HTMLDivElement>(null);
  const selectionMenuRef = useRef<HTMLDivElement>(null);
  const previewCloseTimer = useRef<number | null>(null);

  const load = useCallback(async () => {
    setLoading(true);
    setShowLoadingOverlay(false);
    setError(null);
    const overlayTimer = window.setTimeout(() => setShowLoadingOverlay(true), 120);
    try {
      const nextPage = await commands.loadTablePage({
        profileId,
        schema,
        table,
        offset: pageIndex * previewLimit,
        limit: previewLimit,
        filters: appliedFilters,
        orderBy,
      });
      setPage(nextPage);
      setEditing(null);
      setSelectedRows(new Set());
      setSelectionAnchor(null);
      setContextMenu(null);
      setSelectionMenuOpen(false);
      setChangePreview(null);
      if (!filtersInitialized.current && nextPage.metadata.columns[0]) {
        filtersInitialized.current = true;
        setFilterDrafts([createFilterDraft(nextPage.metadata.columns[0].name)]);
      }
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      window.clearTimeout(overlayTimer);
      setShowLoadingOverlay(false);
      setLoading(false);
    }
  }, [appliedFilters, orderBy, pageIndex, previewLimit, profileId, schema, table]);

  useEffect(() => {
    const timer = window.setTimeout(() => { void load(); }, 0);
    return () => window.clearTimeout(timer);
  }, [load]);

  useEffect(() => {
    if (!error) return;
    const timer = window.setTimeout(() => setError(null), 10_000);
    return () => window.clearTimeout(timer);
  }, [error]);

  useEffect(() => {
    if (!notice) return;
    const timer = window.setTimeout(() => setNotice(null), 6_000);
    return () => window.clearTimeout(timer);
  }, [notice]);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    window.addEventListener("click", close);
    window.addEventListener("blur", close);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("blur", close);
    };
  }, [contextMenu]);

  useDismiss(selectionMenuRef, selectionMenuOpen, () => setSelectionMenuOpen(false));

  useEffect(() => {
    if (!changePreview) return;
    const close = () => setChangePreview(null);
    const grid = gridRef.current;
    window.addEventListener("resize", close);
    grid?.addEventListener("scroll", close, { passive: true });
    return () => {
      window.removeEventListener("resize", close);
      grid?.removeEventListener("scroll", close);
    };
  }, [changePreview]);

  useEffect(() => () => {
    if (previewCloseTimer.current !== null) window.clearTimeout(previewCloseTimer.current);
  }, []);

  const visibleColumns = page?.metadata.columns ?? [];
  const displayRows = page?.rows ?? [];
  const editable = Boolean(page?.metadata.primaryKey.length);
  const pendingCount = Object.keys(pendingRows).length;
  const pendingDeleteCount = Object.values(pendingRows).filter((pending) => pending.deleted).length;
  const pendingEditCount = pendingCount - pendingDeleteCount;
  const rowEntries: RowEntry[] = page ? displayRows.map((row, rowIndex) => {
    const key = tableRowKey(page.metadata, row, page.offset + rowIndex);
    const pending = pendingRows[key];
    return {
      key,
      row,
      rowIndex,
      pending,
      values: pending?.changes ?? row.slice(0, visibleColumns.length),
    };
  }) : [];
  const selectedEntries = rowEntries.filter((entry) => selectedRows.has(entry.key));
  const allSelectedDeleted = selectedEntries.length > 0 && selectedEntries.every((entry) => entry.pending?.deleted);
  const copyableEntries = rowEntries.filter((entry) => !entry.pending?.deleted);
  const defaultOrderBy: OrderSpec | null = page?.metadata.primaryKey[0]
    ? { column: page.metadata.primaryKey[0], descending: false }
    : null;
  const effectiveOrderBy = orderBy ?? defaultOrderBy;
  const hasFiltersToClear = appliedFilters.length > 0 ||
    filterDrafts.length > 1 ||
    filterDrafts.some((filter) => !filterNeedsValue(filter.operator) || filter.value.trim().length > 0);

  useEffect(() => {
    onPendingChange?.(pendingCount);
  }, [onPendingChange, pendingCount]);

  useEffect(() => () => onPendingChange?.(0), [onPendingChange]);

  const columnWidth = (column: TableColumn) => {
    if (collapsedColumns.has(column.name)) return COLLAPSED_COLUMN_WIDTH;
    return columnWidths[column.name] ?? defaultColumnWidth(column);
  };
  const gridWidth = visibleColumns.reduce((total, column) => total + columnWidth(column), 0);

  const pendingFromRow = (row: JsonValue[]): PendingRow => {
    if (!page) throw new Error("Table metadata is unavailable");
    const original = row.slice(0, visibleColumns.length);
    return {
      original,
      changes: [...original],
      primaryKey: page.metadata.primaryKey.map((key) => original[visibleColumns.findIndex((column) => column.name === key)]),
      xmin: toStringValue(row[visibleColumns.length] ?? null),
      deleted: false,
    };
  };

  const setCell = (entry: RowEntry, columnIndex: number, value: string) => {
    setPendingRows((current) => {
      const pending = current[entry.key] ?? pendingFromRow(entry.row);
      const changes = [...pending.changes];
      changes[columnIndex] = parseCell(value);
      const next = { ...current };
      if (!pending.deleted && rowsEqual(changes, pending.original)) delete next[entry.key];
      else next[entry.key] = { ...pending, changes };
      return next;
    });
    setNotice(null);
  };

  const saveChanges = async () => {
    const mutations: MutationBatch["mutations"] = Object.values(pendingRows).map((pending) => ({
      original: pending.original,
      changes: pending.changes,
      primaryKey: pending.primaryKey,
      xmin: pending.xmin,
      deleted: pending.deleted,
    }));
    if (mutations.length === 0) return;
    setSaving(true);
    setError(null);
    try {
      const result = await commands.applyTableMutations({ profileId, schema, table, mutations });
      setPendingRows({});
      setSelectedRows(new Set());
      await load();
      if (result.conflicts.length > 0) {
        setError(`${result.conflicts.length} row conflict(s); the table was refreshed.`);
      } else {
        setNotice(`${result.applied} ${result.applied === 1 ? "change" : "changes"} saved.`);
      }
    } catch (reason) {
      setError(errorMessage(reason));
    } finally {
      setSaving(false);
    }
  };

  const copyEntries = async (entries: RowEntry[], label: string) => {
    try {
      const rows = entries.filter((entry) => !entry.pending?.deleted).map((entry) => entry.values);
      await navigator.clipboard.writeText(csvDocument(visibleColumns.map((column) => column.name), rows));
      setNotice(`Copied ${rows.length} ${label} ${rows.length === 1 ? "row" : "rows"} as CSV.`);
    } catch (reason) {
      setError(errorMessage(reason));
    }
  };

  const exportAllRows = async () => {
    if (!page || exporting) return;
    if (pendingCount > 0) {
      setError(PENDING_EXPORT_ERROR);
      return;
    }
    if (page.totalRows !== null && page.totalRows > LARGE_EXPORT_WARNING_ROWS && !window.confirm(
      `This export contains ${page.totalRows.toLocaleString()} rows and may take a while. Continue?`,
    )) return;

    setExporting(true);
    setExportProgress(0);
    setError(null);
    setNotice(null);
    setExportResult(null);
    let writer: commands.CsvExportWriter | null = null;
    try {
      const suggestedName = `${safeFileName(schema)}.${safeFileName(table)}.csv`;
      writer = await commands.createCsvExportWriter(suggestedName);
      if (!writer) {
        setNotice("Export canceled.");
        return;
      }
      await writer.write(`\uFEFF${csvLine(visibleColumns.map((column) => column.name))}`);
      let offset = 0;
      let hasMore = true;
      while (hasMore) {
        const batch = await commands.loadTablePage({
          profileId,
          schema,
          table,
          offset,
          limit: EXPORT_PAGE_SIZE,
          filters: appliedFilters,
          orderBy,
          includeTotal: false,
        });
        const rows = batch.rows.map((row) => row.slice(0, visibleColumns.length));
        if (rows.length > 0) await writer.write(`\n${rows.map(csvLine).join("\n")}`);
        offset += rows.length;
        setExportProgress(offset);
        hasMore = batch.hasMore && rows.length > 0;
      }
      const path = writer.path;
      await writer.close();
      writer = null;
      setExportResult({ path, rows: offset });
    } catch (reason) {
      await writer?.abort().catch(() => undefined);
      setError(errorMessage(reason));
    } finally {
      setExporting(false);
    }
  };

  const updateFilter = (id: string, patch: Partial<FilterDraft>) => {
    setFilterDrafts((current) => current.map((filter) => filter.id === id ? { ...filter, ...patch } : filter));
  };

  const applyFilters = () => {
    const filters = filterDrafts.flatMap<FilterCondition>((filter) => {
      if (!filter.column) return [];
      if (filterNeedsValue(filter.operator) && !filter.value.trim()) return [];
      return [{
        column: filter.column,
        operator: filter.operator,
        value: filterNeedsValue(filter.operator) ? filter.value.trim() : null,
      }];
    });
    setAppliedFilters(filters);
    setPageIndex(0);
    setNotice(null);
  };

  const clearFilters = () => {
    setFilterDrafts(visibleColumns[0] ? [createFilterDraft(visibleColumns[0].name)] : []);
    setAppliedFilters([]);
    setPageIndex(0);
    setNotice(null);
  };

  const discardAllChanges = () => {
    setPendingRows({});
    setEditing(null);
    setChangePreview(null);
    setNotice(null);
    setError((current) => current === PENDING_EXPORT_ERROR || current === PENDING_REFRESH_ERROR ? null : current);
  };

  const toggleSort = (column: string) => {
    setOrderBy(effectiveOrderBy?.column === column
      ? { column, descending: !effectiveOrderBy.descending }
      : { column, descending: false });
    setPageIndex(0);
    setNotice(null);
  };

  const applyPreviewLimit = () => {
    const parsed = Number(limitInput);
    const next = Number.isFinite(parsed) ? Math.min(MAX_PREVIEW_ROWS, Math.max(1, Math.floor(parsed))) : previewLimit;
    setLimitInput(String(next));
    setPreviewLimit(next);
    setPageIndex(0);
  };

  const stepPreviewLimit = (amount: number) => {
    const parsed = Number(limitInput);
    const current = Number.isFinite(parsed) ? Math.floor(parsed) : previewLimit;
    const next = Math.min(MAX_PREVIEW_ROWS, Math.max(1, current + amount));
    setLimitInput(String(next));
    setPreviewLimit(next);
    setPageIndex(0);
  };

  const selectRow = (event: ReactMouseEvent, rowIndex: number, key: string) => {
    setSelectedRows((current) => {
      if (event.shiftKey && selectionAnchor !== null) {
        const next = event.metaKey || event.ctrlKey ? new Set(current) : new Set<string>();
        const start = Math.min(selectionAnchor, rowIndex);
        const end = Math.max(selectionAnchor, rowIndex);
        rowEntries.slice(start, end + 1).forEach((entry) => next.add(entry.key));
        return next;
      }
      if (event.metaKey || event.ctrlKey) {
        const next = new Set(current);
        if (next.has(key)) next.delete(key);
        else next.add(key);
        return next;
      }
      return new Set([key]);
    });
    setSelectionAnchor(rowIndex);
    setContextMenu(null);
    setSelectionMenuOpen(false);
  };

  const cancelPreviewClose = () => {
    if (previewCloseTimer.current === null) return;
    window.clearTimeout(previewCloseTimer.current);
    previewCloseTimer.current = null;
  };

  const schedulePreviewClose = () => {
    cancelPreviewClose();
    previewCloseTimer.current = window.setTimeout(() => {
      setChangePreview(null);
      previewCloseTimer.current = null;
    }, 140);
  };

  const showChangePreview = (element: HTMLTableRowElement, rowKey: string) => {
    if (changePreview && changePreview.rowKey !== rowKey && previewCloseTimer.current !== null) return;
    cancelPreviewClose();
    const rect = element.getBoundingClientRect();
    const previewWidth = Math.min(680, Math.max(320, window.innerWidth - 32));
    const left = Math.max(16, Math.min(rect.left + 16, window.innerWidth - previewWidth - 16));
    const spaceBelow = Math.max(0, window.innerHeight - rect.bottom - 16);
    const spaceAbove = Math.max(0, rect.top - 16);
    if (spaceBelow >= 260 || spaceBelow >= spaceAbove) {
      setChangePreview({ rowKey, left, top: rect.bottom, maxHeight: Math.max(140, Math.min(520, spaceBelow)) });
    } else {
      setChangePreview({ rowKey, left, bottom: window.innerHeight - rect.top, maxHeight: Math.max(140, Math.min(520, spaceAbove)) });
    }
  };

  const handleExportAction = async (action: "open" | "reveal") => {
    if (!exportResult) return;
    try {
      setError(null);
      if (action === "open") await commands.openExportedFile(exportResult.path);
      else await commands.revealExportedFile(exportResult.path);
    } catch (reason) {
      setError(errorMessage(reason));
    }
  };

  const discardPendingRow = (rowKey: string) => {
    setPendingRows((current) => {
      const pending = current[rowKey];
      if (!pending) return current;
      const next = { ...current };
      if (pending.deleted && !rowsEqual(pending.changes, pending.original)) {
        next[rowKey] = { ...pending, deleted: false };
      } else {
        delete next[rowKey];
      }
      return next;
    });
    setEditing((current) => current?.rowKey === rowKey ? null : current);
    setNotice(null);
  };

  const stageDeleteForSelected = () => {
    if (!editable || selectedEntries.length === 0) return;
    const deleteRows = !allSelectedDeleted;
    setPendingRows((current) => {
      const next = { ...current };
      for (const entry of selectedEntries) {
        const pending = next[entry.key] ?? pendingFromRow(entry.row);
        if (deleteRows) next[entry.key] = { ...pending, deleted: true };
        else if (rowsEqual(pending.changes, pending.original)) delete next[entry.key];
        else next[entry.key] = { ...pending, deleted: false };
      }
      return next;
    });
    setNotice(null);
    setContextMenu(null);
    setSelectionMenuOpen(false);
  };

  const openContextMenu = (event: ReactMouseEvent, entry: RowEntry) => {
    event.preventDefault();
    if (!selectedRows.has(entry.key)) {
      setSelectedRows(new Set([entry.key]));
      setSelectionAnchor(entry.rowIndex);
    }
    setContextMenu({
      x: Math.min(event.clientX, window.innerWidth - 210),
      y: Math.min(event.clientY, window.innerHeight - 120),
    });
  };

  const toggleColumn = (column: string) => {
    setHoveredColumnAction(null);
    setCollapsedColumns((current) => {
      const next = new Set(current);
      if (next.has(column)) next.delete(column);
      else next.add(column);
      return next;
    });
  };

  const startColumnResize = (event: ReactMouseEvent<HTMLDivElement>, column: TableColumn) => {
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const startWidth = columnWidth(column);
    setCollapsedColumns((current) => {
      const next = new Set(current);
      next.delete(column.name);
      return next;
    });
    const move = (moveEvent: MouseEvent) => {
      const width = Math.min(800, Math.max(70, startWidth + moveEvent.clientX - startX));
      setColumnWidths((current) => ({ ...current, [column.name]: width }));
    };
    const stop = () => {
      document.body.classList.remove("resizing-column");
      window.removeEventListener("mousemove", move);
      window.removeEventListener("mouseup", stop);
    };
    document.body.classList.add("resizing-column");
    window.addEventListener("mousemove", move);
    window.addEventListener("mouseup", stop);
  };

  const exportLabel = exporting
    ? `Exporting ${exportProgress.toLocaleString()}${page?.totalRows != null ? ` / ${page.totalRows.toLocaleString()}` : ""}…`
    : `Export all${page?.totalRows != null ? ` (${page.totalRows.toLocaleString()})` : ""}`;
  const columnsReset = collapsedColumns.size > 0 || Object.keys(columnWidths).length > 0;
  const addFilterButton = (
    <Button
      variant="ghost"
      size="sm"
      icon={<IconPlus size={13} />}
      onClick={() => setFilterDrafts((current) => [...current, createFilterDraft(visibleColumns[0]?.name ?? "")])}
      disabled={!page}
    >Add filter</Button>
  );
  const firstRowNumber = page && rowEntries.length > 0 ? page.offset + 1 : 0;
  const lastRowNumber = page ? page.offset + rowEntries.length : 0;

  return (
    <div className="table-view view">
      <div className="view-toolbar">
        <div className="view-title">
          <IconTable size={15} />
          <h2><span className="qualifier">{schema}.</span>{table}</h2>
          <span className="view-title-meta">
            {page ? <>
              <span>{page.totalRows ?? "—"} rows</span>
              <span aria-hidden="true">·</span>
              <span>{page.metadata.columns.length} columns</span>
            </> : null}
            {page && !editable ? <Badge title="This view has no primary key, so DBM cannot map edits back to rows.">Read-only</Badge> : null}
          </span>
        </div>
        <div className="toolbar-actions">
          <Button
            variant="secondary"
            icon={<IconRefresh size={13} />}
            onClick={() => {
              if (pendingCount > 0) {
                setError(PENDING_REFRESH_ERROR);
                return;
              }
              void load();
            }}
            disabled={loading}
            title="Reload the current page with the same filters and sort"
          >{loading ? "Refreshing…" : "Refresh"}</Button>
          <Button
            variant="secondary"
            icon={<IconCopy size={13} />}
            onClick={() => void copyEntries(copyableEntries, "visible")}
            disabled={!page || loading}
            title="Copies only the current preview page"
          >Copy visible ({copyableEntries.length})</Button>
          <Button
            variant="secondary"
            icon={<IconDownload size={13} />}
            onClick={() => void exportAllRows()}
            disabled={!page || loading || exporting}
            loading={exporting}
            title="Prompts for a location and exports every filtered row"
          >{exportLabel}</Button>
          {selectedEntries.length > 0 ? <div className="menu-anchor" ref={selectionMenuRef}>
            <Button
              variant="secondary"
              trailing={<IconChevronDown size={13} />}
              aria-expanded={selectionMenuOpen}
              aria-haspopup="menu"
              onClick={(event) => { event.stopPropagation(); setSelectionMenuOpen((open) => !open); }}
            >{selectedEntries.length} selected</Button>
            {selectionMenuOpen ? <Menu label="Selection actions">
              <MenuItem
                icon={<IconCopy size={14} />}
                label="Copy selected as CSV"
                onClick={() => { void copyEntries(selectedEntries, "selected"); setSelectionMenuOpen(false); }}
              />
              {editable ? <MenuItem
                icon={allSelectedDeleted ? <IconCheck size={14} /> : <IconTrash size={14} />}
                label={allSelectedDeleted
                  ? "Undo staged deletion"
                  : `Stage ${selectedEntries.length === 1 ? "row" : `${selectedEntries.length} rows`} for deletion`}
                danger={!allSelectedDeleted}
                onClick={stageDeleteForSelected}
              /> : null}
              <MenuItem
                icon={<IconX size={14} />}
                label="Clear selection"
                onClick={() => { setSelectedRows(new Set()); setSelectionAnchor(null); setSelectionMenuOpen(false); }}
              />
            </Menu> : null}
          </div> : null}
          {pendingCount > 0 ? <Button
            variant="primary"
            icon={saving ? undefined : <IconCheck size={13} />}
            loading={saving}
            onClick={() => void saveChanges()}
            disabled={saving}
          >{saving ? "Saving…" : `Save changes (${pendingCount})`}</Button> : null}
        </div>
      </div>

      <div className="query-bar">
        {filterDrafts.length === 0 ? (
          <div className="query-bar-row">
            <span className="query-bar-label">Where</span>
            <span className="filter-no-value">No filters — every row is shown.</span>
            {addFilterButton}
          </div>
        ) : null}
        {filterDrafts.map((filter, index) => (
          <div className="query-bar-row" key={filter.id}>
            <span className="query-bar-label">{index === 0 ? "Where" : "and"}</span>
            <select
              className="select filter-column"
              aria-label="Filter column"
              value={filter.column}
              onChange={(event) => updateFilter(filter.id, { column: event.target.value })}
              disabled={!page}
            >
              {visibleColumns.map((column) => <option key={column.name} value={column.name}>{column.name}</option>)}
            </select>
            <select
              className="select filter-operator"
              aria-label="Filter operator"
              value={filter.operator}
              onChange={(event) => updateFilter(filter.id, { operator: event.target.value as FilterOperator })}
              disabled={!page}
            >
              {FILTER_OPERATORS.map((operator) => <option key={operator.value} value={operator.value}>{operator.label}</option>)}
            </select>
            {filterNeedsValue(filter.operator) ? <input
              className="input filter-value"
              aria-label="Filter value"
              placeholder={filter.operator === "in" || filter.operator === "notIn" ? "value 1, value 2, …" : "Value…"}
              value={filter.value}
              onChange={(event) => updateFilter(filter.id, { value: event.target.value })}
              onKeyDown={(event) => { if (event.key === "Enter") applyFilters(); }}
            /> : <span className="filter-no-value">No value needed</span>}
            <IconButton
              icon={<IconX size={14} />}
              label="Remove filter"
              tooltip={false}
              onClick={() => setFilterDrafts((current) => current.filter((candidate) => candidate.id !== filter.id))}
            />
            {index === filterDrafts.length - 1 ? addFilterButton : null}
          </div>
        ))}
        <div className="query-bar-row">
          <span className="query-bar-label">Order by</span>
          <select
            className="select sort-column-select"
            aria-label="Sort by"
            value={effectiveOrderBy?.column ?? ""}
            onChange={(event) => {
              setOrderBy(event.target.value ? { column: event.target.value, descending: false } : null);
              setPageIndex(0);
            }}
          >
            {page?.metadata.primaryKey.length ? null : <option value="">Choose a sort column</option>}
            {visibleColumns.map((column) => (
              <option key={column.name} value={column.name}>
                {column.name}{page?.metadata.primaryKey.includes(column.name) ? " (primary key)" : ""}
              </option>
            ))}
          </select>
          {effectiveOrderBy ? <select
            className="select sort-direction-select"
            aria-label="Sort direction"
            value={effectiveOrderBy.descending ? "desc" : "asc"}
            onChange={(event) => {
              setOrderBy({ ...effectiveOrderBy, descending: event.target.value === "desc" });
              setPageIndex(0);
            }}
          >
            <option value="asc">Ascending</option>
            <option value="desc">Descending</option>
          </select> : null}
          <span className="query-bar-label query-bar-label-inline">Limit</span>
          <span className="limit-field">
            <input
              className="input"
              aria-label="Preview limit"
              type="number"
              min="1"
              max={MAX_PREVIEW_ROWS}
              value={limitInput}
              onChange={(event) => setLimitInput(event.target.value)}
              onBlur={applyPreviewLimit}
              onKeyDown={(event) => { if (event.key === "Enter") applyPreviewLimit(); }}
            />
            <span className="limit-stepper">
              <button
                type="button"
                aria-label="Increase preview limit"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => stepPreviewLimit(1)}
              ><IconChevronUp size={11} /></button>
              <button
                type="button"
                aria-label="Decrease preview limit"
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => stepPreviewLimit(-1)}
              ><IconChevronDown size={11} /></button>
            </span>
          </span>
          <div className="query-bar-actions">
            {columnsReset ? <Button
              variant="ghost"
              size="sm"
              onClick={() => { setCollapsedColumns(new Set()); setColumnWidths({}); }}
            >Reset columns</Button> : null}
            {hasFiltersToClear ? <Button variant="secondary" onClick={clearFilters} disabled={!page}>Clear</Button> : null}
            <Button variant="primary" onClick={applyFilters} disabled={!page}>Apply filters</Button>
          </div>
        </div>
      </div>

      <div className="view-messages">
        {error ? <Notice tone="danger" icon={<IconAlertCircle size={14} />} onDismiss={() => setError(null)}>{error}</Notice> : null}
        {notice ? <Notice tone="success" icon={<IconCheckCircle size={14} />} onDismiss={() => setNotice(null)}>{notice}</Notice> : null}
        {exportResult ? <Notice
          tone="success"
          role="status"
          icon={<IconCheckCircle size={14} />}
          onDismiss={() => setExportResult(null)}
          actions={<Button
            variant="secondary"
            size="sm"
            icon={<IconFolderOpen size={13} />}
            onClick={() => void handleExportAction("reveal")}
          >Show in folder</Button>}
        >
          Exported {exportResult.rows.toLocaleString()} filtered {exportResult.rows === 1 ? "row" : "rows"} to{" "}
          <button type="button" className="export-file-link" onClick={() => void handleExportAction("open")}>{fileName(exportResult.path)}</button>.
        </Notice> : null}
        {pendingCount > 0 ? <div className="pending-changes">
          <IconAlertTriangle size={14} />
          <strong>{pendingCount} pending {pendingCount === 1 ? "change" : "changes"}</strong>
          {pendingEditCount > 0 ? <Badge tone="warning">{pendingEditCount} {pendingEditCount === 1 ? "edited row" : "edited rows"}</Badge> : null}
          {pendingDeleteCount > 0 ? <Badge tone="danger">{pendingDeleteCount} {pendingDeleteCount === 1 ? "deletion" : "deletions"}</Badge> : null}
          <Button
            className="pending-discard-button"
            variant="secondary"
            size="sm"
            onClick={discardAllChanges}
            disabled={saving}
          >Discard changes</Button>
        </div> : null}
      </div>

      <div className="grid-region">
        <div className="grid-scroll" ref={gridRef}>
          {loading && !page ? <div className="grid-skeleton" aria-label="Loading rows">
            <div className="skeleton-header" />
            {Array.from({ length: 8 }, (_, index) => <div className="skeleton-row" key={index} />)}
          </div> : page && rowEntries.length > 0 ? (
            <table
              className={`data-grid resizable-grid ${hoveredColumnAction ? "column-action-active" : ""}`}
              style={{ width: `${gridWidth}px`, minWidth: "100%" }}
            >
              <colgroup>
                {visibleColumns.map((column) => <col key={column.name} style={{ width: `${columnWidth(column)}px` }} />)}
              </colgroup>
              <thead><tr>
                {visibleColumns.map((column) => {
                  const sorted = effectiveOrderBy?.column === column.name;
                  const collapsed = collapsedColumns.has(column.name);
                  const isPrimaryKey = page.metadata.primaryKey.includes(column.name);
                  const focusClass = hoveredColumnAction
                    ? hoveredColumnAction === column.name ? "column-action-target" : "column-action-dimmed"
                    : "";
                  return <th key={column.name} className={[sorted ? "sorted-column" : "", "resizable-header", focusClass].filter(Boolean).join(" ")}>
                    {collapsed ? <button
                      type="button"
                      className="expand-column column-action-button"
                      onClick={() => toggleColumn(column.name)}
                      onMouseEnter={() => setHoveredColumnAction(column.name)}
                      onMouseLeave={() => setHoveredColumnAction(null)}
                      data-tooltip={`Expand ${column.name}`}
                      aria-label={`Expand ${column.name}`}
                    >
                      <IconChevronsRight size={13} />
                      <span className="collapsed-column-name">{column.name}</span>
                    </button> : <div className="column-header-content">
                      <button
                        type="button"
                        className="column-sort"
                        onClick={() => toggleSort(column.name)}
                        aria-label={`Sort by ${column.name}`}
                      >
                        <span className="column-name">
                          <span>{column.name}</span>
                          {isPrimaryKey ? <span className="column-key" title="Primary key">pk</span> : null}
                          <span className="column-type">{column.dataType}</span>
                        </span>
                        <span className={`sort-indicator ${sorted ? "active" : ""}`}>
                          {sorted
                            ? effectiveOrderBy?.descending ? <IconArrowDown size={12} /> : <IconArrowUp size={12} />
                            : <IconChevronsUpDown size={12} />}
                        </span>
                      </button>
                      <button
                        type="button"
                        className="collapse-column column-action-button"
                        onClick={() => toggleColumn(column.name)}
                        onMouseEnter={() => setHoveredColumnAction(column.name)}
                        onMouseLeave={() => setHoveredColumnAction(null)}
                        data-tooltip={`Collapse ${column.name}`}
                        aria-label={`Collapse ${column.name}`}
                      ><IconChevronsLeft size={13} /></button>
                    </div>}
                    {!collapsed ? <div className="column-resize-handle" onMouseDown={(event) => startColumnResize(event, column)} /> : null}
                  </th>;
                })}
              </tr></thead>
              <tbody>{rowEntries.map((entry) => {
                const deleted = Boolean(entry.pending?.deleted);
                const edited = Boolean(entry.pending && !entry.pending.deleted);
                const rowClass = [
                  selectedRows.has(entry.key) ? "selected-row" : "",
                  deleted ? "deleted-row" : edited ? "staged-row" : "",
                ].filter(Boolean).join(" ");
                return <tr
                  key={entry.key}
                  className={rowClass}
                  onClick={(event) => selectRow(event, entry.rowIndex, entry.key)}
                  onContextMenu={(event) => openContextMenu(event, entry)}
                  onMouseEnter={(event) => { if (entry.pending) showChangePreview(event.currentTarget, entry.key); }}
                  onMouseLeave={() => { if (entry.pending) schedulePreviewClose(); }}
                  aria-selected={selectedRows.has(entry.key)}
                >
                  {visibleColumns.map((column, columnIndex) => {
                    const collapsed = collapsedColumns.has(column.name);
                    const isEditing = editing?.rowKey === entry.key && editing.column === columnIndex;
                    const isPrimaryKey = page.metadata.primaryKey.includes(column.name);
                    const displayValue = commands.toDisplayValue(entry.values[columnIndex]);
                    const changed = Boolean(
                      entry.pending &&
                      !entry.pending.deleted &&
                      !rowsEqual([entry.pending.original[columnIndex]], [entry.pending.changes[columnIndex]]),
                    );
                    const focusClass = hoveredColumnAction
                      ? hoveredColumnAction === column.name ? "column-action-target" : "column-action-dimmed"
                      : "";
                    return <td
                      key={column.name}
                      className={[collapsed ? "collapsed-data-cell" : "", changed ? "changed-cell" : "", focusClass].filter(Boolean).join(" ")}
                      onDoubleClick={(event) => {
                        event.stopPropagation();
                        if (editable && !collapsed && !isPrimaryKey && !deleted) {
                          setEditing({
                            rowKey: entry.key,
                            column: columnIndex,
                            value: displayValue === "NULL" ? "" : displayValue,
                          });
                        }
                      }}
                    >
                      {collapsed ? <span className="collapsed-cell">…</span> : isEditing ? <input
                        autoFocus
                        className="cell-input"
                        value={editing?.value ?? ""}
                        onMouseDown={(event) => event.stopPropagation()}
                        onClick={(event) => event.stopPropagation()}
                        onDoubleClick={(event) => event.stopPropagation()}
                        onChange={(event) => {
                          const value = event.target.value;
                          setEditing((current) => current?.rowKey === entry.key && current.column === columnIndex
                            ? { ...current, value }
                            : current);
                        }}
                        onBlur={() => {
                          if (editing?.rowKey === entry.key && editing.column === columnIndex) {
                            setCell(entry, columnIndex, editing.value);
                          }
                          setEditing(null);
                        }}
                        onKeyDown={(event) => {
                          if (event.key === "Enter") {
                            event.preventDefault();
                            event.currentTarget.blur();
                          } else if (event.key === "Escape") {
                            event.preventDefault();
                            setEditing(null);
                          }
                        }}
                      /> : <span className={entry.values[columnIndex] === null ? "null-value" : "cell-value"}>{displayValue}</span>}
                    </td>;
                  })}
                </tr>;
              })}</tbody>
            </table>
          ) : <EmptyState
            icon={<IconTable size={17} />}
            title="No rows match this view"
            description={appliedFilters.length > 0
              ? "Clear or loosen the filters above to see rows again."
              : "This table has no rows yet."}
          />}
          {showLoadingOverlay && page ? <div className="grid-loading-overlay">
            <span><span className="spinner" />Refreshing…</span>
          </div> : null}
        </div>
        <div className="grid-footer">
          <span>
            {rowEntries.length > 0
              ? `Rows ${firstRowNumber.toLocaleString()}–${lastRowNumber.toLocaleString()}${page?.totalRows != null ? ` of ${page.totalRows.toLocaleString()}` : ""}`
              : "No rows"}
          </span>
          {editable ? <span>Double-click a cell to edit</span> : null}
          <span className="grid-footer-spacer" />
          {pageIndex > 0 || page?.hasMore ? <span className="pagination">
            <Button
              variant="ghost"
              size="sm"
              icon={<IconChevronLeft size={12} />}
              disabled={loading || pageIndex === 0}
              onClick={() => setPageIndex((value) => value - 1)}
            >Previous</Button>
            <span className="numeric">Page {pageIndex + 1}</span>
            <Button
              variant="ghost"
              size="sm"
              trailing={<IconChevronRight size={12} />}
              disabled={loading || !page?.hasMore}
              onClick={() => setPageIndex((value) => value + 1)}
            >Next</Button>
          </span> : null}
        </div>
      </div>
      <div className="view-footer-pad" />

      {contextMenu ? <div className="row-context-menu" style={{ left: contextMenu.x, top: contextMenu.y } as CSSProperties}>
        <MenuItem
          icon={<IconCopy size={14} />}
          label="Copy selected as CSV"
          onClick={() => void copyEntries(selectedEntries, "selected")}
        />
        {editable ? <MenuItem
          icon={allSelectedDeleted ? <IconCheck size={14} /> : <IconTrash size={14} />}
          label={allSelectedDeleted
            ? "Undo staged deletion"
            : `Stage ${selectedEntries.length > 1 ? `${selectedEntries.length} rows` : "row"} for deletion`}
          danger={!allSelectedDeleted}
          onClick={stageDeleteForSelected}
        /> : null}
      </div> : null}
      {changePreview && pendingRows[changePreview.rowKey] ? createPortal(<ChangePreview
        pending={pendingRows[changePreview.rowKey]}
        columns={visibleColumns}
        onDiscard={() => { discardPendingRow(changePreview.rowKey); setChangePreview(null); }}
        onMouseEnter={cancelPreviewClose}
        onMouseLeave={schedulePreviewClose}
        style={{ left: changePreview.left, top: changePreview.top, bottom: changePreview.bottom, maxHeight: changePreview.maxHeight }}
      />, document.body) : null}
    </div>
  );
}

function ChangePreview({ pending, columns, onDiscard, onMouseEnter, onMouseLeave, style }: {
  pending: PendingRow;
  columns: TableColumn[];
  onDiscard: () => void;
  onMouseEnter: () => void;
  onMouseLeave: () => void;
  style: CSSProperties;
}) {
  const stopPropagation = (event: ReactMouseEvent) => event.stopPropagation();
  if (pending.deleted) return <div
    className="change-preview delete-preview"
    style={style}
    onMouseEnter={onMouseEnter}
    onMouseLeave={onMouseLeave}
    onMouseDown={stopPropagation}
    onClick={stopPropagation}
  >
    <div className="change-preview-header">
      <strong><IconTrash size={13} />Pending delete</strong>
      <Button
        variant="danger"
        size="sm"
        onClick={(event) => { event.stopPropagation(); onDiscard(); }}
      >Undo delete</Button>
    </div>
    <p>This row is removed when you save changes.</p>
  </div>;

  const changes = columns.flatMap((column, index) => rowsEqual([pending.original[index]], [pending.changes[index]]) ? [] : [{
    column: column.name,
    before: commands.toDisplayValue(pending.original[index]),
    after: commands.toDisplayValue(pending.changes[index]),
  }]);
  return <div
    className="change-preview edit-preview"
    style={style}
    onMouseEnter={onMouseEnter}
    onMouseLeave={onMouseLeave}
    onMouseDown={stopPropagation}
    onClick={stopPropagation}
  >
    <div className="change-preview-header">
      <strong><IconAlertTriangle size={13} />Pending edit</strong>
      <Button
        variant="secondary"
        size="sm"
        onClick={(event) => { event.stopPropagation(); onDiscard(); }}
      >Discard edit</Button>
    </div>
    <div className="change-diff-list">
      {changes.map((change) => {
        const diff = inlineDiff(change.before, change.after);
        return <div className="change-diff" key={change.column}>
          <b>{change.column}</b>
          <div className="change-diff-values">
            <div className="change-diff-before"><span>Before</span><InlineDiffValue parts={diff.before} kind="removed" /></div>
            <span className="change-arrow" aria-hidden="true"><IconArrowRight size={13} /></span>
            <div className="change-diff-after"><span>After</span><InlineDiffValue parts={diff.after} kind="added" /></div>
          </div>
        </div>;
      })}
    </div>
  </div>;
}

function InlineDiffValue({ parts, kind }: { parts: InlineDiffPart[]; kind: "added" | "removed" }) {
  return <pre>{parts.map((part, index) => part.changed
    ? <mark className={`change-inline-${kind}`} key={index}>{part.value}</mark>
    : part.value)}</pre>;
}

function inlineDiff(before: string, after: string): { before: InlineDiffPart[]; after: InlineDiffPart[] } {
  const beforeCharacters = Array.from(before);
  const afterCharacters = Array.from(after);
  let prefixLength = 0;
  while (
    prefixLength < beforeCharacters.length &&
    prefixLength < afterCharacters.length &&
    beforeCharacters[prefixLength] === afterCharacters[prefixLength]
  ) prefixLength += 1;

  let suffixLength = 0;
  while (
    suffixLength < beforeCharacters.length - prefixLength &&
    suffixLength < afterCharacters.length - prefixLength &&
    beforeCharacters[beforeCharacters.length - suffixLength - 1] === afterCharacters[afterCharacters.length - suffixLength - 1]
  ) suffixLength += 1;

  const prefix = beforeCharacters.slice(0, prefixLength).join("");
  const suffix = suffixLength > 0 ? beforeCharacters.slice(beforeCharacters.length - suffixLength).join("") : "";
  const beforeChanged = beforeCharacters.slice(prefixLength, beforeCharacters.length - suffixLength).join("");
  const afterChanged = afterCharacters.slice(prefixLength, afterCharacters.length - suffixLength).join("");
  const parts = (changedValue: string): InlineDiffPart[] => [
    ...(prefix ? [{ value: prefix, changed: false }] : []),
    ...(changedValue ? [{ value: changedValue, changed: true }] : []),
    ...(suffix ? [{ value: suffix, changed: false }] : []),
  ];
  return { before: parts(beforeChanged), after: parts(afterChanged) };
}

function createFilterDraft(column: string): FilterDraft {
  return { id: crypto.randomUUID(), column, operator: "contains", value: "" };
}

function filterNeedsValue(operator: FilterOperator): boolean {
  return operator !== "isNull" && operator !== "isNotNull";
}

function tableRowKey(metadata: TableMetadata, row: JsonValue[], fallbackIndex: number): string {
  if (metadata.primaryKey.length === 0) return `row:${fallbackIndex}:${JSON.stringify(row)}`;
  const values = metadata.primaryKey.map((key) => row[metadata.columns.findIndex((column) => column.name === key)]);
  return `pk:${JSON.stringify(values)}`;
}

function defaultColumnWidth(column: TableColumn): number {
  if (/json|array/i.test(column.dataType)) return 320;
  if (/text|character|timestamp/i.test(column.dataType)) return 220;
  return 160;
}

function parseCell(value: string): JsonValue {
  if (value.trim() === "") return null;
  if (value === "true") return true;
  if (value === "false") return false;
  if (/^-?\d+$/.test(value)) return Number(value);
  if (/^-?\d+\.\d+$/.test(value)) return Number(value);
  return value;
}

function toStringValue(value: JsonValue | undefined): string | null {
  if (value === null || value === undefined) return null;
  return String(value);
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

function rowsEqual(left: JsonValue[], right: JsonValue[]): boolean {
  return JSON.stringify(left) === JSON.stringify(right);
}

function csvDocument(columns: string[], rows: JsonValue[][]): string {
  return [csvLine(columns), ...rows.map(csvLine)].join("\n");
}

function csvLine(values: JsonValue[]): string {
  return values.map((value) => {
    const text = commands.toDisplayValue(value);
    return /[",\r\n]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
  }).join(",");
}

function safeFileName(value: string): string {
  return value.replace(/[\\/:*?"<>|]/g, "_");
}

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}
