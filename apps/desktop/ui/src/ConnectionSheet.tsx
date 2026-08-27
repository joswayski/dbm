import { useEffect, useState } from "react";

import * as commands from "./commands";
import { parseConnectionUrl } from "./connectionUrl";
import { CONNECTION_COLORS, DEFAULT_CONNECTION_COLOR, ENGINE_PRESETS } from "./engines";
import { IconAlertCircle, IconCheckCircle, IconDatabase, IconInfo, IconKey, IconPlug, IconTrash } from "./icons";
import { Button, CheckboxField, Field, Notice, Segmented, Sheet, TextField } from "./ui";
import type { ConnectionProfile, DatabaseEngine, SaveProfileInput } from "./types";

function defaultProfile(profile?: ConnectionProfile): SaveProfileInput {
  const engine = profile?.engine ?? "postgres";
  const preset = ENGINE_PRESETS[engine];
  return {
    id: profile?.id,
    name: profile?.name ?? preset.name,
    color: profile?.color ?? DEFAULT_CONNECTION_COLOR,
    engine,
    host: profile?.host ?? "localhost",
    port: profile?.port ?? preset.port,
    username: profile?.username ?? preset.username,
    defaultDatabase: profile?.defaultDatabase ?? preset.defaultDatabase,
    tlsMode: profile?.tlsMode ?? "preferred",
    caCertPath: profile?.caCertPath ?? null,
    ssh: profile?.ssh ?? null,
    readOnly: profile?.readOnly ?? false,
    password: null,
  };
}

function applyEngineDefaults(form: SaveProfileInput, engine: DatabaseEngine): SaveProfileInput {
  const previous = ENGINE_PRESETS[form.engine];
  const next = ENGINE_PRESETS[engine];
  return {
    ...form,
    engine,
    name: form.name === previous.name ? next.name : form.name,
    port: form.port === previous.port ? next.port : form.port,
    username: form.username === previous.username ? next.username : form.username,
    defaultDatabase: form.defaultDatabase === previous.defaultDatabase ? next.defaultDatabase : form.defaultDatabase,
  };
}

