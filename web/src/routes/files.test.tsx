// SPDX-License-Identifier: AGPL-3.0-or-later
// The files area in jsdom (docs/specs/m1/16-learning-ui-components.md B20; 06-files.md B3 to B12)
// against a stubbed server: the folder's entries and the tree, the course's permissions deciding
// the upload zone and the row actions, uploads with their progress and refusals, rename, move,
// delete and folder rename with theirs, folders by address, paging, and the loading, empty,
// refused, not found, failed and offline states.
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Course, CourseFile, FolderEntry } from "../files/course-files";
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
import { stubXhr } from "../test/xhr";

const courseId = "01900000-0000-7000-8000-0000000000c1";
const PATH = `/c/${courseId}/files`;
const LIST = `/api/v1/courses/${courseId}/files`;
const COURSE = `/api/v1/courses/${courseId}`;

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

const AUTHOR = ["course.content_edit", "course.view", "course.view_all"];
const VIEWER = ["course.view", "course.view_all"];

function fileFixture(name: string, folder = "/", size = 2_400_000): CourseFile {
  return {
    id: `01900000-0000-7000-8000-${name.length.toString().padStart(4, "0")}${name.charCodeAt(0).toString(16).padStart(8, "0")}`,
    course_id: courseId,
    folder,
    name,
    size,
    mime: "application/pdf",
    sha256: "0".repeat(64),
    created_at: "2026-11-02T12:00:00Z",
    updated_at: "2026-11-03T12:00:00Z",
  };
}

const handbook = fileFixture("Handbook.pdf");
const handouts: FolderEntry = { kind: "folder", path: "/handouts", name: "handouts" };
const top: FolderEntry[] = [handouts, { kind: "file", file: handbook }];

function page(items: FolderEntry[], next_cursor: string | null = null) {
  return json({ items, next_cursor });
}

/** Lists each folder from `folders`; an unknown folder is empty. */
function listing(folders: Record<string, FolderEntry[]>): Handler {
  return (request) => {
    const folder = new URL(request.url).searchParams.get("folder") ?? "/";
    return page(folders[folder] ?? []);
  };
}

function server(permissions: string[] = AUTHOR, more: Record<string, Handler> = {}) {
  return stubServer({
    "GET /api/v1/me": () => json(meFixture()),
    [`GET ${COURSE}`]: () => json(courseFixture(permissions)),
    [`GET ${LIST}`]: listing({ "/": top }),
    ...more,
  });
}

const list = () => screen.findByRole("grid", { name: /^Files in/ });

/** The upload zone's file input, once the course has loaded and the zone is shown. */
async function fileInput(): Promise<HTMLInputElement> {
  await screen.findByRole("button", { name: "Choose files" });
  const input = document.querySelector('[data-slot="file-drop"] input[type="file"]');
  if (!(input instanceof HTMLInputElement)) throw new Error("no file input");
  return input;
}

function listQueries(fetch: ReturnType<typeof stubServer>) {
  return requestsTo(fetch, LIST)
    .filter((r) => r.method === "GET")
    .map((r) => Object.fromEntries(new URL(r.url).searchParams));
}

async function openAction(name: string, action: string) {
  const user = userEvent.setup();
  await user.click(await screen.findByRole("button", { name: `Actions for ${name}` }));
  await user.click(await screen.findByRole("menuitem", { name: action }));
  return user;
}

/**
 * The animation frame after the one in which a closed dialog returns focus (React Aria's
 * FocusScope does it in a requestAnimationFrame scheduled as the dialog unmounts). Frames run in
 * order, so once this resolves, that return has run: focus that it would move has moved.
 */
const afterFocusReturn = () =>
  new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

beforeEach(() => stubMatchMedia(true));
afterEach(() => vi.unstubAllGlobals());

