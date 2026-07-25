import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";

import * as commands from "./commands";
import { TableView } from "./TableView";

describe("TableView", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("keeps pending cell edits when sorted and can discard one row from its full diff", async () => {
    render(<TableView profileId="preview" schema="public" table="users" />);

    const email = await screen.findByText("person1@example.com");
    const row = email.closest("tr");
    const cell = email.closest("td");
    expect(row).not.toBeNull();
    expect(cell).not.toBeNull();

    fireEvent.doubleClick(cell!);
    const editor = within(row!).getByRole("textbox");
    fireEvent.change(editor, { target: { value: "person1+NEW@example.com" } });
    expect(editor).toHaveValue("person1+NEW@example.com");
    expect(row).not.toHaveClass("staged-row");
    fireEvent.blur(editor);

    expect(row).toHaveClass("staged-row");
    expect(cell).toHaveClass("changed-cell");
    expect(screen.getByRole("button", { name: "Save changes (1)" })).toBeInTheDocument();
    fireEvent.mouseEnter(row!);
    const preview = screen.getByText("Pending edit").closest(".change-preview");
    expect(preview).toBeInTheDocument();
    expect(row!.contains(preview)).toBe(false);
    expect(screen.getByText("Before").nextElementSibling).toHaveTextContent("person1@example.com");
    expect(screen.getByText("After").nextElementSibling).toHaveTextContent("person1+NEW@example.com");
    expect(screen.getByText("After").parentElement).toHaveClass("change-diff-after");
    expect(screen.getByText("+NEW")).toHaveClass("change-inline-added");
    fireEvent.mouseLeave(row!);

    fireEvent.click(screen.getByRole("button", { name: "Sort by id" }));

    const updatedEmail = await screen.findByText("person1+NEW@example.com");
    const updatedRow = updatedEmail.closest("tr");
    await waitFor(() => expect(updatedRow).toHaveClass("staged-row"));
    fireEvent.mouseEnter(updatedRow!);
    fireEvent.click(screen.getByRole("button", { name: "Discard edit" }));

    expect(await screen.findByText("person1@example.com")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Save changes (1)" })).not.toBeInTheDocument();
  });

  it("selects a range by clicking rows and stages multiple deletions without checkboxes", async () => {
    const { container } = render(<TableView profileId="preview" schema="public" table="users" />);
    await screen.findByText("person1@example.com");

    expect(screen.queryByRole("checkbox")).not.toBeInTheDocument();
    const rows = container.querySelectorAll("tbody tr");
    fireEvent.click(rows[0]);
    fireEvent.click(rows[1], { shiftKey: true });

    expect(rows[0]).toHaveAttribute("aria-selected", "true");
    expect(rows[1]).toHaveAttribute("aria-selected", "true");
    expect(screen.queryByRole("button", { name: /Delete selected/ })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "2 selected" }));
    const stageDeletion = screen.getByRole("menuitem", { name: "Stage 2 rows for deletion" });
    expect(stageDeletion).toHaveClass("danger");
    fireEvent.click(stageDeletion);

    expect(container.querySelectorAll("tr.deleted-row")).toHaveLength(2);
    expect(screen.getByText("2 pending changes")).toBeInTheDocument();
    expect(screen.getByText("2 deletions")).toBeInTheDocument();
    fireEvent.mouseEnter(rows[0]);
    expect(screen.getByText("Pending delete")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Undo delete" }));

    expect(container.querySelectorAll("tr.deleted-row")).toHaveLength(1);
    expect(screen.getByText("1 deletion")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Save changes (1)" })).toBeInTheDocument();
  });

  it("keeps the filter list empty after applying an intentionally empty filter set", async () => {
    render(<TableView profileId="preview" schema="public" table="users" />);
    await screen.findByText("person1@example.com");

    expect(screen.queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Remove filter" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply filters" }));

    await waitFor(() => expect(screen.getByRole("button", { name: /Copy visible/ })).not.toBeDisabled());
    expect(screen.getByRole("button", { name: "Apply filters" })).toHaveClass("primary-button");
    expect(screen.queryByRole("button", { name: "Remove filter" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Clear" })).not.toBeInTheDocument();
  });

  it("uses the primary key as the single default sort and exposes its direction", async () => {
    render(<TableView profileId="preview" schema="public" table="users" />);
    await screen.findByText("person1@example.com");

    const previewLimit = screen.getByRole("spinbutton", { name: "Preview limit" });
    fireEvent.click(screen.getByRole("button", { name: "Decrease preview limit" }));
    expect(previewLimit).toHaveValue(199);
    fireEvent.click(screen.getByRole("button", { name: "Increase preview limit" }));
    expect(previewLimit).toHaveValue(200);

    const sort = screen.getByRole("combobox", { name: "Sort by" });
    expect(sort).toHaveValue("id");
    expect(within(sort).getAllByRole("option", { name: /id/ })).toHaveLength(1);
    expect(within(sort).getByRole("option", { name: "id (primary key)" })).toBeInTheDocument();
    expect(screen.getByLabelText("Sort direction")).toHaveValue("asc");

    fireEvent.change(sort, { target: { value: "email" } });
    expect(screen.getByLabelText("Sort direction")).toHaveValue("asc");
    fireEvent.change(screen.getByLabelText("Sort direction"), { target: { value: "desc" } });
    expect(screen.getByLabelText("Sort direction")).toHaveValue("desc");
    fireEvent.change(sort, { target: { value: "id" } });
    expect(screen.getByLabelText("Sort direction")).toHaveValue("asc");
  });

  it("shows the collapsed column name and focuses the target column immediately", async () => {
    render(<TableView profileId="preview" schema="public" table="users" />);
    await screen.findByText("person1@example.com");

    const collapse = screen.getByRole("button", { name: "Collapse email" });
    expect(collapse).not.toHaveAttribute("title");
    expect(collapse).toHaveAttribute("data-tooltip", "Collapse email");
    fireEvent.mouseEnter(collapse);
    expect(collapse.closest("th")).toHaveClass("column-action-target");
    expect(screen.getByRole("button", { name: "Sort by id" }).closest("th")).toHaveClass("column-action-dimmed");

    fireEvent.click(collapse);
    const expand = screen.getByRole("button", { name: "Expand email" });
    expect(expand).toHaveTextContent("email");
    expect(expand).not.toHaveAttribute("title");
    expect(expand).toHaveAttribute("data-tooltip", "Expand email");
  });

  it("clears the pending-change export warning when all changes are discarded", async () => {
    render(<TableView profileId="preview" schema="public" table="users" />);

    const email = await screen.findByText("person1@example.com");
    const row = email.closest("tr");
    fireEvent.doubleClick(email.closest("td")!);
    const editor = within(row!).getByRole("textbox");
    fireEvent.change(editor, { target: { value: "discard-me@example.com" } });
    fireEvent.blur(editor);

    fireEvent.click(screen.getByRole("button", { name: "Export all (12)" }));
    expect(screen.getByText("Save or discard pending row changes before exporting.")).toBeInTheDocument();

    const pendingWarning = screen.getByText("1 pending change").closest<HTMLElement>(".pending-changes");
    expect(pendingWarning).not.toBeNull();
    fireEvent.click(within(pendingWarning!).getByRole("button", { name: "Discard changes" }));
    expect(screen.queryByText("Save or discard pending row changes before exporting.")).not.toBeInTheDocument();
    expect(screen.getByText("person1@example.com")).toBeInTheDocument();
  });

  it("keeps an edit preview interactive while crossing an adjacent pending row", async () => {
    const { container } = render(<TableView profileId="preview" schema="public" table="users" />);
    const firstEmail = await screen.findByText("person1@example.com");
    const rows = container.querySelectorAll<HTMLElement>("tbody tr");

    fireEvent.doubleClick(firstEmail.closest("td")!);
    const editor = within(rows[0]).getByRole("textbox");
    fireEvent.change(editor, { target: { value: "stay-open@example.com" } });
    fireEvent.blur(editor);

    fireEvent.contextMenu(rows[1]);
    fireEvent.click(screen.getByRole("button", { name: "Stage row for deletion" }));
    expect(rows[1]).toHaveClass("deleted-row");

    fireEvent.mouseEnter(rows[0]);
    expect(screen.getByText("Pending edit")).toBeInTheDocument();
    fireEvent.mouseLeave(rows[0]);
    fireEvent.mouseEnter(rows[1]);
    expect(screen.getByText("Pending edit")).toBeInTheDocument();
    expect(screen.queryByText("Pending delete")).not.toBeInTheDocument();

    const preview = screen.getByText("Pending edit").closest<HTMLElement>(".change-preview");
    fireEvent.mouseEnter(preview!);
    fireEvent.click(within(preview!).getByRole("button", { name: "Discard edit" }));

    expect(await screen.findByText("person1@example.com")).toBeInTheDocument();
    expect(rows[1]).toHaveClass("deleted-row");
  });

  it("offers open-file and show-in-folder actions after export", async () => {
    const writer = {
      path: "/tmp/public.users.csv",
      write: vi.fn().mockResolvedValue(undefined),
      close: vi.fn().mockResolvedValue(undefined),
      abort: vi.fn().mockResolvedValue(undefined),
    };
    vi.spyOn(commands, "createCsvExportWriter").mockResolvedValue(writer);
    const openFile = vi.spyOn(commands, "openExportedFile").mockResolvedValue(undefined);
    const revealFile = vi.spyOn(commands, "revealExportedFile").mockResolvedValue(undefined);
    render(<TableView profileId="preview" schema="public" table="users" />);
    await screen.findByText("person1@example.com");

    fireEvent.click(screen.getByRole("button", { name: "Export all (12)" }));
    const file = await screen.findByRole("button", { name: "public.users.csv" });
    expect(writer.close).toHaveBeenCalled();

    fireEvent.click(file);
    fireEvent.click(screen.getByRole("button", { name: "Show in folder" }));
    await waitFor(() => {
      expect(openFile).toHaveBeenCalledWith("/tmp/public.users.csv");
      expect(revealFile).toHaveBeenCalledWith("/tmp/public.users.csv");
    });
  });
});
