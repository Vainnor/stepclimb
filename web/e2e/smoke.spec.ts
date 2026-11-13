// SPDX-License-Identifier: AGPL-3.0-or-later
// Smoke test of the Compose stack: the binary serves the built SPA, which reads /healthz through
// the API client; the probes and the API docs answer.
import { expect, test } from "@playwright/test";

test("the skeleton route shows the server's health", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "Stepclimb" })).toBeVisible();
  await expect(page.getByTestId("health-status")).toHaveText("ok");
});

test("a deep link is served the SPA", async ({ page }) => {
  const response = await page.goto("/c/example/m/1/i/1");
  expect(response?.status()).toBe(200);
  await expect(page.locator("#root")).toBeAttached();
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
