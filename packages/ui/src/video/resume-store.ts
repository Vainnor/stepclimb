// SPDX-License-Identifier: AGPL-3.0-or-later
// The unfinished video uploads this browser started, kept in local storage so an upload resumes
// after a reload (docs/specs/m1/08-uploaded-video.md B22, D30). They are the signed-in user's in
// their organization: each owner (the caller's organization and user ids) has a key of its own,
// so another person signed in on the same browser never sees or ends them, and an explicit
// sign-out clears them (`clearResumeEntries`). A browser does not hand a page a file again after a
// reload, so an entry is found by the file the person chooses again: its name, size, type and last
// change, in the course it was started in. An entry holds the upload's address, never the file's
// bytes. One stored more than a day ago is dropped, as the server deletes an upload left that long
// (B6).

/** Every owner's key starts with this; the owner follows it. */
export const RESUME_KEY_PREFIX = "stepclimb.video-uploads.";

/** How long the server keeps an untouched upload (B6). */
export const RESUME_MAX_AGE_MS = 24 * 60 * 60 * 1000;

/** One unfinished upload. */
export interface ResumeEntry {
  /** The course it was started in: the caller's `storageKey`. */
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

/** The local storage key of `owner`'s entries. */
export function resumeKey(owner: string): string {
  return `${RESUME_KEY_PREFIX}${owner}`;
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

/** Removes every entry of `owner`: their explicit sign-out (D30). */
export function clearResumeEntries(storage: Storage, owner: string): void {
  try {
    storage.removeItem(resumeKey(owner));
  } catch {
    // Blocked storage holds nothing to clear.
  }
}

/** One owner's entries in `storage`. */
export interface ResumeStore {
  /** The entries of `scope` still young enough to resume, oldest first. */
  entries(scope: string, now: number): ResumeEntry[];
  /** The entry of `scope` for `file`, if this owner started an upload of it here. */
  entryFor(scope: string, file: File, now: number): ResumeEntry | undefined;
  /** Adds `entry`, or replaces the one with its address. */
  save(entry: ResumeEntry): void;
  /** Removes the entry with address `url`. */
  remove(url: string, now: number): void;
}

/**
 * `owner`'s entries in `storage`, keeping only those `accepts` (an address this client may send
 * to). Storage that cannot be read (blocked, or holding something else) reads as empty, and one
 * that refuses a write keeps nothing: the upload still works, without resuming after a reload.
 */
export function resumeStore(
  storage: Storage,
  owner: string,
  accepts: (url: string) => boolean,
): ResumeStore {
  const key = resumeKey(owner);
  const readAll = (now: number): ResumeEntry[] => {
    let parsed: unknown;
    try {
      parsed = JSON.parse(storage.getItem(key) ?? "[]");
    } catch {
      return [];
    }
    if (!Array.isArray(parsed)) return [];
    return parsed
      .filter(isEntry)
      .filter((entry) => now - entry.savedAt < RESUME_MAX_AGE_MS && accepts(entry.url));
  };
  const writeAll = (entries: ResumeEntry[]) => {
    try {
      if (entries.length === 0) storage.removeItem(key);
      else storage.setItem(key, JSON.stringify(entries));
    } catch {
      // Full or blocked storage: the upload goes on; only resuming after a reload is lost.
    }
  };
  const entries = (scope: string, now: number) =>
    readAll(now).filter((entry) => entry.scope === scope);
  return {
    entries,
    entryFor: (scope, file, now) => {
      const print = fingerprint(file);
      return entries(scope, now).find((entry) => entry.fingerprint === print);
    },
    save: (entry) =>
      writeAll([...readAll(entry.savedAt).filter((e) => e.url !== entry.url), entry]),
    remove: (url, now) => writeAll(readAll(now).filter((e) => e.url !== url)),
  };
}

/** The path every upload's address has (B5). */
const UPLOADS_PATH = "/api/v1/video-uploads/";

/**
 * Whether `url` is an upload address on `endpoint`'s origin, so an entry another page wrote into
 * local storage cannot send this page's requests, and their cookies, anywhere else.
 */
export function isUploadAddress(url: string, endpoint: string): boolean {
  try {
    const target = new URL(url);
    return (
      target.origin === new URL(endpoint).origin &&
      target.pathname.startsWith(UPLOADS_PATH) &&
      target.pathname.length > UPLOADS_PATH.length &&
      target.search === "" &&
      target.hash === ""
    );
  } catch {
    return false;
  }
}
