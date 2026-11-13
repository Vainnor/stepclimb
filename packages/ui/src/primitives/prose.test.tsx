// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { markScrollers, Prose } from "./prose";

// jsdom has no layout; widths are set on the elements by hand.
function setWidths(el: Element, scroll: number, client: number) {
  Object.defineProperty(el, "scrollWidth", { configurable: true, value: scroll });
  Object.defineProperty(el, "clientWidth", { configurable: true, value: client });
}

describe("Prose", () => {
  it("wraps the content with its data-slot in the reading face, size, leading and measure", () => {
    render(
      <Prose>
        <h2>Readbacks</h2>
        <p>Read back the clearance.</p>
      </Prose>,
    );
    const root = screen.getByRole("heading", { name: "Readbacks" }).parentElement;
    expect(root).toHaveAttribute("data-slot", "prose");
    expect(root).toHaveClass("font-read", "text-read", "leading-read", "max-w-[66ch]");
    expect(screen.getByText("Read back the clearance.").tagName).toBe("P");
  });

  it("lets className add layout without dropping the recipe", () => {
    const { container } = render(
      <Prose className="mx-auto">
        <p>Text</p>
      </Prose>,
    );
    expect(container.firstElementChild).toHaveClass("mx-auto", "font-read");
  });

  it("makes a table or code block wider than the column a tab stop, and only while it is", () => {
    const { container } = render(
      <Prose>
        <table>
          <tbody>
            <tr>
              <td>wide</td>
            </tr>
          </tbody>
        </table>
        <pre>narrow</pre>
      </Prose>,
    );
    const root = container.firstElementChild as HTMLElement;
    const table = root.querySelector("table") as HTMLElement;
    const pre = root.querySelector("pre") as HTMLElement;
    setWidths(table, 900, 375);
    setWidths(pre, 300, 375);

    markScrollers(root);
    expect(table).toHaveAttribute("tabindex", "0");
    expect(pre).not.toHaveAttribute("tabindex");

    setWidths(table, 375, 375);
    markScrollers(root);
    expect(table).not.toHaveAttribute("tabindex");
  });
});
