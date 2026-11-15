// SPDX-License-Identifier: AGPL-3.0-or-later
// The npm advisory gate (docs/specs/m0/01-repo-and-ci.md B12, Q7), run by `just web-audit`.
//
// Runs `pnpm audit --prod` and `pnpm audit` and fails on a high or critical advisory that
// pnpm-audit-allowlist.json does not list. Lower severities are printed and pass. An allowlist
// entry names a GHSA id, the advisory's package, a reason and an expiry date at most 90 days
// ahead; dates are calendar days in UTC. An expired entry, or one whose advisory is gone or now
// below high, fails, so the list stays reviewed. An advisory with an unknown severity or no GHSA
// id fails too.
//
// Before the audit, the gate asks the registry's bulk advisory endpoint about a package with
// known advisories. No answer at all (a network error or a 15 s timeout) prints SKIPPED, writes
// target/web-audit-skipped for `just ci`'s last line, and passes (Q7). An answer without those
// advisories, or any later audit error, fails: an empty report is trusted only from an endpoint
// that has just reported a known advisory.
//
// Node's fetch uses a proxy only from HTTPS_PROXY / HTTP_PROXY with NODE_USE_ENV_PROXY=1 (the
// recipe sets it); pnpm also reads its own `https-proxy` setting, so a proxy set only there
// fails the gate with the variable to export.
import { execFile } from "node:child_process";
import { mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import { promisify } from "node:util";

export const SEVERITIES = ["info", "low", "moderate", "high", "critical"];
export const FAILING_SEVERITIES = new Set(["high", "critical"]);
export const MAX_ALLOW_DAYS = 90;
// lodash 4.17.20 has had public high advisories since 2021 (GHSA-35jh-r3h4-6jhm).
export const PROBE = { name: "lodash", version: "4.17.20" };
const GHSA = /^GHSA(-[23456789cfghjmpqrvwx]{4}){3}$/;
const ALLOWLIST = "pnpm-audit-allowlist.json";
const SKIP_MARKER = "target/web-audit-skipped";
const DAY_MS = 24 * 60 * 60 * 1000;

/** Whether `text` is a real calendar date written YYYY-MM-DD. */
export function isCalendarDate(text) {
  if (typeof text !== "string" || !/^\d{4}-\d{2}-\d{2}$/.test(text)) return false;
  const date = new Date(`${text}T00:00:00Z`);
  return !Number.isNaN(date.getTime()) && date.toISOString().slice(0, 10) === text;
}

/** `days` after the UTC calendar day `day`, as YYYY-MM-DD. */
export function addDays(day, days) {
  return new Date(Date.parse(`${day}T00:00:00Z`) + days * DAY_MS).toISOString().slice(0, 10);
}

function entryProblems(entry, where, today) {
  const { id, package: name, reason, expires } = entry;
  const problems = [];
  if (typeof name !== "string" || name === "") problems.push(`${where} (${id}): needs "package"`);
  if (typeof reason !== "string" || reason.trim() === "") {
    problems.push(`${where} (${id}): needs a "reason" saying why it does not apply`);
  }
  if (!isCalendarDate(expires)) {
    problems.push(`${where} (${id}): needs "expires" as a real date, YYYY-MM-DD`);
  } else if (expires < today) {
    problems.push(
      `${where} (${id}): expired on ${expires} (UTC); review it, then renew or remove it`,
    );
  } else if (expires > addDays(today, MAX_ALLOW_DAYS)) {
    problems.push(
      `${where} (${id}): expires ${expires}, more than ${MAX_ALLOW_DAYS} days after ${today} (UTC); pick ${addDays(today, MAX_ALLOW_DAYS)} or sooner`,
    );
  }
  return problems;
}

/** Validates the allowlist; `today` is the UTC day. Returns its entries by id and problems. */
export function readAllowlist(data, today) {
  const entries = new Map();
  const problems = [];
  if (!data || !Array.isArray(data.advisories)) {
    return { entries, problems: [`${ALLOWLIST}: needs an "advisories" array`] };
  }
  for (const [index, entry] of data.advisories.entries()) {
    const where = `${ALLOWLIST} advisories[${index}]`;
    if (typeof entry?.id !== "string" || !GHSA.test(entry.id)) {
      problems.push(`${where}: "id" must be a GitHub advisory id (GHSA-xxxx-xxxx-xxxx)`);
      continue;
    }
    problems.push(...entryProblems(entry, where, today));
    if (entries.has(entry.id)) problems.push(`${where} (${entry.id}): listed twice`);
    entries.set(entry.id, entry);
  }
  return { entries, problems };
}

/** The advisories of one `pnpm audit --json` report, and the ones it cannot judge. */
export function advisoriesOf(report) {
  if (!report || typeof report.advisories !== "object" || report.advisories === null) {
    throw new Error("pnpm audit --json printed no advisories object");
  }
  const advisories = [];
  const problems = [];
  for (const [key, advisory] of Object.entries(report.advisories)) {
    const label = `advisory ${key} in ${advisory.module_name}`;
    if (
      typeof advisory.github_advisory_id !== "string" ||
      !GHSA.test(advisory.github_advisory_id)
    ) {
      problems.push(`${label} has no GHSA id, so it cannot be judged or listed: ${advisory.url}`);
    } else if (!SEVERITIES.includes(advisory.severity)) {
      problems.push(`${label} has the unknown severity "${advisory.severity}": ${advisory.url}`);
    } else {
      advisories.push({
        id: advisory.github_advisory_id,
        severity: advisory.severity,
        module: advisory.module_name,
        title: advisory.title,
        url: advisory.url,
      });
    }
  }
  return { advisories, problems };
}

function collect(reports) {
  const seen = new Map();
  const problems = [];
  for (const [scope, report] of Object.entries(reports)) {
    const result = advisoriesOf(report);
    problems.push(...result.problems.map((p) => `${p} (${scope})`));
    for (const advisory of result.advisories) {
      if (!seen.has(advisory.id)) seen.set(advisory.id, { ...advisory, scopes: [] });
      seen.get(advisory.id).scopes.push(scope);
    }
  }
  return { seen, problems };
}

function judge(advisory, entry) {
  const line = `${advisory.severity} ${advisory.id} in ${advisory.module} (${advisory.scopes.join(", ")}): ${advisory.title} ${advisory.url}`;
  if (!FAILING_SEVERITIES.has(advisory.severity)) {
    if (entry)
      return {
        failure: `${ALLOWLIST}: ${advisory.id} is ${advisory.severity} now, below the threshold; remove it`,
      };
    return { note: `below the threshold: ${line}` };
  }
  if (!entry)
    return { failure: `${line}; update the package, or list it in ${ALLOWLIST} with a reason` };
  if (entry.package !== advisory.module) {
    return {
      failure: `${ALLOWLIST}: ${advisory.id} names package "${entry.package}", the advisory is for "${advisory.module}"`,
    };
  }
  return { note: `allowed until ${entry.expires}: ${line}` };
}

/**
 * The verdict over both reports. `reports` is { prod, all }, each a `pnpm audit --json` report;
 * `today` is the UTC day, YYYY-MM-DD. Returns { failures, notes }; failures fail the gate.
 */
export function evaluate({ reports, allowlist, today }) {
  const { entries, problems } = readAllowlist(allowlist, today);
  const { seen, problems: unjudged } = collect(reports);
  const failures = [...problems, ...unjudged];
  const notes = [];
  for (const advisory of seen.values()) {
    const { failure, note } = judge(advisory, entries.get(advisory.id));
    if (failure) failures.push(failure);
    if (note) notes.push(note);
  }
  for (const id of entries.keys()) {
    if (!seen.has(id)) failures.push(`${ALLOWLIST}: ${id} matches no advisory any more; remove it`);
  }
  return { failures, notes };
}

/**
 * Asks the bulk advisory endpoint about PROBE. Returns false when nothing answers; throws when
 * the endpoint answers without PROBE's advisories; true otherwise.
 */
export async function probeAdvisories(registry, timeoutMs = 15_000) {
  const url = new URL("-/npm/v1/security/advisories/bulk", registry);
  let response;
  try {
    response = await fetch(url, {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ [PROBE.name]: [PROBE.version] }),
      signal: AbortSignal.timeout(timeoutMs),
    });
  } catch {
    return false;
  }
  const body = await response.text();
  let found = [];
  try {
    found = JSON.parse(body)[PROBE.name] ?? [];
  } catch {}
  if (!response.ok || !Array.isArray(found) || found.length === 0) {
    throw new Error(
      `the advisory endpoint ${url} answered ${response.status} without the known advisories of ${PROBE.name} ${PROBE.version}, so an empty report from it cannot be trusted`,
    );
  }
  return true;
}

