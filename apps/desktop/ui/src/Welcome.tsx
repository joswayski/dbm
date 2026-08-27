import { IconDatabase, IconPlug, IconPlus, IconSearch } from "./icons";
import { shortcut } from "./platform";
import { Button, Kbd } from "./ui";
import type { ConnectionProfile } from "./types";

export function Welcome({
  hasProfiles,
  profile,
  connected,
  onNewConnection,
  onConnect,
  onOpenPalette,
}: {
  hasProfiles: boolean;
  profile: ConnectionProfile | null;
  connected: boolean;
  onNewConnection: () => void;
  onConnect: () => void;
  onOpenPalette: () => void;
}) {
  const state = profile ? (connected ? "connected" : "disconnected") : hasProfiles ? "idle" : "first-run";
  const title = state === "first-run"
    ? "Welcome to DBM"
    : state === "idle"
      ? "No connection selected"
      : profile!.name;
  const description = state === "first-run"
    ? "A local-first workbench for PostgreSQL and MySQL. Connection profiles, query history, and results stay on this machine — passwords live in your operating system's credential store."
    : state === "idle"
      ? "Pick a connection in the sidebar to browse its schema, or search everything from the command palette."
      : state === "disconnected"
        ? "This connection is selected but not connected yet."
        : "Open a table from the sidebar, or start a new query tab.";

  return (
    <div className="welcome">
      <span className="welcome-mark" aria-hidden="true"><IconDatabase size={22} /></span>
      <h1>{title}</h1>
      <p>{description}</p>
      <div className="welcome-actions">
        {state === "disconnected" ? (
          <Button variant="primary" icon={<IconPlug size={14} />} onClick={onConnect}>Connect</Button>
        ) : (
          <Button variant="primary" icon={<IconPlus size={14} />} onClick={onNewConnection}>Create a connection</Button>
        )}
        <Button variant="secondary" icon={<IconSearch size={14} />} onClick={onOpenPalette}>Open command palette</Button>
      </div>
      <div className="welcome-shortcuts">
        <span className="welcome-shortcut">Search connections and tables<Kbd>{shortcut("K")}</Kbd></span>
        <span className="welcome-shortcut">New query tab<Kbd>{shortcut("T")}</Kbd></span>
        <span className="welcome-shortcut">Run the statement under the cursor<Kbd>{shortcut("↵")}</Kbd></span>
      </div>
    </div>
  );
}
