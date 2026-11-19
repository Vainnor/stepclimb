// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { newTag, TagField } from "./tag-field";

const tagNames = () =>
  screen
    .queryAllByRole("row")
    .map((row) => row.querySelector('[data-slot="tag-field-tag-text"]')?.textContent);

describe("newTag", () => {
  it("trims the text and refuses blank and repeated tags, whatever their case", () => {
    expect(newTag([], "  IFR  ")).toBe("IFR");
    expect(newTag(["IFR"], "ifr")).toBeNull();
    expect(newTag(["IFR"], "   ")).toBeNull();
    expect(newTag(["IFR"], "VFR")).toBe("VFR");
  });
});

describe("TagField", () => {
  it("names the input and the tag list by its label, with its data-slots", async () => {
    const user = userEvent.setup();
    renderWithStrings(<TagField label="Keywords" defaultValue={["Tower"]} />);
    const input = screen.getByRole("textbox", { name: "Keywords" });
    expect(input).toHaveAttribute("data-slot", "tag-field-input");
    const grid = screen.getByRole("grid", { name: "Keywords" });
    expect(grid).toHaveAttribute("data-slot", "tag-field-list");
    await user.click(screen.getByText("Keywords"));
    expect(input).toHaveFocus();
  });

  it("adds the typed tag on Enter without submitting the form, and clears the input", async () => {
    const user = userEvent.setup();
    const onSubmit = vi.fn((event: SubmitEvent) => event.preventDefault());
    const { container } = renderWithStrings(
      <form>
        <TagField label="Keywords" />
      </form>,
    );
    container.querySelector("form")?.addEventListener("submit", onSubmit);
    await user.type(screen.getByRole("textbox"), "  Ground control {Enter}");
    expect(tagNames()).toEqual(["Ground control"]);
    expect(screen.getByRole("textbox")).toHaveValue("");
    expect(onSubmit).not.toHaveBeenCalled();
    // An empty input leaves Enter to the form.
    await user.keyboard("{Enter}");
    expect(onSubmit).toHaveBeenCalledTimes(1);
  });

  it("adds with the Add button, which is off while the input is blank", async () => {
    const user = userEvent.setup();
    renderWithStrings(<TagField label="Keywords" />);
    const add = screen.getByRole("button", { name: "Add" });
    expect(add).toBeDisabled();
    await user.type(screen.getByRole("textbox"), "Approach");
    expect(add).toBeEnabled();
    await user.click(add);
    expect(tagNames()).toEqual(["Approach"]);
  });

  it("does not add a tag it already has", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<TagField label="Keywords" defaultValue={["Tower"]} onChange={onChange} />);
    await user.type(screen.getByRole("textbox"), "tower{Enter}");
    expect(tagNames()).toEqual(["Tower"]);
    expect(onChange).not.toHaveBeenCalled();
  });

  it("removes a tag with its button, named from the catalog and the tag", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <TagField label="Keywords" defaultValue={["Tower", "Ground"]} onChange={onChange} />,
    );
    const tower = screen.getByRole("row", { name: "Tower" });
    await user.click(within(tower).getByRole("button", { name: "Remove Tower" }));
    expect(onChange).toHaveBeenLastCalledWith(["Ground"]);
    expect(tagNames()).toEqual(["Ground"]);
  });

  it("removes the focused tag with Backspace and returns focus to the input after the last", async () => {
    const user = userEvent.setup();
    renderWithStrings(<TagField label="Keywords" defaultValue={["Tower"]} />);
    await user.tab(); // the input
    await user.tab(); // the tag list
    expect(screen.getByRole("row", { name: "Tower" })).toHaveFocus();
    await user.keyboard("{Backspace}");
    expect(screen.queryByRole("grid")).toBeNull();
    expect(screen.getByRole("textbox")).toHaveFocus();
  });

  it("moves between tags with the arrow keys, as one tab stop", async () => {
    const user = userEvent.setup();
    renderWithStrings(<TagField label="Keywords" defaultValue={["Tower", "Ground", "Delivery"]} />);
    await user.tab();
    await user.tab();
    expect(screen.getByRole("row", { name: "Tower" })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("row", { name: "Ground" })).toHaveFocus();
    // Tab leaves the list (React Aria lets it stop on the focused tag's remove button first).
    await user.tab();
    expect(document.activeElement?.getAttribute("role")).not.toBe("row");
    await user.tab();
    expect(document.body).toHaveFocus();
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(<TagField label="Keywords" value={["Tower"]} onChange={onChange} />);
    await user.type(screen.getByRole("textbox"), "Ground{Enter}");
    expect(onChange).toHaveBeenCalledWith(["Tower", "Ground"]);
    expect(tagNames()).toEqual(["Tower"]);
  });

  it("when disabled, takes no entry and offers no removal", () => {
    renderWithStrings(<TagField label="Keywords" defaultValue={["Tower"]} isDisabled />);
    expect(screen.getByRole("textbox")).toBeDisabled();
    expect(screen.queryByRole("button", { name: /Remove/ })).toBeNull();
  });

  it("shows the caller's error and marks the input invalid", () => {
    renderWithStrings(
      <TagField label="Keywords" isInvalid errorMessage="Add at least one keyword." />,
    );
    const input = screen.getByRole("textbox");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription("Add at least one keyword.");
  });

  it("names its buttons only from the strings catalog", () => {
    renderWithStrings(<TagField label="«caller»" defaultValue={["«caller»"]} />, { marked: true });
    expect(screen.getByRole("button", { name: "«tagField.add»" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "«tagField.remove» «caller»" })).toBeInTheDocument();
    // React Aria describes each removable tag with its own translated "Press Delete to remove
    // tag", with no prop to replace it.
    expect(unmarkedText(document.body)).toEqual(["PressDeletetoremovetag"]);
  });
});
