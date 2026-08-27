import { IconLock, IconShield } from "./icons";
import { shortcut } from "./platform";
import { Badge, Kbd } from "./ui";
import type { ConnectionProfile } from "./types";

export function StatusBar({
  profile,
  connected,
  engineLabel,
  tabCount,
}: {
  profile: ConnectionProfile | null;
  connected: boolean;
  engineLabel: string | null;
  tabCount: number;
}) {
  return (
    <footer className="statusbar">
      <span className="statusbar-item">
        <span className={`statusbar-dot ${connected ? "" : "statusbar-dot-offline"}`} aria-hidden="true" />
        {profile ? (connected ? "Connected" : "Not connected") : "No connection"}
      </span>
      {profile && engineLabel ? <span className="statusbar-item">{engineLabel}</span> : null}
      {profile ? <span className="statusbar-item statusbar-item-truncate mono">
        {profile.username}@{profile.host}:{profile.port}/{profile.defaultDatabase}
      </span> : null}
      {profile?.readOnly ? <Badge tone="warning" icon={<IconLock size={11} />}>Read-only</Badge> : null}
      <span className="statusbar-spacer" />
      {tabCount > 0 ? <span className="statusbar-item numeric">{tabCount} {tabCount === 1 ? "tab" : "tabs"}</span> : null}
      <span className="statusbar-item statusbar-local" title="Profiles, history, and results never leave this machine.">
        <IconShield size={12} />
        Local only
      </span>
      <span className="statusbar-item statusbar-hint">
        <Kbd>{shortcut("K")}</Kbd>
        commands
      </span>
    </footer>
  );
}
