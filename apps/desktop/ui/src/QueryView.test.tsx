import { EditorView } from "@uiw/react-codemirror";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { QueryView } from "./App";
import * as commands from "./commands";

describe("QueryView", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("runs the statement under the cursor on Command+Enter without inserting a newline", async () => {
    const runQuery = vi.spyOn(commands, "runQuery");
    render(<QueryView profileId="preview" initialSql={"SELECT now();\n\nSELECT 1;"} />);

    const editor = screen.getByRole("textbox");
    fireEvent.keyDown(editor, { key: "Enter", code: "Enter", metaKey: true });

    await waitFor(() => expect(runQuery).toHaveBeenCalledWith({
      profileId: "preview",
      sql: "SELECT now();",
      maxRows: 10_000,
    }));
    expect(editor).toHaveTextContent("SELECT now();SELECT 1;");
    expect(await screen.findByText("DBM browser preview")).toBeInTheDocument();
  });

  it("uses disabled behavior for Cancel and exposes the current statement target", async () => {
    render(<QueryView profileId="preview" initialSql="SELECT now();" />);

    await waitFor(() => expect(document.querySelector(".history-item")).toBeInTheDocument());
    expect(screen.getByRole("button", { name: "Cancel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: /Run statement/ })).toBeEnabled();
    expect(document.querySelector(".cm-active-sql-line")).toBeInTheDocument();
  });

  it("shows a play action for selected SQL and runs only that selection", async () => {
    const runQuery = vi.spyOn(commands, "runQuery");
    const sql = "SELECT now();\n\nSELECT 1;";
    render(<QueryView profileId="preview" initialSql={sql} />);

    const editor = screen.getByRole("textbox");
    const view = EditorView.findFromDOM(editor);
    expect(view).not.toBeNull();
    act(() => view!.dispatch({ selection: { anchor: 15, head: sql.length } }));

    const runSelection = screen.getByRole("button", { name: "Run selection" });
    expect(runSelection).toHaveAttribute("title", "Run the selected SQL (Command/Ctrl+Enter)");
    expect(document.querySelector(".cm-active-sql-line")).not.toBeInTheDocument();
    fireEvent.click(runSelection);

    await waitFor(() => expect(runQuery).toHaveBeenCalledWith({
      profileId: "preview",
      sql: "SELECT 1;",
      maxRows: 10_000,
    }));
  });
});
