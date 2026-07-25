import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TableView } from "./TableView";

describe("TableView", () => {
  it("keeps pending cell edits when sorted and can discard one row from its full diff", async () => {
    render(<TableView profileId="preview" schema="public" table="users" />);

    const email = await screen.findByText("person1@example.com");
    const row = email.closest("tr");
    const cell = email.closest("td");
    expect(row).not.toBeNull();
    expect(cell).not.toBeNull();

    fireEvent.doubleClick(cell!);
    const editor = within(row!).getByRole("textbox");
    fireEvent.change(editor, { target: { value: "updated@example.com" } });
    fireEvent.blur(editor);

    expect(row).toHaveClass("staged-row");
    expect(cell).toHaveClass("changed-cell");
    expect(screen.getByRole("button", { name: "Save changes (1)" })).toBeInTheDocument();
    fireEvent.mouseEnter(row!);
    const preview = screen.getByText("Pending edit").closest(".change-preview");
    expect(preview).toBeInTheDocument();
    expect(row!.contains(preview)).toBe(false);
    expect(screen.getByText("Before").nextElementSibling).toHaveTextContent("person1@example.com");
    expect(screen.getByText("After").nextElementSibling).toHaveTextContent("updated@example.com");
    expect(screen.getByText("After").parentElement).toHaveClass("change-diff-after");
    fireEvent.mouseLeave(row!);

    fireEvent.click(screen.getByRole("button", { name: "Sort by id" }));

    const updatedEmail = await screen.findByText("updated@example.com");
    const updatedRow = updatedEmail.closest("tr");
    await waitFor(() => expect(updatedRow).toHaveClass("staged-row"));
    fireEvent.mouseEnter(updatedRow!);
    fireEvent.click(screen.getByRole("button", { name: "Discard row edit" }));

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

    fireEvent.click(screen.getByRole("button", { name: "Remove filter" }));
    fireEvent.click(screen.getByRole("button", { name: "Apply filters" }));

    await waitFor(() => expect(screen.getByRole("button", { name: /Copy visible/ })).not.toBeDisabled());
    expect(screen.getByRole("button", { name: "Apply filters" })).toHaveClass("primary-button");
    expect(screen.queryByRole("button", { name: "Remove filter" })).not.toBeInTheDocument();
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

    fireEvent.click(screen.getByRole("button", { name: "Discard changes" }));
    expect(screen.queryByText("Save or discard pending row changes before exporting.")).not.toBeInTheDocument();
    expect(screen.getByText("person1@example.com")).toBeInTheDocument();
  });
});
