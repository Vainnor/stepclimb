// SPDX-License-Identifier: AGPL-3.0-or-later
// Which message a failure on the sign-in screens gets when it is not a problem code
// (docs/specs/m0/11-app-shell.md B17, B29). The message ids stand in for the text.
import type { Strings } from "@stepclimb/ui";
import { describe, expect, it } from "vitest";
import { ApiError, UnreachableError } from "../api";
import { signInProblem } from "./problems";

const t = ((id: string) => id) as Strings["t"];

describe("signInProblem", () => {
  it("says the server could not be reached when nothing answered", () => {
    expect(signInProblem(new UnreachableError(new TypeError("x")), "signIn", t)).toEqual({
      message: "signIn.problem.unreachable",
    });
  });

  it("gives a fault in the page the general message, not an unreachable server", () => {
    expect(signInProblem(new Error("a fault"), "signIn", t)).toEqual({
      message: "signIn.problem.failed",
    });
  });

  it("shows an answer without a known code with its status", () => {
    expect(signInProblem(new ApiError(500, undefined), "enroll", t)).toEqual({
      message: "signIn.problem.unknown",
      code: "HTTP 500",
    });
  });
});
