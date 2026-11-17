// SPDX-License-Identifier: AGPL-3.0-or-later
// The page authoring screen in jsdom (docs/specs/m1/07-pages-and-the-editor.md B3, B4, B10a, B21)
// against a stubbed server: it loads the current version, saves against it with Save and with
// Ctrl+S, offers back unsaved work kept in this browser, and handles the server's conflict and
// body refusals.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import {
  type Handler,
  json,
  meFixture,
  renderApp,
  requestsTo,
  stubMatchMedia,
  stubServer,
} from "../test/app";

const COURSE = "01900000-0000-7000-8000-0000000000c0";
const PAGE = "01900000-0000-7000-8000-0000000000c1";
const AUTHOR = "01900000-0000-7000-8000-000000000002";
const v = (n: number) => `01900000-0000-7000-8000-0000000000d${n}`;
const PATH = `/c/${COURSE}/pages/${PAGE}/edit`;
const BODY_PATH = `/api/v1/pages/${PAGE}/body`;

const summary = (n: number) => ({
  id: v(n),
  number: n,
  author_id: AUTHOR,
  created_at: "2026-11-17T12:00:00Z",
});
const doc = (text: string) => ({
  type: "doc",
  content: [{ type: "paragraph", content: [{ type: "text", text }] }],
});

const bold = {
  type: "doc",
  content: [
    { type: "paragraph", content: [{ type: "text", text: "Readback", marks: [{ type: "bold" }] }] },
  ],
};

function pageSummary(n: number) {
  return {
    id: PAGE,
    course_id: COURSE,
    title: "Taxi",
    slug: "taxi",
    current_version: summary(n),
    created_at: "2026-11-17T12:00:00Z",
    updated_at: "2026-11-17T12:00:00Z",
  };
}

function problemWith(status: number, code: string, extra: Record<string, unknown>): Response {
  const body = { type: "about:blank", title: "Refused", status, detail: "Refused", code, ...extra };
  return new Response(JSON.stringify(body), {
    status,
    headers: { "content-type": "application/problem+json" },
  });
}

/** A server whose page is at version `n` with `text`, plus `more` routes. */
function server(n: number, text: string, more: Record<string, Handler> = {}) {
  return stubServer({
    "GET /api/v1/me": () => json(meFixture()),
    [`GET /api/v1/pages/${PAGE}/versions`]: () => json({ items: [summary(n)], next_cursor: null }),
    [`GET /api/v1/pages/${PAGE}/versions/${v(n)}`]: () => json({ ...summary(n), body: doc(text) }),
    ...more,
  });
}

async function editor() {
  return screen.findByRole("textbox", { name: "Page text" });
}

async function sentBodies(fetch: ReturnType<typeof stubServer>) {
  return Promise.all(requestsTo(fetch, BODY_PATH).map((r) => r.clone().json()));
}

