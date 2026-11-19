// SPDX-License-Identifier: AGPL-3.0-or-later
// The course authoring screen in jsdom (docs/specs/m1/16-learning-ui-components.md B13, B19)
// against a stubbed server that keeps the course's modules: the board over the server's modules;
// adding modules and items; editing, publishing and deleting them with the server's refusals; a
// move and its 409 `order.stale` reload; the rules a module's editor sends; the pages and videos
// panels; and the loading, empty, error, offline, refused, archived and read-only states.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Item, Module, PageSummary } from "../authoring/course-content";
import type { Course } from "../files/course-files";
import {
  apiRequests,
  type Handler,
  json,
  meFixture,
  problem,
  renderApp,
  requestsTo,
  stubMatchMedia,
  stubServer,
} from "../test/app";

const courseId = "01900000-0000-7000-8000-0000000000c1";
const PATH = `/c/${courseId}/edit`;
const COURSE = `/api/v1/courses/${courseId}`;
const MODULES = `${COURSE}/modules`;
const PAGES = `${COURSE}/pages`;

const AUTHOR = ["course.content_edit", "course.view", "course.view_all"];
const VIEWER = ["course.view", "course.view_all"];
const STUDENT = ["course.participate", "course.view"];

function courseFixture(permissions: string[], status: Course["status"] = "draft"): Course {
  return {
    id: courseId,
    account_id: "01900000-0000-7000-8000-0000000000a0",
    code: "GND-101",
    title: "Ground school",
    status,
    template: false,
    permissions,
    settings: { home: { kind: "modules" } },
    start_at: null,
    end_at: null,
    created_at: "2026-11-01T12:00:00Z",
    updated_at: "2026-11-01T12:00:00Z",
  };
}

const STAMP = "2026-11-02T12:00:00Z";

function itemFixture(id: string, moduleId: string, title: string, extra: Partial<Item> = {}): Item {
  return {
    id,
    module_id: moduleId,
    course_id: courseId,
    kind: "external_link",
    title,
    position: 0,
    published: false,
    url: "https://example.com/a",
    requirement: null,
    lock: null,
    created_at: STAMP,
    updated_at: STAMP,
    ...extra,
  };
}

function moduleFixture(id: string, title: string, items: Item[], extra: Partial<Module> = {}) {
  const module: Module = {
    id,
    course_id: courseId,
    title,
    position: 0,
    published: false,
    unlock_at: null,
    prerequisite_ids: [],
    require_sequential: false,
    completion_rule: "all",
    items,
    lock: null,
    created_at: STAMP,
    updated_at: STAMP,
    ...extra,
  };
  return module;
}

function course() {
  return [
    moduleFixture("m1", "Radio basics", [
      itemFixture("i1", "m1", "Phraseology", { requirement: { kind: "view" }, published: true }),
      itemFixture("i2", "m1", "Practice", { kind: "header", url: undefined }),
    ]),
    moduleFixture("m2", "Ground control", [itemFixture("i3", "m2", "Taxi charts")]),
  ];
}

/** A server holding `modules`, which a test changes as the real one would. */
function server(
  permissions: string[] = AUTHOR,
  state: { modules: Module[] } = { modules: course() },
  more: Record<string, Handler> = {},
) {
  return stubServer({
    "GET /api/v1/me": () => json(meFixture()),
    [`GET ${COURSE}`]: () => json(courseFixture(permissions)),
    [`GET ${MODULES}`]: () => json({ modules: state.modules }),
    [`GET ${PAGES}`]: () => json({ items: [], next_cursor: null }),
    [`GET ${COURSE}/videos`]: () => json({ items: [], next_cursor: null }),
    ...more,
  });
}

async function boardRows(): Promise<string[]> {
  await screen.findByRole("grid", { name: "Modules and items" });
  return screen.getAllByRole("row").map((r) => r.getAttribute("aria-label") ?? "");
}

const bodyOf = async (fetch: ReturnType<typeof stubServer>, method: string, path: string) =>
  requestsTo(fetch, path)
    .filter((r) => r.method === method)
    .at(-1)
    ?.clone()
    .json();

