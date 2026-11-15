// SPDX-License-Identifier: AGPL-3.0-or-later
// The npm advisory gate (bin/check-pnpm-audit.mjs; spec 01 B12, Q7). Run by `just web-audit`.
// The reports are shaped like `pnpm audit --json` output from pnpm 12.9, and the bulk endpoint
// answers like registry.npmjs.org's; nothing here leaves the machine (the registry checks use
// a local server and a closed local port).
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createServer } from "node:http";
import { describe, it } from "node:test";
import {
  addDays,
  evaluate,
  isCalendarDate,
  PROBE,
  probeAdvisories,
  readAllowlist,
  unseenProxy,
} from "./check-pnpm-audit.mjs";

const TODAY = "2026-11-14";
const advisory = (id, severity, module = "example-pkg") => ({
  github_advisory_id: id,
  severity,
  module_name: module,
  title: `${module} example advisory`,
  url: `https://github.com/advisories/${id}`,
});
const report = (...advisories) => ({
  advisories: Object.fromEntries(advisories.map((a, index) => [String(index + 1), a])),
});
const HIGH = advisory("GHSA-2222-3333-4444", "high");
const CRITICAL = advisory("GHSA-5555-6666-7777", "critical", "prod-pkg");
const MODERATE = advisory("GHSA-cfgh-jmpq-rvwx", "moderate");
const entry = (id, expires = "2026-12-31", name = "example-pkg") => ({
  id,
  package: name,
  reason: "Reached only from the Storybook build; never in the server or the web bundle.",
  expires,
});
const allow = (...entries) => ({ advisories: entries });
const verdict = (prod, all, allowlist = allow()) =>
  evaluate({ reports: { prod, all }, allowlist, today: TODAY });

describe("the npm advisory gate", () => {
  it("passes when there is no advisory", () => {
    assert.deepEqual(verdict(report(), report()), { failures: [], notes: [] });
  });
  it("passes a moderate advisory and prints it", () => {
    const { failures, notes } = verdict(report(), report(MODERATE));
    assert.deepEqual(failures, []);
    assert.match(
      notes[0],
      /below the threshold: moderate GHSA-cfgh-jmpq-rvwx in example-pkg \(all\)/,
    );
  });
  it("fails on a high advisory in a dev dependency, naming it", () => {
    const { failures } = verdict(report(), report(HIGH));
    assert.equal(failures.length, 1);
    assert.match(failures[0], /^high GHSA-2222-3333-4444 in example-pkg \(all\)/);
    assert.match(failures[0], /pnpm-audit-allowlist\.json/);
  });
  it("fails on a critical advisory in a production dependency once, naming both runs", () => {
    const { failures } = verdict(report(CRITICAL), report(CRITICAL));
    assert.equal(failures.length, 1);
    assert.match(failures[0], /critical GHSA-5555-6666-7777 in prod-pkg \(prod, all\)/);
  });
  it("fails on a severity it does not know", () => {
    const { failures } = verdict(report(), report(advisory("GHSA-2222-3333-4445", "severe")));
    assert.equal(failures.length, 1);
    assert.match(failures[0], /unknown severity "severe"/);
  });
  it("fails on an advisory with no GHSA id", () => {
    const { failures } = verdict(report(), report({ ...HIGH, github_advisory_id: undefined }));
    assert.equal(failures.length, 1);
    assert.match(failures[0], /has no GHSA id/);
  });
  it("passes a listed high advisory until its expiry date", () => {
    const { failures, notes } = verdict(
      report(),
      report(HIGH),
      allow(entry(HIGH.github_advisory_id)),
    );
    assert.deepEqual(failures, []);
    assert.match(notes[0], /allowed until 2026-12-31/);
  });
  it("passes an entry on its expiry date and fails it the day after", () => {
    const onTheDay = verdict(report(), report(HIGH), allow(entry(HIGH.github_advisory_id, TODAY)));
    assert.deepEqual(onTheDay.failures, []);
    const expired = verdict(
      report(),
      report(HIGH),
      allow(entry(HIGH.github_advisory_id, "2026-11-13")),
    );
    assert.equal(expired.failures.length, 1);
    assert.match(expired.failures[0], /expired on 2026-11-13 \(UTC\); review it/);
  });
  it("fails an entry whose package is not the advisory's", () => {
    const listed = allow(entry(HIGH.github_advisory_id, "2026-12-31", "other-pkg"));
    const { failures } = verdict(report(), report(HIGH), listed);
    assert.deepEqual(failures, [
      'pnpm-audit-allowlist.json: GHSA-2222-3333-4444 names package "other-pkg", the advisory is for "example-pkg"',
    ]);
  });
  it("fails an entry that matches no advisory any more", () => {
    const { failures } = verdict(report(), report(), allow(entry(HIGH.github_advisory_id)));
    assert.deepEqual(failures, [
      "pnpm-audit-allowlist.json: GHSA-2222-3333-4444 matches no advisory any more; remove it",
    ]);
  });
  it("fails an entry for an advisory that is below high now", () => {
    const { failures } = verdict(
      report(),
      report(MODERATE),
      allow(entry(MODERATE.github_advisory_id)),
    );
    assert.deepEqual(failures, [
      "pnpm-audit-allowlist.json: GHSA-cfgh-jmpq-rvwx is moderate now, below the threshold; remove it",
    ]);
  });
  it("does not let an entry for one advisory cover another", () => {
    const listed = allow(entry(HIGH.github_advisory_id));
    const { failures } = verdict(report(), report(HIGH, CRITICAL), listed);
    assert.equal(failures.length, 1);
    assert.match(failures[0], /GHSA-5555-6666-7777/);
  });
  it("fails when the audit printed no advisories object", () => {
    assert.throws(() => verdict({ error: "x" }, report()), /printed no advisories object/);
  });
});

