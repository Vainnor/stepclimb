// SPDX-License-Identifier: AGPL-3.0-or-later
// Folder paths as the files area takes them apart and puts them together (docs/specs/m1/06-files.md B9).
import { describe, expect, it } from "vitest";
import {
  childFolder,
  folderFromSearch,
  folderName,
  foldersDownTo,
  normalizeTypedFolder,
  parentOf,
} from "./folders";

describe("folder paths", () => {
  it("reads the search parameter as a folder, the top folder otherwise", () => {
    expect(folderFromSearch("/a/b")).toBe("/a/b");
    expect(folderFromSearch("a")).toBe("/");
    expect(folderFromSearch(3)).toBe("/");
    expect(folderFromSearch(undefined)).toBe("/");
  });

  it("lists a folder and every folder above it from the top", () => {
    expect(foldersDownTo("/")).toEqual(["/"]);
    expect(foldersDownTo("/a/b c/d")).toEqual(["/", "/a", "/a/b c", "/a/b c/d"]);
  });

  it("names a folder by its last segment and finds its parent", () => {
    expect(folderName("/")).toBeUndefined();
    expect(folderName("/a/week-1")).toBe("week-1");
    expect(parentOf("/a/week-1")).toBe("/a");
    expect(parentOf("/a")).toBe("/");
    expect(parentOf("/")).toBe("/");
  });

  it("puts a child folder's path together", () => {
    expect(childFolder("/", "a")).toBe("/a");
    expect(childFolder("/a", "b")).toBe("/a/b");
  });

  it("writes a typed folder as the API does", () => {
    expect(normalizeTypedFolder("  handouts/week-1/ ")).toBe("/handouts/week-1");
    expect(normalizeTypedFolder("/")).toBe("/");
    expect(normalizeTypedFolder("")).toBe("/");
    expect(normalizeTypedFolder("//")).toBe("/");
    expect(normalizeTypedFolder("/a/../b")).toBe("/a/../b");
  });
});