/** The proxy pnpm is set to use that Node's fetch would not see, or "". */
export function unseenProxy(pnpmProxy, env) {
  const value = pnpmProxy.trim();
  if (value === "" || value === "undefined" || value === "null") return "";
  const seen = env.HTTPS_PROXY || env.https_proxy || env.HTTP_PROXY || env.http_proxy;
  return seen ? "" : value;
}

const run = promisify(execFile);

/** One `pnpm audit --json` run; pnpm exits 1 when it finds an advisory, so the JSON decides. */
async function audit(args) {
  const result = await run("pnpm", ["audit", "--json", ...args], {
    maxBuffer: 64 * 1024 * 1024,
  }).catch((error) => error);
  try {
    return JSON.parse(result.stdout);
  } catch {
    throw new Error(`pnpm audit ${args.join(" ")} failed: ${String(result.stderr).trim()}`);
  }
}

async function pnpmConfig(key) {
  return (await run("pnpm", ["config", "get", key])).stdout.trim();
}

async function main() {
  rmSync(SKIP_MARKER, { force: true });
  const proxy = unseenProxy(
    (await pnpmConfig("https-proxy")) || (await pnpmConfig("proxy")),
    process.env,
  );
  if (proxy) {
    throw new Error(
      `pnpm is set to use the proxy ${proxy}, which Node's fetch reads only from the environment; export HTTPS_PROXY=${proxy}`,
    );
  }
  const registry = await pnpmConfig("registry");
  if (!(await probeAdvisories(registry))) {
    console.warn(
      `web-audit: SKIPPED, the npm registry at ${registry} did not answer; the npm advisories were not checked. Run \`just web-audit\` online before pushing.`,
    );
    mkdirSync("target", { recursive: true });
    writeFileSync(SKIP_MARKER, `${new Date().toISOString()} ${registry}\n`);
    return 0;
  }
  const reports = { prod: await audit(["--prod"]), all: await audit([]) };
  const allowlist = JSON.parse(readFileSync(ALLOWLIST, "utf8"));
  const today = new Date().toISOString().slice(0, 10);
  const { failures, notes } = evaluate({ reports, allowlist, today });
  for (const note of notes) console.log(`web-audit: ${note}`);
  for (const failure of failures) console.error(`web-audit: ${failure}`);
  if (failures.length > 0) {
    console.error(`web-audit: FAILED (${failures.length})`);
    return 1;
  }
  console.log("web-audit: ok (no high or critical npm advisory outside the allowlist)");
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  main().then(
    (code) => process.exit(code),
    (error) => {
      console.error(`web-audit: ${error.message}`);
      process.exit(1);
    },
  );
}
