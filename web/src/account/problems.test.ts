// SPDX-License-Identifier: AGPL-3.0-or-later
// The account page's messages for each passkey refusal (docs/specs/m0/04-local-accounts.md B13;
// 11-app-shell.md B17), looked up in the real catalog.
import { createStrings } from "@stepclimb/ui";
import { describe, expect, it } from "vitest";
import { ApiError, UnreachableError } from "../api";
import { catalogs } from "../messages";
import { PasskeyError } from "../sign-in/passkey";
import { isFinalAddRefusal, nameError, passkeyProblem } from "./problems";

const { t } = createStrings({ catalogs, isDevelopment: true });

function refused(status: number, code: string, errors?: { field: string; code: string }[]) {
  return new ApiError(status, {
    type: "about:blank",
    title: "Refused",
    status,
    detail: "Refused",
    code,
    ...(errors === undefined ? {} : { errors }),
  });
}

describe("passkeyProblem", () => {
  it.each([
    [409, "local.last_method", "This passkey is your last way to sign in"],
    [403, "local.passkeys_not_allowed", "sign-in settings for this provider do not allow passkeys"],
    [404, "auth.method_disabled", "Your organization has turned off passkeys"],
    [400, "auth.flow_invalid", "already finished or replaced by one in another tab"],
    [400, "auth.flow_expired", "The passkey registration expired"],
    [400, "auth.passkey_invalid", "Stepclimb could not add that passkey"],
    [404, "passkey.not_found", "This passkey no longer exists"],
  ])("explains %i %s in its own words", (status, code, message) => {
    const problem = passkeyProblem(refused(status, code), t);
    expect(problem.message).toContain(message);
    expect(problem.code).toBeUndefined();
  });

  it("gives an unknown code the general message and the code", () => {
    expect(passkeyProblem(refused(403, "auth.csrf_refused"), t)).toEqual({
      message: t("account.problem.unknown"),
      code: "auth.csrf_refused",
    });
    expect(passkeyProblem(new ApiError(500, undefined), t).code).toBe("HTTP 500");
  });

  it("says when the server could not be reached, and when the page itself failed", () => {
    expect(passkeyProblem(new UnreachableError(new TypeError("x")), t).message).toBe(
      t("account.problem.unreachable"),
    );
    expect(passkeyProblem(new Error("bug"), t).message).toBe(t("account.problem.failed"));
  });

  it("explains a ceremony the browser ended", () => {
    expect(passkeyProblem(new PasskeyError("exists"), t).message).toBe(
      t("account.passkeys.exists"),
    );
    expect(passkeyProblem(new PasskeyError("cancelled"), t).message).toBe(
      t("signIn.passkey.cancelled"),
    );
  });
});

describe("nameError", () => {
  it.each([
    ["name.empty", "Enter a name."],
    ["name.too_long", "Use at most 200 characters."],
    ["text.contains_nul", "This name cannot be used. Try another one."],
  ])("puts %s at the name field", (code, message) => {
    expect(nameError(refused(422, "request.invalid", [{ field: "/name", code }]), t)).toBe(message);
  });

  it("leaves other errors to passkeyProblem", () => {
    expect(
      nameError(refused(422, "request.invalid", [{ field: "/credential", code: "x" }]), t),
    ).toBeUndefined();
    expect(nameError(refused(400, "auth.flow_invalid"), t)).toBeUndefined();
  });
});

describe("isFinalAddRefusal", () => {
  it("is true only for refusals that trying again cannot change", () => {
    expect(isFinalAddRefusal(refused(403, "local.passkeys_not_allowed"))).toBe(true);
    expect(isFinalAddRefusal(refused(404, "auth.method_disabled"))).toBe(true);
    expect(isFinalAddRefusal(refused(400, "auth.flow_expired"))).toBe(false);
  });
});
