import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { TableView } from "./TableView";

describe("TableView", () => {
  it("keeps pending cell edits when the result set is sorted", async () => {
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
    expect(screen.getByRole("button", { name: "Save changes (1)" })).toBeInTheDocument();
    fireEvent.mouseEnter(row!);
    expect(screen.getByText("Pending edits")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Sort by id" }));

    const updatedEmail = await screen.findByText("updated@example.com");
    await waitFor(() => expect(updatedEmail.closest("tr")).toHaveClass("staged-row"));
    expect(screen.getByRole("button", { name: "Save changes (1)" })).toBeInTheDocument();
  });

  it("selects a range and stages multiple rows for deletion", async () => {
    const { container } = render(<TableView profileId="preview" schema="public" table="users" />);
    await screen.findByText("person1@example.com");

    const checkboxes = screen.getAllByRole("checkbox");
    fireEvent.click(checkboxes[1]);
    fireEvent.click(checkboxes[2], { shiftKey: true });

    fireEvent.click(screen.getByRole("button", { name: "Delete selected (2)" }));

    expect(container.querySelectorAll("tr.deleted-row")).toHaveLength(2);
    expect(screen.getByRole("button", { name: "Save changes (2)" })).toBeInTheDocument();
  });
});