describe("the allowlist", () => {
  const problems = (fields) =>
    readAllowlist(allow({ ...entry(HIGH.github_advisory_id), ...fields }), TODAY).problems;
  it("needs an advisories array", () => {
    assert.deepEqual(readAllowlist({}, TODAY).problems, [
      'pnpm-audit-allowlist.json: needs an "advisories" array',
    ]);
  });
  it("needs a GHSA id, the package, a reason and a real date", () => {
    assert.match(problems({ id: "CVE-2026-1" })[0], /must be a GitHub advisory id/);
    assert.match(problems({ package: "" })[0], /needs "package"/);
    assert.match(problems({ reason: "  " })[0], /needs a "reason"/);
    for (const expires of ["31/12/2026", undefined, "2026-02-30", "2026-13-01", "2026-12-1"]) {
      assert.match(problems({ expires })[0], /needs "expires" as a real date/, String(expires));
    }
  });
  it("allows an expiry at most 90 days ahead", () => {
    assert.equal(addDays(TODAY, 90), "2027-02-12");
    assert.deepEqual(problems({ expires: "2027-02-12" }), []);
    assert.match(
      problems({ expires: "2027-02-13" })[0],
      /more than 90 days after 2026-11-14 \(UTC\); pick 2027-02-12/,
    );
  });
  it("reads dates as calendar days", () => {
    assert.equal(isCalendarDate("2028-02-29"), true);
    assert.equal(isCalendarDate("2027-02-29"), false);
  });
  it("refuses the same advisory twice", () => {
    const id = HIGH.github_advisory_id;
    assert.match(readAllowlist(allow(entry(id), entry(id)), TODAY).problems[0], /listed twice/);
  });
  it("accepts the committed file", () => {
    const committed = JSON.parse(
      readFileSync(new URL("../pnpm-audit-allowlist.json", import.meta.url)),
    );
    assert.deepEqual(readAllowlist(committed, TODAY).problems, []);
  });
});

async function withServer(handler, test) {
  const server = createServer(handler);
  await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
  try {
    return await test(`http://127.0.0.1:${server.address().port}/`);
  } finally {
    server.closeAllConnections();
    server.close();
  }
}

const bulk = (status, body) => (request, response) => {
  let received = "";
  request.on("data", (chunk) => {
    received += chunk;
  });
  request.on("end", () => {
    response.writeHead(status, { "content-type": "application/json" });
    response.end(typeof body === "function" ? body(request, received) : body);
  });
};

describe("the advisory endpoint probe", () => {
  it("trusts an endpoint that reports the probe package's advisories", async () => {
    let asked;
    const answer = (request, received) => {
      asked = { method: request.method, url: request.url, body: JSON.parse(received) };
      return JSON.stringify({ [PROBE.name]: [{ id: 1, severity: "high" }] });
    };
    assert.equal(await withServer(bulk(200, answer), (url) => probeAdvisories(url)), true);
    assert.deepEqual(asked, {
      method: "POST",
      url: "/-/npm/v1/security/advisories/bulk",
      body: { [PROBE.name]: [PROBE.version] },
    });
  });
  it("fails an endpoint that answers with no advisories", async () => {
    await assert.rejects(
      withServer(bulk(200, "{}"), (url) => probeAdvisories(url)),
      /answered 200 without the known advisories of lodash 4\.17\.20/,
    );
  });
  it("fails an endpoint that answers with an error status", async () => {
    await assert.rejects(
      withServer(bulk(503, ""), (url) => probeAdvisories(url)),
      /answered 503/,
    );
  });
  it("skips when the connection is refused", async () => {
    const server = createServer();
    await new Promise((resolve) => server.listen(0, "127.0.0.1", resolve));
    const { port } = server.address();
    await new Promise((resolve) => server.close(resolve));
    assert.equal(await probeAdvisories(`http://127.0.0.1:${port}/`), false);
  });
  it("skips when the registry never answers", async () => {
    assert.equal(
      await withServer(
        () => {},
        (url) => probeAdvisories(url, 200),
      ),
      false,
    );
  });
});

describe("the proxy check", () => {
  it("fails a proxy pnpm has that the environment lacks", () => {
    assert.equal(unseenProxy("http://proxy:3128", {}), "http://proxy:3128");
    assert.equal(unseenProxy("http://proxy:3128", { HTTPS_PROXY: "http://proxy:3128" }), "");
    assert.equal(unseenProxy("http://proxy:3128", { https_proxy: "http://proxy:3128" }), "");
  });
  it("passes when pnpm has no proxy", () => {
    assert.equal(unseenProxy("undefined", {}), "");
    assert.equal(unseenProxy("", {}), "");
  });
});
