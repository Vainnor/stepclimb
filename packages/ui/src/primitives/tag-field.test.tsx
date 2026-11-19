// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { newTag, TagField } from "./tag-field";

const status = (container: HTMLElement) =>
  container.querySelector('[data-slot="tag-field-status"]') as HTMLElement;

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

  it("keeps a repeated tag in the input, says it is already added and announces it", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    const { container } = renderWithStrings(
      <TagField label="Keywords" defaultValue={["Tower"]} onChange={onChange} />,
    );
    const input = screen.getByRole("textbox");
    await user.type(input, "tower{Enter}");
    expect(tagNames()).toEqual(["Tower"]);
    expect(onChange).not.toHaveBeenCalled();
    expect(input).toHaveValue("tower");
    expect(input).toHaveAccessibleDescription("Already added");
    expect(status(container)).toHaveTextContent("Already added");
    // Changing the draft takes the note away.
    await user.type(input, "s");
    expect(input).not.toHaveAccessibleDescription("Already added");
    expect(container.querySelector('[data-slot="tag-field-note"]')).toBeNull();
  });

  it("announces each tag it adds, the same words twice included", async () => {
    const user = userEvent.setup();
    const { container } = renderWithStrings(<TagField label="Keywords" />);
    const region = status(container);
    expect(region).toHaveAttribute("role", "status");
    expect(region).toHaveTextContent("");
    await user.type(screen.getByRole("textbox"), "Tower{Enter}");
    expect(region).toHaveTextContent("Added Tower");
    const first = region.firstElementChild;
    await user.click(screen.getByRole("button", { name: "Remove Tower" }));
    await user.type(screen.getByRole("textbox"), "Tower{Enter}");
    expect(region).toHaveTextContent("Added Tower");
    // A new node, so a screen reader hears the same words again.
    expect(region.firstElementChild).not.toBe(first);
  });

  it("shows each tag of a controlled value once, and removes the one tag", async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <TagField label="Keywords" value={["Tower", "Ground", "Tower"]} onChange={onChange} />,
    );
    expect(tagNames()).toEqual(["Tower", "Ground"]);
    await user.click(screen.getByRole("button", { name: "Remove Tower" }));
    expect(onChange).toHaveBeenLastCalledWith(["Ground"]);
    await user.type(screen.getByRole("textbox"), "Approach{Enter}");
    expect(onChange).toHaveBeenLastCalledWith(["Tower", "Ground", "Approach"]);
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

  it("shows the caller's error after the tags, describing the input and the tag list", () => {
    const { container } = renderWithStrings(
      <TagField
        label="Keywords"
        defaultValue={["Tower", "Ground", "Approach"]}
        isInvalid
        errorMessage="Use at most 2 keywords."
      />,
    );
    const input = screen.getByRole("textbox");
    expect(input).toHaveAttribute("aria-invalid", "true");
    expect(input).toHaveAccessibleDescription("Use at most 2 keywords.");
    expect(screen.getByRole("grid")).toHaveAccessibleDescription("Use at most 2 keywords.");
    const slots = [...container.querySelectorAll("[data-slot]")].map((el) =>
      el.getAttribute("data-slot"),
    );
    expect(slots.indexOf("tag-field-error")).toBeGreaterThan(slots.indexOf("tag-field-list"));
  });

  it("shows the catalog's message when invalid with none of its own", () => {
    renderWithStrings(<TagField label="Keywords" isInvalid />);
    expect(screen.getByRole("textbox")).toHaveAccessibleDescription(
      "Check what you entered in this field.",
    );
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
