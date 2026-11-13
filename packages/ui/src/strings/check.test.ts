// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import type { Catalog } from "./catalog";
import { findMessageIds, hardCodedNouns, invalidMessages, missingMessageIds } from "./check";

describe("findMessageIds", () => {
  it("finds literal ids passed to t, in either quote style", () => {
    const source = `const a = t("shell.title"); const b = t( 'shell.signOut', { n: 1 });`;
    expect(findMessageIds(source)).toEqual(["shell.title", "shell.signOut"]);
  });

  it("ignores other calls whose name ends in t", () => {
    expect(findMessageIds(`it("works"); test("x"); format("y"); set("z");`)).toEqual([]);
  });
});

describe("missingMessageIds", () => {
  const catalog: Catalog = { en: { "shell.title": "Home" } };

  it("reports an id the catalog lacks, with its file", () => {
    const sources = { "a.tsx": `t("shell.title"); t("shell.absent")` };
    expect(missingMessageIds(sources, [catalog])).toEqual([{ file: "a.tsx", id: "shell.absent" }]);
  });

  it("reports nothing when every id is present", () => {
    expect(missingMessageIds({ "a.tsx": `t("shell.title")` }, [catalog])).toEqual([]);
  });
});

describe("hardCodedNouns", () => {
  it("flags a term's default noun written as text, including inside a plural branch", () => {
    const catalog: Catalog = {
      en: {
        "a.plain": "Open the course",
        "a.plural": "{n, plural, one {# {course}} other {# Courses}}",
        "a.term": "Open the {course}; {Courses} list",
        "a.word": "Discourse is not a noun we rename",
      },
    };
    expect(hardCodedNouns([catalog])).toEqual([
      { id: "a.plain", noun: "course" },
      { id: "a.plural", noun: "courses" },
    ]);
  });
});

describe("invalidMessages", () => {
  it("reports a message that does not parse as ICU, in any locale", () => {
    const catalog: Catalog = {
      en: { "a.ok": "{n, plural, one {# item} other {# items}}", "a.bad": "{n, plural, one {#}" },
      fr: { "a.ok": "{n, plural, one {# élément} other {# éléments}" },
    };
    expect(invalidMessages([catalog]).map(({ locale, id }) => ({ locale, id }))).toEqual([
      { locale: "en", id: "a.bad" },
      { locale: "fr", id: "a.ok" },
    ]);
  });
});
