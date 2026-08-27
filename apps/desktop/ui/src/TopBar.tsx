import { useRef, useState, type ReactNode } from "react";

import { useAppearanceStore, type Appearance } from "./appearance";
import {
  IconChevronDown,
  IconDatabase,
  IconMonitor,
  IconMoon,
  IconSearch,
  IconSettings,
  IconSun,
} from "./icons";
import { shortcut } from "./platform";
import { UpdateControl } from "./UpdateControl";
import { Kbd, Menu, MenuItem, MenuLabel, MenuSeparator } from "./ui";
import { useDismiss } from "./useDismiss";
import type { ConnectionProfile } from "./types";

const APPEARANCE_OPTIONS: Array<{ value: Appearance; label: string; icon: ReactNode }> = [
  { value: "system", label: "Match system", icon: <IconMonitor size={14} /> },
  { value: "dark", label: "Dark", icon: <IconMoon size={14} /> },
  { value: "light", label: "Light", icon: <IconSun size={14} /> },
];

export function TopBar({
  profile,
  connected,
  engineLabel,
  onOpenPalette,
  onNewConnection,
}: {
  profile: ConnectionProfile | null;
  connected: boolean;
  engineLabel: string | null;
  onOpenPalette: () => void;
  onNewConnection: () => void;
}) {
  return (
    <header className="topbar">
      <div className="brand">
        <span className="brand-mark" aria-hidden="true"><IconDatabase size={13} /></span>
        <span className="brand-name">DBM</span>
      </div>
      <span className="topbar-divider" aria-hidden="true" />
      {profile ? (
        <button type="button" className="identity" onClick={onOpenPalette} title="Switch connection or database">
          <span className={`identity-dot ${connected ? "" : "identity-dot-idle"}`} aria-hidden="true" />
          <span className="identity-name truncate">{profile.name}</span>
          <span className="identity-path truncate">
            {engineLabel ? `${engineLabel.toLowerCase()}://` : ""}{profile.username}@{profile.host}:{profile.port}/{profile.defaultDatabase}
          </span>
          <IconChevronDown size={13} />
        </button>
      ) : (
        <span className="identity-empty">No connection selected</span>
      )}
      <div className="topbar-actions">
        <button type="button" className="palette-trigger" onClick={onOpenPalette}>
          <IconSearch size={13} />
          <span>Search or run a command</span>
          <Kbd>{shortcut("K")}</Kbd>
        </button>
        <UpdateControl />
        <AppearanceMenu onNewConnection={onNewConnection} />
      </div>
    </header>
  );
}

function AppearanceMenu({ onNewConnection }: { onNewConnection: () => void }) {
  const [open, setOpen] = useState(false);
  const anchor = useRef<HTMLDivElement>(null);
  const preference = useAppearanceStore((state) => state.preference);
  const setPreference = useAppearanceStore((state) => state.setPreference);
  useDismiss(anchor, open, () => setOpen(false));

  return (
    <div className="menu-anchor" ref={anchor}>
      <button
        type="button"
        className="icon-btn"
        aria-label="Settings"
        aria-haspopup="menu"
        aria-expanded={open}
        data-tooltip={open ? undefined : "Settings"}
        data-tooltip-align="end"
        onClick={() => setOpen((value) => !value)}
      ><IconSettings size={15} /></button>
      {open ? <Menu label="Settings">
        <MenuLabel>Appearance</MenuLabel>
        {APPEARANCE_OPTIONS.map((option) => (
          <MenuItem
            key={option.value}
            icon={option.icon}
            label={option.label}
            checked={preference === option.value}
            onClick={() => { setPreference(option.value); setOpen(false); }}
          />
        ))}
        <MenuSeparator />
        <MenuItem
          icon={<IconDatabase size={14} />}
          label="New connection…"
          onClick={() => { setOpen(false); onNewConnection(); }}
        />
      </Menu> : null}
    </div>
  );
}
