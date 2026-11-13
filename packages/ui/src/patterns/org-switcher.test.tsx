// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { stubMatchMedia } from "../test/media";
import { renderWithStrings } from "../test/strings";
import { OrgSwitcher } from "./org-switcher";

const organizations = [
  { id: "north", name: "North Training Group" },
  { id: "south", name: "South Flight School" },
];

describe("OrgSwitcher", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("names the current organization and checks it in the menu", async () => {
    stubMatchMedia(() => true);
    const user = userEvent.setup();
    renderWithStrings(
      <OrgSwitcher organizations={organizations} current="north" onChange={vi.fn()} />,
    );
    const trigger = screen.getByRole("button", {
      name: "North Training Group: switch organization",
    });
    expect(trigger).toHaveAttribute("data-slot", "org-switcher");
    expect(trigger).toHaveTextContent("North Training Group");
    await user.click(trigger);
    expect(screen.getByRole("menuitemradio", { name: "North Training Group" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("calls onChange with another organization, and not with the current one", async () => {
    stubMatchMedia(() => true);
    const user = userEvent.setup();
    const onChange = vi.fn();
    renderWithStrings(
      <OrgSwitcher organizations={organizations} current="north" onChange={onChange} />,
    );
    await user.click(screen.getByRole("button"));
    await user.click(screen.getByRole("menuitemradio", { name: "North Training Group" }));
    expect(onChange).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button"));
    await user.click(screen.getByRole("menuitemradio", { name: "South Flight School" }));
    expect(onChange).toHaveBeenCalledWith("south");
  });
});
