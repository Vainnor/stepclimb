// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { RouterProvider } from "react-aria-components";
import { describe, expect, it, vi } from "vitest";
import { ButtonLink, buttonLink } from "./button-link";

// An app types its router's options; this test's stand in for the web app's.
declare module "react-aria-components" {
  interface RouterConfig {
    routerOptions: { reloadDocument?: boolean };
  }
}

describe("ButtonLink", () => {
  it("renders an anchor with its href and data-slot", () => {
    render(<ButtonLink href="/auth/example/start">Sign in with Example</ButtonLink>);
    const el = screen.getByRole("link", { name: "Sign in with Example" });
    expect(el.tagName).toBe("A");
    expect(el).toHaveAttribute("href", "/auth/example/start");
    expect(el).toHaveAttribute("data-slot", "button-link");
  });

  it("draws Button's intents, secondary by default", () => {
    render(
      <>
        <ButtonLink href="/a">Secondary</ButtonLink>
        <ButtonLink href="/b" intent="primary">
          Primary
        </ButtonLink>
      </>,
    );
    expect(screen.getByRole("link", { name: "Secondary" })).toHaveClass(
      "bg-raised",
      "border-line-strong",
    );
    expect(screen.getByRole("link", { name: "Primary" })).toHaveClass("bg-brand", "text-on-brand");
  });

  it("lets a long label wrap and grow the control instead of a fixed height", () => {
    const classes = buttonLink({ size: "md" }).split(" ");
    expect(classes).toContain("whitespace-normal");
    expect(classes).toContain("select-none");
    expect(classes).toContain("min-h-(--sc-control-h)");
    expect(classes).not.toContain("whitespace-nowrap");
    expect(classes).not.toContain("h-(--sc-control-h)");
  });

  it("navigates through the RouterProvider with its routerOptions", async () => {
    const user = userEvent.setup();
    const navigate = vi.fn();
    render(
      <RouterProvider navigate={navigate}>
        <ButtonLink href="/auth/example/start" routerOptions={{ reloadDocument: true }}>
          Sign in with Example
        </ButtonLink>
      </RouterProvider>,
    );
    const el = screen.getByRole("link", { name: "Sign in with Example" });
    await user.click(el);
    el.focus();
    await user.keyboard("{Enter}");
    expect(navigate).toHaveBeenCalledTimes(2);
    expect(navigate).toHaveBeenCalledWith("/auth/example/start", { reloadDocument: true });
  });

  it("does not navigate when disabled", async () => {
    const user = userEvent.setup();
    const navigate = vi.fn();
    render(
      <RouterProvider navigate={navigate}>
        <ButtonLink href="/x" isDisabled>
          Unavailable
        </ButtonLink>
      </RouterProvider>,
    );
    const el = screen.getByRole("link", { name: "Unavailable" });
    await user.click(el);
    expect(navigate).not.toHaveBeenCalled();
    expect(el).toHaveAttribute("data-disabled");
  });

  it("marks keyboard focus with data-focus-visible", async () => {
    const user = userEvent.setup();
    render(<ButtonLink href="/x">Go</ButtonLink>);
    await user.tab();
    expect(screen.getByRole("link", { name: "Go" })).toHaveAttribute("data-focus-visible");
  });

  it("lets className add layout without dropping the recipe", () => {
    render(
      <ButtonLink href="/x" className="w-full">
        Go
      </ButtonLink>,
    );
    expect(screen.getByRole("link", { name: "Go" })).toHaveClass("w-full", "bg-raised");
  });
});
