// SPDX-License-Identifier: AGPL-3.0-or-later
import { act, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { renderWithStrings as render } from "../test/strings";
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
    expect(root).toHaveClass("font-read", "text-read", "leading-read", "max-w-[31.5em]");
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

    markScrollers(root, "Code block");
    expect(table).toHaveAttribute("tabindex", "0");
    expect(table).not.toHaveAttribute("role");
    expect(pre).not.toHaveAttribute("tabindex");

    setWidths(table, 375, 375);
    markScrollers(root, "Code block");
    expect(table).not.toHaveAttribute("tabindex");
  });

  it("names a scrolling code block as a group, and takes only what it added back", () => {
    const { container } = render(
      <Prose>
        <pre>wide</pre>
        <pre tabIndex={-1}>fits</pre>
      </Prose>,
    );
    const root = container.firstElementChild as HTMLElement;
    const [wide, fits] = [...root.querySelectorAll("pre")] as HTMLElement[];
    if (!wide || !fits) throw new Error("no pre");
    setWidths(wide, 900, 375);
    setWidths(fits, 300, 375);
    markScrollers(root, "Code block");
    expect(screen.getByRole("group", { name: "Code block" })).toBe(wide);
    expect(wide).toHaveAttribute("tabindex", "0");
    // An attribute the content set itself stays.
    expect(fits).toHaveAttribute("tabindex", "-1");

    setWidths(wide, 375, 375);
    markScrollers(root, "Code block");
    for (const attr of ["tabindex", "role", "aria-label"]) expect(wide).not.toHaveAttribute(attr);
  });

  describe("after a web font loads", () => {
    const original = Object.getOwnPropertyDescriptor(document, "fonts");
    afterEach(() => {
      if (original) Object.defineProperty(document, "fonts", original);
      else Reflect.deleteProperty(document, "fonts");
    });

    // jsdom has no FontFaceSet; an EventTarget with a `ready` that resolves when asked stands in.
    function fakeFonts() {
      let resolve = () => {};
      const fonts = Object.assign(new EventTarget(), {
        ready: new Promise<void>((r) => {
          resolve = r;
        }),
      });
      Object.defineProperty(document, "fonts", { configurable: true, value: fonts });
      return { fonts, resolve };
    }

    function widePreAfterRender() {
      const { container } = render(
        <Prose>
          <pre>N123AB, runway 27, taxi via A, B3, hold short of runway 18.</pre>
        </Prose>,
      );
      const pre = container.querySelector("pre") as HTMLElement;
      expect(pre).not.toHaveAttribute("tabindex");
      // The font swap widens the text; nothing Prose's ResizeObserver watches changes size.
      setWidths(pre, 626, 565);
      return pre;
    }

    it("re-checks the code blocks when the fonts finish loading", async () => {
      const { fonts } = fakeFonts();
      const pre = widePreAfterRender();
      act(() => {
        fonts.dispatchEvent(new Event("loadingdone"));
      });
      expect(pre).toHaveAttribute("tabindex", "0");
      expect(pre).toHaveAccessibleName("Code block");
    });

    it("re-checks the code blocks when fonts that were already loading are ready", async () => {
      const { resolve } = fakeFonts();
      const pre = widePreAfterRender();
      await act(async () => {
        resolve();
      });
      expect(pre).toHaveAttribute("tabindex", "0");
    });
  });
});
