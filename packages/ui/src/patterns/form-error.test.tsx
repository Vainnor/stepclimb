// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { FormError } from "./form-error";

describe("FormError", () => {
  it("is an alert with its message and an icon", () => {
    render(<FormError>Your account is suspended.</FormError>);
    const alert = screen.getByRole("alert");
    expect(alert).toHaveAttribute("data-slot", "form-error");
    expect(alert).toHaveTextContent("Your account is suspended.");
    expect(alert.querySelector('[data-slot="form-error-icon"] svg')).not.toBeNull();
    expect(alert.querySelector("code")).toBeNull();
  });

  it("shows the code in Code when given one", () => {
    render(<FormError code="auth.teapot">Something went wrong.</FormError>);
    const code = screen.getByText("auth.teapot");
    expect(code.tagName).toBe("CODE");
    expect(code).toHaveAttribute("data-slot", "form-error-code");
  });

  it("uses body-size text", () => {
    render(<FormError>Try again.</FormError>);
    expect(screen.getByRole("alert")).toHaveClass("text-base");
  });
});