describe("the files area", () => {
  it("lists the folder's sub-folders, then its files, under the course's title", async () => {
    server();
    renderApp(PATH);
    expect(await screen.findByRole("heading", { level: 1, name: "Files" })).toBeInTheDocument();
    expect(
      await screen.findByRole("heading", { level: 2, name: "Ground school" }),
    ).toBeInTheDocument();
    const grid = await list();
    const rows = await within(grid).findAllByRole("row");
    // The header row, then the folder, then the file.
    expect(rows[1]).toHaveTextContent("handouts");
    expect(within(rows[1] as HTMLElement).getByRole("link", { name: "handouts" })).toHaveAttribute(
      "href",
      `${PATH}?folder=%2Fhandouts`,
    );
    expect(rows[2]).toHaveTextContent("Handbook.pdf");
    expect(rows[2]).toHaveTextContent("2.4 MB");
    expect(rows[2]).toHaveTextContent("PDF · Changed Nov 3, 2026");
    expect(within(rows[2] as HTMLElement).getByTitle("application/pdf")).toBeInTheDocument();
    expect(rows[2]).toHaveTextContent("MIME type application/pdf");
    expect(screen.getByRole("treegrid", { name: "Folders" })).toBeInTheDocument();
    expect(document.title).toBe("Files · Example Training");
  });

  it("gives upload and row actions to a holder of course.content_edit", async () => {
    server(AUTHOR);
    renderApp(PATH);
    await list();
    expect(await screen.findByRole("button", { name: "Choose files" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Actions for Handbook.pdf" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Actions for handouts" })).toBeInTheDocument();
  });

  it("shows no upload or actions to a holder of course.view_all alone", async () => {
    server(VIEWER);
    renderApp(PATH);
    const grid = await list();
    await within(grid).findByText("Handbook.pdf");
    expect(screen.queryByRole("button", { name: "Choose files" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Actions for/ })).toBeNull();
    expect(within(grid).queryByRole("columnheader", { name: "Actions" })).toBeNull();
  });

  it("offers no change in an archived course and says why", async () => {
    server(AUTHOR, { [`GET ${COURSE}`]: () => json(courseFixture(AUTHOR, "archived")) });
    renderApp(PATH);
    expect(
      await screen.findByText("This course is archived, so its files cannot be changed."),
    ).toBeInTheDocument();
    await within(await list()).findByText("Handbook.pdf");
    expect(screen.queryByRole("button", { name: "Choose files" })).toBeNull();
    expect(screen.queryByRole("button", { name: /^Actions for/ })).toBeNull();
  });

  it("says in words that the files are staff-only when the server refuses the list", async () => {
    server(["course.view"], {
      [`GET ${LIST}`]: () => problem(403, "permission.denied", "Forbidden"),
    });
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { level: 2, name: "You cannot open these files" }),
    ).toBeInTheDocument();
    expect(screen.getByText(/Learners reach files through the items/)).toBeInTheDocument();
    expect(screen.queryByRole("grid")).toBeNull();
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("says the course could not be found for a course.not_found", async () => {
    server(AUTHOR, { [`GET ${COURSE}`]: () => problem(404, "course.not_found", "Not Found") });
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { level: 2, name: "This course could not be found" }),
    ).toBeInTheDocument();
  });

  it("shows a failed list with Try again, which loads it", async () => {
    let up = false;
    server(AUTHOR, {
      [`GET ${LIST}`]: (request) => {
        if (!up) throw new TypeError("network down");
        return listing({ "/": top })(request);
      },
    });
    renderApp(PATH);
    expect(
      await screen.findByRole("heading", { name: "The server could not be reached" }),
    ).toBeInTheDocument();
    up = true;
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
    await within(await list()).findByText("Handbook.pdf");
  });

  it("keeps the list on screen and shows the offline notice when a refetch gets no answer", async () => {
    let up = true;
    server(AUTHOR, {
      [`GET ${LIST}`]: (request) => {
        if (!up) throw new TypeError("network down");
        return listing({ "/": top })(request);
      },
    });
    const app = renderApp(PATH);
    await within(await list()).findByText("Handbook.pdf");
    up = false;
    await app.context.queryClient.refetchQueries();
    expect(
      await screen.findByText(/The server could not be reached, so this page may be out of date/),
    ).toBeInTheDocument();
    expect(within(await list()).getByText("Handbook.pdf")).toBeInTheDocument();
  });

  it("marks the list busy while the folder loads", async () => {
    server(AUTHOR, { [`GET ${LIST}`]: () => new Promise<Response>(() => undefined) });
    renderApp(PATH);
    const grid = await list();
    expect(grid.closest('[data-slot="table-scroller"]')).toHaveAttribute("aria-busy", "true");
    expect(within(grid).queryByText(/no files/)).toBeNull();
  });

  it("says an empty course has no files, and asks an author to upload the first", async () => {
    server(AUTHOR, { [`GET ${LIST}`]: listing({}) });
    renderApp(PATH);
    expect(
      await screen.findByText("This course has no files yet. Upload the first one above."),
    ).toBeInTheDocument();
  });

  it("opens a folder by its address, with its breadcrumbs and an empty state of its own", async () => {
    const fetch = server(AUTHOR);
    renderApp(`${PATH}?folder=%2Fhandouts`);
    expect(await screen.findByText("This folder is empty.")).toBeInTheDocument();
    expect(listQueries(fetch)).toContainEqual({ folder: "/handouts" });
    const crumbs = screen.getByRole("navigation", { name: "Breadcrumbs" });
    expect(within(crumbs).getByRole("link", { name: "Top folder" })).toBeInTheDocument();
    expect(within(crumbs).getByText("handouts")).toBeInTheDocument();
    expect(screen.getByRole("grid", { name: "Files in handouts" })).toBeInTheDocument();
  });

  it("opens a sub-folder from its row's link", async () => {
    const week = fileFixture("Week 1.pdf", "/handouts");
    server(AUTHOR, {
      [`GET ${LIST}`]: listing({ "/": top, "/handouts": [{ kind: "file", file: week }] }),
    });
    renderApp(PATH);
    const user = userEvent.setup();
    await user.click(await within(await list()).findByRole("link", { name: "handouts" }));
    expect(await screen.findByRole("grid", { name: "Files in handouts" })).toBeInTheDocument();
    expect(await screen.findByText("Week 1.pdf")).toBeInTheDocument();
  });

  it("opens a folder chosen in the tree", async () => {
    server(AUTHOR);
    renderApp(PATH);
    const tree = await screen.findByRole("treegrid", { name: "Folders" });
    const row = await within(tree).findByRole("row", { name: "handouts" });
    await userEvent.setup().click(row);
    expect(await screen.findByRole("grid", { name: "Files in handouts" })).toBeInTheDocument();
  });

  it("pages through a long folder with its cursors", async () => {
    const second = fileFixture("Zulu.pdf");
    const fetch = server(AUTHOR, {
      [`GET ${LIST}`]: (request) => {
        const cursor = new URL(request.url).searchParams.get("cursor");
        return cursor === "c2" ? page([{ kind: "file", file: second }]) : page(top, "c2");
      },
    });
    renderApp(PATH);
    await within(await list()).findByText("Handbook.pdf");
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Next" }));
    expect(await screen.findByText("Zulu.pdf")).toBeInTheDocument();
    expect(listQueries(fetch)).toContainEqual({ folder: "/", cursor: "c2" });
    await user.click(screen.getByRole("button", { name: "Previous" }));
    expect(await screen.findByText("Handbook.pdf")).toBeInTheDocument();
  });
  it("says a folder path the server refuses is not valid, and links to the top folder", async () => {
    server(AUTHOR, {
      [`GET ${LIST}`]: () =>
        new Response(
          JSON.stringify({
            type: "about:blank",
            title: "Unprocessable Entity",
            status: 422,
            detail: "x",
            code: "request.invalid",
            errors: [{ field: "/folder", code: "file.folder_invalid" }],
          }),
          { status: 422, headers: { "content-type": "application/problem+json" } },
        ),
    });
    renderApp(`${PATH}?folder=%2Fa%2F..`);
    expect(
      await screen.findByRole("heading", { level: 2, name: "This folder path is not valid" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Go to the top folder" })).toHaveAttribute(
      "href",
      PATH,
    );
    expect(screen.queryByRole("button", { name: "Try again" })).toBeNull();
  });

  it("mounts no tree and asks for no folders under 768 px", async () => {
    stubMatchMedia(false);
    const fetch = server(AUTHOR);
    renderApp(PATH);
    await within(await list()).findByText("Handbook.pdf");
    expect(screen.queryByRole("treegrid")).toBeNull();
    expect(listQueries(fetch).filter((q) => q.limit !== undefined)).toEqual([]);
  });

  it("says when the tree's folders could not be loaded, and loads them on Try again", async () => {
    let up = false;
    server(AUTHOR, {
      [`GET ${LIST}`]: (request) => {
        const limit = new URL(request.url).searchParams.get("limit");
        if (limit !== null && !up) return problem(500, "internal", "Internal Server Error");
        return listing({ "/": top })(request);
      },
    });
    renderApp(PATH);
    expect(await screen.findByText("Some folders could not be loaded.")).toBeInTheDocument();
    up = true;
    const tree = screen.getByRole("treegrid", { name: "Folders" });
    await userEvent.setup().click(screen.getByRole("button", { name: "Try again" }));
    expect(await within(tree).findByRole("row", { name: "handouts" })).toBeInTheDocument();
    await waitFor(() => expect(screen.queryByText("Some folders could not be loaded.")).toBeNull());
  });
});

