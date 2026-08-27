import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import App from "./App";
import * as commands from "./commands";
import { useDbmStore } from "./store";

describe("App connection and query navigation", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useDbmStore.setState({
      profiles: [],
      workspaces: {},
      activeProfileId: null,
      tabs: [],
      activeTabId: null,
    });
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("keeps one connected profile section and manages query tabs from the tab strip", async () => {
    const profile = await commands.saveProfile({
      name: "Unified connection",
      color: "#ef4444",
      engine: "postgres",
      host: "localhost",
      port: 5432,
      username: "postgres",
      defaultDatabase: "postgres",
      tlsMode: "disabled",
      caCertPath: null,
      ssh: null,
      readOnly: false,
    });
    const { container } = render(<App />);

    const profileName = await screen.findByText(profile.name);
    const connectionButton = profileName.closest("button");
    expect(connectionButton).not.toBeNull();
    expect(screen.queryByRole("button", { name: "Connect" })).not.toBeInTheDocument();
    fireEvent.click(connectionButton!);

    await screen.findByLabelText("Database");
    const sidebar = container.querySelector<HTMLElement>(".sidebar");
    const main = container.querySelector<HTMLElement>(".main-pane");
    const tabStrip = container.querySelector<HTMLElement>(".tab-strip");
    const topbar = container.querySelector<HTMLElement>(".topbar");
    expect(sidebar).not.toBeNull();
    expect(main).not.toBeNull();
    expect(tabStrip).not.toBeNull();
    expect(topbar).not.toBeNull();
    expect(within(sidebar!).getAllByText(profile.name, { exact: true })).toHaveLength(1);
    expect(connectionButton).toHaveAttribute("aria-current", "page");
    expect(main).not.toHaveClass("connection-themed");
    expect(main).toHaveStyle("--connection-color: #ef4444");
    // Connecting opens a query tab immediately so the user can run SQL right away.
    // Prefer the tab strip control — full QueryView/CodeMirror mount is expensive in CI.
    expect(await screen.findByRole("button", { name: "Query 1" })).toBeInTheDocument();
    expect(within(tabStrip!).getByRole("button", { name: "New query" })).toBeInTheDocument();
    expect(within(topbar!).queryByRole("button", { name: "New query" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Collapse connection" }));
    expect(screen.queryByLabelText("Database")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand connection" }));
    await screen.findByLabelText("Database");
    expect(screen.queryByRole("menuitem", { name: "Disconnect" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: `Connection actions for ${profile.name}` }));
    expect(screen.getByRole("menuitem", { name: "Disconnect" })).toHaveAttribute("title", "Close this connection and its tabs");
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("menuitem", { name: "Disconnect" })).not.toBeInTheDocument();

    const usersTable = await screen.findByRole("button", { name: "users" });
    fireEvent.click(within(sidebar!).getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Schema is already up to date.")).toBeInTheDocument();

    vi.spyOn(commands, "loadSchemaTree").mockResolvedValueOnce([{
      name: "public",
      kind: "schema",
      schema: "public",
      table: null,
      children: [
        { name: "users", kind: "table", schema: "public", table: "users", children: [] },
        { name: "orders", kind: "table", schema: "public", table: "orders", children: [] },
        { name: "audit_log", kind: "table", schema: "public", table: "audit_log", children: [] },
      ],
    }]);
    fireEvent.click(within(sidebar!).getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Schema refreshed · Added table public.audit_log.")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "audit_log" })).toBeInTheDocument();

    fireEvent.click(usersTable);
    expect(usersTable).toHaveAttribute("aria-current", "page");
    expect(usersTable).toHaveClass("active");
    expect(screen.getByRole("button", { name: "Collapse public.users" })).toBeInTheDocument();
    expect(container.querySelector(".tab-color")).not.toBeInTheDocument();

    // Rename the auto-opened query tab (avoids mounting a second CodeMirror in this flow).
    fireEvent.click(screen.getByRole("button", { name: "Query 1" }));
    fireEvent.click(screen.getByRole("button", { name: "Rename Query 1" }));
    const renameInput = screen.getByRole("textbox", { name: "Rename Query 1" });
    fireEvent.change(renameInput, { target: { value: "Revenue audit" } });
    fireEvent.keyDown(renameInput, { key: "Enter" });
    expect(await screen.findByRole("button", { name: "Revenue audit" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "public.users" }));
    fireEvent.click(screen.getByRole("button", { name: "Collapse public.users" }));
    expect(screen.getByRole("button", { name: "Expand public.users" }).closest(".tab")).toHaveClass("collapsed");
    await waitFor(() => expect(screen.getByRole("button", { name: "Collapse Revenue audit" })).toBeInTheDocument());

    fireEvent.click(connectionButton!);
    expect(screen.getByRole("button", { name: "Collapse Revenue audit" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Revenue audit" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Revenue audit" }));
    fireEvent.click(screen.getByRole("button", { name: "Collapse Revenue audit" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: "public.users" })).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Expand Revenue audit" }).closest(".tab")).toHaveClass("collapsed");

    fireEvent.click(screen.getByRole("button", { name: `Connection actions for ${profile.name}` }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Disconnect" }));
    expect(await screen.findByRole("heading", { name: "No connection selected" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Revenue audit" })).not.toBeInTheDocument();
  }, 15_000);

  it("asks before deleting a saved connection", async () => {
    const profile = await commands.saveProfile({
      name: "Disposable",
      color: "#64748b",
      engine: "postgres",
      host: "localhost",
      port: 5432,
      username: "postgres",
      defaultDatabase: "postgres",
      tlsMode: "disabled",
      caCertPath: null,
      ssh: null,
      readOnly: false,
    });
    const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
    const deleteProfile = vi.spyOn(commands, "deleteProfile");
    render(<App />);

    await screen.findByText(profile.name);
    fireEvent.click(screen.getByRole("button", { name: `Connection actions for ${profile.name}` }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Edit connection" }));
    fireEvent.click(screen.getByRole("button", { name: "Delete" }));

    expect(confirm).toHaveBeenCalledWith(expect.stringContaining(profile.name));
    expect(deleteProfile).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "Edit connection" })).toBeInTheDocument();
  });

  it("closes the connection popup with Escape", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "New connection" }));
    expect(screen.getByRole("dialog", { name: "New connection" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "New connection" })).not.toBeInTheDocument();
  });

  it("checks for a signed update from the top bar", async () => {
    vi.spyOn(commands, "getUpdateStatus").mockResolvedValue({
      state: "idle",
      current_version: "0.1.0",
    });
    vi.spyOn(commands, "listenForUpdateStatus").mockResolvedValue(() => undefined);
    const check = vi.spyOn(commands, "checkForUpdates").mockResolvedValue({
      state: "available",
      current_version: "0.1.0",
      version: "0.2.0",
      notes: "A focused update.",
      installable: true,
      manual_download_url: null,
    });

    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Check for updates" }));

    await waitFor(() => expect(check).toHaveBeenCalledOnce());
    expect(screen.getByRole("button", { name: "Update to 0.2.0" })).toHaveAttribute(
      "title",
      "A focused update.",
    );
  });

  it("switches new-connection defaults to MySQL", () => {
    render(<App />);
    fireEvent.click(screen.getByRole("button", { name: "New connection" }));
    fireEvent.click(screen.getByRole("button", { name: "MySQL" }));
    expect(screen.getByRole("button", { name: "MySQL" })).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByRole("button", { name: "PostgreSQL" })).toHaveAttribute("aria-pressed", "false");
    expect(screen.getByPlaceholderText("mysql://user:password@host:3306/database")).toBeInTheDocument();
    expect(screen.getByPlaceholderText("Local MySQL")).toBeInTheDocument();
    expect(screen.getByDisplayValue(3306)).toBeInTheDocument();
    expect(screen.getByDisplayValue("root")).toBeInTheDocument();
  });

  it("tests Save & connect before persisting the profile", async () => {
    const testProfile = vi.spyOn(commands, "testProfile").mockRejectedValueOnce(new Error("Connection refused"));
    const saveProfile = vi.spyOn(commands, "saveProfile");
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "New connection" }));
    fireEvent.click(screen.getByRole("button", { name: "Save & connect" }));

    expect(await screen.findByText("Connection refused")).toBeInTheDocument();
    expect(testProfile).toHaveBeenCalledOnce();
    expect(saveProfile).not.toHaveBeenCalled();
    expect(screen.getByRole("dialog", { name: "New connection" })).toBeInTheDocument();
  });
});