export function ConnectionSheet({
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
  const [saveStage, setSaveStage] = useState<"testing" | "connecting" | null>(null);
  const [feedback, setFeedback] = useState<{ kind: "success" | "error" | "info"; message: string } | null>(null);
  const saving = saveStage !== null;
  const busy = testing || saving;
  const preset = ENGINE_PRESETS[form.engine];
  const update = <K extends keyof SaveProfileInput>(key: K, value: SaveProfileInput[K]) =>
    setForm((current) => ({ ...current, [key]: value }));

  useEffect(() => {
    const handleEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      onClose();
    };
    window.addEventListener("keydown", handleEscape);
    return () => window.removeEventListener("keydown", handleEscape);
  }, [onClose]);

  const importConnectionUrl = (value: string) => {
    try {
      const imported = parseConnectionUrl(value);
      setForm((current) => ({
        ...current,
        engine: imported.engine,
        name: !profile && (!current.name.trim() || current.name === ENGINE_PRESETS[current.engine].name)
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
    setSaveStage("testing");
    setFeedback({ kind: "info", message: "Testing connection before saving…" });
    try {
      await commands.testProfile(form);
      setSaveStage("connecting");
      setFeedback({ kind: "success", message: "Connection successful. Saving and connecting…" });
      await onSave(form);
    } catch (reason) {
      setFeedback({ kind: "error", message: errorMessage(reason) });
      setSaveStage(null);
    }
  };

  return (
    <Sheet
      title={profile ? "Edit connection" : "New connection"}
      titleId="connection-sheet-title"
      eyebrow={<span className="sheet-eyebrow" aria-hidden="true"><IconDatabase size={14} /></span>}
      onClose={onClose}
      footer={<>
        {onDelete ? (
          <Button variant="danger" icon={<IconTrash size={13} />} onClick={() => void onDelete()} disabled={busy}>Delete</Button>
        ) : null}
        <div className="sheet-footer-actions">
          <Button variant="secondary" onClick={() => void test()} disabled={busy} loading={testing}>
            {testing ? "Testing…" : "Test connection"}
          </Button>
          <Button
            variant="primary"
            icon={saving ? undefined : <IconPlug size={13} />}
            loading={saving}
            onClick={() => void save()}
            disabled={busy}
          >
            {saveStage === "testing" ? "Testing…" : saveStage === "connecting" ? "Connecting…" : "Save & connect"}
          </Button>
        </div>
      </>}
    >
      <div className="form-section">
        <Segmented
          label="Database engine"
          value={form.engine}
          onChange={(engine) => setForm((current) => applyEngineDefaults(current, engine))}
          options={[
            { value: "postgres", label: "PostgreSQL" },
            { value: "mysql", label: "MySQL" },
          ]}
        />
        <Field label="Connection URL" hint="Pasting a URL fills in the fields below. It is never stored as text.">
          <div className="input-with-action">
            <input
              className="input input-mono"
              type="password"
              aria-label="Connection URL"
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
              placeholder={preset.urlPlaceholder}
            />
            <Button
              variant="secondary"
              onClick={() => importConnectionUrl(connectionUrl)}
              disabled={!connectionUrl.trim()}
            >Import URL</Button>
          </div>
        </Field>
      </div>

      <div className="form-section">
        <div className="form-section-title">
          <strong>Identity</strong>
          <span>Shown in the sidebar, tabs, and status bar</span>
        </div>
        <div className="form-grid">
          <TextField
            full
            label="Name"
            value={form.name}
            placeholder={preset.name}
            onChange={(event) => update("name", event.target.value)}
          />
          <Field full label="Connection color">
            <div className="color-picker">
              {CONNECTION_COLORS.map((color) => (
                <button
                  key={color}
                  type="button"
                  className="color-swatch"
                  style={{ background: color }}
                  aria-pressed={form.color === color}
                  aria-label={`Use connection color ${color}`}
                  onClick={() => update("color", color)}
                />
              ))}
              <label className="custom-color" title="Choose a custom color">
                <input
                  type="color"
                  value={form.color ?? DEFAULT_CONNECTION_COLOR}
                  aria-label="Custom connection color"
                  onChange={(event) => update("color", event.target.value)}
                />
                Custom
              </label>
            </div>
          </Field>
        </div>
      </div>

      <div className="form-section">
        <div className="form-section-title">
          <strong>Server</strong>
          <span>{preset.label}</span>
        </div>
        <div className="form-grid">
          <TextField label="Host" value={form.host} onChange={(event) => update("host", event.target.value)} />
          <TextField
            label="Port"
            type="number"
            value={form.port}
            onChange={(event) => update("port", Number(event.target.value))}
          />
          <TextField label="Username" value={form.username} onChange={(event) => update("username", event.target.value)} />
          <TextField
            label="Database"
            value={form.defaultDatabase}
            onChange={(event) => update("defaultDatabase", event.target.value)}
          />
          <TextField
            full
            label="Password"
            type="password"
            value={form.password ?? ""}
            placeholder={profile ? "Leave blank to keep the saved password" : "Stored in your OS credential store"}
            onChange={(event) => update("password", event.target.value || undefined)}
          />
        </div>
      </div>

      <div className="form-section">
        <div className="form-section-title">
          <strong>Security</strong>
          <span>TLS and write protection</span>
        </div>
        <div className="form-grid">
          <Field label="TLS">
            <select
              className="select"
              value={form.tlsMode}
              onChange={(event) => update("tlsMode", event.target.value as SaveProfileInput["tlsMode"])}
            >
              <option value="preferred">Preferred</option>
              <option value="required">Required</option>
              <option value="disabled">Disabled</option>
            </select>
          </Field>
          <TextField
            label="CA certificate path"
            value={form.caCertPath ?? ""}
            placeholder="/path/to/root-ca.pem"
            onChange={(event) => update("caCertPath", event.target.value || null)}
          />
          <div className="full">
            <CheckboxField
              label="Read-only profile"
              hint="Blocks grid edits, staged deletes, and other GUI writes for this connection."
              checked={form.readOnly}
              onChange={(checked) => update("readOnly", checked)}
            />
          </div>
        </div>
        <Notice icon={<IconKey size={14} />}>
          Passwords are stored in your operating system credential manager — never in DBM's profile database.
        </Notice>
      </div>

      {feedback ? <Notice
        role="status"
        tone={feedback.kind === "error" ? "danger" : feedback.kind === "success" ? "success" : "info"}
        icon={feedback.kind === "error"
          ? <IconAlertCircle size={14} />
          : feedback.kind === "success" ? <IconCheckCircle size={14} /> : <IconInfo size={14} />}
      >{feedback.message}</Notice> : null}
    </Sheet>
  );
}

function errorMessage(reason: unknown): string {
  return reason instanceof Error ? reason.message : String(reason);
}
