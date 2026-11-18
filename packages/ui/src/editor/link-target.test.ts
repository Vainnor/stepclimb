// SPDX-License-Identifier: AGPL-3.0-or-later
// The browser's link check against the cases the server's runs (spec 07 B9, T5):
// crates/stepclimb-core/tests/page_doc_editor.rs reads the same file.
import cases from "../../../../crates/stepclimb-core/tests/fixtures/page_doc/link-targets.json";
import { linkTarget } from "./link-target";

describe("linkTarget", () => {
  it.each(cases.refused)("refuses %j", (href) => {
    expect(linkTarget(href)).toBeNull();
  });

  it.each(cases.stored as [string, string][])("stores %j as %j", (given, stored) => {
    expect(linkTarget(given)).toBe(stored);
  });

  it("refuses a target over 2,048 characters", () => {
    expect(linkTarget(`https://example.com/${"a".repeat(2048)}`)).toBeNull();
    expect(linkTarget(`/${"a".repeat(2048)}`)).toBeNull();
  });
});
