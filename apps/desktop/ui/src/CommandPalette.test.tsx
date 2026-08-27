import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import App from "./App";
import { useAppearanceStore } from "./appearance";
import * as commands from "./commands";
import { useDbmStore } from "./store";

async function connectedApp() {
  const profile = await commands.saveProfile({
    name: `Palette ${crypto.randomUUID().slice(0, 8)}`,
    color: "#4ad4e8",
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
  const view = render(<App />);
  const name = await screen.findByText(profile.name);
  fireEvent.click(name.closest("button")!);
  await screen.findByLabelText("Database");
  return { profile, ...view };
}

describe("command palette", () => {
  beforeEach(() => {
    window.localStorage.clear();
    useAppearanceStore.setState({ preference: "system", resolved: "dark" });
    useDbmStore.setState({
      profiles: [],
      workspaces: {},
      activeProfileId: null,
      tabs: [],
      activeTabId: null,
    });
  });

  it("opens a table from the palette with the keyboard", async () => {
    await connectedApp();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const search = await screen.findByRole("textbox", { name: /Search connections/ });
    fireEvent.change(search, { target: { value: "orders" } });

    const palette = screen.getByRole("dialog", { name: "Command palette" });
    const options = within(palette).getAllByRole("option");
    expect(options).toHaveLength(1);
    expect(options[0]).toHaveAttribute("aria-selected", "true");

    fireEvent.keyDown(palette, { key: "Enter" });

    expect(await screen.findByRole("button", { name: "public.orders" })).toBeInTheDocument();
    expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();
  }, 15_000);

  it("dismisses the palette with Escape and reports empty searches", async () => {
    await connectedApp();

    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    const palette = await screen.findByRole("dialog", { name: "Command palette" });
    fireEvent.change(screen.getByRole("textbox", { name: /Search connections/ }), { target: { value: "zzz" } });
    expect(within(palette).queryAllByRole("option")).toHaveLength(0);
    expect(screen.getByText("No matches for “zzz”.")).toBeInTheDocument();

    fireEvent.keyDown(palette, { key: "Escape" });
    expect(screen.queryByRole("dialog", { name: "Command palette" })).not.toBeInTheDocument();
  }, 15_000);

  it("switches appearance from the settings menu with the keyboard", async () => {
    render(<App />);

    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    const menu = screen.getByRole("menu", { name: "Settings" });
    expect(screen.getByRole("menuitem", { name: "Match system" })).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    expect(screen.getByRole("menuitem", { name: "Dark" })).toHaveFocus();
    fireEvent.keyDown(menu, { key: "ArrowDown" });
    const light = screen.getByRole("menuitem", { name: "Light" });
    expect(light).toHaveFocus();
    fireEvent.click(light);

    expect(document.documentElement.dataset.appearance).toBe("light");
    expect(window.localStorage.getItem("dbm.appearance")).toBe("light");

    fireEvent.click(screen.getByRole("button", { name: "Settings" }));
    fireEvent.click(screen.getByRole("menuitem", { name: "Dark" }));
    expect(document.documentElement.dataset.appearance).toBe("dark");
  });
});