describe("uploading", () => {
  it("sends the file's bytes into the folder on screen, shows its progress, then lists it", async () => {
    const xhr = stubXhr();
    xhr.hold = true;
    const stored = fileFixture("Notes.pdf", "/handouts");
    let listed: FolderEntry[] = [];
    const fetch = server(AUTHOR, {
      [`GET ${LIST}`]: () => page(listed),
      [`POST ${LIST}`]: () => {
        listed = [{ kind: "file", file: stored }];
        return json(stored, 201);
      },
    });
    renderApp(`${PATH}?folder=%2Fhandouts`);
    expect(await screen.findByText("Files go into handouts.")).toBeInTheDocument();
    const file = new File(["%PDF-1.7 notes"], "Notes.pdf", { type: "application/pdf" });
    await userEvent.setup().upload(await fileInput(), file);

    await waitFor(() => expect(xhr.sent).toHaveLength(1));
    const [sent] = xhr.sent;
    if (sent === undefined) throw new Error("nothing sent");
    expect(sent.method).toBe("POST");
    const url = new URL(sent.url);
    expect(url.pathname).toBe(LIST);
    expect(Object.fromEntries(url.searchParams)).toEqual({
      folder: "/handouts",
      name: "Notes.pdf",
    });
    expect(sent.headers["Content-Type"] ?? sent.headers["content-type"]).toBe(
      "application/octet-stream",
    );
    expect(sent.body).toBe(file);

    sent.progress(7, 14);
    const bar = await screen.findByRole("progressbar", { name: "Uploading Notes.pdf" });
    await waitFor(() => expect(bar).toHaveAttribute("aria-valuenow", "50"));

    await sent.finish();
    expect(await screen.findByText("Notes.pdf uploaded")).toBeInTheDocument();
    expect(await within(await list()).findByText("Notes.pdf")).toBeInTheDocument();
    expect(screen.queryByRole("progressbar")).toBeNull();
    expect(requestsTo(fetch, LIST).filter((r) => r.method === "POST")).toHaveLength(1);
  });

  it("says storage is not set up when the server answers storage.not_configured", async () => {
    stubXhr();
    server(AUTHOR, {
      [`POST ${LIST}`]: () => problem(503, "storage.not_configured", "Service Unavailable"),
    });
    renderApp(PATH);
    await list();
    await userEvent.setup().upload(await fileInput(), new File(["x"], "a.pdf"));
    expect(
      await screen.findByText(/File storage is not set up on this server.*STEPCLIMB_STORAGE_URL/),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose files" })).toBeEnabled();
  });

  it("stops at the first refusal and counts the files it did not send", async () => {
    const xhr = stubXhr();
    server(AUTHOR, {
      [`POST ${LIST}`]: () => problem(409, "file.name_taken", "Conflict"),
    });
    renderApp(PATH);
    await list();
    await userEvent
      .setup()
      .upload(await fileInput(), [
        new File(["a"], "Handbook.pdf"),
        new File(["b"], "b.pdf"),
        new File(["c"], "c.pdf"),
      ]);
    expect(
      await screen.findByText(/This folder already has a file named Handbook.pdf/),
    ).toBeInTheDocument();
    expect(screen.getByText(/The 2 files chosen after it were not uploaded/)).toBeInTheDocument();
    expect(xhr.sent).toHaveLength(1);
  });

  it("names the code of a refusal it has no words for", async () => {
    stubXhr();
    server(AUTHOR, { [`POST ${LIST}`]: () => problem(500, "internal", "Internal Server Error") });
    renderApp(PATH);
    await list();
    await userEvent.setup().upload(await fileInput(), new File(["x"], "a.pdf"));
    expect(await screen.findByText("internal")).toBeInTheDocument();
  });

  it("says the server could not be reached when the upload gets no answer", async () => {
    stubXhr();
    server(AUTHOR, {
      [`POST ${LIST}`]: () => {
        throw new TypeError("network down");
      },
    });
    renderApp(PATH);
    await list();
    await userEvent.setup().upload(await fileInput(), new File(["x"], "a.pdf"));
    expect(await screen.findByText(/Stepclimb could not reach the server/)).toBeInTheDocument();
  });
  it("keeps focus on Choose files and announces each file politely", async () => {
    const xhr = stubXhr();
    xhr.hold = true;
    server(AUTHOR, { [`POST ${LIST}`]: () => json(fileFixture("a.pdf"), 201) });
    renderApp(PATH);
    const user = userEvent.setup();
    const input = await fileInput();
    const choose = screen.getByRole("button", { name: "Choose files" });
    choose.focus();
    await user.upload(input, new File(["x"], "a.pdf"));
    choose.focus();
    await waitFor(() => expect(xhr.sent).toHaveLength(1));
    const status = document.querySelector('[data-slot="files-upload-status"]');
    expect(status).toHaveAttribute("role", "status");
    expect(status).toHaveTextContent("Uploading a.pdf");
    // Spec 16 B20a: the zone stays pressable while a batch runs (not FileDrop's `isBusy`), so a
    // file chosen meanwhile reaches the page, which says it was not sent.
    expect(choose).toBeEnabled();
    expect(choose).not.toHaveAttribute("aria-disabled");
    expect(choose).toHaveFocus();
    await xhr.sent[0]?.finish();
    await waitFor(() => expect(status).toHaveTextContent("a.pdf uploaded"));
    expect(choose).toHaveFocus();
  });

  it("does not send files chosen while a batch runs, and says why", async () => {
    const xhr = stubXhr();
    xhr.hold = true;
    server(AUTHOR, { [`POST ${LIST}`]: () => json(fileFixture("a.pdf"), 201) });
    renderApp(PATH);
    const user = userEvent.setup();
    const input = await fileInput();
    await user.upload(input, new File(["x"], "a.pdf"));
    await waitFor(() => expect(xhr.sent).toHaveLength(1));
    await user.upload(input, new File(["y"], "b.pdf"));
    expect(
      await screen.findByText("a.pdf is still uploading. Choose more files once it has finished."),
    ).toBeInTheDocument();
    expect(xhr.sent).toHaveLength(1);
  });

  it("aborts the batch when the folder on screen changes", async () => {
    const xhr = stubXhr();
    xhr.hold = true;
    server(AUTHOR, { [`POST ${LIST}`]: () => json(fileFixture("a.pdf"), 201) });
    renderApp(PATH);
    const user = userEvent.setup();
    await user.upload(await fileInput(), [new File(["x"], "a.pdf"), new File(["y"], "b.pdf")]);
    await waitFor(() => expect(xhr.sent).toHaveLength(1));
    await user.click(within(await list()).getByRole("link", { name: "handouts" }));
    expect(await screen.findByRole("grid", { name: "Files in handouts" })).toBeInTheDocument();
    expect(xhr.sent[0]?.aborted).toBe(true);
    expect(xhr.sent).toHaveLength(1);
  });
});

describe("changing files", () => {
  const FILE = `/api/v1/course-files/${handbook.id}`;

  it("renames a file with its new name", async () => {
    const fetch = server(AUTHOR, {
      [`PATCH ${FILE}`]: () => json({ ...handbook, name: "Guide.pdf" }),
    });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Rename");
    const dialog = await screen.findByRole("dialog", { name: "Rename Handbook.pdf" });
    const field = within(dialog).getByRole("textbox", { name: "Name" });
    await user.clear(field);
    await user.type(field, "Guide.pdf");
    await user.click(within(dialog).getByRole("button", { name: "Save name" }));
    expect(await screen.findByText("Renamed to Guide.pdf")).toBeInTheDocument();
    const [patch] = requestsTo(fetch, FILE);
    expect(await patch?.json()).toEqual({ name: "Guide.pdf" });
    await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  });

  it("keeps the rename dialog open with the server's refusal", async () => {
    server(AUTHOR, { [`PATCH ${FILE}`]: () => problem(409, "file.name_taken", "Conflict") });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Rename");
    const dialog = await screen.findByRole("dialog", { name: "Rename Handbook.pdf" });
    await user.click(within(dialog).getByRole("button", { name: "Save name" }));
    expect(
      await within(dialog).findByText(/This folder already has a file named Handbook.pdf/),
    ).toBeInTheDocument();
  });

  it("moves a file to the folder typed, written as the API writes folders", async () => {
    const fetch = server(AUTHOR, {
      [`PATCH ${FILE}`]: () => json({ ...handbook, folder: "/handouts" }),
    });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Move");
    const dialog = await screen.findByRole("dialog", { name: "Move Handbook.pdf" });
    const field = within(dialog).getByRole("textbox", { name: "Folder" });
    await user.clear(field);
    await user.type(field, " handouts/ ");
    await user.click(within(dialog).getByRole("button", { name: "Move file" }));
    expect(await screen.findByText("Handbook.pdf moved to /handouts")).toBeInTheDocument();
    expect(await requestsTo(fetch, FILE)[0]?.json()).toEqual({ folder: "/handouts" });
  });

  it("shows a folder rule the server says a move broke", async () => {
    server(AUTHOR, {
      [`PATCH ${FILE}`]: () =>
        new Response(
          JSON.stringify({
            type: "about:blank",
            title: "Unprocessable Entity",
            status: 422,
            detail: "x",
            code: "request.invalid",
            errors: [{ field: "/folder", code: "file.folder_too_deep" }],
          }),
          { status: 422, headers: { "content-type": "application/problem+json" } },
        ),
    });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Move");
    const dialog = await screen.findByRole("dialog", { name: "Move Handbook.pdf" });
    await user.click(within(dialog).getByRole("button", { name: "Move file" }));
    expect(
      await within(dialog).findByText("Folders can be at most 8 levels deep."),
    ).toBeInTheDocument();
  });

  it("deletes a file only after asking", async () => {
    let listed = top;
    const fetch = server(AUTHOR, {
      [`GET ${LIST}`]: () => page(listed),
      [`DELETE ${FILE}`]: () => {
        listed = [handouts];
        return new Response(null, { status: 204 });
      },
    });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Delete");
    const confirm = await screen.findByRole("alertdialog", { name: "Delete Handbook.pdf?" });
    await user.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(requestsTo(fetch, FILE)).toHaveLength(0);

    await openAction("Handbook.pdf", "Delete");
    const again = await screen.findByRole("alertdialog", { name: "Delete Handbook.pdf?" });
    await user.click(within(again).getByRole("button", { name: "Delete file" }));
    expect(await screen.findByText("Handbook.pdf deleted")).toBeInTheDocument();
    expect(requestsTo(fetch, FILE).map((r) => r.method)).toEqual(["DELETE"]);
    await waitFor(() => expect(screen.queryByText("Handbook.pdf")).toBeNull());
  });

  it("says why a file in use cannot be deleted", async () => {
    server(AUTHOR, { [`DELETE ${FILE}`]: () => problem(409, "file.in_use", "Conflict") });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Delete");
    const confirm = await screen.findByRole("alertdialog", { name: "Delete Handbook.pdf?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete file" }));
    expect(await screen.findByText("Handbook.pdf was not deleted")).toBeInTheDocument();
    expect(screen.getByText(/Handbook.pdf is used by an item or a page/)).toBeInTheDocument();
  });

  it("renames a folder in place", async () => {
    const RENAME = `/api/v1/courses/${courseId}/folders/rename`;
    const fetch = server(AUTHOR, { [`POST ${RENAME}`]: () => json({ moved: 3 }) });
    renderApp(PATH);
    const user = await openAction("handouts", "Rename");
    const dialog = await screen.findByRole("dialog", { name: "Rename folder handouts" });
    const field = within(dialog).getByRole("textbox", { name: "Name" });
    await user.clear(field);
    await user.type(field, "notes");
    await user.click(within(dialog).getByRole("button", { name: "Save name" }));
    expect(await screen.findByText("Folder renamed to notes")).toBeInTheDocument();
    expect(await requestsTo(fetch, RENAME)[0]?.json()).toEqual({ from: "/handouts", to: "/notes" });
  });
  it("moves focus to the next row once a deleted file leaves the list", async () => {
    let listed = top;
    server(AUTHOR, {
      [`GET ${LIST}`]: () => page(listed),
      [`DELETE ${FILE}`]: () => {
        listed = [handouts];
        return new Response(null, { status: 204 });
      },
    });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Delete");
    const confirm = await screen.findByRole("alertdialog", { name: "Delete Handbook.pdf?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete file" }));
    await waitFor(() => expect(screen.queryByText("Handbook.pdf")).toBeNull());
    const [, folderRow] = within(await list()).getAllByRole("row");
    // The row now in its place, or a control in it; never the page's body.
    await waitFor(() => expect(folderRow?.contains(document.activeElement)).toBe(true));
    await afterFocusReturn();
    expect(folderRow?.contains(document.activeElement)).toBe(true);
  });

  it("moves focus to the table once the last file leaves it", async () => {
    let listed: FolderEntry[] = [{ kind: "file", file: handbook }];
    server(AUTHOR, {
      [`GET ${LIST}`]: () => page(listed),
      [`DELETE ${FILE}`]: () => {
        listed = [];
        return new Response(null, { status: 204 });
      },
    });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Delete");
    const confirm = await screen.findByRole("alertdialog", { name: "Delete Handbook.pdf?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete file" }));
    await screen.findByText(/has no files yet/);
    const grid = screen.getByRole("grid", { name: "Files in Top folder" });
    await waitFor(() => expect(grid).toHaveFocus());
    await afterFocusReturn();
    expect(grid).toHaveFocus();
  });

  it("puts the code of an unexplained delete refusal in the toast's sentence", async () => {
    server(AUTHOR, { [`DELETE ${FILE}`]: () => problem(500, "internal", "Internal Server Error") });
    renderApp(PATH);
    const user = await openAction("Handbook.pdf", "Delete");
    const confirm = await screen.findByRole("alertdialog", { name: "Delete Handbook.pdf?" });
    await user.click(within(confirm).getByRole("button", { name: "Delete file" }));
    expect(
      await screen.findByText(
        "Something went wrong. Try again. If it keeps happening, give an administrator this code: internal",
      ),
    ).toBeInTheDocument();
  });

  it("says a folder rename would clash, in words about the folder", async () => {
    const RENAME = `/api/v1/courses/${courseId}/folders/rename`;
    server(AUTHOR, { [`POST ${RENAME}`]: () => problem(409, "file.name_taken", "Conflict") });
    renderApp(PATH);
    const user = await openAction("handouts", "Rename");
    const dialog = await screen.findByRole("dialog", { name: "Rename folder handouts" });
    const field = within(dialog).getByRole("textbox", { name: "Name" });
    await user.clear(field);
    await user.type(field, "notes");
    await user.click(within(dialog).getByRole("button", { name: "Save name" }));
    expect(
      await within(dialog).findByText(/Renaming the folder to notes would put two files/),
    ).toBeInTheDocument();
  });
});
