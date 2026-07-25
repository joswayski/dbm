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
    expect(screen.getByRole("heading", { name: profile.name })).toBeInTheDocument();
    expect(within(topbar!).queryByRole("button", { name: "New query" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Collapse connection" }));
    expect(screen.queryByLabelText("Database")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Expand connection" }));
    await screen.findByLabelText("Database");
    expect(screen.getByRole("button", { name: "Disconnect" })).toHaveAttribute(
      "title",
      "Disconnect and close this connection's tabs",
    );

    const usersTable = await screen.findByRole("button", { name: "users" });
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
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
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    expect(await screen.findByText("Schema refreshed · Added table public.audit_log.")).toBeInTheDocument();
    expect(await screen.findByRole("button", { name: "audit_log" })).toBeInTheDocument();

    fireEvent.click(usersTable);
    expect(usersTable).toHaveAttribute("aria-current", "page");
    expect(usersTable).toHaveClass("active");
    expect(screen.getByRole("button", { name: "Collapse public.users" })).toBeInTheDocument();
    expect(container.querySelector(".tab-color")).not.toBeInTheDocument();

    fireEvent.click(within(tabStrip!).getByRole("button", { name: "New query" }));
    const queryTab = await screen.findByRole("button", { name: "Query 1" });
    expect(queryTab).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "public.users" }));
    fireEvent.click(screen.getByRole("button", { name: "Collapse public.users" }));
    expect(screen.getByRole("button", { name: "Expand public.users" }).closest(".tab")).toHaveClass("collapsed");
    await waitFor(() => expect(screen.getByRole("heading", { name: "Query 1" })).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: "Rename Query 1" }));
    const renameInput = screen.getByRole("textbox", { name: "Rename Query 1" });
    fireEvent.change(renameInput, { target: { value: "Revenue audit" } });
    fireEvent.keyDown(renameInput, { key: "Enter" });
    expect(await screen.findByRole("button", { name: "Revenue audit" })).toBeInTheDocument();

    fireEvent.click(connectionButton!);
    expect(await screen.findByRole("heading", { name: profile.name })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Revenue audit" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Revenue audit" }));
    fireEvent.click(screen.getByRole("button", { name: "Collapse Revenue audit" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: "public.users" })).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Expand Revenue audit" }).closest(".tab")).toHaveClass("collapsed");
  });

  it("closes the connection popup with Escape", () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "New connection" }));
    expect(screen.getByRole("dialog", { name: "New connection" })).toBeInTheDocument();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "New connection" })).not.toBeInTheDocument();
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
