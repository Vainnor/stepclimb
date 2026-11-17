// SPDX-License-Identifier: AGPL-3.0-or-later
// What the files area says for each refusal of the file routes (docs/specs/m1/06-files.md API).
import { createStrings } from "@stepclimb/ui";
import { describe, expect, it } from "vitest";
import { ApiError, UnreachableError } from "../api";
import { catalogs } from "../messages";
import { filesProblem, isGone } from "./problems";

const { t } = createStrings({ catalogs, isDevelopment: true });

function refused(status: number, code: string, field?: string) {
  const errors = field === undefined ? undefined : [{ field: "/folder", code: field }];
  return new ApiError(status, {
    type: "about:blank",
    title: "x",
    status,
    detail: "x",
    code,
    errors,
  });
}

describe("filesProblem", () => {
  it("has its own words for every code the file routes return", () => {
    const codes: [number, string][] = [
      [503, "storage.not_configured"],
      [409, "file.name_taken"],
      [413, "file.too_large"],
      [503, "file.uploads_busy"],
      [408, "file.body_timeout"],
      [409, "file.in_use"],
      [404, "file.not_found"],
      [404, "folder.not_found"],
      [409, "course.archived"],
      [404, "course.not_found"],
      [403, "permission.denied"],
      [403, "entitlement.limit_reached"],
    ];
    for (const [status, code] of codes) {
      const problem = filesProblem(refused(status, code), "a.pdf", t);
      expect(problem.code, code).toBeUndefined();
      expect(problem.message, code).not.toMatch(/^files\./);
    }
  });

  it("names the file a refusal is about", () => {
    expect(filesProblem(refused(409, "file.name_taken"), "a.pdf", t).message).toContain("a.pdf");
  });

  it("explains each folder and name rule of a 422", () => {
    for (const rule of [
      "file.name_invalid",
      "file.name_too_long",
      "file.folder_invalid",
      "file.folder_too_long",
      "file.folder_too_deep",
      "file.folder_root",
    ]) {
      expect(
        filesProblem(refused(422, "request.invalid", rule), "a", t).code,
        rule,
      ).toBeUndefined();
    }
    expect(filesProblem(refused(422, "request.invalid", "other"), "a", t).code).toBe(
      "request.invalid",
    );
  });

  it("gives the code of an answer it has no words for, and the status of one without a body", () => {
    expect(filesProblem(refused(500, "internal"), "a", t).code).toBe("internal");
    expect(filesProblem(new ApiError(502, undefined), "a", t).code).toBe("HTTP 502");
  });

  it("says the server could not be reached for no answer", () => {
    const problem = filesProblem(new UnreachableError(new Error("x")), "a", t);
    expect(problem.message).toMatch(/could not reach the server/);
  });

  it("sees a file or folder that is gone", () => {
    expect(isGone(refused(404, "file.not_found"))).toBe(true);
    expect(isGone(refused(404, "folder.not_found"))).toBe(true);
    expect(isGone(refused(404, "course.not_found"))).toBe(false);
  });
});