beforeAll(() => {
  // jsdom lays nothing out; ProseMirror measures the caret to scroll it into view.
  document.elementFromPoint = () => null;
  Range.prototype.getClientRects = () =>
    Object.assign([], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
});
beforeEach(() => {
  stubMatchMedia(true);
  // jsdom has no ResizeObserver, which the editor uses to follow its bar's height.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
});
afterEach(() => {
  vi.unstubAllGlobals();
  localStorage.clear();
});

describe("the page editor screen", () => {
  it("loads the current version and saves the body against it", async () => {
    const fetch = server(1, "Readback", {
      [`PUT ${BODY_PATH}`]: () => json(pageSummary(2)),
    });
    renderApp(PATH);
    expect(await screen.findByRole("heading", { level: 1, name: "Edit page" })).toBeInTheDocument();
    expect(await editor()).toHaveTextContent("Readback");
    expect(screen.getByText("Version 1, all changes saved")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(await editor());
    await user.keyboard("{Control>}a{/Control}");
    await user.click(screen.getByRole("button", { name: "Bold" }));
    expect(screen.getByText("Unsaved changes")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(async () =>
      expect(await sentBodies(fetch)).toEqual([{ body: bold, base_version_id: v(1) }]),
    );
    expect(await screen.findByText("Saved as version 2")).toBeInTheDocument();
    expect(screen.getByText("Version 2, all changes saved")).toBeInTheDocument();
    // The save did not start the editor over: the bold it just saved can still be undone.
    expect(screen.getByRole("button", { name: "Undo" })).toBeEnabled();
    // The next save is based on the version the last one wrote.
    await user.click(screen.getByRole("button", { name: "Save" }));
    await waitFor(async () => expect(await sentBodies(fetch)).toHaveLength(2));
    expect((await sentBodies(fetch))[1]).toEqual({ body: bold, base_version_id: v(2) });
  });

  it("saves with Ctrl+S", async () => {
    const fetch = server(1, "Readback", {
      [`PUT ${BODY_PATH}`]: () => json(pageSummary(2)),
    });
    renderApp(PATH);
    await editor();
    const user = userEvent.setup();
    await user.keyboard("{Control>}s{/Control}");
    await waitFor(() => expect(requestsTo(fetch, BODY_PATH)).toHaveLength(1));
  });

  it("on a conflict keeps the author's work and saves it over the newer version when asked", async () => {
    let calls = 0;
    const fetch = server(1, "Readback", {
      [`PUT ${BODY_PATH}`]: () => {
        calls += 1;
        return calls === 1
          ? problemWith(409, "page.version_conflict", { current_version: summary(2) })
          : json(pageSummary(3));
      },
    });
    renderApp(PATH);
    await editor();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Save" }));
    const notice = await screen.findByRole("region", { name: "Newer version saved" });
    expect(notice).toHaveTextContent(
      "Someone saved version 2 of this page while you were editing.",
    );
    await user.click(
      within(notice).getByRole("button", { name: "Save my changes as the newest version" }),
    );
    await waitFor(async () => expect(await sentBodies(fetch)).toHaveLength(2));
    const [, second] = await sentBodies(fetch);
    expect(second).toEqual({ body: doc("Readback"), base_version_id: v(2) });
    expect(await screen.findByText("Saved as version 3")).toBeInTheDocument();
    expect(screen.queryByRole("region", { name: "Newer version saved" })).not.toBeInTheDocument();
  });

  it("on a conflict loads the newer version when the author discards theirs", async () => {
    let current = 1;
    stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      [`GET /api/v1/pages/${PAGE}/versions`]: () =>
        json({ items: [summary(current)], next_cursor: null }),
      [`GET /api/v1/pages/${PAGE}/versions/${v(1)}`]: () =>
        json({ ...summary(1), body: doc("Old") }),
      [`GET /api/v1/pages/${PAGE}/versions/${v(2)}`]: () =>
        json({ ...summary(2), body: doc("New") }),
      [`PUT ${BODY_PATH}`]: () => {
        current = 2;
        return problemWith(409, "page.version_conflict", { current_version: summary(2) });
      },
    });
    renderApp(PATH);
    await editor();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Save" }));
    const notice = await screen.findByRole("region", { name: "Newer version saved" });
    await user.click(within(notice).getByRole("button", { name: "Load version 2" }));
    const confirm = await screen.findByRole("alertdialog", {
      name: "Load version 2 and discard your changes?",
    });
    await user.click(within(confirm).getByRole("button", { name: "Discard my changes" }));
    await waitFor(async () => expect(await editor()).toHaveTextContent("New"));
    expect(screen.getByText("Version 2, all changes saved")).toBeInTheDocument();
  });

  it("shows a refusal of an image at its block (B10a)", async () => {
    const image = {
      type: "doc",
      content: [
        { type: "paragraph", content: [{ type: "text", text: "Diagram:" }] },
        {
          type: "image",
          attrs: { course_file_id: v(9), alt: "A diagram", decorative: false },
        },
      ],
    };
    stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      [`GET /api/v1/pages/${PAGE}/versions`]: () =>
        json({ items: [summary(1)], next_cursor: null }),
      [`GET /api/v1/pages/${PAGE}/versions/${v(1)}`]: () => json({ ...summary(1), body: image }),
      [`PUT ${BODY_PATH}`]: () =>
        problemWith(422, "request.invalid", {
          errors: [{ code: "page.image_type", field: "/body/content/1/attrs/course_file_id" }],
        }),
    });
    renderApp(PATH);
    const content = await editor();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(
      await screen.findByText(
        "The outlined image is not a PNG, JPEG, GIF or WebP file. Replace it, then save again.",
      ),
    ).toBeInTheDocument();
    await waitFor(() => expect(content.children.item(1)).toHaveAttribute("data-invalid"));
  });

  it("keeps unsaved work in this browser and offers it back when the page is opened again", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      server(1, "Readback");
      const first = renderApp(PATH);
      const content = await editor();
      const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
      await user.click(content);
      await user.keyboard("{Control>}a{/Control}");
      await user.click(screen.getByRole("button", { name: "Bold" }));
      expect(screen.getByText("Unsaved changes")).toBeInTheDocument();
      await vi.advanceTimersByTimeAsync(1_100);
      const stored = JSON.parse(localStorage.getItem(`stepclimb.page-draft.${PAGE}`) ?? "null");
      expect(stored).toMatchObject({ base_version_id: v(1) });
      expect(stored.body.content[0].content[0].marks).toEqual([{ type: "bold" }]);
      void first.router.navigate({ to: "/" });
      await screen.findByText(/You have no/);
      void first.router.navigate({
        to: "/c/$course/pages/$page/edit",
        params: { course: COURSE, page: PAGE },
      });
      const offer = await screen.findByRole("region", { name: "Unsaved changes" });
      await user.click(within(offer).getByRole("button", { name: "Restore my changes" }));
      await waitFor(() => expect(screen.getByText("Unsaved changes")).toBeInTheDocument());
      expect((await editor()).querySelector("strong")).toHaveTextContent("Readback");
    } finally {
      vi.useRealTimers();
    }
  });
});
