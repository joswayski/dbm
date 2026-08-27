import { useEffect, useMemo, useRef, useState, type CSSProperties, type ReactNode } from "react";

import { IconSearch } from "./icons";
import { Kbd } from "./ui";

export type PaletteCommand = {
  id: string;
  group: string;
  label: string;
  meta?: string;
  keywords?: string;
  icon?: ReactNode;
  color?: string;
  run: () => void;
};

/** Single keyboard-first entry point to connections, tables, tabs, and actions. */
export function CommandPalette({
  commands,
  onClose,
}: {
  commands: PaletteCommand[];
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [highlight, setHighlight] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const matches = useMemo(() => {
    const needle = query.trim().toLowerCase();
    const scored = commands.filter((command) => {
      if (!needle) return true;
      return `${command.label} ${command.meta ?? ""} ${command.keywords ?? ""} ${command.group}`
        .toLowerCase()
        .includes(needle);
    });
    return scored.slice(0, 80);
  }, [commands, query]);

  useEffect(() => {
    const selected = listRef.current?.querySelector<HTMLElement>('[data-selected="true"]');
    selected?.scrollIntoView?.({ block: "nearest" });
  }, [highlight, matches.length]);

  const groups = useMemo(() => {
    const order: string[] = [];
    const byGroup = new Map<string, PaletteCommand[]>();
    for (const command of matches) {
      if (!byGroup.has(command.group)) {
        byGroup.set(command.group, []);
        order.push(command.group);
      }
      byGroup.get(command.group)!.push(command);
    }
    return order.map((group) => ({ group, items: byGroup.get(group)! }));
  }, [matches]);

  const runIndex = (index: number) => {
    const command = matches[index];
    if (!command) return;
    onClose();
    command.run();
  };

  return (
    <div
      className="scrim palette-scrim"
      onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}
    >
      <div
        className="palette"
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
            return;
          }
          if (event.key === "ArrowDown") {
            event.preventDefault();
            setHighlight((current) => (matches.length === 0 ? 0 : (current + 1) % matches.length));
            return;
          }
          if (event.key === "ArrowUp") {
            event.preventDefault();
            setHighlight((current) => (matches.length === 0 ? 0 : (current - 1 + matches.length) % matches.length));
            return;
          }
          if (event.key === "Enter") {
            event.preventDefault();
            runIndex(highlight);
          }
        }}
      >
        <div className="palette-input-row">
          <IconSearch size={16} />
          <input
            className="palette-input"
            autoFocus
            value={query}
            aria-label="Search connections, tables, and commands"
            placeholder="Search connections, tables, and commands…"
            spellCheck={false}
            onChange={(event) => {
              setQuery(event.target.value);
              setHighlight(0);
            }}
          />
          <Kbd>esc</Kbd>
        </div>
        <div className="palette-list" ref={listRef} role="listbox" aria-label="Results">
          {matches.length === 0 ? (
            <p className="palette-empty">No matches for “{query.trim()}”.</p>
          ) : groups.map(({ group, items }) => (
            <div key={group}>
              <div className="palette-group-label">{group}</div>
              {items.map((command) => {
                const index = matches.indexOf(command);
                return (
                  <button
                    type="button"
                    key={command.id}
                    role="option"
                    aria-selected={index === highlight}
                    className="palette-item"
                    data-selected={index === highlight}
                    style={command.color ? ({ "--connection-color": command.color } as CSSProperties) : undefined}
                    onMouseMove={() => setHighlight(index)}
                    onClick={() => runIndex(index)}
                  >
                    {command.color ? <span className="palette-item-dot" aria-hidden="true" /> : command.icon}
                    <span className="palette-item-label">{command.label}</span>
                    {command.meta ? <span className="palette-item-meta">{command.meta}</span> : null}
                  </button>
                );
              })}
            </div>
          ))}
        </div>
        <div className="palette-footer">
          <span><Kbd>↑</Kbd><Kbd>↓</Kbd> navigate</span>
          <span><Kbd>↵</Kbd> open</span>
          <span><Kbd>esc</Kbd> dismiss</span>
        </div>
      </div>
    </div>
  );
}
