// SPDX-License-Identifier: AGPL-3.0-or-later
// The branding screen in jsdom (docs/specs/m1/16-learning-ui-components.md B14, T6; m0/12 B21,
// B27) against a stubbed server: the saved inputs, the preview and report as the fields change, a
// save and what it changes on the page, a draft whose report fails and is never sent, and each
// refusal the server answers, at the field it names.
import { generate } from "@stepclimb/tokens/generate";
import { contrastReport } from "@stepclimb/tokens/report";
import { cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Branding, SavedInputs } from "../branding/branding";
import {
  type Handler,
  json,
  meFixture,
  problem,
  renderApp,
  requestsTo,
  stubMatchMedia,
  stubServer,
} from "../test/app";
import { choiceKey } from "./branding";

/** While on, a draft with round corners gets `failingReports()`: a brand the report refuses. */
const refuseRound = vi.hoisted(() => ({ on: false }));

vi.mock("../branding/draft", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../branding/draft")>();
  return {
    ...actual,
    brandDraft: (inputs: SavedInputs) => {
      const draft = actual.brandDraft(inputs);
      if (!refuseRound.on || !draft.ok || inputs.corner_style !== "round") return draft;
      return { ...draft, reports: failingReports(), pass: false };
    },
  };
});

const PATH = "/admin/branding";
const API = "/api/v1/admin/branding";
const appearances = ["light", "dark", "high-contrast"] as const;

const savedInputs: SavedInputs = {
  primary: "oklch(0.5 0.09 195)",
  ui_font: "Atkinson Hyperlegible Next",
  reading_font: "serif",
  corner_style: "default",
  default_appearance: "system",
};

function brandingOf(inputs: SavedInputs, cssHash = "a".repeat(64)): Branding {
  const themes = generate({ primary: inputs.primary });
  return {
    inputs,
    generator: 1,
    css_hash: cssHash,
    report: appearances.map((a) => contrastReport(themes[a])),
    fallback: null,
  };
}

/** A report whose light `fg-soft` fails on `bg-canvas`, as the server sends a refused brand's. */
function failingReports() {
  const themes = generate({ primary: "#007373" });
  const canvas = themes.light.colors["bg-canvas"];
  themes.light.colors["fg-soft"] = { ...canvas, l: canvas.l - 0.12 };
  return appearances.map((a) => contrastReport(themes[a]));
}

function problemWith(status: number, body: Record<string, unknown>): Response {
  return new Response(
    JSON.stringify({ type: "about:blank", title: "Refused", status, detail: "Refused", ...body }),
    { status, headers: { "content-type": "application/problem+json" } },
  );
}

function server(more: Record<string, Handler> = {}, saved: Branding = brandingOf(savedInputs)) {
  return stubServer({
    "GET /api/v1/me": () => json(meFixture(["branding.manage"])),
    [`GET ${API}`]: () => json(saved),
    ...more,
  });
}

async function bodiesOf(fetch: ReturnType<typeof stubServer>): Promise<unknown[]> {
  const puts = requestsTo(fetch, API).filter((r) => r.method === "PUT");
  return Promise.all(puts.map((r) => r.clone().json()));
}

const colorInput = () => screen.findByRole("textbox", { name: "Brand color" });

/** Types `hex` into the brand color and leaves the field, which commits it. */
async function enterColor(hex: string) {
  const input = await colorInput();
  await userEvent.clear(input);
  await userEvent.type(input, hex);
  await userEvent.tab();
}

const save = () => userEvent.click(screen.getByRole("button", { name: "Save branding" }));

beforeEach(() => {
  stubMatchMedia(true);
  const link = document.createElement("link");
  link.rel = "stylesheet";
  link.href = "/theme.css?v=old";
  document.head.append(link);
});
afterEach(() => {
  refuseRound.on = false;
  vi.unstubAllGlobals();
  for (const link of document.head.querySelectorAll('link[rel="stylesheet"]')) link.remove();
});

describe("choiceKey", () => {
  it("writes a choice as an ICU select key", () => {
    expect(choiceKey("high-contrast")).toBe("highContrast");
    expect(choiceKey("Atkinson Hyperlegible Next")).toBe("atkinsonHyperlegibleNext");
    expect(choiceKey("Roboto")).toBe("roboto");
    expect(choiceKey("system")).toBe("system");
  });
});

