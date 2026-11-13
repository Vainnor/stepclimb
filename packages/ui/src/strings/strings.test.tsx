// SPDX-License-Identifier: AGPL-3.0-or-later
import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { Catalog } from "./catalog";
import { createStrings, StringsProvider, useStepclimbStrings } from "./strings";
import type { TermOverrides } from "./terms";

const app: Catalog = {
  en: {
    "fixture.signedIn": "Signed in as {name}.",
    "fixture.assigned": "{count, plural, one {# {course}} other {# {courses}}} assigned to you.",
    "fixture.heading": "{Courses}",
    "fixture.empty": "No {courses} yet. {Course} progress appears here.",
    "fixture.open": "Open this {course}",
    "fixture.score": "Score: {score, number, percent}",
    "fixture.due": "Due {when, date, medium}",
  },
  fr: { "fixture.signedIn": "Connecté en tant que {name}." },
};

// The organization override the tests use while Q7's delivery (settings, /me) is not built.
const trainingModule: TermOverrides = {
  course: { one: "training module", other: "training modules" },
};

const english = (terms?: TermOverrides) =>
  createStrings({ catalogs: [app], terms, isDevelopment: true });

describe("createStrings", () => {
  it("formats arguments, plurals, numbers and dates with ICU syntax", () => {
    const { t } = english();
    expect(t("fixture.signedIn", { name: "Ada" })).toBe("Signed in as Ada.");
    expect(t("fixture.assigned", { count: 1 })).toBe("1 course assigned to you.");
    expect(t("fixture.assigned", { count: 3 })).toBe("3 courses assigned to you.");
    expect(t("fixture.score", { score: 0.85 })).toBe("Score: 85%");
    expect(t("fixture.due", { when: new Date("2026-11-12T12:00:00Z") })).toBe("Due Nov 12, 2026");
  });

  it("renders the default term forms without an override", () => {
    const { t } = english();
    expect(t("fixture.heading")).toBe("Courses");
    expect(t("fixture.empty")).toBe("No courses yet. Course progress appears here.");
  });

  it("renders an override in singular, plural and at the start of a sentence", () => {
    const { t } = english(trainingModule);
    expect(t("fixture.assigned", { count: 1 })).toBe("1 training module assigned to you.");
    expect(t("fixture.assigned", { count: 2 })).toBe("2 training modules assigned to you.");
    expect(t("fixture.heading")).toBe("Training modules");
    expect(t("fixture.empty")).toBe(
      "No training modules yet. Training module progress appears here.",
    );
    expect(t("fixture.open")).toBe("Open this training module");
  });

  it("changes every message that uses the term when it is overridden", () => {
    const { t } = english(trainingModule);
    const usesCourse = Object.entries(app.en).filter(([, m]) => /\{\s*courses?\s*\}/i.test(m));
    expect(usesCourse.length).toBeGreaterThan(3);
    for (const [id] of usesCourse) {
      const text = t(id, { count: 2 });
      expect(text).toMatch(/training module/i);
      expect(text).not.toMatch(/\bcourses?\b/i);
    }
  });

  it("refuses a value that would replace a term argument", () => {
    const { t } = english();
    expect(() => t("fixture.open", { course: "class" })).toThrow(/term argument/);
    expect(() => t("fixture.open", { Courses: "Classes" })).toThrow(/term argument/);
  });

  it("refuses an id defined in two catalogs", () => {
    const ui: Catalog = { en: { "fixture.open": "Open" } };
    expect(() => createStrings({ catalogs: [ui, app], isDevelopment: true })).toThrow(
      /"fixture.open" is defined in two catalogs/,
    );
  });

  it("uses the locale's message when the locale has it", () => {
    const { t } = createStrings({ catalogs: [app], locale: "fr", isDevelopment: false });
    expect(t("fixture.signedIn", { name: "Ada" })).toBe("Connecté en tant que Ada.");
  });

  it("renders the id in development and English in production for an id the locale lacks", () => {
    const dev = createStrings({ catalogs: [app], locale: "fr", isDevelopment: true });
    const prod = createStrings({ catalogs: [app], locale: "fr", isDevelopment: false });
    expect(dev.t("fixture.open")).toBe("fixture.open");
    expect(prod.t("fixture.open")).toBe("Open this course");
  });

  it("renders an id no catalog has as the id, never an empty string", () => {
    for (const isDevelopment of [true, false]) {
      const { t } = createStrings({ catalogs: [app], isDevelopment });
      expect(t("fixture.absent")).toBe("fixture.absent");
    }
  });
});

function SignedIn() {
  const { t } = useStepclimbStrings();
  return <p>{t("fixture.signedIn", { name: "Ada" })}</p>;
}

describe("useStepclimbStrings", () => {
  it("reads the provider's catalogs", () => {
    render(
      <StringsProvider catalogs={[app]} isDevelopment>
        <SignedIn />
      </StringsProvider>,
    );
    expect(screen.getByText("Signed in as Ada.")).toBeInTheDocument();
  });

  it("throws without a provider", () => {
    const error = vi.spyOn(console, "error").mockImplementation(() => {});
    expect(() => render(<SignedIn />)).toThrow(/needs a <StringsProvider>/);
    error.mockRestore();
  });
});
