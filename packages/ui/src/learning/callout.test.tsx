// SPDX-License-Identifier: AGPL-3.0-or-later
import { screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { renderWithStrings, unmarkedText } from "../test/strings";
import { Callout, calloutKinds } from "./callout";

describe("Callout", () => {
  const names = { info: "Note", tip: "Tip", warning: "Warning", danger: "Danger" } as const;
  const tints = {
    info: "bg-info-soft",
    tip: "bg-success-soft",
    warning: "bg-warning-soft",
    danger: "bg-danger-soft",
  } as const;

  it.each(calloutKinds)("draws %s as a note on its tint, its icon named by the kind", (kind) => {
    renderWithStrings(<Callout kind={kind}>Read back the whole clearance.</Callout>);
    const note = screen.getByRole("note");
    expect(note).toHaveAttribute("data-slot", "callout");
    expect(note).toHaveAttribute("data-kind", kind);
    expect(note).toHaveClass(tints[kind]);
    expect(screen.getByRole("img", { name: names[kind] })).toHaveAttribute("data-slot", "icon");
    expect(note).toHaveTextContent("Read back the whole clearance.");
  });

  it("is never a colored side stripe: no inline-start border, and the edge is transparent", () => {
    for (const kind of calloutKinds) {
      const { unmount } = renderWithStrings(<Callout kind={kind}>Text</Callout>);
      const classes = [...screen.getByRole("note").classList];
      expect(classes.filter((c) => /^border-(s|l|x)\b|^border-(s|l)-/.test(c))).toEqual([]);
      expect(classes).toContain("border-transparent");
      unmount();
    }
  });

  it("tells the kinds apart without color: each has its own icon drawing", () => {
    const drawings = calloutKinds.map((kind) => {
      const { container, unmount } = renderWithStrings(<Callout kind={kind}>Text</Callout>);
      const drawing = container.querySelector('[data-slot="icon"]')?.innerHTML ?? "";
      unmount();
      return drawing;
    });
    expect(new Set(drawings).size).toBe(calloutKinds.length);
  });

  it("shows a title only when given one", () => {
    const { rerender } = renderWithStrings(<Callout kind="tip">Body</Callout>);
    expect(document.querySelector('[data-slot="callout-title"]')).toBeNull();
    rerender(
      <Callout kind="tip" title="Before you start">
        Body
      </Callout>,
    );
    expect(screen.getByText("Before you start")).toHaveAttribute("data-slot", "callout-title");
  });

  it("renders only catalog messages and the caller's content", () => {
    const { container } = renderWithStrings(
      <Callout kind="danger" title="«caller»">
        «caller»
      </Callout>,
      { marked: true },
    );
    expect(unmarkedText(container)).toEqual([]);
  });
});