describe("the branding screen (B14)", () => {
  it("shows the saved inputs, the color in hex, and a preview that passes", async () => {
    server();
    renderApp(PATH);
    expect(await screen.findByRole("heading", { level: 1, name: "Branding" })).toBeInTheDocument();
    expect(await colorInput()).toHaveValue("#007373");
    expect(screen.getByRole("radio", { name: "Slightly rounded" })).toBeChecked();
    expect(screen.getByRole("radio", { name: "Serif, for long pages" })).toBeChecked();
    const report = screen.getByRole("region", { name: "Contrast report" });
    expect(report).toHaveAttribute("data-pass", "true");
    expect(document.querySelectorAll('[data-slot="brand-preview-sample"]')).toHaveLength(3);
  });

  it("previews a new color as it is entered, before anything is saved", async () => {
    const fetch = server();
    renderApp(PATH);
    const sample = () => document.querySelector<HTMLElement>('[data-slot="brand-preview-sample"]');
    await colorInput();
    const before = sample()?.style.getPropertyValue("--sc-brand");
    await enterColor("#500E0E");
    await waitFor(() => expect(sample()?.style.getPropertyValue("--sc-brand")).not.toBe(before));
    expect(await bodiesOf(fetch)).toEqual([]);
  });

  it("saves every field, then shows the new brand on this page and says so", async () => {
    const saved = brandingOf({ ...savedInputs, primary: "#500E0E" }, "b".repeat(64));
    const fetch = server({ [`PUT ${API}`]: () => json(saved) });
    renderApp(PATH);
    await enterColor("#500E0E");
    await userEvent.click(screen.getByRole("radio", { name: "Square" }));
    await userEvent.click(screen.getByRole("radio", { name: "The interface font" }));
    await save();
    expect((await screen.findAllByText("Branding saved")).length).toBeGreaterThan(0);
    expect(await bodiesOf(fetch)).toEqual([
      { ...savedInputs, primary: "#500E0E", corner_style: "square", reading_font: "sans" },
    ]);
    const link = document.head.querySelector('link[rel="stylesheet"]');
    expect(link?.getAttribute("href")).toBe(`/theme.css?v=${"b".repeat(64)}`);
  });

  it("sends the saved primary as written when the color was not changed", async () => {
    const fetch = server({ [`PUT ${API}`]: () => json(brandingOf(savedInputs)) });
    renderApp(PATH);
    await colorInput();
    await userEvent.click(screen.getByRole("radio", { name: "Rounded" }));
    await save();
    await screen.findAllByText("Branding saved");
    expect(await bodiesOf(fetch)).toEqual([{ ...savedInputs, corner_style: "round" }]);
  });

  // Spec 16 T6: a brand the generator refuses cannot be saved.
  it("does not send a draft whose report fails, and says why at the brand color", async () => {
    refuseRound.on = true;
    const fetch = server({ [`PUT ${API}`]: () => json(brandingOf(savedInputs)) });
    renderApp(PATH);
    await colorInput();
    await userEvent.click(screen.getByRole("radio", { name: "Rounded" }));
    const report = screen.getByRole("region", { name: "Contrast report" });
    expect(report).toHaveAttribute("data-pass", "false");
    await save();
    const field = await colorInput();
    await waitFor(() => expect(field).toHaveAttribute("aria-invalid", "true"));
    expect(field).toHaveAccessibleDescription(/too close in contrast to read/);
    expect(await bodiesOf(fetch)).toEqual([]);
  });

  it("shows the server's contrast refusal at the brand color, with its report", async () => {
    server({
      [`PUT ${API}`]: () =>
        problemWith(422, { code: "branding.contrast_failed", report: failingReports() }),
    });
    renderApp(PATH);
    await colorInput();
    await userEvent.click(screen.getByRole("radio", { name: "Square" }));
    await save();
    const field = await colorInput();
    await waitFor(() => expect(field).toHaveAttribute("aria-invalid", "true"));
    expect(field).toHaveAccessibleDescription(/too close in contrast to read/);
    const report = screen.getByRole("region", { name: "Contrast report" });
    expect(report).toHaveAttribute("data-pass", "false");
    expect(within(report).getByRole("tab", { selected: true })).toHaveTextContent(
      /^Light, \d+ fail/,
    );
  });

  it("shows each invalid field the server names at that field", async () => {
    server({
      [`PUT ${API}`]: () =>
        problemWith(422, {
          code: "branding.invalid_input",
          errors: [
            { field: "/primary", code: "branding.invalid_color" },
            { field: "/default_appearance", code: "branding.not_a_choice" },
          ],
        }),
    });
    renderApp(PATH);
    await colorInput();
    await save();
    const field = await colorInput();
    await waitFor(() => expect(field).toHaveAttribute("aria-invalid", "true"));
    expect(field).toHaveAccessibleDescription(/Enter a brand color as six hex digits/);
    const appearance = screen.getByRole("button", { name: /Default appearance/ });
    expect(appearance).toHaveAccessibleDescription(/Choose one of the options/);
  });

  it("says when the plan lacks custom branding, and when the caller may not change it", async () => {
    const cases = [
      ["entitlement.feature_unavailable", "plan does not include custom branding"],
      ["permission.denied", "You do not have permission to change branding"],
    ] as const;
    for (const [code, text] of cases) {
      server({ [`PUT ${API}`]: () => problem(403, code, "Forbidden") });
      renderApp(PATH);
      await colorInput();
      await save();
      expect(await screen.findByRole("alert")).toHaveTextContent(text);
      cleanup();
    }
  });

  it("says in words when the caller cannot open the page", async () => {
    server({ [`GET ${API}`]: () => problem(403, "permission.denied", "Forbidden") });
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { level: 2, name: "You cannot change branding" }),
    ).toBeInTheDocument();
  });

  it("warns when the saved brand is not in use because the generator refused it", async () => {
    server({}, { ...brandingOf(savedInputs), fallback: { report: [] } });
    renderApp(PATH);
    expect(await screen.findByText("Your saved brand is not in use")).toBeInTheDocument();
  });

  it("lists Branding in the navigation for holders of branding.manage", async () => {
    server();
    renderApp("/");
    const links = await screen.findAllByRole("link", { name: "Branding" });
    for (const link of links) expect(link).toHaveAttribute("href", PATH);
  });
});
