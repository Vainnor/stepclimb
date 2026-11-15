// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { applyAppearance, shownTheme } from "./appearance";

describe("applyAppearance (B21)", () => {
  it("sets a chosen theme and compact density on the element", () => {
    const root = document.createElement("html");
    applyAppearance("high-contrast", "compact", root);
    expect(root.getAttribute("data-sc-theme")).toBe("high-contrast");
    expect(root.getAttribute("data-sc-density")).toBe("compact");
  });

  it("removes both attributes for system and comfortable", () => {
    const root = document.createElement("html");
    root.setAttribute("data-sc-theme", "dark");
    root.setAttribute("data-sc-density", "compact");
    applyAppearance("system", "comfortable", root);
    expect(root.hasAttribute("data-sc-theme")).toBe(false);
    expect(root.hasAttribute("data-sc-density")).toBe(false);
  });
});

describe("shownTheme (B21)", () => {
  it("takes the organization's default until the user chooses", () => {
    expect(shownTheme({ theme: null, density: "compact" }, "dark")).toBe("dark");
    expect(shownTheme({ theme: "system", density: "compact" }, "dark")).toBe("system");
    expect(shownTheme({ theme: "light", density: "compact" }, "high-contrast")).toBe("light");
  });
});
