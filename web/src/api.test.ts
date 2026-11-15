// SPDX-License-Identifier: AGPL-3.0-or-later
// How a failed request reaches the views (docs/specs/m0/11-app-shell.md B29): an error answer from
// Stepclimb is an `ApiError`; no answer at all, or a proxy's gateway error, is an `UnreachableError`.
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError, fetchOrUnreachable, isUnreachable, UnreachableError, unwrap } from "./api";

const PROBLEM = "application/problem+json";

function answer(status: number, contentType?: string, body = "") {
  const headers = contentType === undefined ? undefined : { "content-type": contentType };
  const response = new Response(body, { status, headers });
  const error =
    contentType === PROBLEM
      ? { type: "about:blank", title: "Service Unavailable", status, code: "org.none" }
      : body;
  return { error, response };
}

function thrown(fn: () => unknown): unknown {
  try {
    fn();
  } catch (error) {
    return error;
  }
  throw new Error("nothing was thrown");
}

afterEach(() => vi.unstubAllGlobals());

describe("unwrap", () => {
  it.each([
    [502, "text/html; charset=utf-8"],
    [503, "text/plain"],
    [504, undefined],
  ])("treats a proxy's %i (%s) as no answer", (status, type) => {
    const error = thrown(() => unwrap(answer(status, type, "Bad Gateway")));
    expect(error).toBeInstanceOf(UnreachableError);
    expect(isUnreachable(error)).toBe(true);
  });

  it("keeps the server's own problem+json 503 as an answer", () => {
    const error = thrown(() => unwrap(answer(503, PROBLEM)));
    expect(error).toBeInstanceOf(ApiError);
    expect((error as ApiError).problem?.code).toBe("org.none");
    expect(isUnreachable(error)).toBe(false);
  });

  it("keeps any other status as an answer, whatever its body", () => {
    for (const status of [400, 404, 500, 501]) {
      const error = thrown(() => unwrap(answer(status, "text/html")));
      expect(error).toBeInstanceOf(ApiError);
      expect(isUnreachable(error)).toBe(false);
    }
  });
});

describe("fetchOrUnreachable", () => {
  it("turns a rejected fetch into an UnreachableError", async () => {
    vi.stubGlobal("fetch", () => Promise.reject(new TypeError("Failed to fetch")));
    await expect(fetchOrUnreachable(new Request("http://localhost/x"))).rejects.toBeInstanceOf(
      UnreachableError,
    );
  });

  it("keeps an abort the app made as an abort", async () => {
    vi.stubGlobal("fetch", () => Promise.reject(new DOMException("aborted", "AbortError")));
    const error = await fetchOrUnreachable(new Request("http://localhost/x")).catch((e) => e);
    expect(error).toBeInstanceOf(DOMException);
    expect(isUnreachable(error)).toBe(false);
  });
});
