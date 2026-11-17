// SPDX-License-Identifier: AGPL-3.0-or-later
// The unfinished video uploads this browser started, kept in local storage so an upload resumes
// after a reload (docs/specs/m1/08-uploaded-video.md B22). A browser does not hand a page a file
// again after a reload, so an entry is found by the file the person chooses again: its name, size,
// type and last change. An entry holds the upload's address, never the file's bytes. One stored
// more than a day ago is dropped, as the server deletes an upload left that long (B6).

/** The local storage key every entry lives under. */
export const RESUME_KEY = "stepclimb.video-uploads";

/** How long the server keeps an untouched upload (B6). */
export const RESUME_MAX_AGE_MS = 24 * 60 * 60 * 1000;

/** One unfinished upload. */
export interface ResumeEntry {
  /** Whose uploads these are: the caller's `storageKey`, such as a course's id. */
  scope: string;
  /** The file's name, size, type and last change; see `fingerprint`. */
  fingerprint: string;
  /** The upload's address. */
  url: string;
  videoId: string;
  name: string;
  size: number;
  /** The bytes the server held at the last stored chunk. */
  offset: number;
  /** When the server last stored a chunk, in milliseconds since the epoch. */
  savedAt: number;
}

/** What identifies a file chosen again after a reload. */
export function fingerprint(file: File): string {
  return JSON.stringify([file.name, file.size, file.type, file.lastModified]);
}

function isEntry(value: unknown): value is ResumeEntry {
  if (typeof value !== "object" || value === null) return false;
  const v = value as Record<string, unknown>;
  return (
    typeof v.scope === "string" &&
    typeof v.fingerprint === "string" &&
    typeof v.url === "string" &&
    typeof v.videoId === "string" &&
    typeof v.name === "string" &&
    typeof v.size === "number" &&
    typeof v.offset === "number" &&
    typeof v.savedAt === "number"
  );
}

/**
 * Every stored entry still young enough to resume. Storage that cannot be read (blocked, or
 * holding something else) reads as empty: the upload still works, without resuming after a
 * reload.
 */
function readAll(storage: Storage, now: number): ResumeEntry[] {
  let parsed: unknown;
  try {
    parsed = JSON.parse(storage.getItem(RESUME_KEY) ?? "[]");
  } catch {
    return [];
  }
  if (!Array.isArray(parsed)) return [];
  return parsed.filter(isEntry).filter((entry) => now - entry.savedAt < RESUME_MAX_AGE_MS);
}

function writeAll(storage: Storage, entries: ResumeEntry[]): void {
  try {
    if (entries.length === 0) storage.removeItem(RESUME_KEY);
    else storage.setItem(RESUME_KEY, JSON.stringify(entries));
  } catch {
    // Full or blocked storage: the upload goes on; only resuming after a reload is lost.
  }
}

/** The entries of `scope`, oldest first. */
export function resumeEntries(storage: Storage, scope: string, now: number): ResumeEntry[] {
  return readAll(storage, now).filter((entry) => entry.scope === scope);
}

/** Adds `entry`, or replaces the one with its address. */
export function saveEntry(storage: Storage, entry: ResumeEntry): void {
  const others = readAll(storage, entry.savedAt).filter((e) => e.url !== entry.url);
  writeAll(storage, [...others, entry]);
}

/** Removes the entry with address `url`. */
export function removeEntry(storage: Storage, url: string, now: number): void {
  writeAll(
    storage,
    readAll(storage, now).filter((e) => e.url !== url),
  );
}

/** The entry of `scope` for `file`, if this browser started an upload of it. */
export function entryFor(
  storage: Storage,
  scope: string,
  file: File,
  now: number,
): ResumeEntry | undefined {
  const print = fingerprint(file);
  return resumeEntries(storage, scope, now).find((entry) => entry.fingerprint === print);
}
