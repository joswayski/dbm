import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

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
    expect(main).toHaveClass("connection-themed");
    expect(main).toHaveStyle("--connection-color: #ef4444");
    expect(screen.getByRole("heading", { name: profile.name })).toBeInTheDocument();
    expect(within(topbar!).queryByRole("button", { name: "New query" })).not.toBeInTheDocument();

    fireEvent.click(within(tabStrip!).getByRole("button", { name: "New query" }));
    const queryTab = await screen.findByRole("button", { name: "Query 1" });
    expect(queryTab).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Rename Query 1" }));
    const renameInput = screen.getByRole("textbox", { name: "Rename Query 1" });
    fireEvent.change(renameInput, { target: { value: "Revenue audit" } });
    fireEvent.keyDown(renameInput, { key: "Enter" });
    expect(await screen.findByRole("button", { name: "Revenue audit" })).toBeInTheDocument();

    fireEvent.click(connectionButton!);
    expect(await screen.findByRole("heading", { name: profile.name })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Revenue audit" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Revenue audit" }));
    fireEvent.click(screen.getByRole("button", { name: "Minimize Revenue audit" }));
    await waitFor(() => expect(screen.getByRole("heading", { name: profile.name })).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Revenue audit" })).toBeInTheDocument();
  });
});
