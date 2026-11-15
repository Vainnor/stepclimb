// SPDX-License-Identifier: AGPL-3.0-or-later
// Smoke test of the Compose stack: the binary serves the built SPA, which asks /api/v1/me through
// the API client; the probes and the API docs answer.
import { expect, test } from "@playwright/test";

// A browser without a session cookie gets 401 from /me, and the app sends it to the sign-in
// screen with the path it asked for as the return target (spec 11 B10).
test("an anonymous browser is sent to sign in", async ({ page }) => {
  const me = page.waitForResponse((response) => response.url().endsWith("/api/v1/me"));
  await page.goto("/?tab=2");
  expect((await me).status()).toBe(401);
  await expect(page.getByRole("heading", { level: 1, name: "Sign in" })).toBeVisible();
  const url = new URL(page.url());
  expect(url.pathname).toBe("/sign-in");
  expect(url.searchParams.get("return_to")).toBe("/?tab=2");
});

// Spec 11 T18: the M1 deep link is served the SPA, which renders the not-found view.
test("a deep link is served the SPA and renders not found", async ({ page }) => {
  const response = await page.goto("/c/example/m/1/i/1");
  expect(response?.status()).toBe(200);
  await expect(
    page.getByRole("heading", { level: 1, name: "There is no page at /c/example/m/1/i/1" }),
  ).toBeVisible();
});

test("the probes answer", async ({ request }) => {
  expect((await request.get("/healthz")).status()).toBe(200);
  const ready = await request.get("/readyz");
  expect(ready.status()).toBe(200);
  expect(await ready.json()).toEqual({ status: "ok" });
});

test("the API document and its UI are served", async ({ request }) => {
  const doc = await request.get("/api/openapi.json");
  expect(doc.status()).toBe(200);
  expect((await doc.json()).openapi).toBe("3.1.0");
  const docs = await request.get("/docs");
  expect(docs.status()).toBe(200);
  expect(docs.headers()["content-type"]).toContain("text/html");
});

// The server's tests pick an organization with this header, which only a `test-support` build
// reads (crates/stepclimb-server/src/request_org.rs). The release image must ignore it: sign-out
// with a session cookie resolves the request's organization, and answers the same with the
// header naming an organization that does not exist as without it.
test("the release image ignores the test organization header", async ({ request }) => {
  const signOut = (extra: Record<string, string>) =>
    request.post("/auth/sign-out", {
      headers: {
        cookie: "__Host-sc_session=e2e-unknown-token",
        "sec-fetch-site": "same-origin",
        ...extra,
      },
    });
  const plain = await signOut({});
  // The cookie makes the server resolve the request's organization: the e2e stack's one
  // organization (`just e2e` creates it), where the unknown session ends nothing. A header the
  // release image read would name no organization and answer 404 instead.
  expect(plain.status()).toBe(204);
  const named = await signOut({ "x-stepclimb-test-org": "01900000-0000-7000-8000-000000000000" });
  expect(named.status()).toBe(plain.status());
  expect(await named.text()).toBe(await plain.text());
});
