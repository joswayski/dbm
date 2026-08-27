import { useEffect, useState } from "react";

import * as commands from "./commands";
import { IconAlertCircle, IconCheckCircle, IconDownload, IconRefresh } from "./icons";
import { Button, IconButton } from "./ui";
import type { UpdateStatus } from "./types";

export function UpdateControl() {
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
      setActionError(reason instanceof Error ? reason.message : String(reason));
    }
  };

  const downloading = status?.state === "downloading";
  const progress = downloading && status.total
    ? Math.min(100, Math.round((status.downloaded / status.total) * 100))
    : null;
  const installedVersion = status?.current_version ?? "…";
  const failed = status?.state === "error" || Boolean(actionError);
  const detail = actionError ||
    (status?.state === "error" ? status.message : status?.state === "available" ? status.notes : "");

  if (status?.state === "available") {
    return (
      <div className="update-control">
        <Button
          variant="primary"
          size="sm"
          icon={<IconDownload size={13} />}
          onClick={() => void run()}
          title={detail || `Installed version ${installedVersion}`}
        >{status.installable ? `Update to ${status.version}` : `Download ${status.version}`}</Button>
      </div>
    );
  }

  if (downloading) {
    return (
      <div className="update-control">
        <Button variant="secondary" size="sm" loading disabled>
          {progress === null ? "Installing…" : `Installing… ${progress}%`}
        </Button>
        {progress === null ? null : <span className="update-progress" aria-hidden="true"><span style={{ width: `${progress}%` }} /></span>}
      </div>
    );
  }

  if (failed) {
    return (
      <div className="update-control">
        <Button
          variant="secondary"
          size="sm"
          icon={<IconAlertCircle size={13} />}
          onClick={() => void run()}
          title={detail || "The last update check failed."}
        >Retry update</Button>
      </div>
    );
  }

  const checking = status?.state === "checking";
  const upToDate = status?.state === "up_to_date";
  const label = checking ? "Checking…" : upToDate ? "Up to date" : "Check for updates";
  return (
    <div className="update-control">
      <IconButton
        icon={upToDate ? <IconCheckCircle size={15} /> : <IconRefresh size={15} />}
        label={label}
        tooltip={false}
        tooltipAlign="end"
        disabled={checking}
        title={`${label} · installed version ${installedVersion}`}
        onClick={() => void run()}
      />
    </div>
  );
}