/** A 422 naming one member and the rule it broke. */
function invalid(field: string, code: string): Response {
  return new Response(
    JSON.stringify({
      type: "about:blank",
      title: "Unprocessable Content",
      status: 422,
      detail: "x",
      code: "request.invalid",
      errors: [{ field, code }],
    }),
    { status: 422, headers: { "content-type": "application/problem+json" } },
  );
}

/** Waits until no side sheet is open (a toast is a dialog too, so the role cannot say). */
const sheetClosed = () =>
  waitFor(() => expect(document.querySelector('[data-slot="sheet"]')).toBeNull());

beforeEach(() => stubMatchMedia(true));
afterEach(() => vi.unstubAllGlobals());

describe("the course authoring screen", () => {
  it("draws the server's modules on the board, with the course's title, code and status", async () => {
    server();
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { level: 1, name: "Edit course" }),
    ).toBeInTheDocument();
    expect(await screen.findByRole("heading", { level: 2, name: "Ground school" })).toBeVisible();
    expect(screen.getByText("GND-101 · Draft")).toBeInTheDocument();
    expect(await boardRows()).toEqual([
      "1 Radio basics",
      "Phraseology",
      "Practice",
      "2 Ground control",
      "Taxi charts",
    ]);
    expect(screen.getByRole("link", { name: "Open the files of this course" })).toHaveAttribute(
      "href",
      `/c/${courseId}/files`,
    );
    expect(document.title).toBe("Edit course · Example Training");
  });

  it("adds a module and shows it once the server has it", async () => {
    const state = { modules: course() };
    const fetch = server(AUTHOR, state, {
      [`POST ${MODULES}`]: async (request) => {
        const { title } = await request.clone().json();
        const created = moduleFixture("m3", title, []);
        state.modules = [...state.modules, created];
        return json(created, 201);
      },
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Add module" }));
    const sheet = await screen.findByRole("dialog", { name: "Add a module" });
    await user.type(within(sheet).getByRole("textbox", { name: "Title" }), "Clearance delivery");
    await user.click(within(sheet).getByRole("button", { name: "Add module" }));
    await sheetClosed();
    expect(await bodyOf(fetch, "POST", MODULES)).toEqual({ title: "Clearance delivery" });
    expect((await boardRows()).at(-1)).toBe("3 Clearance delivery");
    expect(await screen.findByText("Clearance delivery added")).toBeInTheDocument();
    // Focus goes back to the button that opened the sheet.
    await waitFor(() =>
      expect(document.activeElement).toBe(screen.getByRole("button", { name: "Add module" })),
    );
  });

  it("adds an external link to the module chosen and shows the server's refusal of its address", async () => {
    const state = { modules: course() };
    let refuse = true;
    const fetch = server(AUTHOR, state, {
      "POST /api/v1/modules/m1/items": async (request) => {
        const body = await request.clone().json();
        if (refuse) {
          refuse = false;
          return new Response(
            JSON.stringify({
              type: "about:blank",
              title: "Unprocessable Content",
              status: 422,
              detail: "x",
              code: "request.invalid",
              errors: [{ field: "/url", code: "item.url_invalid" }],
            }),
            { status: 422, headers: { "content-type": "application/problem+json" } },
          );
        }
        const created = itemFixture("i9", "m1", body.title, { url: body.url });
        const [first, ...rest] = state.modules;
        if (first) state.modules = [{ ...first, items: [...first.items, created] }, ...rest];
        return json(created, 201);
      },
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Add item" }));
    const sheet = await screen.findByRole("dialog", { name: "Add an item" });
    expect(within(sheet).getByText(/Pages, files and videos cannot be added to modules yet/));
    // The last module is chosen at first.
    await user.click(within(sheet).getByRole("button", { name: /Module/ }));
    await user.click(await screen.findByRole("option", { name: "1 Radio basics" }));
    await user.type(within(sheet).getByRole("textbox", { name: "Title" }), "Chart supplement");
    const address = within(sheet).getByRole("textbox", { name: "Address" });
    await user.type(address, "https://user:pw@example.com/");
    await user.click(within(sheet).getByRole("button", { name: "Add item" }));
    expect(
      await within(sheet).findByText(/Enter a full address starting with https:\/\//),
    ).toBeInTheDocument();
    expect(address).toHaveAttribute("aria-invalid", "true");
    await user.clear(address);
    await user.type(address, "https://example.com/chart");
    await user.click(within(sheet).getByRole("button", { name: "Add item" }));
    await sheetClosed();
    expect(await bodyOf(fetch, "POST", "/api/v1/modules/m1/items")).toEqual({
      kind: "external_link",
      title: "Chart supplement",
      url: "https://example.com/chart",
    });
    expect((await boardRows())[3]).toBe("Chart supplement");
  });

  it("adds a header without an address", async () => {
    const fetch = server(
      AUTHOR,
      { modules: course() },
      {
        "POST /api/v1/modules/m2/items": async (request) =>
          json(
            itemFixture("i8", "m2", (await request.clone().json()).title, { kind: "header" }),
            201,
          ),
      },
    );
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Add item" }));
    const sheet = await screen.findByRole("dialog", { name: "Add an item" });
    await user.click(within(sheet).getByRole("radio", { name: /Header/ }));
    expect(within(sheet).queryByRole("textbox", { name: "Address" })).toBeNull();
    await user.type(within(sheet).getByRole("textbox", { name: "Title" }), "Review");
    await user.click(within(sheet).getByRole("button", { name: "Add item" }));
    await sheetClosed();
    expect(await bodyOf(fetch, "POST", "/api/v1/modules/m2/items")).toEqual({
      kind: "header",
      title: "Review",
    });
  });

  it("moves an item through the order route and reads the modules again", async () => {
    const state = { modules: course() };
    const fetch = server(AUTHOR, state, {
      "PUT /api/v1/modules/m1/items/order": async (request) => {
        const { item_ids } = await request.clone().json();
        const [first, ...rest] = state.modules;
        if (!first) throw new Error("fixture");
        const items = item_ids.map((id: string) => first.items.find((i) => i.id === id));
        state.modules = [{ ...first, items }, ...rest];
        return json(state.modules[0]);
      },
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    const before = requestsTo(fetch, MODULES).length;
    await user.click(screen.getByRole("button", { name: "Move Phraseology" }));
    await user.click(await screen.findByRole("menuitem", { name: "Move down" }));
    expect(
      await screen.findByText("Moved Phraseology to position 2 of 2 in Radio basics."),
    ).toBeInTheDocument();
    expect(await bodyOf(fetch, "PUT", "/api/v1/modules/m1/items/order")).toEqual({
      item_ids: ["i2", "i1"],
    });
    expect(requestsTo(fetch, MODULES).length).toBe(before + 1);
    expect((await boardRows()).slice(1, 3)).toEqual(["Practice", "Phraseology"]);
  });

  it("reloads on 409 order.stale and shows the other author's order", async () => {
    const state = { modules: course() };
    const fetch = server(AUTHOR, state, {
      "PUT /api/v1/courses/01900000-0000-7000-8000-0000000000c1/modules/order": () => {
        // Another author moved Ground control first meanwhile.
        state.modules = [...state.modules].reverse();
        return problem(409, "order.stale", "Conflict");
      },
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Move 2 Ground control" }));
    await user.click(await screen.findByRole("menuitem", { name: "Move up" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The latest order is shown");
    await waitFor(async () => expect((await boardRows())[0]).toBe("1 Ground control"));
    expect(requestsTo(fetch, MODULES).filter((r) => r.method === "GET")).toHaveLength(2);
  });

  it("saves a module's title, publish state and rules in one PATCH of what changed", async () => {
    const state = { modules: course() };
    const fetch = server(AUTHOR, state, {
      "PATCH /api/v1/modules/m2": async (request) => {
        const patch = await request.clone().json();
        const [first, second] = state.modules;
        if (!first || !second) throw new Error("fixture");
        const saved = { ...second, ...patch };
        state.modules = [first, saved];
        return json(saved);
      },
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Edit 2 Ground control" }));
    const sheet = await screen.findByRole("dialog", { name: "Edit Ground control" });
    const title = within(sheet).getByRole("textbox", { name: "Title" });
    await user.clear(title);
    await user.type(title, "Ground");
    await user.click(within(sheet).getByRole("switch", { name: "Published, so learners see it" }));
    await user.click(within(sheet).getByRole("checkbox", { name: "1 Radio basics" }));
    await user.click(within(sheet).getByRole("switch", { name: "Items in order" }));
    await user.click(
      within(sheet).getByRole("radio", { name: "Any one requirement in the module" }),
    );
    await user.click(within(sheet).getByRole("button", { name: "Save" }));
    await sheetClosed();
    expect(await bodyOf(fetch, "PATCH", "/api/v1/modules/m2")).toEqual({
      title: "Ground",
      published: true,
      prerequisite_ids: ["m1"],
      require_sequential: true,
      completion_rule: "one",
    });
    await waitFor(async () => expect((await boardRows())[3]).toBe("2 Ground"));
    const row = screen.getAllByRole("row")[3] as HTMLElement;
    const drawn = row.querySelector('[data-slot="module-board-row-meta"]');
    expect(drawn).toHaveTextContent("Published");
    expect(drawn).toHaveTextContent(/Unlocks after Radio basics/);
  });

  it("sends an unlock date as an instant and removes it with null", async () => {
    const state = {
      modules: [moduleFixture("m1", "Radio basics", [], { unlock_at: "2026-03-03T15:00:00Z" })],
    };
    const fetch = server(AUTHOR, state, {
      "PATCH /api/v1/modules/m1": async (request) =>
        json({ ...state.modules[0], ...(await request.clone().json()) }),
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Edit 1 Radio basics" }));
    const sheet = await screen.findByRole("dialog", { name: "Edit Radio basics" });
    expect(within(sheet).getByRole("group", { name: "Opens on" })).toHaveTextContent("UTC");
    await user.click(within(sheet).getByRole("button", { name: "Remove the date" }));
    await user.click(within(sheet).getByRole("button", { name: "Save" }));
    await sheetClosed();
    expect(await bodyOf(fetch, "PATCH", "/api/v1/modules/m1")).toEqual({ unlock_at: null });
  });

  it("keeps the module's editor open with the server's cycle refusal", async () => {
    server(
      AUTHOR,
      { modules: course() },
      {
        "PATCH /api/v1/modules/m1": () => problem(409, "module.prerequisite_cycle", "Conflict"),
      },
    );
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Edit 1 Radio basics" }));
    const sheet = await screen.findByRole("dialog", { name: "Edit Radio basics" });
    await user.click(within(sheet).getByRole("checkbox", { name: "2 Ground control" }));
    await user.click(within(sheet).getByRole("button", { name: "Save" }));
    expect(await within(sheet).findByRole("alert")).toHaveTextContent(
      "These modules would wait for each other",
    );
  });

  it("refuses to delete a module with items, and deletes an empty one", async () => {
    const state = { modules: [...course(), moduleFixture("m3", "Spare", [])] };
    server(AUTHOR, state, {
      "DELETE /api/v1/modules/m1": () => problem(409, "module.not_empty", "Conflict"),
      "DELETE /api/v1/modules/m3": () => {
        state.modules = state.modules.filter((m) => m.id !== "m3");
        return new Response(null, { status: 204 });
      },
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Edit 1 Radio basics" }));
    let sheet = await screen.findByRole("dialog", { name: "Edit Radio basics" });
    await user.click(within(sheet).getByRole("button", { name: "Delete module" }));
    let confirm = await screen.findByRole("alertdialog", { name: "Delete Radio basics?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete module" }));
    expect(await within(sheet).findByRole("alert")).toHaveTextContent(
      "This module still holds items",
    );
    await user.click(within(sheet).getByRole("button", { name: "Close" }));
    await sheetClosed();

    await user.click(screen.getByRole("button", { name: "Edit 3 Spare" }));
    sheet = await screen.findByRole("dialog", { name: "Edit Spare" });
    await user.click(within(sheet).getByRole("button", { name: "Delete module" }));
    confirm = await screen.findByRole("alertdialog", { name: "Delete Spare?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete module" }));
    await waitFor(() => expect(screen.queryByRole("row", { name: "3 Spare" })).toBeNull());
    expect(await screen.findByText("Spare deleted")).toBeInTheDocument();
    // Focus is on the board, not lost to the page.
    await waitFor(() => expect(document.activeElement?.closest("[data-key]")).not.toBeNull());
  });

  it("saves an item's requirement and publish state, and says why it cannot be deleted", async () => {
    const state = { modules: course() };
    const fetch = server(AUTHOR, state, {
      "PATCH /api/v1/items/i3": async (request) =>
        json({ ...itemFixture("i3", "m2", "Taxi charts"), ...(await request.clone().json()) }),
      "DELETE /api/v1/items/i3": () => problem(409, "item.has_progress", "Conflict"),
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Edit Taxi charts" }));
    let sheet = await screen.findByRole("dialog", { name: "Edit Taxi charts" });
    expect(within(sheet).getByRole("textbox", { name: "Address" })).toHaveValue(
      "https://example.com/a",
    );
    expect(within(sheet).queryByRole("radio", { name: "Watch part of the video" })).toBeNull();
    await user.click(within(sheet).getByRole("radio", { name: "Mark it as done" }));
    await user.click(within(sheet).getByRole("switch", { name: "Published, so learners see it" }));
    await user.click(within(sheet).getByRole("button", { name: "Save" }));
    await sheetClosed();
    expect(await bodyOf(fetch, "PATCH", "/api/v1/items/i3")).toEqual({
      published: true,
      requirement: { kind: "mark_done" },
    });

    await user.click(screen.getByRole("button", { name: "Edit Taxi charts" }));
    sheet = await screen.findByRole("dialog", { name: "Edit Taxi charts" });
    await user.click(within(sheet).getByRole("button", { name: "Delete item" }));
    const confirm = await screen.findByRole("alertdialog", { name: "Delete Taxi charts?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete item" }));
    expect(await within(sheet).findByRole("alert")).toHaveTextContent(
      "Learners have already made progress on this item",
    );
  });

  it("offers no requirement on a header and links a page item to its editor", async () => {
    const page = "01900000-0000-7000-8000-0000000000p1";
    server(AUTHOR, {
      modules: [
        moduleFixture("m1", "Radio basics", [
          itemFixture("i1", "m1", "Practice", { kind: "header", url: undefined }),
          itemFixture("i2", "m1", "Phraseology", { kind: "page", url: undefined, ref_id: page }),
        ]),
      ],
    });
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Edit Practice" }));
    let sheet = await screen.findByRole("dialog", { name: "Edit Practice" });
    expect(within(sheet).queryByRole("radiogroup")).toBeNull();
    await user.keyboard("{Escape}");
    await sheetClosed();
    await user.click(screen.getByRole("button", { name: "Edit Phraseology" }));
    sheet = await screen.findByRole("dialog", { name: "Edit Phraseology" });
    expect(within(sheet).getByRole("link", { name: "Open the page editor" })).toHaveAttribute(
      "href",
      `/c/${courseId}/pages/${page}/edit`,
    );
  });

  it("shows a refusal of a member the sheet does not draw above the form", async () => {
    server(
      AUTHOR,
      { modules: course() },
      {
        "POST /api/v1/modules/m2/items": () => invalid("/position", "item.position_out_of_range"),
      },
    );
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Add item" }));
    const sheet = await screen.findByRole("dialog", { name: "Add an item" });
    await user.click(within(sheet).getByRole("radio", { name: /Header/ }));
    await user.type(within(sheet).getByRole("textbox", { name: "Title" }), "Review");
    await user.click(within(sheet).getByRole("button", { name: "Add item" }));
    const alert = await within(sheet).findByRole("alert");
    expect(alert).toHaveTextContent("The change was not saved.");
    expect(alert).toHaveTextContent("item.position_out_of_range");
  });

  it("says a control character in an item's address at the address", async () => {
    server(
      AUTHOR,
      { modules: course() },
      {
        "PATCH /api/v1/items/i3": () => invalid("/url", "text.contains_nul"),
      },
    );
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Edit Taxi charts" }));
    const sheet = await screen.findByRole("dialog", { name: "Edit Taxi charts" });
    const address = within(sheet).getByRole("textbox", { name: "Address" });
    await user.type(address, "x");
    await user.click(within(sheet).getByRole("button", { name: "Save" }));
    expect(
      await within(sheet).findByText("This address holds a character that cannot be used."),
    ).toBeInTheDocument();
    expect(address).toHaveAttribute("aria-invalid", "true");
    expect(within(sheet).getByRole("textbox", { name: "Title" })).not.toHaveAttribute(
      "aria-invalid",
    );
  });

  it("says an empty course has no modules yet and offers Add module", async () => {
    server(AUTHOR, { modules: [] });
    renderApp(PATH);
    expect(
      await screen.findByText("This course has no modules yet. Add the first one above."),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add item" })).toBeDisabled();
  });

  it("is read-only in an archived course and says why", async () => {
    stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      [`GET ${COURSE}`]: () => json(courseFixture(AUTHOR, "archived")),
      [`GET ${MODULES}`]: () => json({ modules: course() }),
    });
    renderApp(PATH);
    await boardRows();
    expect(
      screen.getByText("This course is archived, so its content cannot be changed."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add module" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^(Edit|Move) / })).toBeNull();
  });

  it("is read-only without course.content_edit", async () => {
    server(VIEWER);
    renderApp(PATH);
    await boardRows();
    expect(
      screen.getByText("You can see the content of this course but not change it."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Add module" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^(Edit|Move) / })).toBeNull();
  });

  it("refuses a learner without reading the modules", async () => {
    const fetch = server(STUDENT);
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { name: "You cannot edit this course" }),
    ).toBeInTheDocument();
    expect(apiRequests(fetch).map((r) => new URL(r.url).pathname)).not.toContain(MODULES);
  });

  it("says when the course cannot be found", async () => {
    stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      [`GET ${COURSE}`]: () => problem(404, "course.not_found", "Not Found"),
    });
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { name: "This course could not be found" }),
    ).toBeInTheDocument();
  });

  it("shows a failed load of the modules with Try again", async () => {
    let fail = true;
    server(
      AUTHOR,
      { modules: course() },
      {
        [`GET ${MODULES}`]: () =>
          fail ? problem(500, "internal", "Internal Server Error") : json({ modules: course() }),
      },
    );
    const user = userEvent.setup();
    renderApp(PATH);
    expect(await screen.findByRole("heading", { name: "Internal Server Error" })).toBeVisible();
    fail = false;
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect((await boardRows())[0]).toBe("1 Radio basics");
  });

  it("says the server could not be reached when the modules get no answer", async () => {
    server(
      AUTHOR,
      { modules: course() },
      {
        [`GET ${MODULES}`]: () => Promise.reject(new TypeError("Failed to fetch")),
      },
    );
    renderApp(PATH);
    expect(await screen.findByText(/could not be reached/)).toBeInTheDocument();
  });

  it("keeps a write's sheet open and says so when the server cannot be reached", async () => {
    server(
      AUTHOR,
      { modules: course() },
      {
        [`POST ${MODULES}`]: () => Promise.reject(new TypeError("Failed to fetch")),
      },
    );
    const user = userEvent.setup();
    renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("button", { name: "Add module" }));
    const sheet = await screen.findByRole("dialog", { name: "Add a module" });
    await user.type(within(sheet).getByRole("textbox", { name: "Title" }), "Weather");
    await user.click(within(sheet).getByRole("button", { name: "Add module" }));
    expect(await within(sheet).findByRole("alert")).toHaveTextContent(
      "The server could not be reached, so the change was not saved.",
    );
  });
});

describe("the pages and videos panels", () => {
  const pageFixture = (id: string, title: string): PageSummary => ({
    id,
    course_id: courseId,
    title,
    slug: title.toLowerCase(),
    current_version: {
      id: `${id}-v`,
      number: 3,
      author_id: "01900000-0000-7000-8000-000000000002",
      created_at: STAMP,
    },
    created_at: STAMP,
    updated_at: STAMP,
  });

  it("lists the pages, each linking to its editor", async () => {
    server(
      AUTHOR,
      { modules: course() },
      {
        [`GET ${PAGES}`]: () => json({ items: [pageFixture("p1", "Taxi")], next_cursor: null }),
      },
    );
    renderApp(`${PATH}?panel=pages`);
    expect(await screen.findByRole("link", { name: "Taxi" })).toHaveAttribute(
      "href",
      `/c/${courseId}/pages/p1/edit`,
    );
    expect(screen.getByText("Version 3 · Changed Nov 2, 2026")).toBeInTheDocument();
    expect(screen.getByText(/Pages cannot be added to modules yet/)).toBeInTheDocument();
  });

  it("creates a page and opens it in the editor", async () => {
    const fetch = server(
      AUTHOR,
      { modules: course() },
      {
        [`POST ${PAGES}`]: async (request) =>
          json(pageFixture("p2", (await request.clone().json()).title), 201),
        "GET /api/v1/pages/p2/versions": () => new Promise<Response>(() => {}),
      },
    );
    const user = userEvent.setup();
    const app = renderApp(PATH);
    await boardRows();
    await user.click(screen.getByRole("tab", { name: "Pages" }));
    await waitFor(() => expect(app.router.state.location.search).toEqual({ panel: "pages" }));
    await user.click(await screen.findByRole("button", { name: "New page" }));
    const sheet = await screen.findByRole("dialog", { name: "New page" });
    await user.type(within(sheet).getByRole("textbox", { name: "Title" }), "Readbacks");
    await user.click(within(sheet).getByRole("button", { name: "Create and edit" }));
    await waitFor(() =>
      expect(app.router.state.location.pathname).toBe(`/c/${courseId}/pages/p2/edit`),
    );
    expect(await bodyOf(fetch, "POST", PAGES)).toEqual({ title: "Readbacks" });
  });

  it("shows a taken page address at the title", async () => {
    server(
      AUTHOR,
      { modules: course() },
      {
        [`POST ${PAGES}`]: () => problem(409, "page.slug_taken", "Conflict"),
      },
    );
    const user = userEvent.setup();
    renderApp(`${PATH}?panel=pages`);
    await user.click(await screen.findByRole("button", { name: "New page" }));
    const sheet = await screen.findByRole("dialog", { name: "New page" });
    const title = within(sheet).getByRole("textbox", { name: "Title" });
    await user.type(title, "Taxi");
    await user.click(within(sheet).getByRole("button", { name: "Create and edit" }));
    expect(await within(sheet).findByText(/already has a page with an address/)).toBeVisible();
    expect(title).toHaveAttribute("aria-invalid", "true");
  });

  it("says once that an archived course's videos cannot be changed", async () => {
    stubServer({
      "GET /api/v1/me": () => json(meFixture()),
      [`GET ${COURSE}`]: () => json(courseFixture(AUTHOR, "archived")),
      [`GET ${MODULES}`]: () => json({ modules: course() }),
      [`GET ${COURSE}/videos`]: () => json({ items: [], next_cursor: null }),
    });
    renderApp(`${PATH}?panel=videos`);
    expect(
      await screen.findByText("This course is archived, so its content cannot be changed."),
    ).toBeInTheDocument();
    await screen.findByText("This course has no videos.");
    expect(screen.queryByText(/so its videos cannot be changed/)).toBeNull();
    expect(screen.queryByRole("button", { name: "Choose file" })).toBeNull();
  });

  it("holds the video panel", async () => {
    server();
    renderApp(`${PATH}?panel=videos`);
    expect(await screen.findByText("This course has no videos yet. Upload the first one above."));
    expect(screen.getByRole("button", { name: "Choose file" })).toBeInTheDocument();
  });
});
