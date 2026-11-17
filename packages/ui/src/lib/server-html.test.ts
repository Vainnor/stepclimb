// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { type ServerHtml, serverHtml } from "./server-html";

describe("serverHtml", () => {
  it("keeps the string as it is", () => {
    expect(serverHtml("<p>Hi</p>")).toBe("<p>Hi</p>");
  });

  it("is the only way to make a ServerHtml: a plain string does not type-check", () => {
    // tsc (`just typecheck`) fails on this file if a plain string is accepted, since the
    // directive below would then be unused.
    // @ts-expect-error a plain string is not ServerHtml
    const plain: ServerHtml = "<p>Hi</p>";
    const marked: ServerHtml = serverHtml("<p>Hi</p>");
    expect(plain).toBe(marked);
  });
});
