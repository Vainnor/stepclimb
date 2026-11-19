// SPDX-License-Identifier: AGPL-3.0-or-later
// A file item's content (docs/specs/m1/06-files.md B18, Q8) in jsdom against a stubbed server: a
// PDF in a frame or, where the browser shows no PDF, the message with the download; an image; any
// other type as a download; the download asking for an attachment URL; a locked item asking for
// nothing; and the refusals.
import type { components } from "@stepclimb/api-client";
import { StringsProvider } from "@stepclimb/ui";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { catalogs } from "../messages";
import { json, problem, requestsTo, stubServer } from "../test/app";
import { FileItemView } from "./file-item-view";

type Item = components["schemas"]["Item"];

const itemId = "01900000-0000-7000-8000-0000000000d1";
const URL_PATH = `/api/v1/items/${itemId}/file-url`;

function itemFixture(extra: Partial<Item> = {}): Item {
  return {
    id: itemId,
    module_id: "01900000-0000-7000-8000-0000000000b1",
    course_id: "01900000-0000-7000-8000-0000000000c1",
    kind: "file",
    title: "Ground school handbook",
    position: 0,
    published: true,
    ref_id: "01900000-0000-7000-8000-0000000000e1",
    requirement: null,
    lock: null,
    created_at: "2026-11-02T12:00:00Z",
    updated_at: "2026-11-02T12:00:00Z",
    ...extra,
  };
}

function signed(disposition: string, mime = "application/pdf") {
  return {
    url: `https://lms.example.org/files/${disposition}-token`,
    expires_at: "2026-11-02T12:10:00Z",
    disposition,
    name: "Handbook.pdf",
    mime,
    size: 2_400_000,
  };
}

/** Answers the URL route with `mime` for each disposition asked for, kept in order in `asked`. */
function serveUrls(mime = "application/pdf") {
  const asked: string[] = [];
  stubServer({
    [`POST ${URL_PATH}`]: async (request) => {
      const body = (await request.json()) as { disposition: string };
      asked.push(body.disposition);
      return json(signed(body.disposition, mime));
    },
  });
  return asked;
}

function show(item: Item, pdfViewer: boolean) {
  const client = new QueryClient();
  render(
    <QueryClientProvider client={client}>
      <StringsProvider catalogs={catalogs} isDevelopment>
        <FileItemView item={item} level={2} pdfViewer={pdfViewer} />
      </StringsProvider>
    </QueryClientProvider>,
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("FileItemView", () => {
  it("frames a PDF from an inline URL where the browser shows PDFs", async () => {
    const asked = serveUrls();
    show(itemFixture(), true);
    const frame = await screen.findByTitle("Ground school handbook (PDF preview)");
    expect(frame.getAttribute("src")).toBe("https://lms.example.org/files/inline-token");
    expect(screen.getByText("Handbook.pdf, 2.4 MB")).toBeVisible();
    expect(asked).toEqual(["inline"]);
  });

  it("says a browser without a PDF viewer cannot show it and offers the download", async () => {
    serveUrls();
    show(itemFixture(), false);
    expect(
      await screen.findByText(
        "This browser does not show PDF files in the page. Download the file to read it.",
      ),
    ).toBeVisible();
    expect(screen.queryByTitle(/PDF preview/)).toBeNull();
    expect(screen.getByRole("button", { name: "Download" })).toBeVisible();
  });

  it("shows an image with the item's title as its text", async () => {
    serveUrls("image/png");
    show(itemFixture(), false);
    const image = await screen.findByRole("img", { name: "Ground school handbook" });
    expect(image.getAttribute("src")).toBe("https://lms.example.org/files/inline-token");
  });

  it("offers any other type as a download only", async () => {
    serveUrls("application/octet-stream");
    show(itemFixture(), true);
    expect(await screen.findByRole("button", { name: "Download" })).toBeVisible();
    expect(screen.queryByRole("img")).toBeNull();
    expect(screen.queryByTitle(/PDF preview/)).toBeNull();
  });

  it("downloads from a new attachment URL", async () => {
    const asked = serveUrls();
    const assign = vi.fn();
    vi.stubGlobal("location", { ...globalThis.location, assign });
    show(itemFixture(), true);
    await userEvent.setup().click(await screen.findByRole("button", { name: "Download" }));
    await waitFor(() =>
      expect(assign).toHaveBeenCalledWith("https://lms.example.org/files/attachment-token"),
    );
    expect(asked).toEqual(["inline", "attachment"]);
  });

  it("asks for nothing for a locked item or another kind", async () => {
    const fetch = stubServer({});
    show(itemFixture({ lock: { reasons: [{ kind: "module_locked" }] } }), true);
    show(itemFixture({ kind: "external_link", ref_id: undefined }), true);
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(requestsTo(fetch, URL_PATH)).toEqual([]);
  });

  it("says when the server has no file storage", async () => {
    stubServer({
      [`POST ${URL_PATH}`]: () => problem(503, "storage.not_configured", "Service Unavailable"),
    });
    show(itemFixture(), true);
    expect(
      await screen.findByRole("heading", { name: "This file cannot be opened" }),
    ).toBeVisible();
  });

  it("offers to try again after a refusal it does not explain", async () => {
    stubServer({
      [`POST ${URL_PATH}`]: () => problem(500, "internal.error", "Internal Server Error"),
    });
    show(itemFixture(), true);
    expect(await screen.findByRole("button", { name: "Try again" })).toBeVisible();
  });
});
