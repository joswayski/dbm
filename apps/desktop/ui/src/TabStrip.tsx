import { useState, type CSSProperties } from "react";

import { DEFAULT_CONNECTION_COLOR } from "./engines";
import { IconChevronsRight, IconCode, IconPencil, IconPlus, IconTable, IconX } from "./icons";
import { shortcut } from "./platform";
import { IconButton } from "./ui";
import type { ProfileSummary, Tab } from "./types";

export function TabStrip({
  tabs,
  activeTabId,
  profiles,
  canOpenQuery,
  onActivate,
  onRename,
  onCollapse,
  onClose,
  onNewQuery,
}: {
  tabs: Tab[];
  activeTabId: string | null;
  profiles: ProfileSummary[];
  canOpenQuery: boolean;
  onActivate: (tabId: string) => void;
  onRename: (tabId: string, title: string) => void;
  onCollapse: (tabId: string) => void;
  onClose: (tabId: string) => void;
  onNewQuery: () => void;
}) {
  const [renamingTabId, setRenamingTabId] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const startRename = (tab: Tab) => {
    setRenamingTabId(tab.id);
    setDraft(tab.title);
  };

  const commitRename = (tabId: string) => {
    if (renamingTabId !== tabId) return;
    onRename(tabId, draft);
    setRenamingTabId(null);
  };

  return (
    <div className="tab-strip">
      {tabs.map((tab) => {
        const color = profiles.find((summary) => summary.profile.id === tab.profileId)?.profile.color ?? DEFAULT_CONNECTION_COLOR;
        const active = tab.id === activeTabId;
        const style = { "--tab-color": color } as CSSProperties;
        if (tab.collapsed) {
          return (
            <div className="tab collapsed" key={tab.id} style={style}>
              <button
                type="button"
                className="tab-expand"
                aria-label={`Expand ${tab.title}`}
                data-tooltip={tab.title}
                onClick={() => onActivate(tab.id)}
              >
                <span className="tab-dot" aria-hidden="true" />
              </button>
              <span className="tab-actions">
                <IconButton
                  size="sm"
                  icon={<IconX size={13} />}
                  label={`Close ${tab.title}`}
                  tooltip={false}
                  onClick={() => onClose(tab.id)}
                />
              </span>
            </div>
          );
        }
        return (
          <div className={`tab ${active ? "active" : ""}`} key={tab.id} style={style}>
            <span className="tab-dot" aria-hidden="true" />
            {renamingTabId === tab.id ? (
              <input
                className="tab-title-input"
                autoFocus
                value={draft}
                aria-label={`Rename ${tab.title}`}
                onChange={(event) => setDraft(event.target.value)}
                onBlur={() => commitRename(tab.id)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") event.currentTarget.blur();
                  if (event.key === "Escape") {
                    event.preventDefault();
                    setRenamingTabId(null);
                    setDraft(tab.title);
                  }
                }}
              />
            ) : (
              <button
                type="button"
                className="tab-title"
                onClick={() => onActivate(tab.id)}
                onDoubleClick={() => { if (tab.kind === "query") startRename(tab); }}
              >
                {tab.kind === "query" ? <IconCode size={13} /> : <IconTable size={13} />}
                <span>{tab.title}</span>
              </button>
            )}
            <span className="tab-actions">
              {tab.kind === "query" ? <IconButton
                size="sm"
                icon={<IconPencil size={12} />}
                label={`Rename ${tab.title}`}
                tooltip={false}
                onClick={() => startRename(tab)}
              /> : null}
              {active ? <IconButton
                size="sm"
                icon={<IconChevronsRight size={13} />}
                label={`Collapse ${tab.title}`}
                tooltip={false}
                onClick={() => onCollapse(tab.id)}
              /> : null}
              <IconButton
                size="sm"
                icon={<IconX size={13} />}
                label={`Close ${tab.title}`}
                tooltip={false}
                onClick={() => onClose(tab.id)}
              />
            </span>
          </div>
        );
      })}
      {canOpenQuery ? (
        <button
          type="button"
          className="new-tab-button"
          aria-label="New query"
          data-tooltip={`New query · ${shortcut("T")}`}
          data-tooltip-align="start"
          onClick={onNewQuery}
        ><IconPlus size={15} /></button>
      ) : null}
    </div>
  );
}
