import { EditorView } from "@uiw/react-codemirror";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { QueryView } from "./App";
import * as commands from "./commands";
import type { QueryResponse, SchemaNode } from "./types";

const schemaTree: SchemaNode[] = [{
  name: "public",
  kind: "schema",
  schema: "public",
  table: null,
  children: [
    { name: "users", kind: "table", schema: "public", table: "users", children: [] },
    { name: "orders", kind: "table", schema: "public", table: "orders", children: [] },
  ],
}];

describe("QueryView", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("uses Redis command targeting and workbench labels", async () => {
    const runQuery = vi.spyOn(commands, "runQuery");
    render(<QueryView profileId="preview" engine="redis" initialSql={"PING\n\nGET greeting"} title="Redis" />);

    expect(screen.getByText("REDIS WORKBENCH")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Run command/ }));

    await waitFor(() => expect(runQuery).toHaveBeenCalledWith({
      profileId: "preview",
      sql: "PING",
      maxRows: 10_000,
    }));
    expect(await screen.findByText("PONG")).toBeInTheDocument();
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

  it("hides Cancel while idle and exposes the current statement target", async () => {
    render(<QueryView profileId="preview" initialSql="SELECT now();" />);

    expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Run statement/ })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
    expect(document.querySelector(".cm-active-sql-line")).toBeInTheDocument();
  });

  it("refreshes by re-running the last executed statement", async () => {
    const runQuery = vi.spyOn(commands, "runQuery");
    render(<QueryView profileId="preview" initialSql="SELECT now();\n\nSELECT 1;" />);

    fireEvent.click(screen.getByRole("button", { name: /Run statement/ }));
    await waitFor(() => expect(runQuery).toHaveBeenCalledWith({
      profileId: "preview",
      sql: "SELECT now();",
      maxRows: 10_000,
    }));

    const refresh = screen.getByRole("button", { name: "Refresh" });
    expect(refresh).toBeEnabled();
    fireEvent.click(refresh);

    await waitFor(() => expect(runQuery).toHaveBeenCalledTimes(2));
    expect(runQuery).toHaveBeenLastCalledWith({
      profileId: "preview",
      sql: "SELECT now();",
      maxRows: 10_000,
    });

    const editor = screen.getByRole("textbox");
    const view = EditorView.findFromDOM(editor);
    expect(view).not.toBeNull();
    act(() => view!.dispatch({
      changes: { from: 0, to: view!.state.doc.length, insert: "SELECT 1;" },
    }));
    fireEvent.click(screen.getByRole("button", { name: "Refresh" }));
    await waitFor(() => expect(runQuery).toHaveBeenCalledTimes(3));
    expect(runQuery).toHaveBeenLastCalledWith({
      profileId: "preview",
      sql: "SELECT now();",
      maxRows: 10_000,
    });
  });

  it("shares database-scoped history live across query tabs", async () => {
    const profileId = `shared-${crypto.randomUUID()}`;
    const { container } = render(<>
      <QueryView profileId={profileId} database="postgres" initialSql="SELECT 42;" title="Query A" />
      <QueryView profileId={profileId} database="postgres" initialSql="SELECT 7;" title="Query B" />
    </>);
    const views = container.querySelectorAll<HTMLElement>(".query-view");
    const historyPanels = container.querySelectorAll<HTMLElement>(".history-panel");

    fireEvent.click(within(views[0]).getByRole("button", { name: /Run statement/ }));

    await waitFor(() => {
      expect(historyPanels[0].querySelector(".history-sql")).toHaveTextContent("SELECT 42");
      expect(historyPanels[1].querySelector(".history-sql")).toHaveTextContent("SELECT 42");
    });
  });

  it("uses the full table viewer for an exact, uniquely resolved table select", async () => {
    render(
      <QueryView
        profileId="preview"
        schemaTree={schemaTree}
        initialSql="SELECT * FROM users;"
        title="Editable query"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /Run statement/ }));

    expect(await screen.findByText("Editable table")).toBeInTheDocument();
    expect(await screen.findByRole("heading", { name: "public.users" })).toBeInTheDocument();
    expect(await screen.findByText("person1@example.com")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse email" })).toBeInTheDocument();
  });

  it("keeps complex query results read-only and explains that state", async () => {
    render(
      <QueryView
        profileId="preview"
        schemaTree={schemaTree}
        initialSql="SELECT users.id FROM users JOIN orders ON orders.user_id = users.id;"
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: /Run statement/ }));

    const readOnly = await screen.findByText("Read-only result");
    expect(readOnly).toHaveAttribute(
      "title",
      "This query does not resolve to one complete table, so DBM cannot safely map edits back to rows.",
    );
    expect(await screen.findByText("DBM browser preview")).toBeInTheDocument();
  });

  it("blocks another query while the embedded table viewer has pending edits", async () => {
    const runQuery = vi.spyOn(commands, "runQuery");
    render(
      <QueryView
        profileId="preview"
        schemaTree={schemaTree}
        initialSql="SELECT * FROM public.users;"
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: /Run statement/ }));

    const email = await screen.findByText("person1@example.com");
    const row = email.closest("tr");
    fireEvent.doubleClick(email.closest("td")!);
    const editor = within(row!).getByRole("textbox");
    fireEvent.change(editor, { target: { value: "pending@example.com" } });
    fireEvent.blur(editor);
    await waitFor(() => expect(screen.getByText("1 pending change")).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: /Run statement/ }));

    expect(await screen.findByText("Save or discard the pending table changes before running another query.")).toBeInTheDocument();
    expect(runQuery).toHaveBeenCalledTimes(1);
  });

  it("shows Cancel only while a query is running", async () => {
    let resolveQuery: ((response: QueryResponse) => void) | undefined;
    vi.spyOn(commands, "runQuery").mockImplementation(() => new Promise((resolve) => {
      resolveQuery = resolve;
    }));
    render(<QueryView profileId="preview" initialSql="SELECT now();" title="Slow query" />);

    fireEvent.click(screen.getByRole("button", { name: /Run statement/ }));
    expect(await screen.findByRole("button", { name: "Cancel" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Slow query" })).toBeInTheDocument();

    act(() => resolveQuery?.({
      columns: [],
      rows: [],
      rowCount: 0,
      affectedRows: 0,
      durationMs: 1,
      truncated: false,
      notices: [],
    }));

    await waitFor(() => expect(screen.queryByRole("button", { name: "Cancel" })).not.toBeInTheDocument());
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
