// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { Tab, TabList, TabPanel, Tabs } from "./tabs";

function Example(props: { selectedKey?: string; onSelectionChange?: (key: unknown) => void }) {
  return (
    <Tabs {...props}>
      <TabList aria-label="Course">
        <Tab id="modules">Modules</Tab>
        <Tab id="people">People</Tab>
        <Tab id="grades">Grades</Tab>
      </TabList>
      <TabPanel id="modules">Module list</TabPanel>
      <TabPanel id="people">People list</TabPanel>
      <TabPanel id="grades">Grade list</TabPanel>
    </Tabs>
  );
}

describe("Tabs", () => {
  it("has its data-slots and shows the first panel", () => {
    render(<Example />);
    expect(screen.getByRole("tablist", { name: "Course" })).toHaveAttribute(
      "data-slot",
      "tab-list",
    );
    const tab = screen.getByRole("tab", { name: "Modules" });
    expect(tab).toHaveAttribute("data-slot", "tab");
    expect(tab).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Module list");
    expect(screen.getByRole("tabpanel")).toHaveAttribute("data-slot", "tab-panel");
    expect(tab.closest('[data-slot="tabs"]')).not.toBeNull();
  });

  it("moves between tabs with the arrow keys, uncontrolled (T12)", async () => {
    const user = userEvent.setup();
    render(<Example />);
    await user.tab();
    expect(screen.getByRole("tab", { name: "Modules" })).toHaveFocus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "People" })).toHaveFocus();
    expect(screen.getByRole("tabpanel")).toHaveTextContent("People list");
    await user.keyboard("{ArrowLeft}{ArrowLeft}");
    expect(screen.getByRole("tab", { name: "Grades" })).toHaveFocus();
    await user.keyboard("{Home}");
    expect(screen.getByRole("tab", { name: "Modules" })).toHaveFocus();
  });

  it("works controlled", async () => {
    const user = userEvent.setup();
    const onSelectionChange = vi.fn();
    const { rerender } = render(
      <Example selectedKey="modules" onSelectionChange={onSelectionChange} />,
    );
    await user.click(screen.getByRole("tab", { name: "Grades" }));
    expect(onSelectionChange).toHaveBeenCalledWith("grades");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Module list");
    rerender(<Example selectedKey="grades" onSelectionChange={onSelectionChange} />);
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Grade list");
  });
});
