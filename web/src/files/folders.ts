// SPDX-License-Identifier: AGPL-3.0-or-later
// Folder paths as the API writes them (docs/specs/m1/06-files.md B9): `/` for a course's top
// folder, otherwise `/`-separated segments such as `/handouts/week-1`. The server checks every
// path it is sent; these only take paths apart and put them together.

export const TOP_FOLDER = "/";

/** `value` when it reads as a folder path (a string starting with `/`), otherwise the top folder. */
export function folderFromSearch(value: unknown): string {
  return typeof value === "string" && value.startsWith("/") ? value : TOP_FOLDER;
}

/** A folder's segments, from the top down: none for the top folder. */
export function segmentsOf(folder: string): string[] {
  return folder.split("/").filter((segment) => segment !== "");
}

/** `folder` and every folder above it, from the top folder down. */
export function foldersDownTo(folder: string): string[] {
  const segments = segmentsOf(folder);
  return [TOP_FOLDER, ...segments.map((_, i) => `/${segments.slice(0, i + 1).join("/")}`)];
}

/** A folder's last segment, or undefined for the top folder. */
export function folderName(folder: string): string | undefined {
  return segmentsOf(folder).at(-1);
}

/** The folder holding `folder`; the top folder holds itself. */
export function parentOf(folder: string): string {
  const all = foldersDownTo(folder);
  return all.at(-2) ?? TOP_FOLDER;
}

/** The path of the folder named `name` inside `parent`. */
export function childFolder(parent: string, name: string): string {
  return parent === TOP_FOLDER ? `/${name}` : `${parent}/${name}`;
}

/**
 * The folder path a person typed: surrounding spaces trimmed, a leading `/` added and a trailing
 * one dropped. `""` and `"/"` are the top folder. Anything else is left for the server to check.
 */
export function normalizeTypedFolder(typed: string): string {
  let path = typed.trim();
  if (!path.startsWith("/")) path = `/${path}`;
  while (path.length > 1 && path.endsWith("/")) path = path.slice(0, -1);
  return path;
}
