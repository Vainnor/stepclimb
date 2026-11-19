// SPDX-License-Identifier: AGPL-3.0-or-later
// What the course authoring screen says for a refused write (docs/specs/m1/03-modules-and-items.md
// API and its I1 decisions): a 422 lands at a field only when the form draws that field, and a
// message is chosen by the member and the code together.
import { createStrings } from "@stepclimb/ui";
import { describe, expect, it } from "vitest";
import { ApiError } from "../api";
import { catalogs } from "../messages";
import { authoringProblem } from "./problems";

const { t } = createStrings({ catalogs, isDevelopment: true });

function invalid(field: string, code: string) {
  return new ApiError(422, {
    type: "about:blank",
    title: "Unprocessable Content",
    status: 422,
    detail: "x",
    code: "request.invalid",
    errors: [{ field, code }],
  });
}

describe("authoringProblem", () => {
  it("puts a refusal at a field the form draws", () => {
    expect(authoringProblem(invalid("/title", "name.empty"), t, ["title"])).toEqual({
      message: "Enter a title.",
      field: "title",
    });
  });

  it("shows a refusal of a member the form does not draw above the form, with its code", () => {
    const problem = authoringProblem(invalid("/kind", "item.kind_unavailable"), t, ["title"]);
    expect(problem).toEqual({
      message: "The change was not saved.",
      code: "item.kind_unavailable",
    });
  });

  it("treats a known rule on an undrawn member as the form's problem", () => {
    // The add-item form for a header draws no address field.
    const problem = authoringProblem(invalid("/url", "item.url_invalid"), t, ["title"]);
    expect(problem.field).toBeUndefined();
    expect(problem.message).toMatch(/^Enter a full address/);
  });

  it("treats a nested pointer as the form's problem", () => {
    const problem = authoringProblem(invalid("/body/content/0", "page.body_invalid"), t, [
      "title",
      "slug",
    ]);
    expect(problem.field).toBeUndefined();
  });

  it("says a control character in the address about the address, not the title", () => {
    expect(authoringProblem(invalid("/url", "text.contains_nul"), t, ["title", "url"])).toEqual({
      message: "This address holds a character that cannot be used.",
      field: "url",
    });
    expect(authoringProblem(invalid("/title", "text.contains_nul"), t, ["title", "url"])).toEqual({
      message: "This title holds a character that cannot be used.",
      field: "title",
    });
  });
});
