// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import type { Catalog } from "./catalog";
import {
  articlesBeforeTerms,
  dynamicLookups,
  findMessageIds,
  hardCodedNouns,
  invalidMessages,
  missingMessageIds,
} from "./check";

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

describe("dynamicLookups", () => {
  it("reports a variable or template-literal id with its line", () => {
    const source = [
      "const a = t(id);",
      `const b = t(\`shell.\${name}\`);`,
      't("shell.title");',
    ].join("\n");
    expect(dynamicLookups({ "a.tsx": source })).toEqual([
      { file: "a.tsx", line: 1 },
      { file: "a.tsx", line: 2 },
    ]);
  });

  it("reports a destructure that renames t and an optional call of t", () => {
    const source = [
      "const { t: translate } = useStepclimbStrings();",
      "const {",
      "  t: tr,",
      "} = useStepclimbStrings();",
      't?.("shell.title");',
      "const { t } = useStepclimbStrings();",
    ].join("\n");
    expect(dynamicLookups({ "a.tsx": source })).toEqual([
      { file: "a.tsx", line: 1 },
      { file: "a.tsx", line: 2 },
      { file: "a.tsx", line: 5 },
    ]);
  });

  it("accepts a literal id on the next line and an exempted line", () => {
    const source = ["t(", '  "shell.title",', ");", "t(label); // strings-check-ignore: data"];
    expect(dynamicLookups({ "a.tsx": source.join("\n") })).toEqual([]);
  });
});

describe("hardCodedNouns", () => {
  it("flags a noun in literal text, including plural option values", () => {
    const catalog: Catalog = {
      en: {
        "a.plain": "Open the course",
        "a.options": "{n, plural, one {course} other {courses}}",
        "a.term": "Open the {course}; {Courses} list",
        "a.word": "Discourse is not a noun we rename",
      },
    };
    expect(hardCodedNouns([catalog])).toEqual([
      { id: "a.plain", noun: "course" },
      { id: "a.options", noun: "course" },
      { id: "a.options", noun: "courses" },
    ]);
  });

  it("does not treat a select key as text", () => {
    const catalog: Catalog = {
      en: { "a.select": "{kind, select, course {Open the {course}} other {Open}}" },
    };
    expect(hardCodedNouns([catalog])).toEqual([]);
  });
});

describe("articlesBeforeTerms", () => {
  it("flags a or an directly before a term argument, at any depth", () => {
    const catalog: Catalog = {
      en: {
        "a.a": "Open a {course}",
        "a.an": "An {Course} opened",
        "a.plural": "{n, plural, one {a {course}} other {# {courses}}}",
        "a.the": "Open the {course}",
        "a.word": "Data {course} and a {count}",
      },
    };
    expect(articlesBeforeTerms([catalog])).toEqual([
      { id: "a.a", term: "course" },
      { id: "a.an", term: "Course" },
      { id: "a.plural", term: "course" },
    ]);
  });

  it("looks into a tag, plural or select that follows the article", () => {
    const catalog: Catalog = {
      en: {
        "b.tag": "Open a <b>{course}</b>",
        "b.plural": "Open a {n, plural, one {{course}} other {{courses}}}",
        "b.select": "Open a {kind, select, new {{course}} other {draft}}",
        "b.later": "Open a <b>new {course}</b>",
      },
    };
    expect(articlesBeforeTerms([catalog])).toEqual([
      { id: "b.tag", term: "course" },
      { id: "b.plural", term: "course" },
      { id: "b.plural", term: "courses" },
      { id: "b.select", term: "course" },
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
