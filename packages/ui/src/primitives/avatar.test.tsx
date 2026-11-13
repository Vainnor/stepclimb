// SPDX-License-Identifier: AGPL-3.0-or-later
import { fireEvent, render, screen } from "@testing-library/react";
import { I18nProvider } from "react-aria-components";
import { describe, expect, it } from "vitest";
import { Avatar, initials } from "./avatar";

describe("initials", () => {
  it.each([
    ["Ada Lovelace", "AL"],
    ["  grace   brewster murray hopper ", "GH"],
    ["Cher", "C"],
    ["Émile Zola", "ÉZ"],
    ["", ""],
  ])("%j gives %j", (name, expected) => {
    expect(initials(name)).toBe(expected);
  });

  it("keeps a letter built from several code points whole", () => {
    // "E" followed by a combining acute accent.
    expect(initials("Étienne Ba")).toBe("ÉB");
  });

  it("upper-cases in the locale", () => {
    expect(initials("ilker yılmaz", "tr")).toBe("İY");
  });
});

describe("Avatar", () => {
  it("draws the picture, named by the display name, in a pill with its data-slots", () => {
    render(<Avatar name="Ada Lovelace" src="/avatars/ada.png" />);
    const el = screen.getByRole("img", { name: "Ada Lovelace" });
    expect(el).toHaveAttribute("data-slot", "avatar");
    expect(el).toHaveClass("rounded-pill");
    const image = el.querySelector('[data-slot="avatar-image"]');
    expect(image).toHaveAttribute("src", "/avatars/ada.png");
    expect(image).toHaveAttribute("alt", "");
    expect(el.querySelector('[data-slot="avatar-initials"]')).toBeNull();
  });

  it("shows the initials when there is no picture", () => {
    render(<Avatar name="Ada Lovelace" />);
    const el = screen.getByRole("img", { name: "Ada Lovelace" });
    const text = el.querySelector('[data-slot="avatar-initials"]');
    expect(text).toHaveTextContent("AL");
    expect(text).toHaveAttribute("aria-hidden", "true");
  });

  it("falls back to the initials when the picture fails to load (T12)", () => {
    render(<Avatar name="Ada Lovelace" src="/avatars/missing.png" />);
    const el = screen.getByRole("img", { name: "Ada Lovelace" });
    fireEvent.error(el.querySelector("img") as HTMLImageElement);
    expect(el.querySelector("img")).toBeNull();
    expect(el.querySelector('[data-slot="avatar-initials"]')).toHaveTextContent("AL");
  });

  it("tries a new picture after the old one failed", () => {
    const { rerender } = render(<Avatar name="Ada Lovelace" src="/avatars/missing.png" />);
    fireEvent.error(screen.getByRole("img").querySelector("img") as HTMLImageElement);
    rerender(<Avatar name="Ada Lovelace" src="/avatars/ada.png" />);
    expect(screen.getByRole("img").querySelector("img")).toHaveAttribute("src", "/avatars/ada.png");
  });

  it("upper-cases the initials in the locale of its I18nProvider", () => {
    render(
      <I18nProvider locale="tr-TR">
        <Avatar name="ilker yılmaz" />
      </I18nProvider>,
    );
    expect(screen.getByRole("img", { name: "ilker yılmaz" })).toHaveTextContent("İY");
  });

  it("draws a generic person, hidden from assistive technology, for an empty name", () => {
    const { container } = render(<Avatar name="   " />);
    const el = container.querySelector('[data-slot="avatar"]');
    expect(el).toHaveAttribute("aria-hidden", "true");
    expect(el).not.toHaveAttribute("role");
    expect(el?.querySelector('[data-slot="icon"]')).not.toBeNull();
    expect(el?.querySelector('[data-slot="avatar-initials"]')).toBeNull();
  });

  it("takes a size", () => {
    render(<Avatar name="Ada Lovelace" size="lg" />);
    expect(screen.getByRole("img")).toHaveClass("size-12");
  });
});
