// SPDX-License-Identifier: AGPL-3.0-or-later
import { createStrings } from "@stepclimb/ui";
import { describe, expect, it } from "vitest";
import { ApiError, UnreachableError } from "../api";
import { webMessages } from "../messages";
import { saveProblem, uploadMessage } from "./problems";

const { t } = createStrings({ catalogs: [webMessages], isDevelopment: true });

function refused(status: number, code: string, extra: Record<string, unknown> = {}) {
  return new ApiError(status, {
    type: "about:blank",
    title: "x",
    status,
    detail: "x",
    code,
    ...extra,
  });
}

describe("saveProblem", () => {
  it("names the block of an image the server refused", () => {
    const error = refused(422, "request.invalid", {
      errors: [{ code: "page.image_type", field: "/body/content/4/attrs/course_file_id" }],
    });
    expect(saveProblem(error, t)).toEqual({
      message: t("pageEdit.problem.imageType"),
      code: "page.image_type",
      block: 4,
    });
  });

  it("names the block of a node the schema refused, and the whole body when it is too large", () => {
    const node = refused(422, "request.invalid", {
      errors: [
        { code: "page.body_invalid", field: "/body/content/2/content/0/marks/0/attrs/href" },
      ],
    });
    expect(saveProblem(node, t)).toMatchObject({ code: "page.body_invalid", block: 2 });
    const whole = refused(422, "request.invalid", {
      errors: [{ code: "page.body_invalid", field: "/body" }],
    });
    expect(saveProblem(whole, t)).toEqual({
      message: t("pageEdit.problem.tooLarge"),
      code: "page.body_invalid",
    });
  });

  it("explains the other refusals, and shows the code of one it has no words for", () => {
    expect(saveProblem(refused(409, "course.archived"), t).message).toBe(
      t("pageEdit.problem.archived"),
    );
    expect(saveProblem(refused(404, "file.not_found"), t).message).toBe(
      t("pageEdit.problem.fileNotFound"),
    );
    expect(saveProblem(new UnreachableError(new TypeError("x")), t).message).toBe(
      t("pageEdit.problem.unreachable"),
    );
    expect(saveProblem(refused(500, "internal"), t)).toEqual({
      message: t("pageEdit.problem.failed"),
      code: "internal",
    });
  });
});

describe("uploadMessage", () => {
  it("explains each upload refusal", () => {
    expect(uploadMessage(refused(413, "file.too_large"), t)).toBe(t("pageEdit.upload.tooLarge"));
    expect(uploadMessage(refused(403, "entitlement.limit_reached"), t)).toBe(
      t("pageEdit.upload.storageFull"),
    );
    expect(uploadMessage(refused(503, "storage.not_configured"), t)).toBe(
      t("pageEdit.upload.noStorage"),
    );
    expect(uploadMessage(new Error("network"), t)).toBe(t("pageEdit.upload.unreachable"));
  });
});
